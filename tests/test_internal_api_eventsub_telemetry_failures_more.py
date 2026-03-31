from __future__ import annotations

import unittest

from aiohttp.test_utils import TestClient, TestServer

from bot.internal_api import (
    INTERNAL_API_BASE_PATH,
    INTERNAL_TOKEN_HEADER,
    build_internal_api_app,
)


class _TelemetryShapeHarness:
    def __init__(self) -> None:
        self.observability_calls = 0
        self.eventsub_processing_limits: list[int] = []
        self.requeue_work_ids: list[str] = []

    async def observability_snapshot(self) -> list[object]:
        self.observability_calls += 1
        return [{"kind": "eventsub", "state": "degraded"}]

    async def eventsub_processing_debug(self, *, limit: int = 20) -> tuple[str, int]:
        self.eventsub_processing_limits.append(limit)
        return ("eventsub", limit)

    async def eventsub_processing_requeue(self, work_id: str) -> list[object]:
        self.requeue_work_ids.append(work_id)
        return ["requeued", work_id]


class InternalApiEventSubTelemetryFailureMoreTests(unittest.IsolatedAsyncioTestCase):
    def _build_app(self, harness: _TelemetryShapeHarness):
        return build_internal_api_app(
            token="secret-token",
            observability_snapshot_cb=harness.observability_snapshot,
            eventsub_processing_debug_cb=harness.eventsub_processing_debug,
            eventsub_processing_requeue_cb=harness.eventsub_processing_requeue,
        )

    async def test_eventsub_processing_debug_accepts_default_blank_trimmed_and_boundary_limits(
        self,
    ) -> None:
        harness = _TelemetryShapeHarness()
        app = self._build_app(harness)

        async with TestServer(app) as server:
            async with TestClient(server) as client:
                cases = [
                    ("missing", f"{INTERNAL_API_BASE_PATH}/debug/eventsub-processing", 20),
                    ("blank", f"{INTERNAL_API_BASE_PATH}/debug/eventsub-processing?limit=", 20),
                    ("trimmed", f"{INTERNAL_API_BASE_PATH}/debug/eventsub-processing?limit=%20%209%20", 9),
                    ("lower-bound", f"{INTERNAL_API_BASE_PATH}/debug/eventsub-processing?limit=1", 1),
                    ("upper-bound", f"{INTERNAL_API_BASE_PATH}/debug/eventsub-processing?limit=200", 200),
                ]
                for name, path, expected_limit in cases:
                    with self.subTest(case=name):
                        response = await client.get(
                            path,
                            headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                        )
                        payload = await response.json()
                        self.assertEqual(response.status, 200)
                        self.assertEqual(
                            payload.get("eventsubProcessing"),
                            {"value": ["eventsub", expected_limit]},
                        )

        self.assertEqual(harness.eventsub_processing_limits, [20, 20, 9, 1, 200])

    async def test_eventsub_processing_debug_rejects_out_of_range_limit_values(self) -> None:
        harness = _TelemetryShapeHarness()
        app = self._build_app(harness)

        async with TestServer(app) as server:
            async with TestClient(server) as client:
                for raw_limit in ("0", "201", "-1", "9999"):
                    with self.subTest(raw_limit=raw_limit):
                        response = await client.get(
                            f"{INTERNAL_API_BASE_PATH}/debug/eventsub-processing?limit={raw_limit}",
                            headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                        )
                        payload = await response.json()
                        self.assertEqual(response.status, 400)
                        self.assertEqual(payload.get("error"), "bad_request")
                        self.assertEqual(payload.get("message"), "invalid request")

    async def test_eventsub_processing_debug_rejects_non_integer_limit_values(self) -> None:
        harness = _TelemetryShapeHarness()
        app = self._build_app(harness)

        async with TestServer(app) as server:
            async with TestClient(server) as client:
                for raw_limit in ("abc", "1.5", "NaN"):
                    with self.subTest(raw_limit=raw_limit):
                        response = await client.get(
                            f"{INTERNAL_API_BASE_PATH}/debug/eventsub-processing?limit={raw_limit}",
                            headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                        )
                        payload = await response.json()
                        self.assertEqual(response.status, 400)
                        self.assertEqual(payload.get("error"), "bad_request")
                        self.assertEqual(payload.get("message"), "invalid request")

    async def test_observability_debug_wraps_non_dict_snapshot_payload(self) -> None:
        harness = _TelemetryShapeHarness()
        app = self._build_app(harness)

        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    f"{INTERNAL_API_BASE_PATH}/debug/observability",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                )
                payload = await response.json()

        self.assertEqual(response.status, 200)
        self.assertTrue(payload.get("ok"))
        self.assertEqual(
            payload.get("observability"),
            {"value": [{"kind": "eventsub", "state": "degraded"}]},
        )

    async def test_eventsub_processing_debug_wraps_non_dict_payload(self) -> None:
        harness = _TelemetryShapeHarness()
        app = self._build_app(harness)

        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    f"{INTERNAL_API_BASE_PATH}/debug/eventsub-processing?limit=25",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                )
                payload = await response.json()

        self.assertEqual(response.status, 200)
        self.assertEqual(
            payload.get("eventsubProcessing"),
            {"value": ["eventsub", 25]},
        )

    async def test_eventsub_processing_requeue_uses_fallback_shape_when_callback_returns_non_dict(
        self,
    ) -> None:
        harness = _TelemetryShapeHarness()
        app = self._build_app(harness)

        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.post(
                    f"{INTERNAL_API_BASE_PATH}/eventsub/processing/requeue",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                    json={"work_id": "dead-7"},
                )
                payload = await response.json()

        self.assertEqual(response.status, 200)
        self.assertEqual(
            payload,
            {
                "ok": True,
                "workId": "dead-7",
                "requeued": True,
            },
        )
        self.assertEqual(harness.requeue_work_ids, ["dead-7"])

    async def test_eventsub_processing_requeue_rejects_blank_work_id(self) -> None:
        harness = _TelemetryShapeHarness()
        app = self._build_app(harness)

        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.post(
                    f"{INTERNAL_API_BASE_PATH}/eventsub/processing/requeue",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                    json={"work_id": "   "},
                )
                payload = await response.json()

        self.assertEqual(response.status, 400)
        self.assertEqual(payload.get("error"), "bad_request")
        self.assertEqual(payload.get("message"), "invalid request body")
        self.assertEqual(harness.requeue_work_ids, [])
