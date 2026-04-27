from __future__ import annotations

import asyncio
import logging

from discord.ext import commands

from .approval import (
    ApprovalService,
    iter_approved_clips_pending_queue,
    iter_clips_needing_approval_dm,
)
from .clip_manager import ClipManager

log = logging.getLogger("TwitchStreams.SocialMediaApprovalWorker")


class SocialMediaApprovalWorker(commands.Cog):
    """Versendet Approval-DMs und zieht freigegebene Uploads in die Queue."""

    def __init__(
        self,
        bot,
        clip_manager: ClipManager,
        *,
        approval_service: ApprovalService | None = None,
    ) -> None:
        self.bot = bot
        self.clip_manager = clip_manager
        self.enabled = True
        self.interval_seconds = 60
        self.batch_size = 10
        self._approval_service = approval_service or ApprovalService(bot, clip_manager)
        self._task = bot.loop.create_task(self._approval_loop())
        log.info(
            "Social media approval worker started (interval=%ss, batch=%s)",
            self.interval_seconds,
            self.batch_size,
        )

    def cog_unload(self):
        if self._task:
            self._task.cancel()
        log.info("Social media approval worker stopped")

    async def _approval_loop(self) -> None:
        await self.bot.wait_until_ready()
        await asyncio.sleep(20)

        while not self.bot.is_closed() and self.enabled:
            try:
                await self._dispatch_pending_dms()
                await self._queue_approved_uploads()
            except Exception:
                log.exception("Social media approval run failed")
            await asyncio.sleep(self.interval_seconds)

    async def _dispatch_pending_dms(self) -> None:
        admin_user_id = self._approval_service.default_admin_user_id()
        for clip_db_id in iter_clips_needing_approval_dm(limit=self.batch_size):
            try:
                await self._approval_service.send_dm(clip_db_id, admin_user_id)
            except Exception:
                log.warning(
                    "Approval DM send failed for clip_db_id=%s; retry in next loop",
                    clip_db_id,
                    exc_info=True,
                )
                continue

    async def _queue_approved_uploads(self) -> None:
        for clip_db_id in iter_approved_clips_pending_queue(limit=self.batch_size):
            try:
                queued = self._approval_service.ensure_queued_uploads(clip_db_id)
            except Exception:
                log.warning(
                    "Approval queue sync failed for clip_db_id=%s",
                    clip_db_id,
                    exc_info=True,
                )
                continue
            if queued:
                log.info(
                    "Approval queued uploads for clip_db_id=%s platforms=%s",
                    clip_db_id,
                    ",".join(item["platform"] for item in queued),
                )


async def setup(bot):
    await bot.add_cog(SocialMediaApprovalWorker(bot, ClipManager()))
