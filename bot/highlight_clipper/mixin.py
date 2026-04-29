from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .worker import HighlightClipperWorker


class HighlightClipperMixin:
    """Beta-Mixin: Auto-Clips fuer EarlySalty per Discord DM."""

    _highlight_clipper_worker: HighlightClipperWorker | None = None

    async def _hc_start(self) -> None:
        if self._highlight_clipper_worker is None:
            from .worker import HighlightClipperWorker

            self._highlight_clipper_worker = HighlightClipperWorker(self.bot)
        await self._highlight_clipper_worker.start()

    async def _hc_stop(self) -> None:
        if self._highlight_clipper_worker is None:
            return
        await self._highlight_clipper_worker.stop()
        self._highlight_clipper_worker = None
