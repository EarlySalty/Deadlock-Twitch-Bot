from __future__ import annotations

from datetime import datetime, UTC
import unittest

from bot.raid.observability import RaidObservabilityEvent, RaidObservabilityService


class RaidObservabilityTests(unittest.TestCase):
    def test_counter_incrementing_tracks_named_counters(self) -> None:
        service = RaidObservabilityService()

        self.assertEqual(service.increment_counter("raid_pending_timeout_total"), 1)
        self.assertEqual(service.increment_counter("raid_pending_timeout_total", 2), 3)
        self.assertEqual(service.increment_counter("   "), 0)
        self.assertEqual(service.counters()["raid_pending_timeout_total"], 3)

    def test_payload_normalization_and_truncation_matches_expected_shape(self) -> None:
        service = RaidObservabilityService()
        payload = service.format_fields(
            details={
                "at": datetime(2026, 3, 27, 12, 30, tzinfo=UTC),
                "tags": {"beta", "alpha"},
                "nested": ["line1\r\nline2", {"x": 1}],
            },
            decision="  accepted  ",
            empty=None,
        )
        self.assertIn("decision=accepted", payload)
        self.assertIn('"at":"2026-03-27T12:30:00+00:00"', payload)
        self.assertIn('"tags":["alpha","beta"]', payload)
        self.assertEqual(service.normalize_value("line1\r\nline2"), "line1  line2")

        truncated = service.normalize_value({"text": "x" * 400}, limit=32)
        self.assertTrue(truncated.endswith("..."))
        self.assertLessEqual(len(truncated), 35)

    def test_event_payload_creation_normalizes_entity_fields_and_calls_sink(self) -> None:
        captured: list[RaidObservabilityEvent] = []
        service = RaidObservabilityService(event_sink=captured.append, time_source=lambda: 123.456)

        flow_id = service.next_flow_id(prefix="Raid")
        event = service.emit_event(
            flow_type="raid",
            flow_id=flow_id,
            step="  arrival_confirmed  ",
            decision="  matched  ",
            from_broadcaster_login="Source_Login",
            from_broadcaster_id=" 1001 ",
            to_broadcaster_login="TargetLogin",
            to_broadcaster_id=" 9009 ",
            details={"viewer_count": 42},
        )

        self.assertEqual(flow_id, "raid-123456-1")
        self.assertEqual(event.flow_type, "raid")
        self.assertEqual(event.step, "arrival_confirmed")
        self.assertEqual(event.decision, "matched")
        self.assertEqual(event.entity_login, "targetlogin")
        self.assertEqual(event.entity_id, "9009")
        self.assertEqual(event.details, {"viewer_count": 42})
        self.assertEqual(
            event.as_log_fields(),
            {
                "raid_flow_id": "raid-123456-1",
                "step": "arrival_confirmed",
                "decision": "matched",
                "from_broadcaster_login": "source_login",
                "from_broadcaster_id": "1001",
                "to_broadcaster_login": "targetlogin",
                "to_broadcaster_id": "9009",
                "details": {"viewer_count": 42},
            },
        )
        self.assertEqual(
            event.as_storage_payload(),
            {
                "flow_type": "raid",
                "flow_id": "raid-123456-1",
                "entity_login": "targetlogin",
                "entity_id": "9009",
                "step": "arrival_confirmed",
                "decision": "matched",
                "details": {"viewer_count": 42},
            },
        )
        self.assertEqual(captured, [event])


if __name__ == "__main__":
    unittest.main()
