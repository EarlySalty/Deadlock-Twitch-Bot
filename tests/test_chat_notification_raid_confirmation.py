from __future__ import annotations

import contextlib
import sqlite3
import time
import unittest
from types import SimpleNamespace
from unittest.mock import ANY, AsyncMock, patch

from bot.chat.bot import RaidChatBot
from bot.chat.connection import ConnectionMixin
from bot.raid.bot import RaidBot
from tests.sqlite_twitch_schema import ensure_sqlite_twitch_schema


class _JoinHarness(ConnectionMixin):
    def __init__(self, *, bot_scopes: set[str] | None = None) -> None:
        self._client_id = "client-id"
        self._bot_token = "oauth:test-token"
        self._bot_refresh_token = "refresh-token"
        self._token_manager = SimpleNamespace(scopes=set(bot_scopes or set()))
        self.bot_id_safe = "9999"
        self.bot_id = "9999"
        self._monitored_streamers: set[str] = {"targetlogin"}
        self._channel_subscription_types: dict[str, set[str]] = {
            "targetlogin": {"channel.chat.message"}
        }
        self._channel_subscription_state: dict[str, dict[str, dict[str, str]]] = {}
        self._channel_ids: dict[str, str] = {"targetlogin": "9009"}
        self._mod_retry_cooldown: dict[str, object] = {}
        self._monitored_only_channels: set[str] = set()
        self.subscribe_calls: list[str] = []

    async def fetch_user(self, login: str):
        return SimpleNamespace(id="9009")

    async def _ensure_bot_token_registered(self) -> None:
        return None

    async def subscribe_websocket(self, payload) -> None:
        self.subscribe_calls.append(type(payload).__name__)

    def _is_monitored_only(self, channel_name: str) -> bool:
        return False


class _JoinFailureHarness(_JoinHarness):
    async def subscribe_websocket(self, payload) -> None:
        raise Exception("403 subscription missing proper authorization")


class ChatJoinNotificationSubscriptionTests(unittest.IsolatedAsyncioTestCase):
    async def test_join_subscribes_missing_chat_notification_subscription(self) -> None:
        harness = _JoinHarness(bot_scopes={"user:read:chat"})

        with self.assertLogs("TwitchStreams.ChatBot", level="INFO") as captured:
            result = await harness.join("targetlogin", channel_id="9009")

        self.assertTrue(result)
        self.assertEqual(harness.subscribe_calls, ["ChatNotificationSubscription"])
        self.assertTrue(
            harness.is_channel_subscription_ready("targetlogin", "channel.chat.notification")
        )
        self.assertTrue(
            any(
                "join_decision" in entry
                and "decision=joined" in entry
                and "flow_id=" in entry
                for entry in captured.output
            )
        )

    async def test_join_records_missing_broadcaster_scope(self) -> None:
        conn = sqlite3.connect(":memory:")
        conn.row_factory = sqlite3.Row
        ensure_sqlite_twitch_schema(conn)
        conn.execute(
            """
            INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, scopes)
            VALUES (?, ?, ?)
            """,
            ("9009", "targetlogin", "channel:manage:raids"),
        )
        conn.commit()

        harness = _JoinFailureHarness(bot_scopes={"user:read:chat"})
        try:
            with patch(
                "bot.chat.connection.get_conn",
                side_effect=lambda: contextlib.nullcontext(conn),
            ):
                result = await harness.join("targetlogin", channel_id="9009")
        finally:
            conn.close()

        self.assertFalse(result)
        state = harness.get_channel_subscription_state("targetlogin")
        self.assertEqual(
            state["channel.chat.notification"]["state"],
            "missing_broadcaster_scope",
        )
        self.assertIn("channel:bot", state["channel.chat.notification"]["detail"])

    async def test_join_records_missing_bot_scope(self) -> None:
        conn = sqlite3.connect(":memory:")
        conn.row_factory = sqlite3.Row
        ensure_sqlite_twitch_schema(conn)
        conn.execute(
            """
            INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, scopes)
            VALUES (?, ?, ?)
            """,
            ("9009", "targetlogin", "channel:bot"),
        )
        conn.commit()

        harness = _JoinFailureHarness(bot_scopes={"user:write:chat"})
        try:
            with patch(
                "bot.chat.connection.get_conn",
                side_effect=lambda: contextlib.nullcontext(conn),
            ):
                result = await harness.join("targetlogin", channel_id="9009")
        finally:
            conn.close()

        self.assertFalse(result)
        state = harness.get_channel_subscription_state("targetlogin")
        self.assertEqual(
            state["channel.chat.notification"]["state"],
            "missing_bot_scope",
        )
        self.assertIn("user:read:chat", state["channel.chat.notification"]["detail"])

    async def test_join_records_unknown_bot_scope_state_without_mod_retry(self) -> None:
        conn = sqlite3.connect(":memory:")
        conn.row_factory = sqlite3.Row
        ensure_sqlite_twitch_schema(conn)
        conn.execute(
            """
            INSERT INTO twitch_streamers (twitch_login, twitch_user_id, is_monitored_only)
            VALUES (?, ?, 0)
            """,
            ("targetlogin", "9009"),
        )
        conn.execute(
            """
            INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, scopes)
            VALUES (?, ?, ?)
            """,
            ("9009", "targetlogin", "channel:bot"),
        )
        conn.commit()

        harness = _JoinFailureHarness(bot_scopes=set())
        harness._ensure_bot_is_mod = AsyncMock(return_value=False)
        try:
            with patch(
                "bot.chat.connection.get_conn",
                side_effect=lambda: contextlib.nullcontext(conn),
            ):
                result = await harness.join("targetlogin", channel_id="9009")
        finally:
            conn.close()

        self.assertFalse(result)
        harness._ensure_bot_is_mod.assert_not_awaited()
        state = harness.get_channel_subscription_state("targetlogin")
        self.assertEqual(
            state["channel.chat.notification"]["state"],
            "unknown_bot_scope_state",
        )

    async def test_join_records_missing_broadcaster_authorization_without_mod_retry(self) -> None:
        conn = sqlite3.connect(":memory:")
        conn.row_factory = sqlite3.Row
        ensure_sqlite_twitch_schema(conn)
        conn.execute(
            """
            INSERT INTO twitch_streamers (twitch_login, twitch_user_id, is_monitored_only)
            VALUES (?, ?, 0)
            """,
            ("targetlogin", "9009"),
        )
        conn.execute(
            """
            INSERT INTO twitch_partners (
                twitch_user_id,
                twitch_login,
                raid_bot_enabled,
                silent_raid,
                manual_verified_at,
                status
            ) VALUES (?, ?, 1, 0, CURRENT_TIMESTAMP, 'active')
            """,
            ("9009", "targetlogin"),
        )
        conn.commit()

        harness = _JoinFailureHarness(bot_scopes={"user:read:chat"})
        harness._ensure_bot_is_mod = AsyncMock(return_value=False)
        try:
            with patch(
                "bot.chat.connection.get_conn",
                side_effect=lambda: contextlib.nullcontext(conn),
            ):
                result = await harness.join("targetlogin", channel_id="9009")
        finally:
            conn.close()

        self.assertFalse(result)
        harness._ensure_bot_is_mod.assert_not_awaited()
        state = harness.get_channel_subscription_state("targetlogin")
        self.assertEqual(
            state["channel.chat.notification"]["state"],
            "missing_broadcaster_authorization",
        )


class ChatNotificationRaidCorrelationTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        self.conn = sqlite3.connect(":memory:")
        self.conn.row_factory = sqlite3.Row
        ensure_sqlite_twitch_schema(self.conn)

    def tearDown(self) -> None:
        self.conn.close()

    def _insert_partner(self, login: str, user_id: str, *, silent_raid: int = 0) -> None:
        self.conn.execute(
            """
            INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login)
            VALUES (?, ?)
            """,
            (user_id, login),
        )
        self.conn.execute(
            """
            INSERT INTO twitch_partners (
                twitch_user_id,
                twitch_login,
                raid_bot_enabled,
                silent_raid,
                manual_verified_at,
                status
            ) VALUES (?, ?, 1, ?, CURRENT_TIMESTAMP, 'active')
            """,
            (user_id, login, silent_raid),
        )
        self.conn.commit()

    def _insert_streamer_identity(self, login: str, user_id: str) -> None:
        self.conn.execute(
            """
            INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login)
            VALUES (?, ?)
            """,
            (user_id, login),
        )
        self.conn.commit()

    def _build_raid_bot(self) -> RaidBot:
        raid_bot = RaidBot.__new__(RaidBot)
        raid_bot._pending_raids = {}
        raid_bot._recent_raid_arrivals = {}
        raid_bot._orphan_chat_raid_notifications = {}
        raid_bot._manual_raid_suppression = {}
        raid_bot._raid_readiness_by_flow_id = {}
        raid_bot._raid_observability_counter_store = {}
        raid_bot._user_scope_fallback_warned = set()
        raid_bot.chat_bot = None
        raid_bot._bot_id = None
        raid_bot._cog = None
        raid_bot._session = None
        raid_bot._refresh_partner_score_cache_if_available = AsyncMock()
        raid_bot._send_partner_raid_message = AsyncMock()
        raid_bot._send_recruitment_message_now = AsyncMock()
        return raid_bot

    async def test_chat_notification_confirms_pending_raid_and_second_signal_is_telemetry(self) -> None:
        self._insert_partner("targetlogin", "9009")
        self._insert_streamer_identity("source_login", "1001")
        raid_bot = self._build_raid_bot()
        raid_bot._pending_raids["9009"] = raid_bot._build_pending_raid_record(
            from_broadcaster_login="source_login",
            to_broadcaster_id="9009",
            target_stream_data={"_partner_score": {"final_score": 1.15}},
            is_partner_raid=True,
            viewer_count=42,
            offline_trigger_ts=None,
            raid_flow_id="raid-flow-1",
            channel_raid_ready=True,
            channel_raid_ready_detail=None,
            chat_notification_state="subscribed",
            chat_notification_detail=None,
        )

        with (
            patch(
                "bot.raid.bot.get_conn",
                side_effect=lambda: contextlib.nullcontext(self.conn),
            ),
            patch(
                "bot.raid.bot.track_confirmed_partner_raid",
                return_value=321,
            ) as track_mock,
        ):
            await raid_bot.on_chat_raid_notification(
                to_broadcaster_id="9009",
                to_broadcaster_login="targetlogin",
                from_broadcaster_login="source_login",
                from_broadcaster_id="1001",
                viewer_count=42,
                message_id="raid-msg-1",
            )
            await raid_bot.on_raid_arrival(
                to_broadcaster_id="9009",
                to_broadcaster_login="targetlogin",
                from_broadcaster_login="source_login",
                from_broadcaster_id="1001",
                viewer_count=42,
            )

        raid_bot._send_partner_raid_message.assert_awaited_once()
        track_mock.assert_called_once()
        row = self.conn.execute(
            """
            SELECT classification, confirmation_signals
            FROM twitch_raid_arrival_tracking
            """
        ).fetchone()
        self.assertIsNotNone(row)
        self.assertEqual(row["classification"], "ours_to_partner")
        self.assertEqual(
            set(str(row["confirmation_signals"]).split(",")),
            {"channel.chat.notification", "channel.raid"},
        )

    async def test_chat_unraid_does_not_confirm_pending_raid(self) -> None:
        self._insert_partner("targetlogin", "9009")
        raid_bot = self._build_raid_bot()
        raid_bot._pending_raids["9009"] = raid_bot._build_pending_raid_record(
            from_broadcaster_login="source_login",
            to_broadcaster_id="9009",
            target_stream_data=None,
            is_partner_raid=True,
            viewer_count=12,
            offline_trigger_ts=None,
            raid_flow_id="raid-flow-2",
            channel_raid_ready=True,
            channel_raid_ready_detail=None,
            chat_notification_state="subscribed",
            chat_notification_detail=None,
        )

        with patch(
            "bot.raid.bot.get_conn",
            side_effect=lambda: contextlib.nullcontext(self.conn),
        ):
            await raid_bot.on_chat_unraid_notification(
                to_broadcaster_id="9009",
                to_broadcaster_login="targetlogin",
                from_broadcaster_login="source_login",
                event_timestamp="2026-03-16T12:00:00+00:00",
            )

        self.assertIn("9009", raid_bot._pending_raids)
        raid_bot._send_partner_raid_message.assert_not_awaited()
        count = self.conn.execute(
            "SELECT COUNT(*) AS c FROM twitch_raid_arrival_tracking"
        ).fetchone()["c"]
        self.assertEqual(count, 0)

    async def test_source_self_unraid_cancels_pending_raid(self) -> None:
        raid_bot = self._build_raid_bot()
        raid_bot._pending_raids["9009"] = raid_bot._build_pending_raid_record(
            from_broadcaster_login="source_login",
            to_broadcaster_id="9009",
            target_stream_data={"user_login": "targetlogin"},
            is_partner_raid=True,
            viewer_count=12,
            offline_trigger_ts=None,
            raid_flow_id="raid-flow-source-unraid",
            channel_raid_ready=True,
            channel_raid_ready_detail=None,
            chat_notification_state="subscribed",
            chat_notification_detail=None,
        )

        await raid_bot.on_source_self_unraid_notification(
            broadcaster_id="1001",
            broadcaster_login="source_login",
            message_id="msg-unraid-1",
            event_timestamp="2026-03-17T16:03:01+00:00",
        )

        self.assertEqual(raid_bot._pending_raids, {})
        raid_bot._send_partner_raid_message.assert_not_awaited()
        raid_bot._send_recruitment_message_now.assert_not_awaited()

    async def test_independent_partner_raid_is_classified_external(self) -> None:
        self._insert_partner("targetlogin", "9009")
        raid_bot = self._build_raid_bot()

        with patch(
            "bot.raid.bot.get_conn",
            side_effect=lambda: contextlib.nullcontext(self.conn),
        ):
            await raid_bot.on_raid_arrival(
                to_broadcaster_id="9009",
                to_broadcaster_login="targetlogin",
                from_broadcaster_login="outside_streamer",
                from_broadcaster_id="7777",
                viewer_count=18,
            )

        row = self.conn.execute(
            """
            SELECT classification, correlation_status, confirmation_signals
            FROM twitch_raid_arrival_tracking
            """
        ).fetchone()
        self.assertIsNotNone(row)
        self.assertEqual(row["classification"], "external_to_partner")
        self.assertEqual(row["correlation_status"], "independent_channel_raid")
        self.assertEqual(row["confirmation_signals"], "channel.raid")

    async def test_register_pending_raid_matches_orphan_notification_even_when_subscription_exists(self) -> None:
        self._insert_partner("targetlogin", "9009")
        self._insert_streamer_identity("source_login", "1001")
        raid_bot = self._build_raid_bot()
        raid_bot._cog = SimpleNamespace(_eventsub_has_sub=lambda sub_type, user_id: True)

        with (
            patch(
                "bot.raid.bot.get_conn",
                side_effect=lambda: contextlib.nullcontext(self.conn),
            ),
            patch(
                "bot.raid.bot.track_confirmed_partner_raid",
                return_value=654,
            ) as track_mock,
        ):
            await raid_bot.on_chat_raid_notification(
                to_broadcaster_id="9009",
                to_broadcaster_login="targetlogin",
                from_broadcaster_login="source_login",
                from_broadcaster_id="1001",
                viewer_count=21,
                message_id="raid-msg-2",
            )
            await raid_bot._register_pending_raid(
                from_broadcaster_login="source_login",
                to_broadcaster_id="9009",
                to_broadcaster_login="targetlogin",
                target_stream_data={"_partner_score": {"final_score": 1.05}},
                is_partner_raid=True,
                viewer_count=21,
                offline_trigger_ts=None,
                channel_raid_ready=True,
            )

        self.assertEqual(raid_bot._pending_raids, {})
        self.assertEqual(raid_bot._orphan_chat_raid_notifications, {})
        raid_bot._send_partner_raid_message.assert_awaited_once()
        track_mock.assert_called_once()
        row = self.conn.execute(
            """
            SELECT confirmation_signals, primary_signal
            FROM twitch_raid_arrival_tracking
            """
        ).fetchone()
        self.assertIsNotNone(row)
        self.assertEqual(row["confirmation_signals"], "channel.chat.notification")
        self.assertEqual(row["primary_signal"], "channel.chat.notification")

    async def test_ensure_raid_arrival_subscription_ready_does_not_trust_local_tracking_only(self) -> None:
        raid_bot = self._build_raid_bot()
        ensure_ready = AsyncMock(return_value=(False, "status:missing"))
        raid_bot._cog = SimpleNamespace(
            _eventsub_has_sub=lambda sub_type, user_id: True,
            ensure_raid_target_dynamic_ready=ensure_ready,
        )

        ready = await raid_bot._ensure_raid_arrival_subscription_ready(
            to_broadcaster_id="9009",
            to_broadcaster_login="targetlogin",
        )

        self.assertFalse(ready)
        ensure_ready.assert_awaited_once_with("9009", "targetlogin", raid_flow_id=ANY)

    async def test_register_pending_raid_recreates_subscription_when_remote_readiness_failed(self) -> None:
        raid_bot = self._build_raid_bot()
        subscribe_dynamic = AsyncMock(return_value=True)
        raid_bot._cog = SimpleNamespace(
            _eventsub_has_sub=lambda sub_type, user_id: True,
            subscribe_raid_target_dynamic=subscribe_dynamic,
        )

        await raid_bot._register_pending_raid(
            from_broadcaster_login="source_login",
            to_broadcaster_id="9009",
            to_broadcaster_login="targetlogin",
            target_stream_data=None,
            is_partner_raid=False,
            viewer_count=7,
            offline_trigger_ts=None,
            channel_raid_ready=False,
        )

        subscribe_dynamic.assert_awaited_once_with("9009", "targetlogin")


class ChatNotificationPayloadParsingTests(unittest.IsolatedAsyncioTestCase):
    async def test_event_chat_notification_routes_self_unraid_to_source_handler(self) -> None:
        raid_bot = SimpleNamespace(
            on_chat_raid_notification=AsyncMock(),
            on_chat_unraid_notification=AsyncMock(),
            on_source_self_unraid_notification=AsyncMock(),
        )
        chat_bot = RaidChatBot.__new__(RaidChatBot)
        chat_bot._raid_bot = raid_bot

        payload = SimpleNamespace(
            broadcaster=SimpleNamespace(id="9009", name="denoshock"),
            notice_type="unraid",
            chatter=SimpleNamespace(id="9009", name="denoshock"),
            id="msg-123",
            timestamp="2026-03-17T16:03:01+00:00",
        )

        await chat_bot.event_chat_notification(payload)

        raid_bot.on_chat_raid_notification.assert_not_awaited()
        raid_bot.on_chat_unraid_notification.assert_not_awaited()
        raid_bot.on_source_self_unraid_notification.assert_awaited_once_with(
            broadcaster_id="9009",
            broadcaster_login="denoshock",
            message_id="msg-123",
            event_timestamp="2026-03-17T16:03:01+00:00",
        )


if __name__ == "__main__":
    unittest.main()
