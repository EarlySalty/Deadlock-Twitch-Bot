from __future__ import annotations

import asyncio
import unittest
from types import SimpleNamespace
from unittest.mock import patch

from bot.raid.bot import RaidBot


class _FakeTask:
    def __init__(self) -> None:
        self.cancelled = False
        self._done = False

    def done(self) -> bool:
        return self._done

    def cancel(self) -> None:
        self.cancelled = True
        self._done = True

    def add_done_callback(self, callback) -> None:
        if self._done:
            callback(self)

    def get_name(self) -> str:
        return "raid.bot.test"


class RaidBotLifecycleTests(unittest.IsolatedAsyncioTestCase):
    async def test_start_delegates_cleanup_task_creation_to_lifecycle(self) -> None:
        cleanup_task = _FakeTask()

        with (
            patch("bot.raid.bot.RaidAuthManager", autospec=True),
            patch("bot.raid.bot.RaidExecutor", autospec=True),
        ):
            bot = RaidBot(
                "client-id",
                "client-secret",
                "https://raid.example/twitch/raid/callback",
                SimpleNamespace(closed=False),
            )
        self.assertIsNone(bot._cleanup_task)

        with patch.object(bot._lifecycle, "start", return_value=cleanup_task) as start_mock:
            bot.start()

        start_mock.assert_called_once_with()
        self.assertIs(bot._cleanup_task, cleanup_task)

    async def test_cleanup_cancels_managed_background_tasks(self) -> None:
        bot = object.__new__(RaidBot)
        bot._managed_bg_tasks = set()

        gate = asyncio.Event()

        async def _job() -> str:
            await gate.wait()
            return "done"

        task = RaidBot._spawn_bg_task(bot, _job(), "raid.bot.test")
        self.assertIsNotNone(task)
        self.assertEqual(len(bot._managed_bg_tasks), 1)

        await bot.cleanup()

        self.assertTrue(task.cancelled())
        self.assertEqual(len(bot._managed_bg_tasks), 0)


if __name__ == "__main__":
    unittest.main()
