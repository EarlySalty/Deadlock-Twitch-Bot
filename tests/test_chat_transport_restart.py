from __future__ import annotations

import asyncio
import contextlib
import sqlite3
import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from bot.chat import bot as chat_bot_module
from bot.chat.connection import ConnectionMixin
from tests.sqlite_twitch_schema import ensure_sqlite_twitch_schema


class _CompatConn:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    def execute(self, sql: str, params=()):
        return self._conn.execute(str(sql).replace("%s", "?"), params)

    def __getattr__(self, name: str):
        return getattr(self._conn, name)


class _PartChannelsHarness(ConnectionMixin):
    def __init__(self) -> None:
        self._initial_channels = ["partner_channel", "cemo_336", "dragskope"]
        self._monitored_streamers = {"partner_channel", "cemo_336", "dragskope"}
        self._channel_subscription_types = {}
        self._channel_subscription_state = {}
        self._channel_ids = {
            "partner_channel": "1001",
            "cemo_336": "494921554",
            "dragskope": "128660506",
        }
        self._monitored_only_channels = {"cemo_336", "dragskope"}

    async def join(self, channel_login: str, channel_id: str | None = None):
        normalized = str(channel_login or "").strip().lower().lstrip("#")
        self._monitored_streamers.add(normalized)
        return True


class _StaleJoinHarness(ConnectionMixin):
    def __init__(self) -> None:
        self._client_id = "client-id"
        self._bot_token = "oauth:test-token"
        self._bot_refresh_token = "refresh-token"
        self._token_manager = SimpleNamespace(scopes={"user:read:chat"})
        self.bot_id_safe = "9999"
        self.bot_id = "9999"
        self._monitored_streamers = {"cemo_336"}
        self._channel_subscription_types = {"cemo_336": {"channel.chat.message"}}
        self._channel_subscription_state = {}
        self._channel_ids = {"cemo_336": "494921554"}
        self._mod_retry_cooldown = {}
        self._monitored_only_channels = set()
        self._initial_channels = ["cemo_336"]
        self._ensure_bot_is_mod = AsyncMock(return_value=False)

    async def fetch_user(self, login: str):
        return SimpleNamespace(id="494921554")

    async def _ensure_bot_token_registered(self) -> bool:
        return True

    async def subscribe_websocket(self, payload) -> None:
        raise Exception("403 subscription missing proper authorization")

    def _is_monitored_only(self, channel_name: str) -> bool:
        return False


class _DummyRoute:
    def __init__(self, token_for: str) -> None:
        self.token_for = token_for
        self.headers: dict[str, str] = {}

    def update_headers(self, values: dict[str, str]) -> None:
        self.headers.update(values)


class _FakeHttpError(RuntimeError):
    def __init__(self, *, status: int, message: str) -> None:
        super().__init__(message)
        self.status = status
        self.extra = {"message": message}


class _StaticTokenHttpBase:
    def __init__(self) -> None:
        self.requests: list[_DummyRoute] = []
        self._raise_invalid_token = False

    async def request(self, route: _DummyRoute):
        self.requests.append(route)
        if self._raise_invalid_token:
            raise _FakeHttpError(status=401, message="invalid oauth token")
        return {"ok": True}


class _StaticTokenHttp(_StaticTokenHttpBase):
    def __init__(self, validate_token) -> None:
        super().__init__()
        self.validate_token = validate_token
        self._tokens: dict[str, dict[str, str]] = {}


class ChatTransportRestartTests(unittest.IsolatedAsyncioTestCase):
    @unittest.skipUnless(
        getattr(chat_bot_module, "TWITCHIO_AVAILABLE", False)
        and hasattr(chat_bot_module, "Scopes"),
        "TwitchIO scopes unavailable in test environment",
    )
    async def test_twitchio_scope_compat_accepts_manage_suspicious_users(self) -> None:
        scopes = chat_bot_module.Scopes(["moderator:manage:suspicious_users"])

        self.assertIn("moderator_manage_suspicious_users", vars(chat_bot_module.Scopes))
        self.assertIn("moderator:manage:suspicious_users", {str(scope) for scope in scopes.selected})

    async def test_scope_compat_error_classifier_matches_attribute_error(self) -> None:
        self.assertTrue(
            chat_bot_module.RaidChatBot._is_twitchio_scope_compat_error(
                AttributeError("'Scopes' object has no attribute 'moderator_manage_suspicious_users'")
            )
        )
        self.assertFalse(
            chat_bot_module.RaidChatBot._is_twitchio_scope_compat_error(
                RuntimeError("invalid oauth token")
            )
        )

    async def test_register_bot_token_without_refresh_uses_static_token_support(self) -> None:
        async def _validate_token(_token: str):
            return SimpleNamespace(user_id="9999", expires_in=7200)

        dummy = SimpleNamespace(
            bot_id="9999",
            _http=_StaticTokenHttp(_validate_token),
        )

        registered = await ConnectionMixin._register_bot_token_with_twitchio(
            dummy,
            access_token="oauth:test-token",
            refresh_token=None,
        )

        self.assertTrue(registered)
        self.assertEqual(dummy._http._tokens, {})
        self.assertEqual(
            dummy._http._codex_non_refreshable_user_tokens["9999"],
            "test-token",
        )
        route = _DummyRoute("9999")
        response = await dummy._http.request(route)
        self.assertEqual(response, {"ok": True})
        self.assertEqual(route.headers["Authorization"], "Bearer test-token")

    async def test_non_refreshable_static_token_raises_clear_error_when_expired(self) -> None:
        async def _validate_token(_token: str):
            return SimpleNamespace(user_id="9999", expires_in=7200)

        dummy = SimpleNamespace(
            bot_id="9999",
            _http=_StaticTokenHttp(_validate_token),
        )

        registered = await ConnectionMixin._register_bot_token_with_twitchio(
            dummy,
            access_token="oauth:test-token",
            refresh_token=None,
        )

        self.assertTrue(registered)
        dummy._http._raise_invalid_token = True

        with self.assertRaises(RuntimeError) as ctx:
            await dummy._http.request(_DummyRoute("9999"))

        self.assertIn("cannot be auto-refreshed", str(ctx.exception))
        self.assertNotIn("9999", dummy._http._codex_non_refreshable_user_tokens)

    @unittest.skipUnless(
        getattr(chat_bot_module, "TWITCHIO_AVAILABLE", False)
        and hasattr(chat_bot_module, "RaidChatBot"),
        "TwitchIO chat bot unavailable in test environment",
    )
    async def test_setup_hook_does_not_log_auth_added_when_registration_returns_false(self) -> None:
        dummy = SimpleNamespace(
            _token_manager=None,
            _bot_token="oauth:test-token",
            _bot_refresh_token=None,
            bot_id="9999",
            _register_bot_token_with_twitchio=AsyncMock(return_value=False),
            _persist_bot_tokens=AsyncMock(),
            _is_twitchio_scope_compat_error=chat_bot_module.RaidChatBot._is_twitchio_scope_compat_error,
        )

        with (
            patch.object(chat_bot_module.log, "info") as info_mock,
            patch.object(chat_bot_module.log, "warning") as warning_mock,
        ):
            await chat_bot_module.RaidChatBot.setup_hook(dummy)

        info_messages = [call.args[0] for call in info_mock.call_args_list if call.args]
        self.assertNotIn("Bot auth added (refresh available: %s).", info_messages)
        warning_messages = [call.args[0] for call in warning_mock.call_args_list if call.args]
        self.assertIn(
            "Bot auth is present but could not be registered in TwitchIO (refresh available: %s).",
            warning_messages,
        )
        dummy._persist_bot_tokens.assert_awaited_once()

    @unittest.skipUnless(
        getattr(chat_bot_module, "TWITCHIO_AVAILABLE", False)
        and hasattr(chat_bot_module, "RaidChatBot"),
        "TwitchIO chat bot unavailable in test environment",
    )
    async def test_token_refresh_logs_warning_when_registration_returns_false(self) -> None:
        dummy = SimpleNamespace(
            _bot_token=None,
            _bot_refresh_token=None,
            _register_bot_token_with_twitchio=AsyncMock(return_value=False),
            _is_twitchio_scope_compat_error=chat_bot_module.RaidChatBot._is_twitchio_scope_compat_error,
        )

        with patch.object(chat_bot_module.log, "warning") as warning_mock:
            await chat_bot_module.RaidChatBot._on_token_manager_refresh(
                dummy,
                access_token="oauth:test-token",
                refresh_token=None,
                _expires_at=None,
            )

        warning_messages = [call.args[0] for call in warning_mock.call_args_list if call.args]
        self.assertIn(
            "Refreshed Bot-Token ist vorhanden, konnte aber nicht in TwitchIO registriert werden.",
            warning_messages,
        )

    async def test_part_channels_prunes_initial_channel_cache(self) -> None:
        harness = _PartChannelsHarness()

        removed = await harness.part_channels(["cemo_336"])

        self.assertEqual(removed, 0)
        self.assertEqual(harness._initial_channels, ["partner_channel", "dragskope"])
        self.assertNotIn("cemo_336", harness._monitored_only_channels)
        self.assertNotIn("cemo_336", harness._monitored_streamers)

    async def test_join_channels_can_skip_monitored_only_marking(self) -> None:
        harness = _PartChannelsHarness()
        harness._monitored_only_channels.clear()

        joined = await harness.join_channels(
            ["partner_channel", "derechtecoolys"],
            rate_limit_delay=0,
            mark_monitored_only=False,
        )

        self.assertEqual(joined, 2)
        self.assertEqual(harness._monitored_only_channels, set())

    @unittest.skipUnless(
        getattr(chat_bot_module, "TWITCHIO_AVAILABLE", False)
        and hasattr(chat_bot_module, "RaidChatBot"),
        "TwitchIO chat bot unavailable in test environment",
    )
    async def test_event_ready_skips_initial_join_once_after_managed_restart(self) -> None:
        dummy = SimpleNamespace(
            user=SimpleNamespace(name="deutschedeadlockcommunity"),
            commands={},
            _skip_initial_join_once=True,
            _initial_channels=["cemo_336", "dragskope"],
            join=AsyncMock(return_value=True),
            _promo_task=None,
            _next_chat_observability_flow_id=lambda prefix: f"{prefix}-test",
            _log_chat_runtime_snapshot=lambda **kwargs: None,
        )

        with patch.object(chat_bot_module, "PROMO_MESSAGES", []):
            await chat_bot_module.RaidChatBot.event_ready(dummy)

        dummy.join.assert_not_awaited()
        self.assertFalse(dummy._skip_initial_join_once)

    @unittest.skipUnless(
        getattr(chat_bot_module, "TWITCHIO_AVAILABLE", False)
        and hasattr(chat_bot_module, "RaidChatBot"),
        "TwitchIO chat bot unavailable in test environment",
    )
    async def test_restart_after_transport_failure_clears_subscription_caches(self) -> None:
        created_coroutines = []

        def _fake_create_task(coro, *, name=None):
            created_coroutines.append((coro, name))
            if asyncio.iscoroutine(coro):
                coro.close()
            return SimpleNamespace(name=name)

        dummy = SimpleNamespace(
            _restart_lock=asyncio.Lock(),
            _managed_start_with_adapter=False,
            _managed_load_tokens=False,
            _managed_save_tokens=False,
            _skip_initial_join_once=False,
            _monitored_streamers={"partner_channel"},
            _channel_subscription_types={"partner_channel": {"channel.chat.message"}},
            _channel_subscription_state={
                "partner_channel": {"channel.chat.message": {"state": "ok"}}
            },
            _channel_ids={"partner_channel": "1001"},
            _websockets={"9999": {"stale-session": object()}},
            _login_called=True,
            _has_closed=True,
            _ready_event=asyncio.Event(),
            close=AsyncMock(),
            start=AsyncMock(),
            _rejoin_channels_after_restart=AsyncMock(),
            adapter=None,
        )
        dummy._bounded_runtime_sample = lambda values, limit=8: chat_bot_module.RaidChatBot._bounded_runtime_sample(  # type: ignore[assignment]
            values,
            limit=limit,
        )
        dummy._iter_eventsub_websockets = lambda token_for=None: chat_bot_module.RaidChatBot._iter_eventsub_websockets(  # type: ignore[assignment]
            dummy,
            token_for=token_for,
        )
        dummy._snapshot_chat_runtime_state = lambda: chat_bot_module.RaidChatBot._snapshot_chat_runtime_state(dummy)  # type: ignore[assignment]
        dummy._chat_observability_normalize = lambda value, limit=240: ConnectionMixin._chat_observability_normalize(  # type: ignore[assignment]
            value,
            limit=limit,
        )
        dummy._format_chat_observability_fields = lambda **fields: ConnectionMixin._format_chat_observability_fields(dummy, **fields)  # type: ignore[assignment]
        dummy._log_chat_runtime_snapshot = lambda **kwargs: chat_bot_module.RaidChatBot._log_chat_runtime_snapshot(dummy, **kwargs)  # type: ignore[assignment]
        dummy._reset_managed_transport_restart_state = lambda: chat_bot_module.RaidChatBot._reset_managed_transport_restart_state(dummy)  # type: ignore[assignment]

        with self.assertLogs("TwitchStreams.ChatBot", level="INFO") as captured:
            with patch("bot.chat.bot.asyncio.sleep", new=AsyncMock(return_value=None)):
                with patch(
                    "bot.chat.bot.asyncio.create_task",
                    side_effect=_fake_create_task,
                ):
                    await chat_bot_module.RaidChatBot._restart_after_transport_failure(
                        dummy,
                        channel_list=["partner_channel"],
                        reason="broken transport",
                        flow_id="restart-test-1",
                        failed_channel="partner_channel",
                    )

        self.assertEqual(dummy._monitored_streamers, set())
        self.assertEqual(dummy._channel_subscription_types, {})
        self.assertEqual(dummy._channel_subscription_state, {})
        self.assertEqual(dummy._channel_ids, {})
        self.assertEqual(dummy._websockets, {})
        self.assertFalse(dummy._login_called)
        self.assertFalse(dummy._has_closed)
        self.assertTrue(dummy._skip_initial_join_once)
        self.assertEqual(len(created_coroutines), 2)
        self.assertTrue(
            any(
                "chat_runtime_snapshot" in entry
                and "flow_id=restart-test-1" in entry
                and "phase=restart_begin" in entry
                for entry in captured.output
            )
        )

    @unittest.skipUnless(
        getattr(chat_bot_module, "TWITCHIO_AVAILABLE", False)
        and hasattr(chat_bot_module, "RaidChatBot"),
        "TwitchIO chat bot unavailable in test environment",
    )
    async def test_ensure_eventsub_transport_ready_reuses_connected_websocket(self) -> None:
        existing_socket = SimpleNamespace(connected=True, session_id="session-existing")
        dummy = SimpleNamespace(
            bot_id="9999",
            _http=object(),
            _websockets={"9999": {"session-existing": existing_socket}},
            _log_chat_runtime_snapshot=lambda **kwargs: None,
        )
        dummy._iter_eventsub_websockets = lambda token_for=None: chat_bot_module.RaidChatBot._iter_eventsub_websockets(  # type: ignore[assignment]
            dummy,
            token_for=token_for,
        )
        dummy._find_connected_eventsub_websocket = lambda token_for: chat_bot_module.RaidChatBot._find_connected_eventsub_websocket(  # type: ignore[assignment]
            dummy,
            token_for=token_for,
        )

        with patch.object(chat_bot_module, "Websocket") as websocket_cls:
            ready = await chat_bot_module.RaidChatBot._ensure_eventsub_transport_ready(
                dummy,
                flow_id="restart-test-ready-1",
                reason="broken transport",
                channel_list=["partner_channel"],
            )

        self.assertTrue(ready)
        websocket_cls.assert_not_called()

    @unittest.skipUnless(
        getattr(chat_bot_module, "TWITCHIO_AVAILABLE", False)
        and hasattr(chat_bot_module, "RaidChatBot"),
        "TwitchIO chat bot unavailable in test environment",
    )
    async def test_ensure_eventsub_transport_ready_creates_new_websocket(self) -> None:
        created = []

        class _FakeWebsocket:
            def __init__(self, *, client, token_for, http):
                self.client = client
                self.token_for = token_for
                self.http = http
                self.connected = False
                self.session_id = None
                created.append(self)

            async def connect(self, *, fail_once=False):
                self.connected = True
                self.session_id = "session-new"

        dummy = SimpleNamespace(
            bot_id="9999",
            _http=object(),
            _websockets={"9999": {}},
            _log_chat_runtime_snapshot=lambda **kwargs: None,
            _restart_transport_ready_attempts=1,
            _restart_transport_ready_backoff_seconds=0.01,
        )
        dummy._iter_eventsub_websockets = lambda token_for=None: chat_bot_module.RaidChatBot._iter_eventsub_websockets(  # type: ignore[assignment]
            dummy,
            token_for=token_for,
        )
        dummy._find_connected_eventsub_websocket = lambda token_for: chat_bot_module.RaidChatBot._find_connected_eventsub_websocket(  # type: ignore[assignment]
            dummy,
            token_for=token_for,
        )

        with patch.object(chat_bot_module, "Websocket", _FakeWebsocket):
            ready = await chat_bot_module.RaidChatBot._ensure_eventsub_transport_ready(
                dummy,
                flow_id="restart-test-ready-2",
                reason="broken transport",
                channel_list=["partner_channel"],
            )

        self.assertTrue(ready)
        self.assertEqual(len(created), 1)
        self.assertIn("session-new", dummy._websockets["9999"])

    @unittest.skipUnless(
        getattr(chat_bot_module, "TWITCHIO_AVAILABLE", False)
        and hasattr(chat_bot_module, "RaidChatBot"),
        "TwitchIO chat bot unavailable in test environment",
    )
    async def test_rejoin_after_restart_defers_when_transport_not_ready(self) -> None:
        dummy = SimpleNamespace(
            wait_until_ready=AsyncMock(),
            _normalize_channel_login=lambda channel: str(channel or "").strip().lower().lstrip("#"),
            _log_chat_runtime_snapshot=lambda **kwargs: None,
            _ensure_eventsub_transport_ready=AsyncMock(return_value=False),
            join_channels=AsyncMock(),
            _restart_rejoin_retry_attempts=0,
        )

        await chat_bot_module.RaidChatBot._rejoin_channels_after_restart(
            dummy,
            ["partner_channel"],
            flow_id="restart-test-rejoin-1",
            reason="broken transport",
        )

        dummy.join_channels.assert_not_awaited()

    @unittest.skipUnless(
        getattr(chat_bot_module, "TWITCHIO_AVAILABLE", False)
        and hasattr(chat_bot_module, "RaidChatBot"),
        "TwitchIO chat bot unavailable in test environment",
    )
    async def test_rejoin_after_restart_retries_deferred_transport(self) -> None:
        dummy = SimpleNamespace(
            wait_until_ready=AsyncMock(),
            _normalize_channel_login=lambda channel: str(channel or "").strip().lower().lstrip("#"),
            _log_chat_runtime_snapshot=lambda **kwargs: None,
            _ensure_eventsub_transport_ready=AsyncMock(side_effect=[False, True]),
            join_channels=AsyncMock(return_value=1),
            _increment_chat_observability_counter=lambda *args, **kwargs: None,
            _restart_rejoin_retry_attempts=1,
            _restart_rejoin_retry_backoff_seconds=0.01,
        )
        dummy._rejoin_channels_after_restart = lambda channels, *, flow_id, reason, transport_retry=0: chat_bot_module.RaidChatBot._rejoin_channels_after_restart(  # type: ignore[assignment]
            dummy,
            channels,
            flow_id=flow_id,
            reason=reason,
            transport_retry=transport_retry,
        )

        with patch("bot.chat.bot.asyncio.sleep", new=AsyncMock(return_value=None)):
            await chat_bot_module.RaidChatBot._rejoin_channels_after_restart(
                dummy,
                ["partner_channel"],
                flow_id="restart-test-rejoin-2",
                reason="broken transport",
            )

        self.assertEqual(dummy._ensure_eventsub_transport_ready.await_count, 2)
        dummy.join_channels.assert_awaited_once()

    async def test_join_purges_stale_removed_channel_before_mod_retry(self) -> None:
        conn = sqlite3.connect(":memory:")
        conn.row_factory = sqlite3.Row
        ensure_sqlite_twitch_schema(conn)
        harness = _StaleJoinHarness()
        try:
            with patch(
                "bot.chat.connection.readonly_connection",
                side_effect=lambda: contextlib.nullcontext(_CompatConn(conn)),
            ):
                result = await harness.join("cemo_336", channel_id="494921554")
        finally:
            conn.close()

        self.assertFalse(result)
        harness._ensure_bot_is_mod.assert_not_awaited()
        self.assertNotIn("cemo_336", harness._initial_channels)
        self.assertNotIn("cemo_336", harness._monitored_streamers)
        state = harness.get_channel_subscription_state("cemo_336")
        self.assertEqual(state, {})


if __name__ == "__main__":
    unittest.main()
