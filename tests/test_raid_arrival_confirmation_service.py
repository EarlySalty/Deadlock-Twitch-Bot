from __future__ import annotations

import unittest

from bot.raid.arrival_confirmation import ArrivalConfirmationService
from bot.raid.pending_raids import PendingRaid


class _PartnerLookup:
    def __init__(self, row: object | None) -> None:
        self.row = row
        self.calls: list[dict[str, str | None]] = []

    def __call__(
        self,
        *,
        twitch_user_id: str | None = None,
        twitch_login: str | None = None,
    ) -> object | None:
        self.calls.append(
            {
                "twitch_user_id": twitch_user_id,
                "twitch_login": twitch_login,
            }
        )
        return self.row


class _KnownStreamerLookup:
    def __init__(self, row: object | None) -> None:
        self.row = row
        self.calls: list[dict[str, str | None]] = []

    def __call__(
        self,
        *,
        broadcaster_id: str | None = None,
        broadcaster_login: str | None = None,
    ) -> object | None:
        self.calls.append(
            {
                "broadcaster_id": broadcaster_id,
                "broadcaster_login": broadcaster_login,
            }
        )
        return self.row


def _pending_raid(*, is_partner_raid: bool, to_broadcaster_id: str = "9009") -> PendingRaid:
    return PendingRaid(
        from_broadcaster_login="source_login",
        to_broadcaster_id=to_broadcaster_id,
        target_stream_data={"user_login": "targetlogin"},
        registered_ts=123.0,
        is_partner_raid=is_partner_raid,
        registered_viewer_count=42,
        offline_trigger_ts=None,
        raid_flow_id="raid-flow-1",
    ).normalize()


class ArrivalConfirmationServiceTests(unittest.TestCase):
    def test_partner_pending_raid_is_normalized_to_ours_to_partner(self) -> None:
        service = ArrivalConfirmationService(
            partner_lookup=_PartnerLookup({"twitch_user_id": "9009"}),
            known_streamer_lookup=_KnownStreamerLookup(None),
        )

        decision = service.confirm_pending_raid_arrival(
            pending_raid=_pending_raid(is_partner_raid=True),
            signal_type="channel.raid",
            to_broadcaster_id="9009",
            to_broadcaster_login="TargetLogin",
            from_broadcaster_login="Source_Login",
            from_broadcaster_id="1001",
            viewer_count=42,
        )

        self.assertIsNotNone(decision)
        assert decision is not None
        self.assertEqual(decision.classification, "ours_to_partner")
        self.assertEqual(decision.source_resolution, "pending_partner_raid")
        self.assertEqual(decision.follow_up_kind, "partner")
        self.assertTrue(decision.should_refresh_partner_score_cache)
        self.assertTrue(decision.should_track_confirmed_partner_raid)
        self.assertTrue(decision.should_send_partner_raid_message)
        self.assertFalse(decision.should_send_recruitment_message)
        self.assertTrue(decision.should_load_recent_raid_history_reference)
        self.assertTrue(decision.should_delete_external_recruitment_blacklist_pending)

    def test_non_partner_pending_raid_uses_external_path(self) -> None:
        service = ArrivalConfirmationService(
            partner_lookup=_PartnerLookup(None),
            known_streamer_lookup=_KnownStreamerLookup(None),
        )

        decision = service.confirm_pending_raid_arrival(
            pending_raid=_pending_raid(is_partner_raid=False),
            signal_type="channel.raid",
            to_broadcaster_id="9009",
            to_broadcaster_login="TargetLogin",
            from_broadcaster_login="Source_Login",
            from_broadcaster_id="1001",
            viewer_count=42,
        )

        self.assertIsNotNone(decision)
        assert decision is not None
        self.assertIsNone(decision.classification)
        self.assertEqual(decision.source_resolution, "non_partner_target")
        self.assertEqual(decision.follow_up_kind, "external")
        self.assertTrue(decision.should_persist_confirmed_external_recruitment_raid)
        self.assertTrue(decision.should_schedule_external_recruitment_blacklist_pending)
        self.assertTrue(decision.should_send_recruitment_message)
        self.assertFalse(decision.should_send_partner_raid_message)
        self.assertFalse(decision.should_refresh_partner_score_cache)

    def test_pending_partner_raid_resolving_non_partner_suppresses_recruitment(self) -> None:
        service = ArrivalConfirmationService(
            partner_lookup=_PartnerLookup(None),
            known_streamer_lookup=_KnownStreamerLookup(None),
        )

        decision = service.confirm_pending_raid_arrival(
            pending_raid=_pending_raid(is_partner_raid=True),
            signal_type="channel.chat.notification",
            to_broadcaster_id="9009",
            to_broadcaster_login="TargetLogin",
            from_broadcaster_login="Source_Login",
            from_broadcaster_id="1001",
            viewer_count=42,
        )

        self.assertIsNotNone(decision)
        assert decision is not None
        self.assertIsNone(decision.classification)
        self.assertEqual(decision.source_resolution, "non_partner_target")
        self.assertEqual(decision.follow_up_kind, "suppressed_external")
        self.assertEqual(
            decision.suppression_reason,
            "pending_partner_raid_later_resolved_non_partner",
        )
        self.assertFalse(decision.should_send_recruitment_message)
        self.assertFalse(decision.should_persist_confirmed_external_recruitment_raid)
        self.assertFalse(decision.should_schedule_external_recruitment_blacklist_pending)
        self.assertFalse(decision.should_refresh_partner_score_cache)

    def test_external_to_partner_does_not_trigger_external_recruitment_side_effects(self) -> None:
        service = ArrivalConfirmationService(
            partner_lookup=_PartnerLookup({"twitch_user_id": "9009"}),
            known_streamer_lookup=_KnownStreamerLookup(None),
        )

        decision = service.confirm_pending_raid_arrival(
            pending_raid=_pending_raid(is_partner_raid=False),
            signal_type="channel.raid",
            to_broadcaster_id="9009",
            to_broadcaster_login="TargetLogin",
            from_broadcaster_login="Unknown_Source",
            from_broadcaster_id="9999",
            viewer_count=42,
        )

        self.assertIsNotNone(decision)
        assert decision is not None
        self.assertEqual(decision.classification, "external_to_partner")
        self.assertEqual(decision.follow_up_kind, "suppressed_external")
        self.assertEqual(
            decision.suppression_reason,
            "partner_target_without_our_raid_confirmation",
        )
        self.assertFalse(decision.should_persist_confirmed_external_recruitment_raid)
        self.assertFalse(decision.should_schedule_external_recruitment_blacklist_pending)
        self.assertFalse(decision.should_send_recruitment_message)
        self.assertFalse(decision.should_send_partner_raid_message)
        self.assertFalse(decision.should_refresh_partner_score_cache)


if __name__ == "__main__":
    unittest.main()
