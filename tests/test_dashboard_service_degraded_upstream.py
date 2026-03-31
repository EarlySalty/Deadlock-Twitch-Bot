import asyncio
import html
import hmac
import hashlib
import json
import unittest
from datetime import UTC, datetime
from types import SimpleNamespace
from urllib.parse import parse_qs, urlencode, urlsplit
from unittest.mock import ANY, AsyncMock, patch

from aiohttp import web
from aiohttp.test_utils import TestClient, TestServer

from bot import storage
from bot.core.constants import log
from bot.dashboard.live.live import DashboardLiveMixin
from bot.dashboard.route_deps import EntryRouteDeps
from bot.dashboard.routes_entry import discord_link as entry_discord_link
from bot.dashboard_service.app import DASHBOARD_EVENTSUB_BRIDGE_KEY, build_dashboard_service_app
from bot.dashboard_service.client import BotApiClientError
from bot.dashboard_service.eventsub_bridge import DashboardEventSubBridgeRuntime
from bot.monitoring.eventsub_state_store import EventSubStateStore
from tests.eventsub_state_store_test_helpers import InMemoryEventSubStateRepository


def _query_params(location: str) -> dict[str, list[str]]:
    return parse_qs(urlsplit(location).query)


class _DummyLiveWriteHandler(DashboardLiveMixin):
    def __init__(self, *, payload: dict[str, str], upstream_error: BotApiClientError) -> None:
        self._payload = payload
        self._upstream_error = upstream_error

    def _require_token(self, request):
        del request

    async def _read_post_with_csrf(self, request, *, fallback_path: str = "/twitch/admin"):
        del request, fallback_path
        return self._payload

    def _redirect_location(
        self,
        request,
        *,
        ok: str | None = None,
        err: str | None = None,
        default_path: str = "/twitch/stats",
    ) -> str:
        del request, default_path
        params = {}
        if ok is not None:
            params["ok"] = ok
        if err is not None:
            params["err"] = err
        return f"/twitch?{urlencode(params)}" if params else "/twitch"

    def _safe_internal_redirect(self, location: str, *, fallback: str = "/twitch/stats") -> str:
        del fallback
        return location

    async def _do_add(self, raw: str) -> str:
        del raw
        raise self._upstream_error

    async def _remove(self, login: str) -> str:
        del login
        raise self._upstream_error

    async def _verify(self, login: str, mode: str) -> str:
        del login, mode
        raise self._upstream_error

    async def _archive(self, login: str, mode: str) -> str:
        del login, mode
        raise self._upstream_error

    async def _discord_flag(self, login: str, is_on_discord: bool) -> str:
        del login, is_on_discord
        raise self._upstream_error

    async def _discord_profile(
        self,
        login: str,
        discord_user_id: str | None,
        discord_display_name: str | None,
        mark_member: bool,
    ) -> str:
        del login, discord_user_id, discord_display_name, mark_member
        raise self._upstream_error


class _DummyEntryServer:
    def __init__(self, *, upstream_error: BotApiClientError) -> None:
        self._upstream_error = upstream_error

    def _require_token(self, request):
        del request

    def _csrf_verify_token(self, request, csrf_token: str) -> bool:
        del request, csrf_token
        return True

    def _redirect_location(
        self,
        request,
        *,
        ok: str | None = None,
        err: str | None = None,
        default_path: str = "/twitch/stats",
    ) -> str:
        del request, default_path
        params = {}
        if ok is not None:
            params["ok"] = ok
        if err is not None:
            params["err"] = err
        return f"/twitch?{urlencode(params)}" if params else "/twitch"

    def _safe_internal_redirect(self, location: str, *, fallback: str = "/twitch/stats") -> str:
        del fallback
        return location

    async def _discord_profile(
        self,
        login: str,
        *,
        discord_user_id: str | None,
        discord_display_name: str | None,
        mark_member: bool,
    ) -> str:
        del login, discord_user_id, discord_display_name, mark_member
        raise self._upstream_error


class _FakeDashboardApp:
    def __init__(self) -> None:
        self.store: dict[str, object] = {}
        self.on_startup: list[object] = []
        self.on_cleanup: list[object] = []

    def __setitem__(self, key, value):
        self.store[key] = value

    def __getitem__(self, key):
        return self.store[key]


class _UpstreamFailingBotApiClient:
    def __init__(self, **_kwargs) -> None:
        pass

    async def get_raid_go_url(self, state: str) -> str | None:
        del state
        raise BotApiClientError(
            status=503,
            code="upstream_unavailable",
            message="Bot internal API is unavailable.",
        )

    async def send_raid_requirements(self, login: str) -> str:
        del login
        raise BotApiClientError(
            status=503,
            code="upstream_unavailable",
            message="Bot internal API is unavailable.",
        )

    async def close(self) -> None:
        return None


class _FakeEventSubHandler:
    def __init__(self) -> None:
        self.callbacks: dict[str, object] = {}
        self.dispatch_activated = False

    def set_callback(self, sub_type: str, callback) -> None:
        self.callbacks[sub_type] = callback

    def activate_notification_dispatch(self) -> None:
        self.dispatch_activated = True


class _ForwardingBotApiClient:
    def __init__(self, **_kwargs) -> None:
        self.dispatch_eventsub_notification = AsyncMock(return_value={"ok": True})
        self.healthz = AsyncMock(return_value={})

    async def close(self) -> None:
        return None


class _MissingCallbackBotApiClient:
    def __init__(self, **_kwargs) -> None:
        self.dispatch_eventsub_notification = AsyncMock(
            return_value={"ok": False, "message": "EventSub dispatch unavailable"}
        )
        self.healthz = AsyncMock(return_value={})

    async def close(self) -> None:
        return None


class _InMemoryBridgeStore:
    def __init__(self) -> None:
        self.rows: dict[str, dict[str, object]] = {}
        self.dead_letters: dict[str, dict[str, object]] = {}

    def enqueue(self, *, message_id: str, sub_type: str, payload: dict, now: float) -> bool:
        if message_id in self.rows:
            return False
        self.rows[message_id] = {
            "message_id": message_id,
            "sub_type": sub_type,
            "payload_json": json.dumps(payload),
            "attempt_count": 0,
            "next_attempt_at": float(now),
            "queued_at": float(now),
        }
        return True

    def lease_due(self, *, now: float, lease_seconds: float, limit: int) -> list[dict[str, object]]:
        del lease_seconds
        leased: list[dict[str, object]] = []
        for row in sorted(self.rows.values(), key=lambda item: float(item.get("queued_at") or 0.0)):
            if len(leased) >= limit:
                break
            if float(row.get("next_attempt_at") or 0.0) > float(now):
                continue
            leased.append(dict(row))
            row["next_attempt_at"] = float(now) + 60.0
        return leased

    def mark_delivered(self, *, message_id: str) -> None:
        self.rows.pop(message_id, None)

    def mark_retry(
        self,
        *,
        message_id: str,
        attempt_count: int,
        error_message: str,
        next_attempt_at: float,
    ) -> None:
        row = self.rows.get(message_id)
        if not row:
            return
        row["attempt_count"] = int(attempt_count)
        row["last_error"] = error_message
        row["next_attempt_at"] = float(next_attempt_at)

    def mark_deferred(
        self,
        *,
        message_id: str,
        error_message: str,
        next_attempt_at: float,
    ) -> None:
        row = self.rows.get(message_id)
        if not row:
            return
        row["last_error"] = error_message
        row["next_attempt_at"] = float(next_attempt_at)

    def mark_dead_letter(
        self,
        *,
        message_id: str,
        sub_type: str,
        payload_json: str,
        queued_at: float,
        attempt_count: int,
        error_message: str,
        dead_lettered_at: float,
    ) -> None:
        self.dead_letters[message_id] = {
            "message_id": message_id,
            "sub_type": sub_type,
            "payload_json": payload_json,
            "queued_at": float(queued_at),
            "attempt_count": int(attempt_count),
            "last_error": error_message,
            "dead_lettered_at": float(dead_lettered_at),
        }
        self.rows.pop(message_id, None)


class DashboardServiceDegradedUpstreamTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        super().setUp()
        self._eventsub_state_store_patch = patch(
            "bot.monitoring.eventsub_state_store.EventSubStateStore",
            side_effect=lambda *args, **kwargs: EventSubStateStore(
                repository=InMemoryEventSubStateRepository(),
                logger=kwargs.get("logger"),
            ),
        )
        self._eventsub_state_store_patch.start()
        self._eventsub_bridge_runtime_patch = patch(
            "bot.dashboard_service.app.DashboardEventSubBridgeRuntime",
            side_effect=lambda *args, **kwargs: self._build_bridge_runtime(*args, **kwargs),
        )
        self._eventsub_bridge_runtime_patch.start()

    def tearDown(self) -> None:
        self._eventsub_bridge_runtime_patch.stop()
        self._eventsub_state_store_patch.stop()
        super().tearDown()

    @staticmethod
    async def _wait_for_async_mock_awaits(mock: AsyncMock, *, minimum: int, timeout: float = 1.0) -> None:
        deadline = asyncio.get_running_loop().time() + timeout
        while mock.await_count < minimum:
            if asyncio.get_running_loop().time() >= deadline:
                raise AssertionError(
                    f"expected async mock to be awaited at least {minimum} times, got {mock.await_count}"
                )
            await asyncio.sleep(0.02)

    def _build_bridge_runtime(self, *args, **kwargs) -> DashboardEventSubBridgeRuntime:
        runtime = DashboardEventSubBridgeRuntime(
            *args,
            store=_InMemoryBridgeStore(),
            **kwargs,
        )
        runtime._retry_delay_seconds = lambda _attempts: 0.05  # type: ignore[method-assign]
        return runtime

    @staticmethod
    def _signed_eventsub_headers(
        *,
        secret: str,
        body: dict[str, object],
        message_id: str,
        message_type: str = "notification",
        subscription_type: str = "stream.offline",
    ) -> dict[str, str]:
        raw_body = json.dumps(body).encode("utf-8")
        timestamp = datetime.now(UTC).isoformat(timespec="seconds").replace("+00:00", "Z")
        digest = hmac.new(
            secret.encode("utf-8"),
            message_id.encode("utf-8") + timestamp.encode("utf-8") + raw_body,
            hashlib.sha256,
        ).hexdigest()
        return {
            "Content-Type": "application/json",
            "Twitch-Eventsub-Message-Id": message_id,
            "Twitch-Eventsub-Message-Timestamp": timestamp,
            "Twitch-Eventsub-Message-Signature": f"sha256={digest}",
            "Twitch-Eventsub-Message-Type": message_type,
            "Twitch-Eventsub-Subscription-Type": subscription_type,
        }

    def test_dashboard_service_wires_eventsub_webhook_handler_when_secret_present(
        self,
    ) -> None:
        captured: dict[str, object] = {}
        sentinel_handler = _FakeEventSubHandler()

        def _fake_build_v2_app(**kwargs):
            captured.update(kwargs)
            return _FakeDashboardApp()

        def _fake_secret(name: str, *args, **kwargs) -> str:
            del args, kwargs
            if name == "TWITCH_WEBHOOK_SECRET":
                return "webhook-secret"
            return ""

        with (
            patch(
                "bot.dashboard_service.app.analytics_db_fingerprint_details",
                return_value={"fingerprint": "local", "hostHash": "h", "databaseHash": "d", "portHash": "p"},
            ),
            patch("bot.dashboard_service.app.build_v2_app", side_effect=_fake_build_v2_app),
            patch("bot.dashboard_service.app.load_secret_value", side_effect=_fake_secret),
            patch(
                "bot.monitoring.eventsub_webhook.EventSubWebhookHandler",
                return_value=sentinel_handler,
            ) as handler_cls,
        ):
            build_dashboard_service_app(
                internal_api_base_url="http://127.0.0.1:1234",
                internal_api_token="",
                internal_api_allow_non_loopback=False,
                internal_api_timeout_seconds=1.0,
                dashboard_token="dash-token",
                partner_token="partner-token",
                noauth=False,
                oauth_client_id="client-id",
                oauth_client_secret="client-secret",
                oauth_redirect_uri="https://example.com/callback",
                session_ttl_seconds=3600,
                legacy_stats_url="https://example.com/stats",
            )

        handler_cls.assert_called_once_with(
            secret="webhook-secret",
            logger=log,
            synchronous_notifications=True,
            state_store=ANY,
        )
        services = captured["dashboard_services"]
        assert services is not None
        self.assertIs(services.eventsub_webhook_handler, sentinel_handler)
        self.assertIs(captured["eventsub_webhook_handler"], sentinel_handler)
        self.assertIn("stream.offline", sentinel_handler.callbacks)
        self.assertIn("stream.online", sentinel_handler.callbacks)
        self.assertIn("channel.update", sentinel_handler.callbacks)
        self.assertTrue(sentinel_handler.dispatch_activated)

    async def test_dashboard_service_eventsub_bridge_forwards_offline_notification_to_internal_api(
        self,
    ) -> None:
        captured: dict[str, object] = {}
        sentinel_handler = _FakeEventSubHandler()
        fake_client = _ForwardingBotApiClient()

        def _fake_build_v2_app(**kwargs):
            captured.update(kwargs)
            return _FakeDashboardApp()

        def _fake_secret(name: str, *args, **kwargs) -> str:
            del args, kwargs
            if name == "TWITCH_WEBHOOK_SECRET":
                return "webhook-secret"
            return ""

        with (
            patch(
                "bot.dashboard_service.app.analytics_db_fingerprint_details",
                return_value={"fingerprint": "local", "hostHash": "h", "databaseHash": "d", "portHash": "p"},
            ),
            patch("bot.dashboard_service.app.build_v2_app", side_effect=_fake_build_v2_app),
            patch("bot.dashboard_service.app.load_secret_value", side_effect=_fake_secret),
            patch(
                "bot.monitoring.eventsub_webhook.EventSubWebhookHandler",
                return_value=sentinel_handler,
            ),
            patch(
                "bot.dashboard_service.app.BotApiClient",
                return_value=fake_client,
            ),
        ):
            build_dashboard_service_app(
                internal_api_base_url="http://127.0.0.1:1234",
                internal_api_token="internal-token",
                internal_api_allow_non_loopback=False,
                internal_api_timeout_seconds=1.0,
                dashboard_token="dash-token",
                partner_token="partner-token",
                noauth=False,
                oauth_client_id="client-id",
                oauth_client_secret="client-secret",
                oauth_redirect_uri="https://example.com/callback",
                session_ttl_seconds=3600,
                legacy_stats_url="https://example.com/stats",
            )

        callback = sentinel_handler.callbacks["stream.offline"]
        await callback(
            "520300019",
            "derechtecoolys",
            {
                "broadcaster_user_id": "520300019",
                "broadcaster_user_login": "derechtecoolys",
                "type": "live",
            },
            message_id="msg-offline-1",
        )

        fake_client.dispatch_eventsub_notification.assert_awaited_once_with(
            sub_type="stream.offline",
            message_id="msg-offline-1",
            payload={
                "subscription": {
                    "type": "stream.offline",
                    "condition": {"broadcaster_user_id": "520300019"},
                },
                "event": {
                    "broadcaster_user_id": "520300019",
                    "broadcaster_user_login": "derechtecoolys",
                    "type": "live",
                },
            },
        )

    async def test_dashboard_service_eventsub_bridge_surfaces_missing_bot_callback_for_raid(
        self,
    ) -> None:
        captured: dict[str, object] = {}
        sentinel_handler = _FakeEventSubHandler()
        fake_client = _MissingCallbackBotApiClient()

        def _fake_build_v2_app(**kwargs):
            captured.update(kwargs)
            return _FakeDashboardApp()

        def _fake_secret(name: str, *args, **kwargs) -> str:
            del args, kwargs
            if name == "TWITCH_WEBHOOK_SECRET":
                return "webhook-secret"
            return ""

        with (
            patch(
                "bot.dashboard_service.app.analytics_db_fingerprint_details",
                return_value={"fingerprint": "local", "hostHash": "h", "databaseHash": "d", "portHash": "p"},
            ),
            patch("bot.dashboard_service.app.build_v2_app", side_effect=_fake_build_v2_app),
            patch("bot.dashboard_service.app.load_secret_value", side_effect=_fake_secret),
            patch(
                "bot.monitoring.eventsub_webhook.EventSubWebhookHandler",
                return_value=sentinel_handler,
            ),
            patch(
                "bot.dashboard_service.app.BotApiClient",
                return_value=fake_client,
            ),
        ):
            build_dashboard_service_app(
                internal_api_base_url="http://127.0.0.1:1234",
                internal_api_token="internal-token",
                internal_api_allow_non_loopback=False,
                internal_api_timeout_seconds=1.0,
                dashboard_token="dash-token",
                partner_token="partner-token",
                noauth=False,
                oauth_client_id="client-id",
                oauth_client_secret="client-secret",
                oauth_redirect_uri="https://example.com/callback",
                session_ttl_seconds=3600,
                legacy_stats_url="https://example.com/stats",
            )

        callback = sentinel_handler.callbacks["channel.raid"]
        with self.assertRaisesRegex(RuntimeError, "EventSub dispatch unavailable"):
            await callback(
                "520300019",
                "derechtecoolys",
                {
                    "to_broadcaster_user_id": "520300019",
                    "to_broadcaster_user_login": "derechtecoolys",
                    "from_broadcaster_user_id": "9901",
                    "from_broadcaster_user_login": "raider_login",
                    "viewers": 42,
                },
                message_id="msg-raid-missing-callback-1",
                subscription={
                    "type": "channel.raid",
                    "condition": {"to_broadcaster_user_id": "520300019"},
                },
            )

        fake_client.dispatch_eventsub_notification.assert_awaited_once_with(
            sub_type="channel.raid",
            message_id="msg-raid-missing-callback-1",
            payload={
                "subscription": {
                    "type": "channel.raid",
                    "condition": {"to_broadcaster_user_id": "520300019"},
                },
                "event": {
                    "to_broadcaster_user_id": "520300019",
                    "to_broadcaster_user_login": "derechtecoolys",
                    "from_broadcaster_user_id": "9901",
                    "from_broadcaster_user_login": "raider_login",
                    "viewers": 42,
                },
            },
        )

    async def test_dashboard_service_eventsub_bridge_preserves_subscription_condition_fields(
        self,
    ) -> None:
        captured: dict[str, object] = {}
        sentinel_handler = _FakeEventSubHandler()
        fake_client = _ForwardingBotApiClient()

        def _fake_build_v2_app(**kwargs):
            captured.update(kwargs)
            return _FakeDashboardApp()

        def _fake_secret(name: str, *args, **kwargs) -> str:
            del args, kwargs
            if name == "TWITCH_WEBHOOK_SECRET":
                return "webhook-secret"
            return ""

        with (
            patch(
                "bot.dashboard_service.app.analytics_db_fingerprint_details",
                return_value={"fingerprint": "local", "hostHash": "h", "databaseHash": "d", "portHash": "p"},
            ),
            patch("bot.dashboard_service.app.build_v2_app", side_effect=_fake_build_v2_app),
            patch("bot.dashboard_service.app.load_secret_value", side_effect=_fake_secret),
            patch(
                "bot.monitoring.eventsub_webhook.EventSubWebhookHandler",
                return_value=sentinel_handler,
            ),
            patch(
                "bot.dashboard_service.app.BotApiClient",
                return_value=fake_client,
            ),
        ):
            build_dashboard_service_app(
                internal_api_base_url="http://127.0.0.1:1234",
                internal_api_token="internal-token",
                internal_api_allow_non_loopback=False,
                internal_api_timeout_seconds=1.0,
                dashboard_token="dash-token",
                partner_token="partner-token",
                noauth=False,
                oauth_client_id="client-id",
                oauth_client_secret="client-secret",
                oauth_redirect_uri="https://example.com/callback",
                session_ttl_seconds=3600,
                legacy_stats_url="https://example.com/stats",
            )

        callback = sentinel_handler.callbacks["channel.follow"]
        await callback(
            "520300019",
            "derechtecoolys",
            {
                "broadcaster_user_id": "520300019",
                "broadcaster_user_login": "derechtecoolys",
                "user_id": "111",
                "user_login": "newfollower",
            },
            message_id="msg-follow-1",
            subscription={
                "type": "channel.follow",
                "condition": {
                    "broadcaster_user_id": "520300019",
                    "moderator_user_id": "999",
                },
            },
        )

        fake_client.dispatch_eventsub_notification.assert_awaited_once_with(
            sub_type="channel.follow",
            message_id="msg-follow-1",
            payload={
                "subscription": {
                    "type": "channel.follow",
                    "condition": {
                        "broadcaster_user_id": "520300019",
                        "moderator_user_id": "999",
                    },
                },
                "event": {
                    "broadcaster_user_id": "520300019",
                    "broadcaster_user_login": "derechtecoolys",
                    "user_id": "111",
                    "user_login": "newfollower",
                },
            },
        )

    async def test_dashboard_eventsub_bridge_dead_letters_after_max_attempts(self) -> None:
        fake_client = _MissingCallbackBotApiClient()
        store = _InMemoryBridgeStore()
        runtime = DashboardEventSubBridgeRuntime(
            client=fake_client,
            logger=log,
            store=store,
            now=lambda: 1000.0,
        )
        runtime._retry_delay_seconds = lambda _attempts: 0.0  # type: ignore[method-assign]

        await runtime.start()
        try:
            await runtime.dispatch_or_enqueue(
                sub_type="channel.raid",
                message_id="dead-letter-msg-1",
                payload={
                    "subscription": {
                        "type": "channel.raid",
                        "condition": {"to_broadcaster_user_id": "520300019"},
                    },
                    "event": {
                        "to_broadcaster_user_id": "520300019",
                        "to_broadcaster_user_login": "derechtecoolys",
                        "from_broadcaster_user_id": "9901",
                        "from_broadcaster_user_login": "raider_login",
                    },
                },
            )
            await self._wait_for_async_mock_awaits(
                fake_client.dispatch_eventsub_notification,
                minimum=5,
            )
            deadline = asyncio.get_running_loop().time() + 1.0
            while "dead-letter-msg-1" not in store.dead_letters:
                if asyncio.get_running_loop().time() >= deadline:
                    raise AssertionError("expected bridge message to be dead-lettered")
                await asyncio.sleep(0.02)
        finally:
            await runtime.stop()

        self.assertNotIn("dead-letter-msg-1", store.rows)
        self.assertEqual(
            store.dead_letters["dead-letter-msg-1"]["attempt_count"],
            5,
        )
        self.assertIn(
            "EventSub dispatch unavailable",
            str(store.dead_letters["dead-letter-msg-1"]["last_error"]),
        )

    async def test_dashboard_eventsub_bridge_does_not_spend_attempts_while_bot_dispatch_is_inactive(
        self,
    ) -> None:
        fake_client = _ForwardingBotApiClient()
        fake_client.dispatch_eventsub_notification = AsyncMock(
            side_effect=BotApiClientError(
                status=503,
                code="upstream_unavailable",
                message="eventsub notification dispatch inactive",
            )
        )
        store = _InMemoryBridgeStore()
        current_time = {"now": 1000.0}
        runtime = DashboardEventSubBridgeRuntime(
            client=fake_client,
            logger=log,
            store=store,
            now=lambda: current_time["now"],
        )
        await runtime.start()
        try:
            await runtime.dispatch_or_enqueue(
                sub_type="stream.offline",
                message_id="startup-msg-1",
                payload={
                    "subscription": {"type": "stream.offline"},
                    "event": {"broadcaster_user_id": "42"},
                },
            )
            self.assertIn("startup-msg-1", store.rows)
            self.assertEqual(store.rows["startup-msg-1"]["attempt_count"], 0)

            await runtime._process_due_batch()
            self.assertEqual(fake_client.dispatch_eventsub_notification.await_count, 1)
            self.assertEqual(store.rows["startup-msg-1"]["attempt_count"], 0)
            self.assertNotIn("startup-msg-1", store.dead_letters)

            current_time["now"] = float(store.rows["startup-msg-1"]["next_attempt_at"])
            await runtime._process_due_batch()
            self.assertEqual(fake_client.dispatch_eventsub_notification.await_count, 2)
            self.assertEqual(store.rows["startup-msg-1"]["attempt_count"], 0)
            self.assertNotIn("startup-msg-1", store.dead_letters)
            self.assertEqual(
                store.rows["startup-msg-1"]["last_error"],
                "eventsub notification dispatch inactive",
            )
        finally:
            await runtime.stop()

    async def test_dashboard_eventsub_bridge_treats_structured_inactive_response_as_startup_pending(
        self,
    ) -> None:
        fake_client = _ForwardingBotApiClient()
        fake_client.dispatch_eventsub_notification = AsyncMock(
            return_value={
                "ok": False,
                "message": "eventsub notification dispatch inactive",
            }
        )
        store = _InMemoryBridgeStore()
        current_time = {"now": 1000.0}
        runtime = DashboardEventSubBridgeRuntime(
            client=fake_client,
            logger=log,
            store=store,
            now=lambda: current_time["now"],
        )
        await runtime.start()
        try:
            await runtime.dispatch_or_enqueue(
                sub_type="stream.offline",
                message_id="startup-msg-structured-1",
                payload={
                    "subscription": {"type": "stream.offline"},
                    "event": {"broadcaster_user_id": "42"},
                },
            )
            self.assertIn("startup-msg-structured-1", store.rows)
            self.assertEqual(store.rows["startup-msg-structured-1"]["attempt_count"], 0)

            await runtime._process_due_batch()
            self.assertEqual(fake_client.dispatch_eventsub_notification.await_count, 1)
            self.assertEqual(store.rows["startup-msg-structured-1"]["attempt_count"], 0)
            self.assertNotIn("startup-msg-structured-1", store.dead_letters)
            self.assertEqual(
                store.rows["startup-msg-structured-1"]["last_error"],
                "eventsub notification dispatch inactive",
            )

            current_time["now"] = float(store.rows["startup-msg-structured-1"]["next_attempt_at"])
            await runtime._process_due_batch()
            self.assertEqual(fake_client.dispatch_eventsub_notification.await_count, 2)
            self.assertEqual(store.rows["startup-msg-structured-1"]["attempt_count"], 0)
            self.assertNotIn("startup-msg-structured-1", store.dead_letters)
        finally:
            await runtime.stop()

    async def test_dashboard_eventsub_bridge_counts_missing_webhook_handler_as_real_failure(
        self,
    ) -> None:
        fake_client = _ForwardingBotApiClient()
        fake_client.dispatch_eventsub_notification = AsyncMock(
            side_effect=BotApiClientError(
                status=503,
                code="upstream_unavailable",
                message="eventsub webhook handler unavailable",
            )
        )
        store = _InMemoryBridgeStore()
        current_time = {"now": 1000.0}
        runtime = DashboardEventSubBridgeRuntime(
            client=fake_client,
            logger=log,
            store=store,
            now=lambda: current_time["now"],
        )
        await runtime.start()
        try:
            await runtime.dispatch_or_enqueue(
                sub_type="stream.offline",
                message_id="missing-handler-msg-1",
                payload={
                    "subscription": {"type": "stream.offline"},
                    "event": {"broadcaster_user_id": "42"},
                },
            )
            self.assertIn("missing-handler-msg-1", store.rows)

            await runtime._process_due_batch()
            self.assertEqual(fake_client.dispatch_eventsub_notification.await_count, 1)
            self.assertEqual(store.rows["missing-handler-msg-1"]["attempt_count"], 1)
            self.assertNotIn("missing-handler-msg-1", store.dead_letters)
            self.assertEqual(
                store.rows["missing-handler-msg-1"]["last_error"],
                "eventsub webhook handler unavailable",
            )
            self.assertGreater(
                float(store.rows["missing-handler-msg-1"]["next_attempt_at"]),
                current_time["now"],
            )
        finally:
            await runtime.stop()

    async def test_dashboard_service_eventsub_callback_route_forwards_signed_request_and_deduplicates_replays(
        self,
    ) -> None:
        fake_client = _ForwardingBotApiClient()

        def _fake_secret(name: str, *args, **kwargs) -> str:
            del args, kwargs
            if name == "TWITCH_WEBHOOK_SECRET":
                return "webhook-secret"
            return ""

        with (
            patch(
                "bot.dashboard_service.app.analytics_db_fingerprint_details",
                return_value={"fingerprint": "local", "hostHash": "h", "databaseHash": "d", "portHash": "p"},
            ),
            patch("bot.dashboard_service.app.load_secret_value", side_effect=_fake_secret),
            patch("bot.dashboard_service.app.BotApiClient", return_value=fake_client),
            patch("bot.dashboard.server_v2.storage_pg.prepare_runtime_storage", return_value=None),
        ):
            app = build_dashboard_service_app(
                internal_api_base_url="http://127.0.0.1:1234",
                internal_api_token="internal-token",
                internal_api_allow_non_loopback=False,
                internal_api_timeout_seconds=1.0,
                dashboard_token="dash-token",
                partner_token="partner-token",
                noauth=False,
                oauth_client_id="client-id",
                oauth_client_secret="client-secret",
                oauth_redirect_uri="https://example.com/callback",
                session_ttl_seconds=3600,
                legacy_stats_url="https://example.com/stats",
            )

        body = {
            "subscription": {
                "type": "stream.offline",
                "condition": {"broadcaster_user_id": "520300019"},
            },
            "event": {
                "broadcaster_user_id": "520300019",
                "broadcaster_user_login": "derechtecoolys",
            },
        }
        headers = self._signed_eventsub_headers(
            secret="webhook-secret",
            body=body,
            message_id="msg-route-offline-1",
        )

        async with TestServer(app) as server:
            async with TestClient(server) as client:
                first = await client.post("/twitch/eventsub/callback", json=body, headers=headers)
                second = await client.post("/twitch/eventsub/callback", json=body, headers=headers)
                await self._wait_for_async_mock_awaits(
                    fake_client.dispatch_eventsub_notification,
                    minimum=1,
                )

        self.assertEqual(first.status, 204)
        self.assertEqual(second.status, 204)
        self.assertEqual(fake_client.dispatch_eventsub_notification.await_count, 1)
        only_call = fake_client.dispatch_eventsub_notification.await_args_list[0]
        self.assertEqual(only_call.kwargs["sub_type"], "stream.offline")
        self.assertEqual(only_call.kwargs["message_id"], "msg-route-offline-1")
        self.assertEqual(
            only_call.kwargs["payload"]["subscription"]["condition"]["broadcaster_user_id"],
            "520300019",
        )
        self.assertEqual(
            only_call.kwargs["payload"]["event"]["broadcaster_user_login"],
            "derechtecoolys",
        )

    async def test_dashboard_service_eventsub_callback_route_persists_retry_after_bridge_failure(
        self,
    ) -> None:
        fake_client = _MissingCallbackBotApiClient()

        def _fake_secret(name: str, *args, **kwargs) -> str:
            del args, kwargs
            if name == "TWITCH_WEBHOOK_SECRET":
                return "webhook-secret"
            return ""

        with (
            patch(
                "bot.dashboard_service.app.analytics_db_fingerprint_details",
                return_value={"fingerprint": "local", "hostHash": "h", "databaseHash": "d", "portHash": "p"},
            ),
            patch("bot.dashboard_service.app.load_secret_value", side_effect=_fake_secret),
            patch("bot.dashboard_service.app.BotApiClient", return_value=fake_client),
            patch("bot.dashboard.server_v2.storage_pg.prepare_runtime_storage", return_value=None),
        ):
            app = build_dashboard_service_app(
                internal_api_base_url="http://127.0.0.1:1234",
                internal_api_token="internal-token",
                internal_api_allow_non_loopback=False,
                internal_api_timeout_seconds=1.0,
                dashboard_token="dash-token",
                partner_token="partner-token",
                noauth=False,
                oauth_client_id="client-id",
                oauth_client_secret="client-secret",
                oauth_redirect_uri="https://example.com/callback",
                session_ttl_seconds=3600,
                legacy_stats_url="https://example.com/stats",
            )

        body = {
            "subscription": {
                "type": "channel.raid",
                "condition": {"to_broadcaster_user_id": "520300019"},
            },
            "event": {
                "to_broadcaster_user_id": "520300019",
                "to_broadcaster_user_login": "derechtecoolys",
                "from_broadcaster_user_id": "9901",
                "from_broadcaster_user_login": "raider_login",
                "viewers": 42,
            },
        }
        headers = self._signed_eventsub_headers(
            secret="webhook-secret",
            body=body,
            message_id="msg-route-raid-failure-1",
            subscription_type="channel.raid",
        )

        async with TestServer(app) as server:
            async with TestClient(server) as client:
                first = await client.post("/twitch/eventsub/callback", json=body, headers=headers)
                second = await client.post("/twitch/eventsub/callback", json=body, headers=headers)
                await self._wait_for_async_mock_awaits(
                    fake_client.dispatch_eventsub_notification,
                    minimum=1,
                )

        self.assertEqual(first.status, 204)
        self.assertEqual(second.status, 204)
        self.assertEqual(fake_client.dispatch_eventsub_notification.await_count, 1)
        runtime = app[DASHBOARD_EVENTSUB_BRIDGE_KEY]
        self.assertIn("msg-route-raid-failure-1", runtime._store.rows)
        self.assertEqual(
            runtime._store.rows["msg-route-raid-failure-1"]["attempt_count"],
            1,
        )
        for call in fake_client.dispatch_eventsub_notification.await_args_list:
            self.assertEqual(call.kwargs["sub_type"], "channel.raid")
            self.assertEqual(call.kwargs["message_id"], "msg-route-raid-failure-1")

    async def test_write_callbacks_raise_bot_api_error_when_internal_api_is_missing(self) -> None:
        captured: dict[str, object] = {}

        def _fake_build_v2_app(**kwargs):
            captured.update(kwargs)
            return _FakeDashboardApp()

        with patch("bot.dashboard_service.app.analytics_db_fingerprint_details", return_value={"fingerprint": "local", "hostHash": "h", "databaseHash": "d", "portHash": "p"}), patch("bot.dashboard_service.app.build_v2_app", side_effect=_fake_build_v2_app):
            build_dashboard_service_app(
                internal_api_base_url="http://127.0.0.1:1234",
                internal_api_token="",
                internal_api_allow_non_loopback=False,
                internal_api_timeout_seconds=1.0,
                dashboard_token="dash-token",
                partner_token="partner-token",
                noauth=False,
                oauth_client_id="client-id",
                oauth_client_secret="client-secret",
                oauth_redirect_uri="https://example.com/callback",
                session_ttl_seconds=3600,
                legacy_stats_url="https://example.com/stats",
            )

        services = captured["dashboard_services"]
        assert services is not None

        expectations = [
            (services.add_cb, ("partner_one", False)),
            (services.remove_cb, ("partner_one",)),
            (services.verify_cb, ("partner_one", "check")),
            (services.archive_cb, ("partner_one", "toggle")),
            (services.discord_flag_cb, ("partner_one", True)),
            (services.discord_profile_cb, ("partner_one", "123", "Partner One", True)),
        ]
        for callback, args in expectations:
            with self.subTest(callback=getattr(callback, "__name__", repr(callback))):
                with self.assertRaises(BotApiClientError) as ctx:
                    await callback(*args)
                self.assertEqual(ctx.exception.status, 503)
                self.assertEqual(ctx.exception.code, "upstream_unavailable")

    async def test_raid_callbacks_raise_bot_api_error_when_internal_api_is_missing(self) -> None:
        captured: dict[str, object] = {}

        def _fake_build_v2_app(**kwargs):
            captured.update(kwargs)
            return _FakeDashboardApp()

        with patch(
            "bot.dashboard_service.app.analytics_db_fingerprint_details",
            return_value={"fingerprint": "local", "hostHash": "h", "databaseHash": "d", "portHash": "p"},
        ), patch("bot.dashboard_service.app.build_v2_app", side_effect=_fake_build_v2_app):
            build_dashboard_service_app(
                internal_api_base_url="http://127.0.0.1:1234",
                internal_api_token="",
                internal_api_allow_non_loopback=False,
                internal_api_timeout_seconds=1.0,
                dashboard_token="dash-token",
                partner_token="partner-token",
                noauth=False,
                oauth_client_id="client-id",
                oauth_client_secret="client-secret",
                oauth_redirect_uri="https://example.com/callback",
                session_ttl_seconds=3600,
                legacy_stats_url="https://example.com/stats",
            )

        services = captured["dashboard_services"]
        assert services is not None

        expectations = [
            (services.raid_go_url_cb, ("state-token",)),
            (services.raid_requirements_cb, ("partner_one",)),
        ]
        for callback, args in expectations:
            with self.subTest(callback=getattr(callback, "__name__", repr(callback))):
                with self.assertRaises(BotApiClientError) as ctx:
                    await callback(*args)
                self.assertEqual(ctx.exception.status, 503)
                self.assertEqual(ctx.exception.code, "upstream_unavailable")

    async def test_raid_callbacks_raise_bot_api_error_when_upstream_client_fails(self) -> None:
        captured: dict[str, object] = {}

        def _fake_build_v2_app(**kwargs):
            captured.update(kwargs)
            return _FakeDashboardApp()

        with patch(
            "bot.dashboard_service.app.analytics_db_fingerprint_details",
            return_value={"fingerprint": "local", "hostHash": "h", "databaseHash": "d", "portHash": "p"},
        ), patch("bot.dashboard_service.app.build_v2_app", side_effect=_fake_build_v2_app), patch(
            "bot.dashboard_service.app.BotApiClient",
            _UpstreamFailingBotApiClient,
        ):
            build_dashboard_service_app(
                internal_api_base_url="http://127.0.0.1:1234",
                internal_api_token="internal-token",
                internal_api_allow_non_loopback=False,
                internal_api_timeout_seconds=1.0,
                dashboard_token="dash-token",
                partner_token="partner-token",
                noauth=False,
                oauth_client_id="client-id",
                oauth_client_secret="client-secret",
                oauth_redirect_uri="https://example.com/callback",
                session_ttl_seconds=3600,
                legacy_stats_url="https://example.com/stats",
            )

        services = captured["dashboard_services"]
        assert services is not None

        expectations = [
            (services.raid_go_url_cb, ("state-token",)),
            (services.raid_requirements_cb, ("partner_one",)),
        ]
        for callback, args in expectations:
            with self.subTest(callback=getattr(callback, "__name__", repr(callback))):
                with self.assertRaises(BotApiClientError) as ctx:
                    await callback(*args)
                self.assertEqual(ctx.exception.status, 503)
                self.assertEqual(ctx.exception.code, "upstream_unavailable")

    async def test_live_write_routes_redirect_to_err_on_bot_api_error(self) -> None:
        upstream_error = BotApiClientError(
            status=503,
            code="upstream_unavailable",
            message="Bot internal API is unavailable.",
        )
        cases = [
            ("add_any", {"q": "partner_one"}),
            ("add_url", {"url": "partner_one"}),
            ("add_login", {"login": "partner_one"}),
            ("add_streamer", {"login": "partner_one", "discord_user_id": "123", "discord_display_name": "Partner One", "member_flag": "on"}),
            ("discord_flag", {"login": "partner_one", "mode": "on"}),
            ("discord_link", {"login": "partner_one", "discord_user_id": "123", "discord_display_name": "Partner One", "member_flag": "on"}),
            ("remove", {"login": "partner_one"}),
            ("verify", {"login": "partner_one", "mode": "check"}),
            ("archive", {"login": "partner_one", "mode": "toggle"}),
        ]

        for method_name, payload in cases:
            handler = _DummyLiveWriteHandler(payload=payload, upstream_error=upstream_error)
            request = SimpleNamespace(match_info={})
            with self.subTest(method=method_name):
                with self.assertRaises(web.HTTPFound) as ctx:
                    await getattr(handler, method_name)(request)
                params = _query_params(ctx.exception.location)
                self.assertIn("err", params)
                self.assertNotIn("ok", params)

    async def test_entry_discord_link_redirects_to_err_on_bot_api_error(self) -> None:
        upstream_error = BotApiClientError(
            status=503,
            code="upstream_unavailable",
            message="Bot internal API is unavailable.",
        )
        server = _DummyEntryServer(upstream_error=upstream_error)
        request = SimpleNamespace(
            post=lambda: SimpleNamespace(),
            path="/twitch/discord_link",
        )

        async def _post():
            return {
                "csrf_token": "token",
                "login": "partner_one",
                "discord_user_id": "123",
                "discord_display_name": "Partner One",
                "member_flag": "on",
            }

        request.post = _post
        deps = EntryRouteDeps(
            critical_scopes=(),
            dashboard_v2_login_url="/twitch/auth/login",
            dashboards_discord_login_url="/twitch/auth/discord/login",
            dashboards_login_url="/twitch/auth/login",
            html=html,
            json=json,
            log=log,
            required_scopes=(),
            scope_column_labels={},
            storage=storage,
        )

        with self.assertRaises(web.HTTPFound) as ctx:
            await entry_discord_link(server, request, deps=deps)

        params = _query_params(ctx.exception.location)
        self.assertIn("err", params)
        self.assertNotIn("ok", params)


if __name__ == "__main__":
    unittest.main()
