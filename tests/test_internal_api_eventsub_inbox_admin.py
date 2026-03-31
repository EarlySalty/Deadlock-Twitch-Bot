import unittest

from aiohttp.test_utils import TestClient, TestServer

from bot.internal_api import (
    INTERNAL_API_BASE_PATH,
    INTERNAL_TOKEN_HEADER,
    InternalApiCallbacks,
    build_internal_api_app,
)


class _EventSubInboxAdminHarness:
    def __init__(self) -> None:
        self.pending_inbox: list[dict[str, object]] = [
            {
                "work_id": "work-1",
                "work_type": "stream.offline",
                "message_id": "msg-1",
                "attempt_count": 0,
                "payload": {"broadcaster_id": "1234"},
            }
        ]
        self.dead_letters: list[dict[str, object]] = [
            {
                "work_id": "dead-1",
                "work_type": "channel.raid",
                "message_id": "msg-dead-1",
                "attempt_count": 5,
                "payload": {
                    "subscription": {"type": "channel.raid"},
                    "event": {"to_broadcaster_user_id": "1234"},
                },
            }
        ]
        self.requeued: list[dict[str, object]] = []

    async def observability_snapshot(self) -> dict[str, object]:
        return {
            "eventsubProcessing": {
                "pendingCount": len(self.pending_inbox),
                "deadLetterCount": len(self.dead_letters),
                "pending": list(self.pending_inbox),
                "deadLetters": list(self.dead_letters),
            }
        }

    async def eventsub_processing_debug(self, *, limit: int = 20) -> dict[str, object]:
        del limit
        return {
            "pendingCount": len(self.pending_inbox),
            "deadLetterCount": len(self.dead_letters),
            "pending": list(self.pending_inbox),
            "deadLetters": list(self.dead_letters),
        }

    async def eventsub_processing_requeue(
        self,
        work_id: str,
    ) -> dict[str, object]:
        match = next(
            (row for row in self.dead_letters if row["work_id"] == work_id),
            None,
        )
        if match is None:
            raise ValueError("dead-letter record not found")
        self.dead_letters.remove(match)
        requeued = {
            "work_id": str(work_id),
            "work_type": str(match["work_type"]),
            "message_id": str(match["message_id"]),
            "payload": dict(match["payload"]),
        }
        self.pending_inbox.append(requeued)
        self.requeued.append(requeued)
        return {"ok": True, "requeued": True, "workId": work_id}


class InternalApiEventSubInboxAdminTests(unittest.IsolatedAsyncioTestCase):
    async def test_observability_debug_lists_pending_inbox_and_dead_letters(self) -> None:
        harness = _EventSubInboxAdminHarness()
        app = build_internal_api_app(
            token="secret-token",
            callbacks=InternalApiCallbacks(
                observability_snapshot=harness.observability_snapshot,
                eventsub_processing_debug=harness.eventsub_processing_debug,
                eventsub_processing_requeue=harness.eventsub_processing_requeue,
            ),
        )

        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    f"{INTERNAL_API_BASE_PATH}/debug/observability",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                )
                payload = await response.json()

        self.assertEqual(response.status, 200)
        eventsub = payload.get("observability", {}).get("eventsubProcessing", {})
        self.assertEqual(len(eventsub.get("pending", [])), 1)
        self.assertEqual(len(eventsub.get("deadLetters", [])), 1)
        self.assertEqual(eventsub["deadLetters"][0]["message_id"], "msg-dead-1")

    async def test_eventsub_processing_debug_lists_pending_inbox_and_dead_letters(self) -> None:
        harness = _EventSubInboxAdminHarness()
        app = build_internal_api_app(
            token="secret-token",
            callbacks=InternalApiCallbacks(
                observability_snapshot=harness.observability_snapshot,
                eventsub_processing_debug=harness.eventsub_processing_debug,
                eventsub_processing_requeue=harness.eventsub_processing_requeue,
            ),
        )

        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    f"{INTERNAL_API_BASE_PATH}/debug/eventsub-processing",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                )
                payload = await response.json()

        self.assertEqual(response.status, 200)
        processing = payload.get("eventsubProcessing", {})
        self.assertEqual(len(processing.get("pending", [])), 1)
        self.assertEqual(len(processing.get("deadLetters", [])), 1)
        self.assertEqual(processing["deadLetters"][0]["work_id"], "dead-1")

    async def test_eventsub_processing_requeue_moves_dead_letter_back_into_inbox(self) -> None:
        harness = _EventSubInboxAdminHarness()
        app = build_internal_api_app(
            token="secret-token",
            callbacks=InternalApiCallbacks(
                observability_snapshot=harness.observability_snapshot,
                eventsub_processing_debug=harness.eventsub_processing_debug,
                eventsub_processing_requeue=harness.eventsub_processing_requeue,
            ),
        )

        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.post(
                    f"{INTERNAL_API_BASE_PATH}/eventsub/processing/requeue",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                    json={
                        "work_id": "dead-1",
                    },
                )
                payload = await response.json()

        self.assertEqual(response.status, 200)
        self.assertTrue(payload.get("requeued"))
        self.assertEqual(len(harness.dead_letters), 0)
        self.assertEqual(len(harness.pending_inbox), 2)
        self.assertEqual(harness.requeued[0]["message_id"], "msg-dead-1")

    async def test_eventsub_processing_requeue_rejects_unknown_dead_letter(self) -> None:
        harness = _EventSubInboxAdminHarness()
        app = build_internal_api_app(
            token="secret-token",
            callbacks=InternalApiCallbacks(
                observability_snapshot=harness.observability_snapshot,
                eventsub_processing_debug=harness.eventsub_processing_debug,
                eventsub_processing_requeue=harness.eventsub_processing_requeue,
            ),
        )

        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.post(
                    f"{INTERNAL_API_BASE_PATH}/eventsub/processing/requeue",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                    json={"work_id": "missing-dead-letter"},
                )
                payload = await response.json()

        self.assertEqual(response.status, 400)
        self.assertEqual(payload.get("error"), "bad_request")
        self.assertEqual(payload.get("message"), "invalid request body")

    async def test_eventsub_processing_requeue_rejects_missing_work_id(self) -> None:
        harness = _EventSubInboxAdminHarness()
        app = build_internal_api_app(
            token="secret-token",
            callbacks=InternalApiCallbacks(
                observability_snapshot=harness.observability_snapshot,
                eventsub_processing_debug=harness.eventsub_processing_debug,
                eventsub_processing_requeue=harness.eventsub_processing_requeue,
            ),
        )

        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.post(
                    f"{INTERNAL_API_BASE_PATH}/eventsub/processing/requeue",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                    json={},
                )
                payload = await response.json()

        self.assertEqual(response.status, 400)
        self.assertEqual(payload.get("error"), "bad_request")
        self.assertEqual(payload.get("message"), "invalid request body")
