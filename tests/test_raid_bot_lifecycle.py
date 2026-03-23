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
    async def test_init_spawns_cleanup_task_through_helper(self) -> None:
        spawned: list[str] = []
        cleanup_task = _FakeTask()

        def _spawn_bg_task(self, coro, name: str):
            spawned.append(name)
            coro.close()
            return cleanup_task

        with (
            patch("bot.raid.bot.RaidAuthManager", autospec=True),
            patch("bot.raid.bot.RaidExecutor", autospec=True),
            patch.object(RaidBot, "_spawn_bg_task", new=_spawn_bg_task),
        ):
            bot = RaidBot(
                "client-id",
                "client-secret",
                "https://raid.example/twitch/raid/callback",
                SimpleNamespace(closed=False),
            )

        self.assertEqual(spawned, ["raid.bot.periodic_cleanup"])
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
