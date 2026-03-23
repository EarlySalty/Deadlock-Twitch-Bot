import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch
import logging

from bot.analytics.mixin import TwitchAnalyticsMixin


class _AuthManager:
    def __init__(self, scopes_by_user: dict[str, list[str]] | None = None) -> None:
        self._scopes_by_user = scopes_by_user or {}

    def get_scopes(self, twitch_user_id: str) -> list[str]:
        return list(self._scopes_by_user.get(str(twitch_user_id), []))


class _BotTokenManager:
    def __init__(
        self,
        *,
        token: str = "bot-token",
        bot_id: str = "9999",
        scopes: set[str] | None = None,
    ) -> None:
        self._token = token
        self._bot_id = bot_id
        self.scopes = set(scopes or set())

    async def get_valid_token(self, force_refresh: bool = False) -> tuple[str, str]:
        return self._token, self._bot_id


class _ChatBot:
    def __init__(
        self,
        *,
        bot_id: str = "9999",
        monitored: set[str] | None = None,
        initial_channels: list[str] | None = None,
        monitored_only: set[str] | None = None,
        channel_ids: dict[str, str] | None = None,
        subscription_types: dict[str, set[str]] | None = None,
    ) -> None:
        self.bot_id = bot_id
        self._bot_id_stored = bot_id
        self._monitored_streamers = set(monitored or set())
        self._initial_channels = list(initial_channels or [])
        self._monitored_only_channels = set(monitored_only or set())
        self._channel_ids = dict(channel_ids or {})
        self._channel_subscription_types = dict(subscription_types or {})

    @property
    def bot_id_safe(self) -> str:
        return self._bot_id_stored

    def is_channel_subscription_ready(self, channel_login: str, sub_type: str | None = None) -> bool:
        required_types = {"channel.chat.message", "channel.chat.notification"}
        tracked = self._channel_subscription_types.get(str(channel_login), set())
        if sub_type:
            return str(sub_type) in tracked
        return required_types.issubset(tracked)


class _AnalyticsHarness(TwitchAnalyticsMixin):
    def __init__(
        self,
        *,
        streamer_scopes: dict[str, list[str]] | None = None,
        bot_scopes: set[str] | None = None,
        monitored: set[str] | None = None,
        initial_channels: list[str] | None = None,
        monitored_only: set[str] | None = None,
        channel_ids: dict[str, str] | None = None,
        subscription_types: dict[str, set[str]] | None = None,
    ) -> None:
        self.api = SimpleNamespace(get_chatters=AsyncMock())
        self._raid_bot = SimpleNamespace(auth_manager=_AuthManager(streamer_scopes))
        self._bot_token_manager = _BotTokenManager(scopes=bot_scopes)
        self._twitch_chat_bot = _ChatBot(
            monitored=monitored or {"partner_one"},
            initial_channels=initial_channels,
            monitored_only=monitored_only,
            channel_ids=channel_ids,
            subscription_types=subscription_types,
        )
        self._chatters_scope_warned: set[tuple[str, int]] = set()


class ChattersBotFallbackTests(unittest.IsolatedAsyncioTestCase):
    async def test_subscriptions_success_logs_at_debug(self) -> None:
        harness = _AnalyticsHarness()

        with (
            patch("bot.analytics.mixin.storage.insert_observability_event"),
            patch("bot.analytics.mixin.log.log") as log_mock,
        ):
            harness._log_analytics_decision(
                flow_id="subscriptions-1",
                flow="subscriptions",
                login="partner_one",
                decision="success",
                reason="subscriptions_collected",
                request_attempted=True,
                request_result="success",
                http_status=200,
                scope_state={"streamer": "present"},
                runtime_state=harness._build_analytics_runtime_state("partner_one"),
                snapshot_total=5,
                snapshot_points=5,
            )

        self.assertEqual(log_mock.call_args.args[0], logging.DEBUG)

    async def test_ads_success_logs_at_debug(self) -> None:
        harness = _AnalyticsHarness()

        with (
            patch("bot.analytics.mixin.storage.insert_observability_event"),
            patch("bot.analytics.mixin.log.log") as log_mock,
        ):
            harness._log_analytics_decision(
                flow_id="ads-1",
                flow="ads",
                login="partner_one",
                decision="success",
                reason="ads_collected",
                request_attempted=True,
                request_result="success",
                http_status=200,
                scope_state={"streamer": "present"},
                runtime_state=harness._build_analytics_runtime_state("partner_one"),
                next_ad_at="2026-03-21T02:39:49+00:00",
            )

        self.assertEqual(log_mock.call_args.args[0], logging.DEBUG)

    async def test_chat_bot_unavailable_failure_logs_at_debug(self) -> None:
        harness = _AnalyticsHarness()

        with (
            patch("bot.analytics.mixin.storage.insert_observability_event"),
            patch("bot.analytics.mixin.log.log") as log_mock,
        ):
            harness._log_analytics_decision(
                flow_id="chatters-1",
                flow="chatters",
                login="partner_one",
                session_id=77,
                decision="failed",
                reason="chat_bot_unavailable",
                request_attempted="none",
                request_result="not_attempted",
                http_status=None,
                scope_state={"bot": "unknown", "streamer": "absent"},
                runtime_state=harness._build_analytics_runtime_state("partner_one"),
            )

        self.assertEqual(log_mock.call_args.args[0], logging.DEBUG)

    async def test_channel_not_tracked_failure_logs_at_debug(self) -> None:
        harness = _AnalyticsHarness()

        with (
            patch("bot.analytics.mixin.storage.insert_observability_event"),
            patch("bot.analytics.mixin.log.log") as log_mock,
        ):
            harness._log_analytics_decision(
                flow_id="chatters-1b",
                flow="chatters",
                login="partner_one",
                session_id=79,
                decision="failed",
                reason="channel_not_tracked_in_chat_runtime",
                request_attempted="none",
                request_result="not_attempted",
                http_status=None,
                scope_state={"bot": "unknown", "streamer": "absent"},
                runtime_state=harness._build_analytics_runtime_state("partner_one"),
            )

        self.assertEqual(log_mock.call_args.args[0], logging.DEBUG)

    async def test_helix_not_moderator_failure_logs_at_debug(self) -> None:
        harness = _AnalyticsHarness()

        with (
            patch("bot.analytics.mixin.storage.insert_observability_event"),
            patch("bot.analytics.mixin.log.log") as log_mock,
        ):
            harness._log_analytics_decision(
                flow_id="chatters-1c",
                flow="chatters",
                login="partner_one",
                session_id=80,
                decision="failed",
                reason="helix_403_not_moderator",
                request_attempted="bot",
                request_result="failed",
                http_status=403,
                scope_state={"bot": "present", "streamer": "absent"},
                runtime_state=harness._build_analytics_runtime_state("partner_one"),
            )

        self.assertEqual(log_mock.call_args.args[0], logging.DEBUG)

    async def test_bot_path_success_logs_at_debug(self) -> None:
        harness = _AnalyticsHarness()

        with (
            patch("bot.analytics.mixin.storage.insert_observability_event"),
            patch("bot.analytics.mixin.log.log") as log_mock,
        ):
            harness._log_analytics_decision(
                flow_id="chatters-1d",
                flow="chatters",
                login="partner_one",
                session_id=81,
                decision="success",
                reason="bot_path_success",
                request_attempted="bot",
                request_result="success",
                http_status=200,
                scope_state={"bot": "present", "streamer": "present"},
                runtime_state=harness._build_analytics_runtime_state("partner_one"),
                chatter_count=6,
            )

        self.assertEqual(log_mock.call_args.args[0], logging.DEBUG)

    async def test_other_chatters_failures_remain_info(self) -> None:
        harness = _AnalyticsHarness()

        with (
            patch("bot.analytics.mixin.storage.insert_observability_event"),
            patch("bot.analytics.mixin.log.log") as log_mock,
        ):
            harness._log_analytics_decision(
                flow_id="chatters-2",
                flow="chatters",
                login="partner_one",
                session_id=78,
                decision="failed",
                reason="bot_scope_missing",
                request_attempted="bot",
                request_result="failed",
                http_status=403,
                scope_state={"bot": "missing", "streamer": "absent"},
                runtime_state=harness._build_analytics_runtime_state("partner_one"),
            )

        self.assertEqual(log_mock.call_args.args[0], logging.INFO)

    async def test_poll_chatters_prefers_bot_scope_when_streamer_scope_is_also_available(self) -> None:
        harness = _AnalyticsHarness(
            streamer_scopes={"1001": ["moderator:read:chatters"]},
            bot_scopes={"moderator:read:chatters"},
        )
        harness.api.get_chatters.return_value = [{"user_login": "lurker_a", "user_id": "42"}]

        result = await harness._poll_chatters_single(
            "1001",
            "partner_one",
            77,
            "2026-03-15T10:00:00+00:00",
            token="streamer-token",
        )

        self.assertEqual(result, (77, "partner_one", [{"user_login": "lurker_a", "user_id": "42"}]))
        harness.api.get_chatters.assert_awaited_once_with(
            broadcaster_id="1001",
            moderator_id="9999",
            user_token="bot-token",
        )

    async def test_poll_chatters_falls_back_to_bot_scope_when_streamer_scope_missing(self) -> None:
        harness = _AnalyticsHarness(
            streamer_scopes={"1001": ["chat:read"]},
            bot_scopes={"moderator:read:chatters"},
        )
        harness.api.get_chatters.return_value = [{"user_login": "lurker_b", "user_id": "84"}]

        result = await harness._poll_chatters_single(
            "1001",
            "partner_one",
            88,
            "2026-03-15T10:00:00+00:00",
            token="streamer-token",
        )

        self.assertEqual(result, (88, "partner_one", [{"user_login": "lurker_b", "user_id": "84"}]))
        harness.api.get_chatters.assert_awaited_once_with(
            broadcaster_id="1001",
            moderator_id="9999",
            user_token="bot-token",
        )
        self.assertEqual(harness._chatters_scope_warned, set())

    async def test_poll_chatters_uses_bot_scope_for_authorized_channel_even_when_runtime_cache_is_empty(self) -> None:
        harness = _AnalyticsHarness(
            streamer_scopes={"1001": ["moderator:read:chatters"]},
            bot_scopes={"moderator:read:chatters"},
            monitored=set(),
        )
        harness.api.get_chatters.return_value = [{"user_login": "lurker_auth", "user_id": "123"}]

        result = await harness._poll_chatters_single(
            "1001",
            "partner_one",
            90,
            "2026-03-15T10:00:00+00:00",
            token="streamer-token",
        )

        self.assertEqual(
            result,
            (90, "partner_one", [{"user_login": "lurker_auth", "user_id": "123"}]),
        )
        harness.api.get_chatters.assert_awaited_once_with(
            broadcaster_id="1001",
            moderator_id="9999",
            user_token="bot-token",
        )

    async def test_poll_chatters_uses_bot_scope_for_initial_channel_without_monitored_flag(self) -> None:
        harness = _AnalyticsHarness(
            streamer_scopes={},
            bot_scopes={"moderator:read:chatters"},
            monitored=set(),
            initial_channels=["partner_one"],
        )
        harness.api.get_chatters.return_value = [{"user_login": "lurker_init", "user_id": "321"}]

        result = await harness._poll_chatters_single(
            "1001",
            "partner_one",
            91,
            "2026-03-15T10:00:00+00:00",
            token=None,
        )

        self.assertEqual(
            result,
            (91, "partner_one", [{"user_login": "lurker_init", "user_id": "321"}]),
        )
        harness.api.get_chatters.assert_awaited_once_with(
            broadcaster_id="1001",
            moderator_id="9999",
            user_token="bot-token",
        )

    async def test_poll_chatters_uses_bot_scope_when_chat_bot_runtime_is_not_initialized_yet(self) -> None:
        harness = _AnalyticsHarness(
            streamer_scopes={"1001": ["moderator:read:chatters"]},
            bot_scopes={"moderator:read:chatters"},
            monitored=set(),
        )
        harness._twitch_chat_bot = None
        harness.api.get_chatters.return_value = [{"user_login": "lurker_boot", "user_id": "777"}]

        result = await harness._poll_chatters_single(
            "1001",
            "partner_one",
            92,
            "2026-03-15T10:00:00+00:00",
            token="streamer-token",
        )

        self.assertEqual(
            result,
            (92, "partner_one", [{"user_login": "lurker_boot", "user_id": "777"}]),
        )
        harness.api.get_chatters.assert_awaited_once_with(
            broadcaster_id="1001",
            moderator_id="9999",
            user_token="bot-token",
        )

    async def test_poll_chatters_uses_streamer_scope_as_legacy_fallback_when_bot_is_unavailable(self) -> None:
        harness = _AnalyticsHarness(
            streamer_scopes={"1001": ["moderator:read:chatters"]},
            bot_scopes={"user:read:chat"},
            monitored={"other_channel"},
        )
        harness.api.get_chatters.return_value = [{"user_login": "lurker_c", "user_id": "21"}]

        result = await harness._poll_chatters_single(
            "1001",
            "partner_one",
            89,
            "2026-03-15T10:00:00+00:00",
            token="streamer-token",
        )

        self.assertEqual(result, (89, "partner_one", [{"user_login": "lurker_c", "user_id": "21"}]))
        harness.api.get_chatters.assert_awaited_once_with(
            broadcaster_id="1001",
            moderator_id="1001",
            user_token="streamer-token",
        )

    async def test_poll_chatters_returns_none_when_neither_streamer_nor_bot_have_scope(self) -> None:
        harness = _AnalyticsHarness(
            streamer_scopes={"1001": ["chat:read"]},
            bot_scopes={"user:read:chat"},
        )

        result = await harness._poll_chatters_single(
            "1001",
            "partner_one",
            99,
            "2026-03-15T10:00:00+00:00",
            token="streamer-token",
        )

        self.assertIsNone(result)
        harness.api.get_chatters.assert_not_awaited()
        self.assertEqual(harness._chatters_scope_warned, {("1001", 99)})
