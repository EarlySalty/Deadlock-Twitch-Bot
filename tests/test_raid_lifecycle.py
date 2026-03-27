from __future__ import annotations

import asyncio
import unittest

from bot.raid.lifecycle import RaidBotLifecycle


class RaidBotLifecycleTests(unittest.IsolatedAsyncioTestCase):
    async def test_start_creates_and_tracks_cleanup_task(self) -> None:
        started = 0
        release = asyncio.Event()

        async def _cleanup() -> None:
            nonlocal started
            started += 1
            await release.wait()

        lifecycle = RaidBotLifecycle(_cleanup)

        task = lifecycle.start()
        await asyncio.sleep(0)

        self.assertIsNotNone(task)
        self.assertTrue(lifecycle.started)
        self.assertEqual(started, 1)
        self.assertEqual(len(lifecycle.managed_tasks), 1)

        release.set()
        await lifecycle.stop()

    async def test_start_is_idempotent_while_running(self) -> None:
        started = 0
        release = asyncio.Event()

        async def _cleanup() -> None:
            nonlocal started
            started += 1
            await release.wait()

        lifecycle = RaidBotLifecycle(_cleanup)

        first = lifecycle.start()
        second = lifecycle.start()
        await asyncio.sleep(0)

        self.assertIs(first, second)
        self.assertEqual(started, 1)
        self.assertEqual(len(lifecycle.managed_tasks), 1)

        release.set()
        await lifecycle.stop()

    async def test_stop_cancels_managed_tasks_and_clears_registry(self) -> None:
        cancelled = asyncio.Event()

        async def _cleanup() -> None:
            try:
                await asyncio.Future()
            except asyncio.CancelledError:
                cancelled.set()
                raise

        lifecycle = RaidBotLifecycle(_cleanup)
        task = lifecycle.start()
        self.assertIsNotNone(task)
        await asyncio.sleep(0)

        await lifecycle.stop()

        self.assertTrue(cancelled.is_set())
        self.assertTrue(task.cancelled())
        self.assertFalse(lifecycle.started)
        self.assertEqual(len(lifecycle.managed_tasks), 0)

    def test_start_without_running_loop_returns_none_and_closes_coro(self) -> None:
        closed = False

        class _ClosableAwaitable:
            def __await__(self):
                if False:
                    yield None
                return None

            def close(self) -> None:
                nonlocal closed
                closed = True

        lifecycle = RaidBotLifecycle(_ClosableAwaitable)

        task = lifecycle.start()

        self.assertIsNone(task)
        self.assertFalse(lifecycle.started)
        self.assertTrue(closed)
        self.assertEqual(len(lifecycle.managed_tasks), 0)


if __name__ == "__main__":
    unittest.main()
