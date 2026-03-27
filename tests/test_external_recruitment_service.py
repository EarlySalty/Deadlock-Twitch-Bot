from __future__ import annotations

import unittest

from bot.raid.services.external_recruitment import (
    ExternalRecruitmentPolicy,
    ExternalRecruitmentService,
)


class ExternalRecruitmentServiceTests(unittest.TestCase):
    def test_record_confirmed_raid_normalizes_inputs_and_persists(self) -> None:
        calls: list[dict[str, object]] = []

        def _persist_confirmed_raid(**kwargs):
            calls.append(kwargs)
            return 3

        service = ExternalRecruitmentService(
            persist_confirmed_raid=_persist_confirmed_raid,
            count_confirmed_raids=None,
            schedule_pending_blacklist=lambda **_kwargs: None,
            delete_pending_blacklist=lambda _target_id: None,
            is_target_partner=lambda **_kwargs: False,
        )

        result = service.record_confirmed_raid(
            raid_flow_id="  flow-1  ",
            from_broadcaster_id=" 1001 ",
            from_broadcaster_login=" Source_Login ",
            to_broadcaster_id=" 9009 ",
            to_broadcaster_login=" TargetLogin ",
            viewer_count=42,
            confirmation_signal=" channel.raid ",
        )

        self.assertTrue(result.persisted)
        self.assertEqual(result.persisted_count, 3)
        self.assertEqual(result.record.from_broadcaster_login, "source_login")
        self.assertEqual(result.record.to_broadcaster_login, "targetlogin")
        self.assertEqual(result.record.raid_flow_id, "flow-1")
        self.assertEqual(
            calls,
            [
                {
                    "raid_flow_id": "flow-1",
                    "from_broadcaster_id": "1001",
                    "from_broadcaster_login": "source_login",
                    "to_broadcaster_id": "9009",
                    "to_broadcaster_login": "targetlogin",
                    "viewer_count": 42,
                    "confirmation_signal": "channel.raid",
                }
            ],
        )

    def test_maybe_schedule_blacklist_only_schedules_for_threshold_hit_and_non_partner(self) -> None:
        scheduled: list[dict[str, object]] = []
        deleted: list[str] = []

        service = ExternalRecruitmentService(
            persist_confirmed_raid=lambda **_kwargs: None,
            count_confirmed_raids=None,
            schedule_pending_blacklist=lambda **kwargs: scheduled.append(kwargs),
            delete_pending_blacklist=lambda target_id: deleted.append(target_id),
            is_target_partner=lambda **kwargs: kwargs["target_id"] == "partner-id",
            policy=ExternalRecruitmentPolicy(raid_threshold=4, blacklist_grace_seconds=3600),
        )

        below_threshold = service.maybe_schedule_blacklist(
            target_id="non-partner-id",
            target_login="TargetLogin",
            confirmed_raid_count=3,
            raid_flow_id="flow-1",
        )
        self.assertEqual(below_threshold.action, "noop")
        self.assertEqual(below_threshold.reason, "threshold_not_reached")
        self.assertEqual(scheduled, [])
        self.assertEqual(deleted, [])

        threshold_hit = service.maybe_schedule_blacklist(
            target_id="non-partner-id",
            target_login="TargetLogin",
            confirmed_raid_count=4,
            raid_flow_id="flow-2",
        )
        self.assertEqual(threshold_hit.action, "scheduled")
        self.assertEqual(threshold_hit.reason, "threshold_reached")
        self.assertEqual(
            scheduled,
            [
                {
                    "target_id": "non-partner-id",
                    "target_login": "targetlogin",
                    "confirmed_raid_count": 4,
                    "raid_flow_id": "flow-2",
                }
            ],
        )
        self.assertEqual(deleted, [])

        partner_hit = service.maybe_schedule_blacklist(
            target_id="partner-id",
            target_login="PartnerLogin",
            confirmed_raid_count=5,
            raid_flow_id="flow-3",
        )
        self.assertEqual(partner_hit.action, "cleared")
        self.assertEqual(partner_hit.reason, "target_is_partner")
        self.assertEqual(deleted, ["partner-id"])

    def test_clear_pending_blacklist_deletes_when_target_is_partner(self) -> None:
        deleted: list[str] = []

        service = ExternalRecruitmentService(
            persist_confirmed_raid=lambda **_kwargs: None,
            count_confirmed_raids=None,
            schedule_pending_blacklist=lambda **_kwargs: None,
            delete_pending_blacklist=lambda target_id: deleted.append(target_id),
            is_target_partner=lambda **_kwargs: True,
        )

        decision = service.clear_pending_blacklist(
            target_id=" 9009 ",
            target_login=" TargetLogin ",
        )

        self.assertEqual(decision.action, "cleared")
        self.assertEqual(decision.reason, "target_is_partner")
        self.assertEqual(deleted, ["9009"])


if __name__ == "__main__":
    unittest.main()
