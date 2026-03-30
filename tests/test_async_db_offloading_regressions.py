from __future__ import annotations

import json
import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from bot.analytics.api_admin import _AnalyticsAdminMixin
from bot.analytics.api_chat_deep import _AnalyticsChatDeepMixin
from bot.analytics.api_insights import _AnalyticsInsightsMixin
from bot.analytics.api_performance import _AnalyticsPerformanceMixin
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
        self._discord_bot = SimpleNamespace(
            get_user=lambda _user_id: self._discord_user,
            fetch_user=AsyncMock(return_value=self._discord_user),
        )
        self._raid_bot = SimpleNamespace(auth_manager=SimpleNamespace(_discord_bot=self._discord_bot))

    @staticmethod
    def _normalize_login(login: str) -> str | None:
        normalized = str(login or "").strip().lower()
        return normalized or None

    async def _dashboard_list(self):
        return [{"twitch_login": "partner_one"}]

    def _dashboard_bot_service(self):
        return SimpleNamespace(
            auth_manager=lambda: getattr(self._raid_bot, "auth_manager", None),
            discord_bot=lambda: self._discord_bot,
            twitch_api=lambda: None,
        )


class _DummyMonitoringOffload(TwitchMonitoringMixin):
    pass


class _DummyRaidOffload(TwitchRaidMixin):
    pass


class _DummyAdminRequest(dict):
    def __init__(self, **kwargs) -> None:
        super().__init__()
        for key, value in kwargs.items():
            setattr(self, key, value)

    async def json(self):
        return dict(getattr(self, "_body", {}) or {})


class _DummyAdminOffload(_AnalyticsAdminMixin):
    def _require_v2_admin_api(self, _request):
        return None

    def _csrf_verify_token(self, _request, token: str) -> bool:
        return token == "csrf-ok"

    def _get_discord_admin_session(self, _request):
        return {"discord_user_id": "42"}


class _DummyAnalyticsRequest(SimpleNamespace):
    def __init__(self, query: dict[str, str]) -> None:
        super().__init__(
            query=query,
            headers={},
            path="/twitch/api/v2/test",
            host="dashboard.example",
            remote="203.0.113.10",
            transport=None,
            rel_url=SimpleNamespace(path_qs="/twitch/api/v2/test"),
        )


class _DummyPerformanceOffload(_AnalyticsPerformanceMixin):
    def _require_v2_auth(self, _request):
        return None

    def _require_extended_plan(self, _request):
        return None


class _DummyInsightsOffload(_AnalyticsInsightsMixin):
    def _require_v2_auth(self, _request):
        return None

    def _require_extended_plan(self, _request):
        return None


class _DummyChatDeepOffload(_AnalyticsChatDeepMixin):
    def _require_v2_auth(self, _request):
        return None

    def _require_extended_plan(self, _request):
        return None


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
            "bot.dashboard.streamer_admin_mixin.asyncio.to_thread",
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
            "bot.dashboard.streamer_admin_mixin.asyncio.to_thread",
            new=AsyncMock(side_effect=lambda func, *args, **kwargs: func(*args, **kwargs)),
        ) as mocked_to_thread:
            result = await handler._dashboard_archive("Alpha", "archive")

        self.assertEqual(result, "alpha archiviert")
        mocked_loader.assert_called_once_with("alpha", "archive")
        mocked_to_thread.assert_awaited_once()
        self.assertIs(mocked_to_thread.await_args.args[0], mocked_loader)

    async def test_dashboard_save_discord_profile_db_steps_are_offloaded_to_thread(self) -> None:
        handler = _DummyDashboardOffload()

        async def _to_thread(func, /, *args, **kwargs):
            return func(*args, **kwargs)

        with patch.object(
            handler,
            "_dashboard_load_twitch_user_id_from_raid_auth_sync",
            return_value="1001",
        ) as mocked_lookup, patch.object(
            handler,
            "_dashboard_save_discord_profile_sync",
            return_value=None,
        ) as mocked_save, patch(
            "bot.dashboard.streamer_admin_mixin.asyncio.to_thread",
            new=AsyncMock(side_effect=_to_thread),
        ) as mocked_to_thread:
            result = await handler._dashboard_save_discord_profile(
                "Alpha",
                discord_user_id="123",
                discord_display_name="Viewer",
                mark_member=True,
            )

        self.assertEqual(result, "Discord-Daten für alpha aktualisiert")
        self.assertEqual(mocked_to_thread.await_count, 2)
        self.assertIs(mocked_to_thread.await_args_list[0].args[0], mocked_lookup)
        self.assertIs(mocked_to_thread.await_args_list[1].args[0], mocked_save)

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

    async def test_admin_billing_subscriptions_lookup_is_offloaded_to_thread(self) -> None:
        handler = _DummyAdminOffload()

        with patch(
            "bot.analytics.api_admin.load_admin_billing_subscriptions",
            return_value={"items": [], "count": 0},
        ) as mocked_loader, patch(
            "bot.analytics.api_admin.asyncio.to_thread",
            new=AsyncMock(side_effect=lambda func, *args, **kwargs: func(*args, **kwargs)),
        ) as mocked_to_thread:
            response = await handler._api_admin_billing_subscriptions(SimpleNamespace())

        self.assertEqual(response.status, 200)
        mocked_loader.assert_called_once_with()
        mocked_to_thread.assert_awaited_once()
        self.assertIs(mocked_to_thread.await_args.args[0], mocked_loader)

    async def test_admin_affiliate_detail_lookup_is_offloaded_to_thread(self) -> None:
        handler = _DummyAdminOffload()
        request = _DummyAdminRequest(match_info={"login": "Alpha"})

        with patch(
            "bot.analytics.api_admin.load_admin_affiliate_detail",
            return_value={"affiliate": {"login": "alpha"}, "claims": [], "stats": {}},
        ) as mocked_loader, patch(
            "bot.analytics.api_admin.asyncio.to_thread",
            new=AsyncMock(side_effect=lambda func, *args, **kwargs: func(*args, **kwargs)),
        ) as mocked_to_thread:
            response = await handler._api_admin_affiliate_detail(request)

        self.assertEqual(response.status, 200)
        self.assertEqual(mocked_loader.call_args.args[0], "alpha")
        mocked_to_thread.assert_awaited_once()
        self.assertIs(mocked_to_thread.await_args.args[0], mocked_loader)

    async def test_admin_affiliate_toggle_is_offloaded_to_thread(self) -> None:
        handler = _DummyAdminOffload()
        request = _DummyAdminRequest(
            match_info={"login": "Alpha"},
            headers={},
            _body={"csrf_token": "csrf-ok"},
        )

        with patch(
            "bot.analytics.api_admin.toggle_admin_affiliate",
            return_value={"login": "alpha", "active": False},
        ) as mocked_loader, patch(
            "bot.analytics.api_admin.asyncio.to_thread",
            new=AsyncMock(side_effect=lambda func, *args, **kwargs: func(*args, **kwargs)),
        ) as mocked_to_thread:
            response = await handler._api_admin_affiliate_toggle(request)

        self.assertEqual(response.status, 200)
        mocked_loader.assert_called_once_with("alpha")
        mocked_to_thread.assert_awaited_once()
        self.assertIs(mocked_to_thread.await_args.args[0], mocked_loader)

    async def test_performance_analytics_db_endpoints_are_offloaded_to_thread(self) -> None:
        handler = _DummyPerformanceOffload()
        cases = [
            ("_api_v2_title_performance", "_load_title_performance_payload_sync", {"streamer": "Alpha", "days": "30", "limit": "20"}, {"titles": []}),
            ("_api_v2_rankings", "_load_rankings_payload_sync", {"metric": "viewers", "days": "30", "limit": "20"}, []),
            ("_api_v2_category_comparison", "_load_category_comparison_payload_sync", {"streamer": "Alpha", "days": "30"}, {"yourStats": {}}),
            ("_api_v2_viewer_timeline", "_load_viewer_timeline_payload_sync", {"streamer": "Alpha", "days": "7"}, []),
            ("_api_v2_category_leaderboard", "_load_category_leaderboard_payload_sync", {"streamer": "Alpha", "days": "30", "limit": "25"}, {"leaderboard": []}),
            ("_api_v2_category_timings", "_load_category_timings_payload_sync", {"days": "30", "source": "category"}, {"hourly": [], "weekly": []}),
            ("_api_v2_category_activity_series", "_load_category_activity_series_payload_sync", {"days": "30"}, {"hourly": [], "weekly": []}),
            ("_api_v2_retention_curve", "_load_retention_curve_payload_sync", {"streamer": "Alpha", "days": "30"}, {"retention_curve": [], "drop_events": [], "sessions_used": 0}),
        ]

        async def _to_thread(func, /, *args, **kwargs):
            return func(*args, **kwargs)

        for endpoint_name, loader_name, query, payload in cases:
            request = _DummyAnalyticsRequest(query)
            with self.subTest(endpoint=endpoint_name), patch.object(
                handler,
                loader_name,
                return_value=payload,
            ) as mocked_loader, patch(
                "bot.analytics.api_performance.asyncio.to_thread",
                new=AsyncMock(side_effect=_to_thread),
            ) as mocked_to_thread:
                response = await getattr(handler, endpoint_name)(request)

            self.assertEqual(response.status, 200)
            mocked_loader.assert_called_once()
            mocked_to_thread.assert_awaited_once()
            self.assertIs(mocked_to_thread.await_args.args[0], mocked_loader)

    async def test_insights_monetization_db_endpoint_is_offloaded_to_thread(self) -> None:
        handler = _DummyInsightsOffload()
        request = _DummyAnalyticsRequest({"streamer": "alpha", "days": "30"})
        payload = {"ads": {}, "hype_train": {}, "bits": {}, "subs": {}, "window_days": 30}

        with patch(
            "bot.analytics.api_insights.load_monetization_payload",
            return_value=payload,
        ) as mocked_loader, patch(
            "bot.analytics.api_insights.asyncio.to_thread",
            new=AsyncMock(side_effect=lambda func, *args, **kwargs: func(*args, **kwargs)),
        ) as mocked_to_thread:
            response = await handler._api_v2_monetization(request)

        self.assertEqual(response.status, 200)
        mocked_loader.assert_called_once_with(streamer="alpha", days=30)
        mocked_to_thread.assert_awaited_once()
        self.assertIs(mocked_to_thread.await_args.args[0], mocked_loader)

    async def test_chat_social_graph_db_endpoint_is_offloaded_to_thread(self) -> None:
        handler = _DummyChatDeepOffload()
        request = _DummyAnalyticsRequest({"streamer": "alpha", "days": "30"})
        payload = {"totalMentions": 0, "uniqueMentioners": 0, "uniqueMentioned": 0, "hubs": [], "topPairs": [], "mentionDistribution": {}, "rawChatStatus": {}}

        with patch(
            "bot.analytics.api_chat_deep.load_chat_social_graph_payload",
            return_value=payload,
        ) as mocked_loader, patch(
            "bot.analytics.api_chat_deep.asyncio.to_thread",
            new=AsyncMock(side_effect=lambda func, *args, **kwargs: func(*args, **kwargs)),
        ) as mocked_to_thread:
            response = await handler._api_v2_chat_social_graph(request)

        self.assertEqual(response.status, 200)
        mocked_loader.assert_called_once_with(streamer="alpha", days=30)
        mocked_to_thread.assert_awaited_once()
        self.assertIs(mocked_to_thread.await_args.args[0], mocked_loader)

    async def test_performance_endpoints_reject_invalid_integer_query_params(self) -> None:
        handler = _DummyPerformanceOffload()
        cases = [
            ("_api_v2_tag_analysis_extended", {"streamer": "Alpha", "days": "abc"}),
            ("_api_v2_tag_analysis_extended", {"streamer": "Alpha", "limit": "abc"}),
            ("_api_v2_title_performance", {"streamer": "Alpha", "days": "abc"}),
            ("_api_v2_title_performance", {"streamer": "Alpha", "limit": "abc"}),
            ("_api_v2_rankings", {"metric": "viewers", "days": "abc"}),
            ("_api_v2_rankings", {"metric": "viewers", "limit": "abc"}),
            ("_api_v2_category_comparison", {"streamer": "Alpha", "days": "abc"}),
            ("_api_v2_category_timings", {"days": "abc"}),
            ("_api_v2_category_activity_series", {"days": "abc"}),
            ("_api_v2_retention_curve", {"streamer": "Alpha", "days": "abc"}),
        ]

        for endpoint_name, query in cases:
            request = _DummyAnalyticsRequest(query)
            with self.subTest(endpoint=endpoint_name, query=query), patch(
                "bot.analytics.api_performance.asyncio.to_thread",
                new=AsyncMock(),
            ) as mocked_to_thread:
                response = await getattr(handler, endpoint_name)(request)

            self.assertEqual(response.status, 400)
            payload = json.loads(response.body.decode("utf-8"))
            self.assertIn("must be an integer", payload["error"])
            mocked_to_thread.assert_not_awaited()

    async def test_insights_endpoints_reject_invalid_integer_query_params(self) -> None:
        handler = _DummyInsightsOffload()
        cases = [
            ("_api_v2_chat_analytics", {"streamer": "alpha", "days": "abc"}),
            ("_api_v2_coaching", {"streamer": "alpha", "days": "abc"}),
            ("_api_v2_monetization", {"streamer": "alpha", "days": "abc"}),
        ]

        for endpoint_name, query in cases:
            request = _DummyAnalyticsRequest(query)
            with self.subTest(endpoint=endpoint_name, query=query), patch(
                "bot.analytics.api_insights.asyncio.to_thread",
                new=AsyncMock(),
            ) as mocked_to_thread:
                response = await getattr(handler, endpoint_name)(request)

            self.assertEqual(response.status, 400)
            payload = json.loads(response.body.decode("utf-8"))
            self.assertEqual(payload["error"], "days must be an integer")
            mocked_to_thread.assert_not_awaited()


if __name__ == "__main__":
    unittest.main()
