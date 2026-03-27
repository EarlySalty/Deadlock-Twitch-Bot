from __future__ import annotations

import unittest

from bot.raid.pending_raids import PendingRaid
from bot.raid.signal_correlation import RaidSignalCorrelationService


def _pending_raid(*, from_login: str = "source_login") -> PendingRaid:
    return PendingRaid(
        from_broadcaster_login=from_login,
        to_broadcaster_id="9009",
        target_stream_data={"user_login": "targetlogin"},
        registered_ts=123.0,
        is_partner_raid=True,
        registered_viewer_count=42,
        offline_trigger_ts=None,
        raid_flow_id="raid-flow-1",
    ).normalize()


class RaidSignalCorrelationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.service = RaidSignalCorrelationService()

    def test_pending_match_plans_confirm_and_store_actions(self) -> None:
        plan = self.service.plan_chat_notification(
            to_broadcaster_id="9009",
            to_broadcaster_login="TargetLogin",
            from_broadcaster_login="Source_Login",
            from_broadcaster_id="1001",
            viewer_count=42,
            message_id="msg-1",
            event_timestamp="2026-03-27T12:00:00+00:00",
            pending_raid=_pending_raid(),
            recent_arrival_present=False,
        )

        self.assertEqual(plan.outcome, "pending_matched")
        self.assertEqual(
            [action.kind for action in plan.actions],
            ["record_pending_observation", "store_pending_raid", "confirm_pending_raid"],
        )
        self.assertEqual(plan.actions[0].data["status"], "matched_pending")
        self.assertEqual(plan.actions[2].data["signal_type"], "channel.chat.notification")

    def test_pending_mismatch_plans_ignore_and_store_actions(self) -> None:
        plan = self.service.plan_raid_arrival(
            to_broadcaster_id="9009",
            to_broadcaster_login="TargetLogin",
            from_broadcaster_login="Other_Source",
            from_broadcaster_id="1001",
            viewer_count=42,
            pending_raid=_pending_raid(from_login="source_login"),
            recent_arrival_present=False,
            independent_manual_detected=False,
        )

        self.assertEqual(plan.outcome, "pending_mismatch")
        self.assertEqual(
            [action.kind for action in plan.actions],
            ["record_pending_observation", "store_pending_raid"],
        )
        self.assertEqual(plan.reason, "source_target_mismatch")
        self.assertIn("expected=source_login actual=other_source", plan.actions[0].data["detail"])

    def test_orphan_chat_notification_plans_store_orphan_action(self) -> None:
        plan = self.service.plan_chat_notification(
            to_broadcaster_id="9009",
            to_broadcaster_login="TargetLogin",
            from_broadcaster_login="Source_Login",
            from_broadcaster_id="1001",
            viewer_count=42,
            message_id="msg-1",
            event_timestamp="2026-03-27T12:00:00+00:00",
            pending_raid=None,
            recent_arrival_present=False,
        )

        self.assertEqual(plan.outcome, "orphan_chat_notification")
        self.assertEqual([action.kind for action in plan.actions], ["store_orphan_chat_notification"])
        payload = plan.actions[0].data["payload"]
        self.assertEqual(payload["from_broadcaster_login"], "source_login")
        self.assertEqual(payload["message_id"], "msg-1")

    def test_no_pending_independent_manual_raid_plans_manual_actions(self) -> None:
        plan = self.service.plan_raid_arrival(
            to_broadcaster_id="9009",
            to_broadcaster_login="TargetLogin",
            from_broadcaster_login="Source_Login",
            from_broadcaster_id="1001",
            viewer_count=42,
            pending_raid=None,
            recent_arrival_present=False,
            independent_manual_detected=True,
            manual_raid_source_key="1001",
        )

        self.assertEqual(plan.outcome, "independent_manual_arrival")
        self.assertEqual(
            [action.kind for action in plan.actions],
            ["mark_manual_raid_started", "record_independent_raid_arrival"],
        )
        self.assertEqual(plan.actions[0].data["ttl_seconds"], 180.0)


if __name__ == "__main__":
    unittest.main()
