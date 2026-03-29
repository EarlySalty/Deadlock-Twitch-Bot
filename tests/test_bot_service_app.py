from __future__ import annotations

import asyncio
import unittest
from unittest.mock import AsyncMock, patch

from bot.bot_service.app import run_bot_service


class BotServiceAppTests(unittest.IsolatedAsyncioTestCase):
    async def test_run_bot_service_calls_cog_load_before_waiting(self) -> None:
        cog = AsyncMock()
        wait_forever = AsyncMock(side_effect=asyncio.CancelledError())

        with (
            patch("bot.bot_service.app.enforce_internal_api_runtime"),
            patch("bot.bot_service.app.runtime_pid_lock") as runtime_lock,
            patch("bot.bot_service.app.asyncio.Event") as event_cls,
            patch("bot.bot_service.app.HeadlessBot") as headless_bot_cls,
            patch("bot.cog.TwitchStreamCog") as twitch_cog_cls,
        ):
            runtime_lock.return_value.__enter__.return_value = None
            runtime_lock.return_value.__exit__.return_value = False
            event_cls.return_value.wait = wait_forever
            twitch_cog_cls.return_value = cog

            await run_bot_service(port=8776)

        headless_bot_cls.assert_called_once_with()
        twitch_cog_cls.assert_called_once_with(headless_bot_cls.return_value)
        cog.cog_load.assert_awaited_once()
        cog.cog_unload.assert_awaited_once()
        wait_forever.assert_awaited_once()


if __name__ == "__main__":
    unittest.main()
