from __future__ import annotations

import asyncio
import logging
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from typing import Any

from discord.ext import commands

from ...storage import readonly_connection
from ..credential_manager import SocialMediaCredentialManager
from ..uploaders.instagram import InstagramUploader
from ..uploaders.tiktok import TikTokUploader
from ..uploaders.youtube import YouTubeUploader
from . import BUCKETS, PLATFORMS, upsert_clip_analytics, utcnow

log = logging.getLogger("TwitchStreams.SocialMedia.InsightsWorker")

SUCCESS_POLL_DELAYS = {
    "24h": timedelta(hours=6),
    "7d": timedelta(hours=24),
    "30d": timedelta(days=3),
}
RETRY_DELAY = timedelta(hours=1)


@dataclass(frozen=True)
class AnalyticsTarget:
    clip_db_id: int
    streamer_login: str
    platform: str
    platform_video_id: str
    bucket: str


class SocialMediaInsightsWorker(commands.Cog):
    """Pollt Plattform-Analytics und persistiert 24h/7d/30d-Snapshots."""

    def __init__(
        self,
        bot,
        *,
        credential_manager: SocialMediaCredentialManager | None = None,
        client_factory: dict[str, Any] | None = None,
    ) -> None:
        self.bot = bot
        self.enabled = True
        self.interval_seconds = 30 * 60
        self.batch_size = 18
        self.credential_manager = credential_manager or SocialMediaCredentialManager()
        self.client_factory = client_factory or {}
        self._task = bot.loop.create_task(self._worker_loop())
        log.info(
            "Social media insights worker started (interval=%ss, batch=%s)",
            self.interval_seconds,
            self.batch_size,
        )

    def cog_unload(self):
        if self._task:
            self._task.cancel()
        log.info("Social media insights worker stopped")

    async def _worker_loop(self):
        await self.bot.wait_until_ready()
        await asyncio.sleep(75)
        while not self.bot.is_closed() and self.enabled:
            try:
                await self._process_due_targets()
            except Exception:
                log.exception("Social media insights run failed")
            await asyncio.sleep(self.interval_seconds)

    async def _process_due_targets(self) -> None:
        targets = self._collect_due_targets(limit=self.batch_size)
        if not targets:
            return

        client_cache: dict[tuple[str, str], Any] = {}
        for target in targets:
            cache_key = (target.platform, target.streamer_login)
            client = client_cache.get(cache_key)
            if client is None:
                client = await self._resolve_client(target.platform, target.streamer_login)
                client_cache[cache_key] = client
            if client is None:
                self._schedule_retry(target, provider=f"error:{target.platform}:missing_client")
                continue
            try:
                metrics = await client.fetch_video_analytics(target.platform_video_id, target.bucket)
            except Exception:
                log.warning(
                    "Analytics fetch failed for clip=%s platform=%s bucket=%s",
                    target.clip_db_id,
                    target.platform,
                    target.bucket,
                    exc_info=True,
                )
                self._schedule_retry(target, provider=f"error:{target.platform}:api")
                continue

            provider = str(metrics.get("provider") or target.platform).strip()
            views = int(metrics.get("views") or 0)
            likes = int(metrics.get("likes") or 0)
            comments = int(metrics.get("comments") or 0)
            shares = int(metrics.get("shares") or 0)
            engagement_rate = metrics.get("engagement_rate")
            if engagement_rate is None and views > 0:
                engagement_rate = ((likes + comments + shares) / views) * 100.0

            upsert_clip_analytics(
                clip_db_id=target.clip_db_id,
                platform=target.platform,
                bucket=target.bucket,
                views=views,
                likes=likes,
                comments=comments,
                shares=shares,
                watch_time_seconds=_maybe_int(metrics.get("watch_time_seconds")),
                ctr_percent=_maybe_float(metrics.get("ctr_percent")),
                engagement_rate=_maybe_float(engagement_rate),
                provider=provider,
                synced_at=utcnow(),
                next_pull_at=utcnow() + SUCCESS_POLL_DELAYS.get(target.bucket, timedelta(days=1)),
            )

    def _collect_due_targets(self, *, limit: int) -> list[AnalyticsTarget]:
        now = utcnow().isoformat()
        with readonly_connection() as conn:
            clip_rows = conn.execute(
                """
                SELECT id, streamer_login,
                       uploaded_tiktok, uploaded_youtube, uploaded_instagram,
                       tiktok_video_id, youtube_video_id, instagram_media_id
                  FROM twitch_clips_social_media
                 WHERE discarded_at IS NULL
                   AND (
                       (uploaded_tiktok = 1 AND tiktok_video_id IS NOT NULL)
                    OR (uploaded_youtube = 1 AND youtube_video_id IS NOT NULL)
                    OR (uploaded_instagram = 1 AND instagram_media_id IS NOT NULL)
                   )
                 ORDER BY created_at DESC, id DESC
                 LIMIT %s
                """,
                (max(limit * 4, limit),),
            ).fetchall()

            analytics_rows = conn.execute(
                """
                SELECT clip_id, platform, bucket, next_pull_at
                  FROM twitch_clips_social_analytics
                """
            ).fetchall()

        existing: dict[tuple[int, str, str], str | None] = {}
        for row in analytics_rows:
            existing[(int(row["clip_id"]), str(row["platform"]), str(row["bucket"]))] = (
                str(row["next_pull_at"]) if row["next_pull_at"] else None
            )

        due: list[AnalyticsTarget] = []
        for row in clip_rows:
            platform_ids = {
                "tiktok": row["tiktok_video_id"] if row["uploaded_tiktok"] else None,
                "youtube": row["youtube_video_id"] if row["uploaded_youtube"] else None,
                "instagram": row["instagram_media_id"] if row["uploaded_instagram"] else None,
            }
            for platform in PLATFORMS:
                platform_video_id = str(platform_ids.get(platform) or "").strip()
                if not platform_video_id:
                    continue
                for bucket in BUCKETS:
                    next_pull_at = existing.get((int(row["id"]), platform, bucket))
                    if next_pull_at and next_pull_at > now:
                        continue
                    due.append(
                        AnalyticsTarget(
                            clip_db_id=int(row["id"]),
                            streamer_login=str(row["streamer_login"]),
                            platform=platform,
                            platform_video_id=platform_video_id,
                            bucket=bucket,
                        )
                    )
                    if len(due) >= limit:
                        return due
        return due

    async def _resolve_client(self, platform: str, streamer_login: str):
        custom_factory = self.client_factory.get(platform)
        if callable(custom_factory):
            return await _maybe_await(custom_factory(streamer_login))

        credentials = self.credential_manager.get_credentials(platform, streamer_login)
        if not credentials:
            return None

        if platform == "youtube":
            client = YouTubeUploader(
                credentials["client_id"],
                credentials.get("client_secret", ""),
            )
            authenticated = await client.authenticate(
                {
                    "access_token": credentials.get("access_token"),
                    "refresh_token": credentials.get("refresh_token"),
                }
            )
            return client if authenticated else None

        if platform == "tiktok":
            client = TikTokUploader(
                credentials["client_id"],
                credentials.get("client_secret", ""),
            )
            client.access_token = credentials.get("access_token")
            return client

        if platform == "instagram":
            if not credentials.get("platform_user_id"):
                return None
            return InstagramUploader(
                credentials["access_token"],
                credentials["platform_user_id"],
            )

        return None

    def _schedule_retry(self, target: AnalyticsTarget, *, provider: str) -> None:
        upsert_clip_analytics(
            clip_db_id=target.clip_db_id,
            platform=target.platform,
            bucket=target.bucket,
            provider=provider,
            synced_at=utcnow(),
            next_pull_at=utcnow() + RETRY_DELAY,
        )


async def _maybe_await(value):
    if asyncio.iscoroutine(value):
        return await value
    return value


def _maybe_int(value: Any) -> int | None:
    if value is None or value == "":
        return None
    return int(value)


def _maybe_float(value: Any) -> float | None:
    if value is None or value == "":
        return None
    return float(value)


async def setup(bot):
    await bot.add_cog(SocialMediaInsightsWorker(bot))
