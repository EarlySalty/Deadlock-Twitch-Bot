import asyncio
import unittest
from unittest.mock import patch

from bot.base import TwitchBaseCog
from bot.runtime_bootstrap import TwitchRuntimeBootstrap


class _FakeLoop:
    def __init__(self) -> None:
        self.start_calls = 0
        self.running = False

    def is_running(self) -> bool:
        return self.running

    def start(self) -> None:
        self.start_calls += 1
        self.running = True

    def cancel(self) -> None:
        self.running = False


class _LifecycleHarness(TwitchBaseCog):
    def __init__(self) -> None:
        pass

    async def _startup_db_warmup(self) -> None:
        return None

    async def _init_twitch_chat_bot(self) -> None:
        return None

    async def _ensure_category_id(self) -> None:
        return None

    async def _load_invite_codes_from_db(self) -> None:
        return None

    async def _start_internal_api(self) -> None:
        return None

    async def _start_dashboard(self) -> None:
        return None

    async def _refresh_all_invites(self) -> None:
        return None

    async def _start_eventsub_listener(self) -> None:
        return None

    async def _sync_missing_user_ids(self) -> None:
        return None

    async def _scout_deadlock_channels(self) -> None:
        return None

    async def _register_views_after_ready(self) -> None:
        return None


class TwitchBaseBootstrapLifecycleTests(unittest.IsolatedAsyncioTestCase):
    async def test_cog_load_starts_runtime_once(self) -> None:
        harness = _LifecycleHarness()
        harness._runtime_started = False
        harness._runtime_bootstrap = TwitchRuntimeBootstrap(harness)
        harness.poll_streams = _FakeLoop()
        harness.api = object()
        harness._raid_bot = object()
        harness._twitch_bot_token = "oauth:test"
        harness._dashboard_embedded = True
        harness._managed_bg_tasks = set()
        harness.spawned_names: list[str] = []
        harness.sync_calls: list[tuple[bool, bool]] = []

        def _spawn_bg_task(coro, name: str):
            harness.spawned_names.append(name)
            coro.close()
            return None

        def _sync_poll_interval_from_storage(*, force: bool, startup: bool) -> None:
            harness.sync_calls.append((force, startup))

        harness._spawn_bg_task = _spawn_bg_task
        harness._sync_poll_interval_from_storage = _sync_poll_interval_from_storage

        with patch("bot.runtime_bootstrap.storage_pg.prepare_runtime_storage") as prepare_storage:
            await TwitchBaseCog.cog_load(harness)
            await TwitchBaseCog.cog_load(harness)

        self.assertEqual(harness.poll_streams.start_calls, 1)
        self.assertEqual(harness.sync_calls, [(True, True)])
        prepare_storage.assert_called_once()
        self.assertEqual(
            harness.spawned_names,
            [
                "twitch.db_warmup",
                "twitch.chat_bot",
                "twitch.ensure_category_id",
                "twitch.load_invites",
                "twitch.start_internal_api",
                "twitch.start_dashboard",
                "twitch.refresh_all_invites",
                "twitch.eventsub",
                "twitch.sync_user_ids",
                "twitch.scout_deadlock",
                "twitch.views_warmup",
            ],
        )

    async def test_cog_load_retries_runtime_start_after_partial_failure(self) -> None:
        harness = _LifecycleHarness()
        harness._runtime_started = False
        harness._runtime_bootstrap = TwitchRuntimeBootstrap(harness)
        harness.api = object()
        harness._raid_bot = None
        harness._twitch_bot_token = ""
        harness._dashboard_embedded = False
        harness._managed_bg_tasks = set()
        harness._spawn_bg_task = lambda coro, name: coro.close()
        harness._sync_poll_interval_from_storage = lambda **kwargs: None

        class _FailOnceLoop(_FakeLoop):
            def __init__(self) -> None:
                super().__init__()
                self.fail_first = True

            def start(self) -> None:
                self.start_calls += 1
                if self.fail_first:
                    self.fail_first = False
                    raise RuntimeError("loop boom")
                self.running = True

        harness.poll_streams = _FailOnceLoop()

        async def _cancel_managed_bg_tasks() -> None:
            return None

        harness._cancel_managed_bg_tasks = _cancel_managed_bg_tasks

        with patch("bot.runtime_bootstrap.storage_pg.prepare_runtime_storage"):
            with self.assertRaisesRegex(RuntimeError, "loop boom"):
                await TwitchBaseCog.cog_load(harness)

            self.assertFalse(harness._runtime_started)

            await TwitchBaseCog.cog_load(harness)

        self.assertTrue(harness._runtime_started)
        self.assertEqual(harness.poll_streams.start_calls, 2)

    async def test_spawn_bg_task_tracks_and_discards_completed_tasks(self) -> None:
        harness = _LifecycleHarness()
        harness._managed_bg_tasks = set()
        gate = asyncio.Event()

        async def _job() -> str:
            await gate.wait()
            return "done"

        task = TwitchBaseCog._spawn_bg_task(harness, _job(), "bootstrap.test")

        self.assertIsNotNone(task)
        self.assertEqual(len(harness._managed_bg_tasks), 1)

        gate.set()
        await asyncio.sleep(0)
        await asyncio.sleep(0)

        self.assertEqual(len(harness._managed_bg_tasks), 0)


if __name__ == "__main__":
    unittest.main()
