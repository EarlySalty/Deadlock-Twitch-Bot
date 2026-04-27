from __future__ import annotations

import asyncio
import logging

from discord.ext import commands

from .enrichment import (
    ClipEnrichmentPipeline,
    iter_pending_enrichments,
)

log = logging.getLogger("TwitchStreams.SocialMediaEnrichmentWorker")


class SocialMediaEnrichmentWorker(commands.Cog):
    """Background-Worker, der pending Clips per Whisper+Vocab+LLM anreichert."""

    def __init__(self, bot, *, pipeline: ClipEnrichmentPipeline | None = None) -> None:
        self.bot = bot
        self.enabled = True
        self.interval_seconds = 90
        self.batch_size = 3
        self._pipeline = pipeline or ClipEnrichmentPipeline()
        self._task = bot.loop.create_task(self._enrichment_loop())
        log.info(
            "Social media enrichment worker started (interval=%ss, batch=%s)",
            self.interval_seconds,
            self.batch_size,
        )

    def cog_unload(self):
        if self._task:
            self._task.cancel()
        log.info("Social media enrichment worker stopped")

    async def _enrichment_loop(self):
        await self.bot.wait_until_ready()
        await asyncio.sleep(45)

        while not self.bot.is_closed() and self.enabled:
            try:
                await self._process_pending()
            except Exception:
                log.exception("Social media enrichment run failed")
            await asyncio.sleep(self.interval_seconds)

    async def _process_pending(self) -> None:
        try:
            pending = iter_pending_enrichments(limit=self.batch_size)
        except Exception:
            log.exception("iter_pending_enrichments failed")
            return
        if not pending:
            return

        log.info("Enrichment-Worker verarbeitet %s Clips", len(pending))
        for clip_db_id in pending:
            try:
                outcome = await self._pipeline.run(clip_db_id, force=False)
                log.info(
                    "Enrichment-Outcome clip_db_id=%s status=%s provider=%s model=%s",
                    clip_db_id,
                    outcome.status,
                    outcome.provider,
                    outcome.model,
                )
            except Exception:
                log.exception("Enrichment fuer Clip %s fehlgeschlagen", clip_db_id)


async def setup(bot):
    await bot.add_cog(SocialMediaEnrichmentWorker(bot))
