from __future__ import annotations

import asyncio
import logging
from datetime import UTC, datetime
from pathlib import Path

from discord.ext import commands

from .retention import delete_clips_by_ids
from .retention import is_clip_published_on_all_active_platforms
from .retention import iter_expired_clips_for_retention

log = logging.getLogger("TwitchStreams.SocialMediaRetentionWorker")


def _utcnow() -> datetime:
    return datetime.now(UTC)


class SocialMediaRetentionWorker(commands.Cog):
    """Delete expired social-media clips after publication or discard."""

    def __init__(self, bot) -> None:
        self.bot = bot
        self.enabled = True
        self.interval_seconds = 30 * 60
        self._task = bot.loop.create_task(self._retention_loop())
        log.info("Social media retention worker started (interval=%ss)", self.interval_seconds)

    def cog_unload(self):
        if self._task:
            self._task.cancel()
        log.info("Social media retention worker stopped")

    async def _retention_loop(self):
        await self.bot.wait_until_ready()
        await asyncio.sleep(30)

        while not self.bot.is_closed() and self.enabled:
            try:
                await self._cleanup_expired_clips()
            except Exception:
                log.exception("Social media retention run failed")
            await asyncio.sleep(self.interval_seconds)

    async def _cleanup_expired_clips(self) -> None:
        now = _utcnow()
        candidates = iter_expired_clips_for_retention(now)
        deleted_ids: list[int] = []

        for clip in candidates:
            clip_id = int(clip["id"])
            if not clip.get("discarded_at") and not is_clip_published_on_all_active_platforms(clip_id):
                continue

            file_path = str(clip.get("upload_local_path") or clip.get("local_file_path") or "").strip()
            if file_path:
                try:
                    Path(file_path).unlink(missing_ok=True)
                except OSError:
                    log.warning("Retention could not delete file for clip_db_id=%s", clip_id, exc_info=True)
                    continue

            deleted_ids.append(clip_id)
            log.info(
                "Retention deleted clip_db_id=%s clip_id=%s source_kind=%s",
                clip_id,
                clip.get("clip_id"),
                clip.get("source_kind"),
            )

        delete_clips_by_ids(deleted_ids)


async def setup(bot):
    await bot.add_cog(SocialMediaRetentionWorker(bot))
