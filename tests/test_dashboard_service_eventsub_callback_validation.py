from __future__ import annotations

import hashlib
import hmac
import json
import unittest
from datetime import UTC, datetime
from unittest.mock import patch

from aiohttp.test_utils import TestClient, TestServer

from bot.dashboard_service.app import build_dashboard_service_app
from bot.internal_api import build_internal_api_app
from bot.monitoring.eventsub_state_store import EventSubStateStore
from tests.eventsub_state_store_test_helpers import InMemoryEventSubStateRepository


def _signed_eventsub_headers(
    *,
    secret: str,
    body: dict[str, object],
    message_id: str,
    timestamp: str | None = None,
    message_type: str = "notification",
    subscription_type: str = "stream.offline",
) -> dict[str, str]:
    raw_body = json.dumps(body).encode("utf-8")
    timestamp = timestamp or datetime.now(UTC).isoformat(timespec="seconds").replace("+00:00", "Z")
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


class _RecordingBridgeRuntime:
    def __init__(self, *args, **kwargs) -> None:
        del args, kwargs
        self.start_calls = 0
        self.stop_calls = 0
        self.dispatch_calls: list[dict[str, object]] = []

    @property
    def active(self) -> bool:
        return self.start_calls > self.stop_calls

    async def start(self) -> None:
        self.start_calls += 1

    async def stop(self) -> None:
        self.stop_calls += 1

    async def dispatch_or_enqueue(
        self,
        *,
        sub_type: str,
        payload: dict[str, object],
        message_id: str | None,
    ) -> None:
        self.dispatch_calls.append(
            {
                "sub_type": sub_type,
                "payload": payload,
                "message_id": message_id,
            }
        )


class DashboardServiceEventSubCallbackValidationTests(unittest.IsolatedAsyncioTestCase):
    def _bridge_runtime_factory(self, *args, **kwargs) -> _RecordingBridgeRuntime:
        del args, kwargs
        runtime = _RecordingBridgeRuntime()
        self._bridge_runtime = runtime
        return runtime

    async def _build_app_pair(
        self,
        *,
        eventsub_dispatch_cb,
    ) -> tuple[TestServer, TestServer, _RecordingBridgeRuntime]:
        self._bridge_runtime = _RecordingBridgeRuntime()

        with (
            patch(
                "bot.internal_api.app.analytics_db_fingerprint_details",
                return_value={"fingerprint": "local", "hostHash": "h", "databaseHash": "d", "portHash": "p"},
            ),
            patch(
                "bot.dashboard_service.app.analytics_db_fingerprint_details",
                return_value={"fingerprint": "local", "hostHash": "h", "databaseHash": "d", "portHash": "p"},
            ),
            patch("bot.dashboard_service.app.load_secret_value", return_value="webhook-secret"),
            patch(
                "bot.monitoring.eventsub_state_store.EventSubStateStore",
                side_effect=lambda *args, **kwargs: EventSubStateStore(
                    repository=InMemoryEventSubStateRepository(),
                    logger=kwargs.get("logger"),
                ),
            ),
            patch(
                "bot.dashboard_service.app.DashboardEventSubBridgeRuntime",
                side_effect=self._bridge_runtime_factory,
            ),
            patch("bot.dashboard.server_v2.storage_pg.prepare_runtime_storage", return_value=None),
            patch("bot.dashboard.server_v2.DashboardV2Server.attach", return_value=None),
        ):
            internal_app = build_internal_api_app(
                token="secret-token",
                eventsub_dispatch_cb=eventsub_dispatch_cb,
            )
            internal_server = TestServer(internal_app)
            await internal_server.start_server()
            try:
                dashboard_app = build_dashboard_service_app(
                    internal_api_base_url=str(internal_server.make_url("/")).rstrip("/"),
                    internal_api_token="secret-token",
                    internal_api_allow_non_loopback=False,
                    internal_api_timeout_seconds=2.0,
                    dashboard_token="dash-token",
                    partner_token="partner-token",
                    noauth=False,
                    oauth_client_id="client-id",
                    oauth_client_secret="client-secret",
                    oauth_redirect_uri="https://example.com/callback",
                    session_ttl_seconds=3600,
                    legacy_stats_url="https://example.com/stats",
                )
                dashboard_server = TestServer(dashboard_app)
                await dashboard_server.start_server()
            except Exception:
                await internal_server.close()
                raise

        return internal_server, dashboard_server, self._bridge_runtime

    async def test_missing_twitch_headers_returns_400_and_never_forwards(self) -> None:
        seen: list[dict[str, object]] = []

        async def _eventsub_dispatch_cb(
            *,
            sub_type: str,
            message_id: str | None,
            payload: dict[str, object],
        ) -> dict[str, object]:
            seen.append(
                {
                    "sub_type": sub_type,
                    "message_id": message_id,
                    "payload": payload,
                }
            )
            return {"ok": True}

        internal_server, dashboard_server, bridge_runtime = await self._build_app_pair(
            eventsub_dispatch_cb=_eventsub_dispatch_cb,
        )
        try:
            async with TestClient(dashboard_server) as client:
                response = await client.post(
                    "/twitch/eventsub/callback",
                    json={
                        "subscription": {
                            "type": "stream.offline",
                            "condition": {"broadcaster_user_id": "520300019"},
                        },
                        "event": {
                            "broadcaster_user_id": "520300019",
                            "broadcaster_user_login": "derechtecoolys",
                        },
                    },
                )
        finally:
            await dashboard_server.close()
            await internal_server.close()

        self.assertEqual(response.status, 400)
        self.assertEqual(seen, [])
        self.assertEqual(bridge_runtime.dispatch_calls, [])

    async def test_invalid_signature_returns_403_and_never_forwards(self) -> None:
        seen: list[dict[str, object]] = []

        async def _eventsub_dispatch_cb(
            *,
            sub_type: str,
            message_id: str | None,
            payload: dict[str, object],
        ) -> dict[str, object]:
            seen.append(
                {
                    "sub_type": sub_type,
                    "message_id": message_id,
                    "payload": payload,
                }
            )
            return {"ok": True}

        internal_server, dashboard_server, bridge_runtime = await self._build_app_pair(
            eventsub_dispatch_cb=_eventsub_dispatch_cb,
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
        headers = _signed_eventsub_headers(
            secret="webhook-secret",
            body=body,
            message_id="msg-invalid-signature-1",
        )
        headers["Twitch-Eventsub-Message-Signature"] = "sha256=deadbeef"

        try:
            async with TestClient(dashboard_server) as client:
                response = await client.post(
                    "/twitch/eventsub/callback",
                    json=body,
                    headers=headers,
                )
        finally:
            await dashboard_server.close()
            await internal_server.close()

        self.assertEqual(response.status, 403)
        self.assertEqual(seen, [])
        self.assertEqual(bridge_runtime.dispatch_calls, [])

    async def test_replayed_valid_message_id_is_not_forwarded_again(self) -> None:
        async def _eventsub_dispatch_cb(
            *,
            sub_type: str,
            message_id: str | None,
            payload: dict[str, object],
        ) -> dict[str, object]:
            del sub_type, message_id, payload
            return {"ok": True}

        internal_server, dashboard_server, bridge_runtime = await self._build_app_pair(
            eventsub_dispatch_cb=_eventsub_dispatch_cb,
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
        headers = _signed_eventsub_headers(
            secret="webhook-secret",
            body=body,
            message_id="msg-replay-1",
        )

        try:
            async with TestClient(dashboard_server) as client:
                first = await client.post(
                    "/twitch/eventsub/callback",
                    json=body,
                    headers=headers,
                )
                second = await client.post(
                    "/twitch/eventsub/callback",
                    json=body,
                    headers=headers,
                )
        finally:
            await dashboard_server.close()
            await internal_server.close()

        self.assertEqual(first.status, 204)
        self.assertEqual(second.status, 204)
        self.assertEqual(len(bridge_runtime.dispatch_calls), 1)
        self.assertEqual(bridge_runtime.dispatch_calls[0]["message_id"], "msg-replay-1")
        self.assertEqual(bridge_runtime.dispatch_calls[0]["sub_type"], "stream.offline")

    async def test_future_timestamp_is_rejected_without_forwarding(self) -> None:
        seen: list[dict[str, object]] = []

        async def _eventsub_dispatch_cb(
            *,
            sub_type: str,
            message_id: str | None,
            payload: dict[str, object],
        ) -> dict[str, object]:
            seen.append(
                {
                    "sub_type": sub_type,
                    "message_id": message_id,
                    "payload": payload,
                }
            )
            return {"ok": True}

        internal_server, dashboard_server, bridge_runtime = await self._build_app_pair(
            eventsub_dispatch_cb=_eventsub_dispatch_cb,
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
        headers = _signed_eventsub_headers(
            secret="webhook-secret",
            body=body,
            message_id="msg-future-ts-1",
            timestamp="2100-01-01T00:00:00Z",
        )

        try:
            async with TestClient(dashboard_server) as client:
                response = await client.post(
                    "/twitch/eventsub/callback",
                    json=body,
                    headers=headers,
                )
        finally:
            await dashboard_server.close()
            await internal_server.close()

        self.assertEqual(response.status, 403)
        self.assertEqual(seen, [])
        self.assertEqual(bridge_runtime.dispatch_calls, [])

