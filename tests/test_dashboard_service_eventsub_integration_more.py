from __future__ import annotations

import asyncio
import hashlib
import hmac
import json
import threading
import unittest
from datetime import UTC, datetime
from unittest.mock import patch

from aiohttp.test_utils import TestClient, TestServer

from bot.dashboard_service.app import build_dashboard_service_app
from bot.dashboard_service.eventsub_bridge import DashboardEventSubBridgeRuntime
from bot.internal_api import build_internal_api_app
from bot.monitoring.eventsub_state_store import EventSubStateStore
from tests.eventsub_state_store_test_helpers import InMemoryEventSubStateRepository


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


class _InMemoryBridgeStore:
    def __init__(self) -> None:
        self._lock = threading.Lock()
        self.rows: dict[str, dict[str, object]] = {}
        self.dead_letters: dict[str, dict[str, object]] = {}

    def enqueue(self, *, message_id: str, sub_type: str, payload: dict[str, object], now: float) -> bool:
        with self._lock:
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
        with self._lock:
            for row in sorted(self.rows.values(), key=lambda item: float(item.get("queued_at") or 0.0)):
                if len(leased) >= limit:
                    break
                if float(row.get("next_attempt_at") or 0.0) > float(now):
                    continue
                leased.append(dict(row))
                row["next_attempt_at"] = float(now) + 60.0
        return leased

    def mark_delivered(self, *, message_id: str) -> None:
        with self._lock:
            self.rows.pop(message_id, None)

    def mark_retry(
        self,
        *,
        message_id: str,
        attempt_count: int,
        error_message: str,
        next_attempt_at: float,
    ) -> None:
        with self._lock:
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
        with self._lock:
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
        with self._lock:
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


class DashboardServiceEventSubIntegrationMoreTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        self._stores: list[_InMemoryBridgeStore] = []
        self._runtimes: list[DashboardEventSubBridgeRuntime] = []

    @staticmethod
    async def _wait_for(
        predicate,
        *,
        timeout: float = 8.0,
        interval: float = 0.05,
        failure_message: str = "condition was not met in time",
    ) -> None:
        deadline = asyncio.get_running_loop().time() + timeout
        while not predicate():
            if asyncio.get_running_loop().time() >= deadline:
                raise AssertionError(failure_message)
            await asyncio.sleep(interval)

    def _bridge_runtime_factory(self, *args, **kwargs) -> DashboardEventSubBridgeRuntime:
        store = _InMemoryBridgeStore()
        runtime = DashboardEventSubBridgeRuntime(
            *args,
            store=store,
            **kwargs,
        )
        runtime._retry_delay_seconds = lambda _attempts: 0.01  # type: ignore[method-assign]
        self._stores.append(store)
        self._runtimes.append(runtime)
        return runtime

    async def test_duplicate_callback_recovers_from_startup_pending_and_delivers_once(self) -> None:
        ready = {"value": False}
        attempts = {"count": 0}
        delivered: list[dict[str, object]] = []

        async def _eventsub_dispatch_cb(
            *,
            sub_type: str,
            message_id: str | None,
            payload: dict[str, object],
        ) -> dict[str, object]:
            attempts["count"] += 1
            if not ready["value"]:
                raise RuntimeError("eventsub notification dispatch inactive")
            delivered.append(
                {
                    "sub_type": sub_type,
                    "message_id": message_id,
                    "payload": payload,
                }
            )
            return {"ok": True}

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
            patch("bot.dashboard_service.eventsub_bridge._OUTBOX_STARTUP_WAIT_SECONDS", 0.01),
            patch("bot.dashboard_service.eventsub_bridge._OUTBOX_IDLE_WAIT_SECONDS", 0.01),
        ):
            internal_app = build_internal_api_app(
                token="secret-token",
                eventsub_dispatch_cb=_eventsub_dispatch_cb,
            )
            async with TestServer(internal_app) as internal_server:
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

                async with TestServer(dashboard_app) as dashboard_server:
                    async with TestClient(dashboard_server) as client:
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
                            message_id="msg-integration-recovery-duplicate-1",
                        )

                        first = await client.post("/twitch/eventsub/callback", json=body, headers=headers)
                        second = await client.post("/twitch/eventsub/callback", json=body, headers=headers)

                        await self._wait_for(
                            lambda: bool(self._stores)
                            and "msg-integration-recovery-duplicate-1" in self._stores[0].rows,
                            failure_message="expected startup-pending message to remain queued",
                        )

                        ready["value"] = True
                        self._runtimes[0]._wakeup.set()

                        await self._wait_for(
                            lambda: len(delivered) == 1
                            and "msg-integration-recovery-duplicate-1" not in self._stores[0].rows,
                            failure_message="expected duplicate startup-pending event to recover once",
                        )

        self.assertEqual(first.status, 204)
        self.assertEqual(second.status, 204)
        self.assertGreaterEqual(attempts["count"], 2)
        self.assertEqual(len(delivered), 1)
        self.assertEqual(delivered[0]["message_id"], "msg-integration-recovery-duplicate-1")
        self.assertEqual(delivered[0]["sub_type"], "stream.offline")

    async def test_shared_replay_state_does_not_drop_bridged_stream_offline(self) -> None:
        delivered: list[dict[str, object]] = []
        shared_repository = InMemoryEventSubStateRepository()

        async def _eventsub_dispatch_cb(
            *,
            sub_type: str,
            message_id: str | None,
            payload: dict[str, object],
        ) -> dict[str, object]:
            delivered.append(
                {
                    "sub_type": sub_type,
                    "message_id": message_id,
                    "payload": payload,
                }
            )
            return {"ok": True}

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
                    repository=shared_repository,
                    logger=kwargs.get("logger"),
                ),
            ),
            patch(
                "bot.dashboard_service.app.DashboardEventSubBridgeRuntime",
                side_effect=self._bridge_runtime_factory,
            ),
            patch("bot.dashboard.server_v2.storage_pg.prepare_runtime_storage", return_value=None),
            patch("bot.dashboard.server_v2.DashboardV2Server.attach", return_value=None),
            patch("bot.dashboard_service.eventsub_bridge._OUTBOX_IDLE_WAIT_SECONDS", 0.01),
        ):
            internal_app = build_internal_api_app(
                token="secret-token",
                eventsub_dispatch_cb=_eventsub_dispatch_cb,
            )
            async with TestServer(internal_app) as internal_server:
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

                async with TestServer(dashboard_app) as dashboard_server:
                    async with TestClient(dashboard_server) as client:
                        body = {
                            "subscription": {
                                "type": "stream.offline",
                                "condition": {"broadcaster_user_id": "993954638"},
                            },
                            "event": {
                                "broadcaster_user_id": "993954638",
                                "broadcaster_user_login": "denoshock",
                            },
                        }
                        headers = _signed_eventsub_headers(
                            secret="webhook-secret",
                            body=body,
                            message_id="msg-integration-shared-replay-1",
                        )

                        response = await client.post("/twitch/eventsub/callback", json=body, headers=headers)

                        await self._wait_for(
                            lambda: len(delivered) == 1,
                            failure_message="expected bridged stream.offline event to reach bot exactly once",
                        )

        self.assertEqual(response.status, 204)
        self.assertEqual(len(delivered), 1)
        self.assertEqual(delivered[0]["sub_type"], "stream.offline")
        self.assertEqual(delivered[0]["message_id"], "msg-integration-shared-replay-1")
        self.assertEqual(
            delivered[0]["payload"].get("event", {}).get("broadcaster_user_login"),
            "denoshock",
        )

    async def test_callback_retries_until_dead_letter_over_real_http(self) -> None:
        attempts = {"count": 0}

        async def _eventsub_dispatch_cb(
            *,
            sub_type: str,
            message_id: str | None,
            payload: dict[str, object],
        ) -> dict[str, object]:
            del sub_type, message_id, payload
            attempts["count"] += 1
            raise RuntimeError("eventsub webhook handler unavailable")

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
            patch("bot.dashboard_service.eventsub_bridge._OUTBOX_IDLE_WAIT_SECONDS", 0.01),
        ):
            internal_app = build_internal_api_app(
                token="secret-token",
                eventsub_dispatch_cb=_eventsub_dispatch_cb,
            )
            async with TestServer(internal_app) as internal_server:
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

                async with TestServer(dashboard_app) as dashboard_server:
                    async with TestClient(dashboard_server) as client:
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
                            message_id="msg-integration-dead-letter-1",
                        )

                        response = await client.post("/twitch/eventsub/callback", json=body, headers=headers)

                        await self._wait_for(
                            lambda: bool(self._stores)
                            and "msg-integration-dead-letter-1" in self._stores[0].dead_letters,
                            failure_message="expected repeated dispatch failures to dead-letter the message",
                        )

        self.assertEqual(response.status, 204)
        self.assertEqual(attempts["count"], 5)
        self.assertNotIn("msg-integration-dead-letter-1", self._stores[0].rows)
        self.assertEqual(
            self._stores[0].dead_letters["msg-integration-dead-letter-1"]["attempt_count"],
            5,
        )
        self.assertIn(
            "eventsub webhook handler unavailable",
            str(self._stores[0].dead_letters["msg-integration-dead-letter-1"]["last_error"]),
        )
