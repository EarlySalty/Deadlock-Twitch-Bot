from __future__ import annotations

import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from bot.base import TwitchBaseCog
from bot.dashboard.mixin import TwitchDashboardMixin
from bot.monitoring.monitoring import TwitchMonitoringMixin
from bot.raid.mixin import TwitchRaidMixin


class _DummyDiscordUser:
    def __init__(self) -> None:
        self.send = AsyncMock(return_value=None)


class _DummyDashboardOffload(TwitchDashboardMixin):
    def __init__(self) -> None:
        self._discord_user = _DummyDiscordUser()
        discord_bot = SimpleNamespace(
            get_user=lambda _user_id: self._discord_user,
            fetch_user=AsyncMock(return_value=self._discord_user),
        )
        self._raid_bot = SimpleNamespace(auth_manager=SimpleNamespace(_discord_bot=discord_bot))

    @staticmethod
    def _normalize_login(login: str) -> str | None:
        normalized = str(login or "").strip().lower()
        return normalized or None

    async def _dashboard_list(self):
        return [{"twitch_login": "partner_one"}]


class _DummyMonitoringOffload(TwitchMonitoringMixin):
    pass


class _DummyRaidOffload(TwitchRaidMixin):
    pass


class AsyncDbOffloadingRegressionTests(unittest.IsolatedAsyncioTestCase):
    async def test_monitoring_poll_interval_resync_is_offloaded_to_thread(self) -> None:
        handler = _DummyMonitoringOffload()
        handler._poll_interval_resync_interval_seconds = 60.0
        handler._poll_interval_last_sync_monotonic = 0.0
        handler._poll_interval_seconds = 120
        handler._admin_polling_interval_seconds = 120
        handler.poll_streams = SimpleNamespace(change_interval=lambda **_kwargs: None)

        with patch.object(
            handler,
            "_read_persisted_poll_interval_seconds",
            return_value=45,
        ) as mocked_reader, patch.object(
            handler,
            "_apply_poll_interval_seconds",
            return_value=45,
        ) as mocked_apply, patch(
            "bot.monitoring.monitoring.asyncio.to_thread",
            new=AsyncMock(side_effect=lambda func, *args, **kwargs: func(*args, **kwargs)),
        ) as mocked_to_thread:
            result = await handler._sync_poll_interval_from_storage_async(force=True, startup=True)

        self.assertEqual(result, 45)
        mocked_reader.assert_called_once_with()
        mocked_apply.assert_called_once_with(45, reason="startup")
        mocked_to_thread.assert_awaited_once()
        self.assertIs(mocked_to_thread.await_args.args[0], mocked_reader)

    async def test_monitoring_tracked_streamer_load_is_offloaded_to_thread(self) -> None:
        handler = _DummyMonitoringOffload()
        expected = ([{"login": "alpha"}], {"alpha"})

        with patch.object(
            handler,
            "_load_tracked_streamers",
            return_value=expected,
        ) as mocked_loader, patch(
            "bot.monitoring.monitoring.asyncio.to_thread",
            new=AsyncMock(side_effect=lambda func, *args, **kwargs: func(*args, **kwargs)),
        ) as mocked_to_thread:
            result = await handler._load_tracked_streamers_async()

        self.assertEqual(result, expected)
        mocked_loader.assert_called_once_with()
        mocked_to_thread.assert_awaited_once()
        self.assertIs(mocked_to_thread.await_args.args[0], mocked_loader)

    async def test_dashboard_raid_requirements_identity_lookup_is_offloaded_to_thread(self) -> None:
        handler = _DummyDashboardOffload()

        with patch.object(
            handler,
            "_dashboard_load_streamer_identity_sync",
            return_value={"discord_user_id": "123"},
        ) as mocked_loader, patch(
            "bot.dashboard.mixin.RaidAuthGenerateView",
            return_value=object(),
        ), patch(
            "bot.dashboard.mixin.build_raid_requirements_embed",
            return_value=object(),
        ), patch(
            "bot.dashboard.mixin.asyncio.to_thread",
            new=AsyncMock(side_effect=lambda func, *args, **kwargs: func(*args, **kwargs)),
        ) as mocked_to_thread:
            result = await handler._dashboard_raid_requirements("Alpha")

        self.assertEqual(result, "Anforderungen per Discord an @alpha gesendet")
        mocked_loader.assert_called_once_with("alpha")
        mocked_to_thread.assert_awaited_once()
        self.assertIs(mocked_to_thread.await_args.args[0], mocked_loader)
        handler._discord_user.send.assert_awaited_once()

    async def test_dashboard_analytics_suggestions_db_lookup_is_offloaded_to_thread(self) -> None:
        handler = _DummyDashboardOffload()
        expected_extras = [{"twitch_login": "candidate_one"}]

        with patch.object(
            handler,
            "_dashboard_load_analytics_suggestions_sync",
            return_value=expected_extras,
        ) as mocked_loader, patch(
            "bot.dashboard.mixin.asyncio.to_thread",
            new=AsyncMock(side_effect=lambda func, *args, **kwargs: func(*args, **kwargs)),
        ) as mocked_to_thread:
            payload = await handler._dashboard_analytics_suggestions(days=30, limit=10)

        self.assertEqual(payload["extras"], expected_extras)
        mocked_loader.assert_called_once()
        self.assertEqual(mocked_loader.call_args.args[1], 10)
        self.assertEqual(mocked_loader.call_args.args[2], {"partner_one"})
        mocked_to_thread.assert_awaited_once()
        self.assertIs(mocked_to_thread.await_args.args[0], mocked_loader)

    async def test_dashboard_set_discord_flag_write_is_offloaded_to_thread(self) -> None:
        handler = _DummyDashboardOffload()

        with patch.object(
            handler,
            "_dashboard_set_discord_flag_sync",
            return_value=None,
        ) as mocked_loader, patch(
            "bot.dashboard.mixin.asyncio.to_thread",
            new=AsyncMock(side_effect=lambda func, *args, **kwargs: func(*args, **kwargs)),
        ) as mocked_to_thread:
            result = await handler._dashboard_set_discord_flag("Alpha", True)

        self.assertEqual(result, "alpha als Discord-Mitglied markiert")
        mocked_loader.assert_called_once_with("alpha", True)
        mocked_to_thread.assert_awaited_once()
        self.assertIs(mocked_to_thread.await_args.args[0], mocked_loader)

    async def test_dashboard_archive_write_is_offloaded_to_thread(self) -> None:
        handler = _DummyDashboardOffload()

        with patch.object(
            handler,
            "_dashboard_archive_sync",
            return_value="alpha archiviert",
        ) as mocked_loader, patch(
            "bot.dashboard.mixin.asyncio.to_thread",
            new=AsyncMock(side_effect=lambda func, *args, **kwargs: func(*args, **kwargs)),
        ) as mocked_to_thread:
            result = await handler._dashboard_archive("Alpha", "archive")

        self.assertEqual(result, "alpha archiviert")
        mocked_loader.assert_called_once_with("alpha", "archive")
        mocked_to_thread.assert_awaited_once()
        self.assertIs(mocked_to_thread.await_args.args[0], mocked_loader)

    async def test_raid_history_lookup_is_offloaded_to_thread(self) -> None:
        handler = _DummyRaidOffload()

        with patch.object(
            handler,
            "_dashboard_raid_history_sync",
            return_value=[{"from_broadcaster_login": "alpha"}],
        ) as mocked_loader, patch(
            "bot.raid.mixin.asyncio.to_thread",
            new=AsyncMock(side_effect=lambda func, *args, **kwargs: func(*args, **kwargs)),
        ) as mocked_to_thread:
            payload = await handler._dashboard_raid_history(limit=5, from_broadcaster="Alpha")

        self.assertEqual(payload, [{"from_broadcaster_login": "alpha"}])
        mocked_loader.assert_called_once_with(5, "Alpha")
        mocked_to_thread.assert_awaited_once()
        self.assertIs(mocked_to_thread.await_args.args[0], mocked_loader)

    async def test_internal_chatters_debug_live_state_lookup_is_offloaded_to_thread(self) -> None:
        harness = SimpleNamespace(
            _build_analytics_runtime_state=lambda _login: {},
            _resolve_bot_chatters_fallback=AsyncMock(return_value=(None, None, set(), {})),
            _internal_observability_snapshot=AsyncMock(return_value={}),
            _raid_bot=None,
            api=None,
        )

        with patch.object(
            harness,
            "_load_internal_chatters_live_state_sync",
            return_value=("123", 7, True),
            create=True,
        ) as mocked_loader, patch(
            "bot.base.asyncio.to_thread",
            new=AsyncMock(side_effect=lambda func, *args, **kwargs: func(*args, **kwargs)),
        ) as mocked_to_thread:
            payload = await TwitchBaseCog._internal_chatters_debug(harness, "Alpha")

        self.assertEqual(payload["currentUserId"], "123")
        self.assertEqual(payload["currentSessionId"], 7)
        self.assertTrue(payload["isLive"])
        mocked_loader.assert_called_once_with("alpha")
        mocked_to_thread.assert_awaited_once()
        self.assertIs(mocked_to_thread.await_args.args[0], mocked_loader)


if __name__ == "__main__":
    unittest.main()
