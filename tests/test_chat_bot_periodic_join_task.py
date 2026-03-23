import asyncio
import unittest
from unittest.mock import patch

from bot.base import TwitchBaseCog


class _FakeTask:
    def __init__(self, *, done: bool = False) -> None:
        self._done = done
        self.cancel_called = False
        self.done_callbacks = []

    def done(self) -> bool:
        return self._done

    def cancel(self) -> None:
        self.cancel_called = True
        self._done = True
        for callback in list(self.done_callbacks):
            callback(self)

    def add_done_callback(self, callback) -> None:
        self.done_callbacks.append(callback)
        if self._done:
            callback(self)

    def __await__(self):
        async def _wait():
            if self.cancel_called:
                raise asyncio.CancelledError
            return None

        return _wait().__await__()


class _PeriodicJoinHarness(TwitchBaseCog):
    def __init__(self) -> None:
        pass

    async def _periodic_channel_join(self):
        await asyncio.sleep(0)


class ChatBotPeriodicJoinTaskTests(unittest.IsolatedAsyncioTestCase):
    async def test_ensure_periodic_join_task_deduplicates_running_task(self) -> None:
        harness = _PeriodicJoinHarness()
        harness._periodic_channel_join_task = None

        created_names: list[str | None] = []
        fake_task = _FakeTask()

        def _fake_spawn_bg_task(coro, name: str):
            created_names.append(name)
            coro.close()
            return fake_task

        with patch.object(harness, "_spawn_bg_task", side_effect=_fake_spawn_bg_task):
            first = harness._ensure_periodic_channel_join_task()
            second = harness._ensure_periodic_channel_join_task()

        self.assertIs(first, fake_task)
        self.assertIs(second, fake_task)
        self.assertEqual(created_names, ["twitch.chat_bot.join_channels"])
        self.assertIs(harness._periodic_channel_join_task, fake_task)

    async def test_cancel_periodic_join_task_cancels_running_task_and_clears_handle(self) -> None:
        harness = _PeriodicJoinHarness()
        fake_task = _FakeTask()
        harness._periodic_channel_join_task = fake_task

        await harness._cancel_periodic_channel_join_task()

        self.assertTrue(fake_task.cancel_called)
        self.assertIsNone(harness._periodic_channel_join_task)
