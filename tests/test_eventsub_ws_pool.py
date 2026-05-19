import asyncio
import unittest

from bot.monitoring.eventsub_ws_pool import EventSubWSListenerPool


class _FakePoolListener:
    def __init__(self, *, api, logger, token_resolver, state_store=None) -> None:
        del api, logger, token_resolver, state_store
        self._planned: list[tuple[str, str, dict[str, str]]] = []
        self._registered: list[tuple[str, str, dict[str, str]]] = []
        self._callbacks: dict[str, object] = {}
        self._ready = False
        self._failed = False
        self._stop = False
        self._initial_registration_complete = False

    def set_callback(self, sub_type: str, callback) -> None:
        self._callbacks[sub_type] = callback

    def add_subscription(self, sub_type: str, broadcaster_id: str, condition: dict | None = None):
        self._planned.append((sub_type, str(broadcaster_id), dict(condition or {})))

    async def add_subscription_dynamic(
        self,
        sub_type: str,
        broadcaster_id: str,
        condition: dict | None = None,
        oauth_token: str | None = None,
    ) -> bool:
        del oauth_token
        entry = (sub_type, str(broadcaster_id), dict(condition or {}))
        self._planned.append(entry)
        self._registered.append(entry)
        return True

    async def run(self) -> None:
        self._ready = True
        self._registered = list(self._planned)
        self._initial_registration_complete = True
        while not self._stop:
            await asyncio.sleep(0.01)

    def stop(self) -> None:
        self._stop = True
        self._ready = False

    @property
    def has_capacity(self) -> bool:
        return len(self._planned) < 10

    @property
    def is_ready(self) -> bool:
        return self._ready

    @property
    def is_failed(self) -> bool:
        return self._failed

    @property
    def subscription_count(self) -> int:
        return len(self._planned)

    @property
    def registered_subscription_count(self) -> int:
        return len(self._registered)

    @property
    def initial_registration_complete(self) -> bool:
        return self._initial_registration_complete

    async def wait_until_ready(self, timeout: float = 8.0, poll_interval: float = 0.1) -> bool:
        del timeout, poll_interval
        await asyncio.sleep(0)
        return self._ready and not self._failed

    def has_registered_subscription(
        self,
        sub_type: str,
        broadcaster_id: str,
        condition: dict | None = None,
    ) -> bool:
        wanted = (sub_type, str(broadcaster_id), dict(condition or {}))
        return wanted in self._registered

    def is_subscription_ready(
        self,
        sub_type: str,
        broadcaster_id: str,
        condition: dict | None = None,
    ) -> bool:
        return self._ready and self.has_registered_subscription(
            sub_type,
            broadcaster_id,
            condition,
        )

    def get_tracked_subscriptions(self) -> list[dict[str, object]]:
        return [
            {
                "type": sub_type,
                "broadcaster_id": broadcaster_id,
                "condition": condition,
            }
            for sub_type, broadcaster_id, condition in self._registered
        ]


class _RetryingPoolListener(_FakePoolListener):
    def __init__(self, *, api, logger, token_resolver, state_store=None) -> None:
        super().__init__(
            api=api,
            logger=logger,
            token_resolver=token_resolver,
            state_store=state_store,
        )
        self.dynamic_results: list[bool] = []

    async def add_subscription_dynamic(
        self,
        sub_type: str,
        broadcaster_id: str,
        condition: dict | None = None,
        oauth_token: str | None = None,
    ) -> bool:
        del oauth_token
        entry = (sub_type, str(broadcaster_id), dict(condition or {}))
        self._planned.append(entry)
        result = self.dynamic_results.pop(0) if self.dynamic_results else True
        if result:
            self._registered.append(entry)
        return result


class _CrashingPoolListener(_FakePoolListener):
    async def run(self) -> None:
        self._initial_registration_complete = False
        raise RuntimeError("callback failed")


class EventSubWSListenerPoolTests(unittest.IsolatedAsyncioTestCase):
    async def test_subscription_readiness_is_bound_to_transport_holding_subscription(self) -> None:
        pool = EventSubWSListenerPool(
            api=object(),
            listener_factory=_FakePoolListener,
        )

        first = pool._create_listener()
        second = pool._create_listener()

        first._registered.append(
            ("channel.raid", "123", {"to_broadcaster_user_id": "123"})
        )
        first._ready = False
        second._ready = True

        self.assertTrue(pool.is_ready)
        self.assertTrue(
            pool.has_registered_subscription(
                "channel.raid",
                "123",
                {"to_broadcaster_user_id": "123"},
            )
        )
        self.assertFalse(
            pool.is_subscription_ready(
                "channel.raid",
                "123",
                {"to_broadcaster_user_id": "123"},
            )
        )

    async def test_pool_spreads_startup_subscriptions_across_multiple_transports(self) -> None:
        pool = EventSubWSListenerPool(
            api=object(),
            listener_factory=_FakePoolListener,
        )

        for broadcaster_id in ("1", "2", "3", "4"):
            self.assertTrue(
                pool.add_subscription(
                    "stream.online",
                    broadcaster_id,
                    {"broadcaster_user_id": broadcaster_id},
                )
            )
            self.assertTrue(
                pool.add_subscription(
                    "stream.offline",
                    broadcaster_id,
                    {"broadcaster_user_id": broadcaster_id},
                )
            )
            self.assertTrue(
                pool.add_subscription(
                    "channel.update",
                    broadcaster_id,
                    {"broadcaster_user_id": broadcaster_id},
                )
            )

        self.assertEqual(pool.listener_count, 2)
        self.assertEqual(pool.get_tracked_subscriptions(), [])
        self.assertEqual(
            pool.get_capacity_rows(),
            [
                {"idx": 1, "ready": 0, "failed": 0, "subscriptions": 0, "free_slots": 10},
                {"idx": 2, "ready": 0, "failed": 0, "subscriptions": 0, "free_slots": 10},
            ],
        )

        run_task = asyncio.create_task(pool.run())
        try:
            self.assertTrue(await pool.wait_until_initial_registration(timeout=1.0, poll_interval=0.01))
            tracked = pool.get_tracked_subscriptions()
            self.assertEqual(len(tracked), 12)
            self.assertEqual({int(row["listener_idx"]) for row in tracked}, {1, 2})
            self.assertEqual(
                pool.get_capacity_rows(),
                [
                    {"idx": 1, "ready": 1, "failed": 0, "subscriptions": 10, "free_slots": 0},
                    {"idx": 2, "ready": 1, "failed": 0, "subscriptions": 2, "free_slots": 8},
                ],
            )
        finally:
            pool.stop()
            await asyncio.wait_for(run_task, timeout=1.0)

    async def test_completed_listener_is_removed_before_dynamic_capacity_check(self) -> None:
        created: list[_FakePoolListener] = []

        class _SwitchingPoolListener(_FakePoolListener):
            def __init__(self, *, api, logger, token_resolver, state_store=None) -> None:
                super().__init__(
                    api=api,
                    logger=logger,
                    token_resolver=token_resolver,
                    state_store=state_store,
                )
                self._crash_on_run = not created
                created.append(self)

            async def run(self) -> None:
                self._initial_registration_complete = True
                if self._crash_on_run:
                    raise RuntimeError("transport crashed")
                await super().run()

        pool = EventSubWSListenerPool(
            api=object(),
            listener_factory=_SwitchingPoolListener,
            max_transports=1,
        )
        dead_listener = pool._create_listener()
        pool._run_started = True
        dead_task = pool._start_listener_task(dead_listener)
        await asyncio.sleep(0)
        await asyncio.sleep(0)
        self.assertTrue(dead_task.done())

        try:
            success = await pool.add_subscription_dynamic(
                "channel.raid",
                "123",
                {"to_broadcaster_user_id": "123"},
            )

            self.assertTrue(success)
            self.assertEqual(pool.listener_count, 1)
            self.assertNotIn(dead_listener, pool._listeners)
            self.assertNotIn(dead_listener, pool._listener_tasks)
            self.assertEqual(len(created), 2)
            self.assertEqual(
                created[1]._registered,
                [("channel.raid", "123", {"to_broadcaster_user_id": "123"})],
            )
        finally:
            pool.stop()
            await asyncio.gather(*pool._listener_tasks.values(), return_exceptions=True)

    async def test_dynamic_subscription_retries_other_ready_transport_after_failure(self) -> None:
        pool = EventSubWSListenerPool(
            api=object(),
            listener_factory=_RetryingPoolListener,
            max_transports=2,
        )

        first = pool._create_listener()
        second = pool._create_listener()
        first._ready = True
        second._ready = True
        first.dynamic_results = [False]
        second.dynamic_results = [True]

        success = await pool.add_subscription_dynamic(
            "channel.raid",
            "123",
            {"to_broadcaster_user_id": "123"},
        )

        self.assertTrue(success)
        self.assertEqual(
            first._planned,
            [("channel.raid", "123", {"to_broadcaster_user_id": "123"})],
        )
        self.assertEqual(first._registered, [])
        self.assertEqual(
            second._registered,
            [("channel.raid", "123", {"to_broadcaster_user_id": "123"})],
        )

    async def test_pool_stops_remaining_transports_after_transport_failure(self) -> None:
        created = 0

        def _listener_factory(*, api, logger, token_resolver, state_store=None):
            nonlocal created
            created += 1
            if created == 1:
                return _CrashingPoolListener(
                    api=api,
                    logger=logger,
                    token_resolver=token_resolver,
                    state_store=state_store,
                )
            return _FakePoolListener(
                api=api,
                logger=logger,
                token_resolver=token_resolver,
                state_store=state_store,
            )

        pool = EventSubWSListenerPool(
            api=object(),
            listener_factory=_listener_factory,
            max_transports=2,
        )

        pool._create_listener()
        second = pool._create_listener()

        run_task = asyncio.create_task(pool.run())
        try:
            await asyncio.wait_for(run_task, timeout=1.0)
        finally:
            pool.stop()
            await asyncio.gather(*pool._listener_tasks.values(), return_exceptions=True)

        self.assertTrue(pool._stop)
        self.assertTrue(second._stop)


if __name__ == "__main__":
    unittest.main()
