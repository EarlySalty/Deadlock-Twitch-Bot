from __future__ import annotations

import unittest

from bot.raid.partner_raid_delivery import (
    PartnerRaidDeliveryConfig,
    PartnerRaidDeliveryRequest,
    plan_partner_raid_delivery,
)


class PartnerRaidDeliveryTests(unittest.TestCase):
    def test_viewer_wording_switches_between_singular_and_plural(self) -> None:
        singular = plan_partner_raid_delivery(
            PartnerRaidDeliveryRequest(
                from_broadcaster_login="source_login",
                to_broadcaster_login="target_login",
                to_broadcaster_id="9009",
                viewer_count=1,
                received_raid_count=7,
            )
        )
        plural = plan_partner_raid_delivery(
            PartnerRaidDeliveryRequest(
                from_broadcaster_login="source_login",
                to_broadcaster_login="target_login",
                to_broadcaster_id="9009",
                viewer_count=2,
                received_raid_count=7,
            )
        )

        self.assertEqual(singular.status, "ready")
        self.assertEqual(singular.viewer_word, "Viewer")
        self.assertIn("mit 1 Viewer geraidet", singular.message or "")
        self.assertIn("Raid Nr. 7", singular.message or "")

        self.assertEqual(plural.status, "ready")
        self.assertEqual(plural.viewer_word, "Viewern")
        self.assertIn("mit 2 Viewern geraidet", plural.message or "")
        self.assertIn("Raid Nr. 7", plural.message or "")

    def test_preserves_configured_delay_metadata(self) -> None:
        plan = plan_partner_raid_delivery(
            PartnerRaidDeliveryRequest(
                from_broadcaster_login="source_login",
                to_broadcaster_login="target_login",
                to_broadcaster_id="9009",
                viewer_count=3,
                received_raid_count=12,
            ),
            config=PartnerRaidDeliveryConfig(delay_seconds=8.5),
        )

        self.assertEqual(plan.status, "ready")
        self.assertEqual(plan.delay_seconds, 8.5)
        self.assertIn("delay_elapsed", plan.prerequisites)

    def test_blocks_when_chat_bot_is_unavailable(self) -> None:
        plan = plan_partner_raid_delivery(
            PartnerRaidDeliveryRequest(
                from_broadcaster_login="source_login",
                to_broadcaster_login="target_login",
                to_broadcaster_id="9009",
                viewer_count=3,
                received_raid_count=12,
                chat_bot_available=False,
            )
        )

        self.assertEqual(plan.status, "blocked")
        self.assertEqual(plan.reason, "chat_bot_unavailable")
        self.assertIsNone(plan.message)
        self.assertIsNone(plan.viewer_word)
        self.assertIn("chat_bot_available", plan.prerequisites)


if __name__ == "__main__":
    unittest.main()
