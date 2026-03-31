from __future__ import annotations

import unittest

from aiohttp.test_utils import TestClient, TestServer

from bot.internal_api import (
    INTERNAL_API_BASE_PATH,
    INTERNAL_TOKEN_HEADER,
    build_internal_api_app,
)


class _FailingEventSubTelemetryHarness:
    async def eventsub_processing_debug(self, *, limit: int = 20) -> dict[str, object]:
        del limit
        raise RuntimeError("eventsub processing debug failed token=super-secret")

    async def eventsub_processing_requeue(self, work_id: str) -> dict[str, object]:
        del work_id
        raise RuntimeError("eventsub requeue failed token=super-secret")


class InternalApiEventSubTelemetryFailureTests(unittest.IsolatedAsyncioTestCase):
    async def test_eventsub_processing_debug_runtime_error_is_sanitized(self) -> None:
        harness = _FailingEventSubTelemetryHarness()
        app = build_internal_api_app(
            token="secret-token",
            eventsub_processing_debug_cb=harness.eventsub_processing_debug,
            eventsub_processing_requeue_cb=harness.eventsub_processing_requeue,
        )

        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    f"{INTERNAL_API_BASE_PATH}/debug/eventsub-processing?limit=20",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                )
                payload = await response.json()

        self.assertEqual(response.status, 500)
        self.assertEqual(payload.get("error"), "internal_error")
        self.assertEqual(payload.get("message"), "failed to build eventsub processing payload")

    async def test_eventsub_processing_requeue_runtime_error_is_sanitized(self) -> None:
        harness = _FailingEventSubTelemetryHarness()
        app = build_internal_api_app(
            token="secret-token",
            eventsub_processing_debug_cb=harness.eventsub_processing_debug,
            eventsub_processing_requeue_cb=harness.eventsub_processing_requeue,
        )

        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.post(
                    f"{INTERNAL_API_BASE_PATH}/eventsub/processing/requeue",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                    json={"work_id": "work-1"},
                )
                payload = await response.json()

        self.assertEqual(response.status, 500)
        self.assertEqual(payload.get("error"), "internal_error")
        self.assertEqual(payload.get("message"), "failed to requeue eventsub processing entry")
