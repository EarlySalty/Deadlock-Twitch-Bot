"""Standalone Twitch worker service — runs TwitchStreamCog without Discord."""

from __future__ import annotations

import asyncio
from typing import Any

from ..core.constants import log
from ..runtime_lock import runtime_pid_lock
from ..runtime_mode import INTERNAL_API_PORT, enforce_internal_api_runtime


class HeadlessBot:
    """Duck-typing stub for discord.ext.commands.Bot used by TwitchStreamCog."""

    def __init__(self) -> None:
        self.guilds: list[Any] = []
        self._ready = asyncio.Event()
        self._ready.set()
        self.loop = asyncio.get_running_loop()

    async def wait_until_ready(self) -> None:
        await self._ready.wait()

    def is_closed(self) -> bool:
        return False

    # --- guild / channel / user lookups (all no-op in headless mode) ---

    def get_guild(self, guild_id: int) -> None:
        return None

    def get_channel(self, channel_id: int) -> None:
        return None

    async def fetch_channel(self, channel_id: int) -> None:
        return None

    def get_user(self, user_id: int) -> None:
        return None

    async def fetch_user(self, user_id: int) -> None:
        return None

    # --- command/view/extension management (no-op in headless mode) ---

    def add_view(self, view: Any, *, message_id: int | None = None) -> None:
        return None

    def get_command(self, name: str) -> None:
        return None

    def remove_command(self, name: str) -> None:
        return None

    async def load_extension(self, name: str) -> None:
        return None

    async def unload_extension(self, name: str) -> None:
        return None


async def run_bot_service(*, port: int | None = None) -> None:
    """Run standalone Twitch worker service until cancelled."""
    resolved_port = port if port is not None else INTERNAL_API_PORT
    enforce_internal_api_runtime(port=resolved_port)

    with runtime_pid_lock("twitch_worker", port=resolved_port):
        log.info("twitch_worker: initialising TwitchStreamCog in standalone mode")

        from ..cog import TwitchStreamCog

        bot = HeadlessBot()
        cog = TwitchStreamCog(bot)

        try:
            await cog.cog_load()
            await asyncio.Event().wait()
        except asyncio.CancelledError:
            log.info("twitch_worker: shutdown requested")
        finally:
            await cog.cog_unload()


__all__ = ["HeadlessBot", "run_bot_service"]
