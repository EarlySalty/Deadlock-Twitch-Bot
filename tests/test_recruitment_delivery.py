from __future__ import annotations

import unittest

from bot.raid.recruitment_delivery import (
    RecruitmentDeliveryConfig,
    RecruitmentDeliveryRequest,
    plan_recruitment_delivery,
)


class RecruitmentDeliveryTests(unittest.TestCase):
    def test_blocks_when_recent_raids_exceed_threshold(self) -> None:
        plan = plan_recruitment_delivery(
            RecruitmentDeliveryRequest(
                from_broadcaster_login="source_login",
                to_broadcaster_login="target_login",
                target_id="9009",
                recent_raid_count=3,
                total_recruitment_raid_count=1,
                followers_total=100,
            )
        )

        self.assertEqual(plan.status, "blocked")
        self.assertEqual(plan.reason, "recent_raids_exceed_threshold")
        self.assertIn("recent_raid_count_within_threshold", plan.prerequisites)
        self.assertEqual(plan.message_variant, None)
        self.assertEqual(plan.invite_variant, None)

    def test_allows_when_recent_raids_are_within_threshold(self) -> None:
        plan = plan_recruitment_delivery(
            RecruitmentDeliveryRequest(
                from_broadcaster_login="source_login",
                to_broadcaster_login="target_login",
                target_id="9009",
                recent_raid_count=2,
                total_recruitment_raid_count=2,
                followers_total=100,
            )
        )

        self.assertEqual(plan.status, "ready")
        self.assertIsNone(plan.reason)
        self.assertEqual(plan.message_variant, "second")
        self.assertEqual(plan.invite_variant, "direct")
        self.assertIn("delay_elapsed", plan.prerequisites)

    def test_preserves_configured_delay_metadata(self) -> None:
        plan = plan_recruitment_delivery(
            RecruitmentDeliveryRequest(
                from_broadcaster_login="source_login",
                to_broadcaster_login="target_login",
                target_id="9009",
                recent_raid_count=0,
                total_recruitment_raid_count=0,
                followers_total=500,
            ),
            config=RecruitmentDeliveryConfig(delay_seconds=27.5),
        )

        self.assertEqual(plan.status, "ready")
        self.assertEqual(plan.delay_seconds, 27.5)


if __name__ == "__main__":
    unittest.main()
