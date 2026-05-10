"""
Upload Worker - Processes Upload Queue and Orchestrates Uploads.

Workflow:
1. Fetch pending uploads from queue
2. Download Twitch clip (if not already downloaded)
3. Convert video to platform-specific format (9:16, max duration)
4. Upload to platform
5. Update queue status (completed/failed)
"""

import asyncio
import logging
from datetime import UTC, datetime
from pathlib import Path

from discord.ext import commands

from ..storage import transaction
from .approval import is_clip_approved_for
from .clip_manager import ClipManager, UPLOAD_PROCESSING_STALE_AFTER
from .credential_manager import SocialMediaCredentialManager
from .uploaders import VideoProcessor

log = logging.getLogger("TwitchStreams.UploadWorker")


class UploadWorker(commands.Cog):
    """Processes upload queue and uploads to platforms."""

    def __init__(self, bot, clip_manager: ClipManager):
        """
        Args:
            bot: Discord bot instance
            clip_manager: ClipManager instance
        """
        self.bot = bot
        self.clip_manager = clip_manager
        self.enabled = True
        self.interval_seconds = 60  # Check queue every minute
        self.max_parallel = 2  # Max parallel uploads per run

        self.credential_manager = SocialMediaCredentialManager()
        self.video_processor = VideoProcessor()

        # Start background worker
        self._task = bot.loop.create_task(self._worker_loop())
        log.info(
            "Upload worker started (interval=%ss, max_parallel=%s)",
            self.interval_seconds,
            self.max_parallel,
        )

    def cog_unload(self):
        """Cleanup on cog unload."""
        if self._task:
            self._task.cancel()

    async def _build_uploader(self, platform: str, credentials: dict):
        """Create and authenticate a platform uploader for one credential record."""
        if platform == "tiktok":
            if not credentials.get("client_id") or not credentials.get("access_token"):
                return None

            from .uploaders import TikTokUploader

            uploader = TikTokUploader(
                credentials["client_id"],
                credentials.get("client_secret", ""),
            )
            uploader.access_token = credentials["access_token"]
            return uploader

        if platform == "youtube":
            if not credentials.get("client_id") or not credentials.get("access_token"):
                return None

            from .uploaders import YouTubeUploader

            uploader = YouTubeUploader(
                credentials["client_id"],
                credentials.get("client_secret", ""),
            )
            authenticated = await uploader.authenticate(
                {
                    "access_token": credentials["access_token"],
                    "refresh_token": credentials.get("refresh_token"),
                }
            )
            return uploader if authenticated else None

        if platform == "instagram":
            if not credentials.get("access_token") or not credentials.get("platform_user_id"):
                return None

            from .uploaders import InstagramUploader

            return InstagramUploader(
                credentials["access_token"],
                credentials["platform_user_id"],
            )

        log.warning("Skipping upload for unsupported platform=%s", platform)
        return None

    async def _resolve_uploader(
        self,
        platform: str,
        streamer_login: str | None,
        uploader_cache: dict[tuple[str, int], object | None],
    ):
        """Resolve a platform uploader for the queue item's streamer, with global fallback."""
        credentials = self.credential_manager.get_credentials(platform, streamer_login)
        if not credentials:
            return None

        cache_key = (platform, credentials["id"])
        if cache_key in uploader_cache:
            return uploader_cache[cache_key]

        try:
            uploader = await self._build_uploader(platform, credentials)
        except Exception:
            log.exception(
                "Failed to initialize uploader for platform=%s, streamer=%s",
                platform,
                credentials.get("streamer_login") or streamer_login or "<global>",
            )
            uploader = None

        if uploader and streamer_login and credentials.get("streamer_login") is None:
            log.info(  # nosemgrep: python.lang.security.audit.logging.logger-credential-leak.python-logger-credential-disclosure
                "Using global %s credentials as fallback for streamer=%s",
                platform,
                streamer_login,
            )

        uploader_cache[cache_key] = uploader
        return uploader

    async def _worker_loop(self):
        """Main worker loop - runs every minute."""
        await self.bot.wait_until_ready()
        await asyncio.sleep(10)  # Initial delay

        while not self.bot.is_closed() and self.enabled:
            try:
                await self._process_queue()
            except Exception:
                log.exception("Upload worker run failed")

            await asyncio.sleep(self.interval_seconds)

    async def _process_queue(self):
        """Process pending uploads from queue."""
        stats = {"processed": 0, "success": 0, "failed": 0}
        queue_scan_limit = max(self.max_parallel * 10, self.max_parallel)
        stale_cutoff = (datetime.now(UTC) - UPLOAD_PROCESSING_STALE_AFTER).isoformat()
        queue = self.clip_manager.get_upload_queue(
            status="pending",
            limit=queue_scan_limit,
            reclaim_stale_processing_before=stale_cutoff,
        )

        if not queue:
            return

        uploader_cache: dict[tuple[str, int], object | None] = {}
        batch: list[tuple[dict, object]] = []

        for item in queue:
            uploader = await self._resolve_uploader(
                item["platform"],
                item.get("streamer_login"),
                uploader_cache,
            )
            if not uploader:
                log.warning(  # nosemgrep: python.lang.security.audit.logging.logger-credential-leak.python-logger-credential-disclosure
                    "No uploader credentials available for queue_id=%s, platform=%s, streamer=%s",
                    item["id"],
                    item["platform"],
                    item.get("streamer_login") or "<global>",
                )
                continue

            batch.append((item, uploader))
            if len(batch) >= self.max_parallel:
                break

        if not batch:
            return

        log.info(
            "Processing %s uploads across %s platform/account combinations",
            len(batch),
            len({(item['platform'], item.get('streamer_login')) for item, _ in batch}),
        )

        tasks = [self._process_upload(item, uploader) for item, uploader in batch]
        results = await asyncio.gather(*tasks, return_exceptions=True)

        for result in results:
            stats["processed"] += 1
            if result is True:
                stats["success"] += 1
            else:
                stats["failed"] += 1

        if stats["processed"] > 0:
            log.info(
                "Upload batch complete: %s processed, %s success, %s failed",
                stats["processed"],
                stats["success"],
                stats["failed"],
            )

    async def _process_upload(self, queue_item: dict, uploader) -> bool:
        """
        Process single upload.

        Args:
            queue_item: Queue item dict
            uploader: PlatformUploader instance

        Returns:
            True wenn erfolgreich
        """
        queue_id = queue_item["id"]
        clip_db_id = int(queue_item.get("clip_db_id") or queue_item["clip_id"])
        platform = queue_item["platform"]

        try:
            if not is_clip_approved_for(clip_db_id, platform):
                log.info(
                    "Skipping upload without approval: clip_db_id=%s platform=%s queue_id=%s",
                    clip_db_id,
                    platform,
                    queue_id,
                )
                self.clip_manager.update_upload_status(
                    queue_id,
                    "failed",
                    error="approval_required",
                )
                return False

            # Mark as processing
            self.clip_manager.update_upload_status(queue_id, "processing")

            # Get clip details
            clip_url = queue_item["clip_url"]
            clip_title = queue_item["clip_title"]
            local_path = queue_item.get("local_file_path")

            # Download clip if not already downloaded
            if not local_path or not Path(local_path).exists():
                local_path = await self._download_clip(clip_url, clip_db_id)
                self.clip_manager.update_upload_status(queue_id, "processing")

            # Convert to vertical format (9:16)
            converted_path = await self._convert_to_vertical(local_path, platform)
            self.clip_manager.update_upload_status(queue_id, "processing")

            # Upload to platform
            title = queue_item.get("title") or clip_title
            description = queue_item.get("description") or ""
            hashtags = queue_item.get("hashtags")

            if hashtags:
                import json

                hashtags = json.loads(hashtags) if isinstance(hashtags, str) else hashtags
            else:
                hashtags = []

            external_id = await uploader.upload_video(
                video_path=converted_path,
                title=title,
                description=description,
                hashtags=hashtags,
            )

            # Mark as completed
            self.clip_manager.update_upload_status(
                queue_id,
                "completed",
                external_video_id=external_id,
            )

            log.info("Upload successful: Clip %s -> %s (%s)", clip_db_id, platform, external_id)
            return True

        except Exception as e:
            log.exception("Upload failed: Clip %s -> %s", clip_db_id, platform)

            self.clip_manager.update_upload_status(
                queue_id,
                "failed",
                error=str(e),
            )

            return False

    async def _download_clip(self, clip_url: str, clip_id: int) -> str:
        """
        Download Twitch clip.

        Args:
            clip_url: Twitch clip URL
            clip_id: Clip DB ID

        Returns:
            Lokaler Pfad zur heruntergeladenen Datei
        """
        output_dir = Path("data/clips")
        output_dir.mkdir(parents=True, exist_ok=True)
        output_path = output_dir / f"{clip_id}.mp4"

        # Skip if already downloaded
        if output_path.exists():
            log.debug("Clip already downloaded: %s", output_path)
            return str(output_path)

        # Use yt-dlp to download
        log.info("Downloading clip: %s", clip_url)

        proc = await asyncio.create_subprocess_exec(
            "yt-dlp",
            "-f",
            "best",
            "-o",
            str(output_path),
            clip_url,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )

        stdout, stderr = await proc.communicate()

        if proc.returncode != 0:
            error = stderr.decode()
            log.error("Clip download failed: %s", error)
            raise Exception(f"yt-dlp failed: {error}")

        if not output_path.exists():
            raise Exception(f"Downloaded file not found: {output_path}")

        # Update DB with local path
        with transaction() as conn:
            conn.execute(
                """
                UPDATE twitch_clips_social_media
                   SET local_file_path = %s, downloaded_at = %s
                 WHERE id = %s
                """,
                (str(output_path), datetime.now(UTC).isoformat(), clip_id),
            )

        log.info("Clip downloaded: %s", output_path)
        return str(output_path)

    async def _convert_to_vertical(self, input_path: str, platform: str) -> str:
        """
        Convert video to 9:16 format.

        Args:
            input_path: Input video path
            platform: Platform name (for duration limits)

        Returns:
            Pfad zur konvertierten Datei
        """
        output_path = input_path.replace(".mp4", f"_{platform}_vertical.mp4")

        # Skip if already converted
        if Path(output_path).exists():
            log.debug("Video already converted: %s", output_path)
            return output_path

        # Platform-specific duration limits
        max_duration = {
            "tiktok": 60,
            "youtube": 60,
            "instagram": 90,
        }.get(platform, 60)

        # Convert (trim + crop to 9:16)
        log.info("Converting video to 9:16 (max %ss): %s", max_duration, input_path)

        await self.video_processor.convert_and_trim(
            input_path=input_path,
            output_path=output_path,
            max_duration=max_duration,
            target_width=1080,
            target_height=1920,
        )

        log.info("Video converted: %s", output_path)
        return output_path


async def setup(bot):
    """Setup function for Discord.py cog."""
    # This cog is loaded by TwitchCog, not directly
    pass
