from __future__ import annotations

import contextlib
import unittest
from unittest.mock import MagicMock

from bot.raid.services.partner_arrival_tracking import (
    PartnerArrivalTrackingDependencies,
    PartnerArrivalTrackingService,
)


class PartnerArrivalTrackingServiceTests(unittest.TestCase):
    def _make_service(self) -> tuple[PartnerArrivalTrackingService, dict[str, object]]:
        remember_recent = MagicMock()
        mark_manual = MagicMock()
        logger = MagicMock()
        service = PartnerArrivalTrackingService(
            PartnerArrivalTrackingDependencies(
                readonly_connection=lambda: contextlib.nullcontext(None),
                transaction=lambda: contextlib.nullcontext(None),
                load_active_partner=lambda *_args, **_kwargs: None,
                load_streamer_identity=lambda *_args, **_kwargs: None,
                resolve_streamer_id_by_login=lambda _login: "resolved-source-id",
                mark_manual_raid_started=mark_manual,
                remember_recent_raid_arrival=remember_recent,
                logger=logger,
            )
        )
        return service, {
            "remember_recent": remember_recent,
            "mark_manual": mark_manual,
            "logger": logger,
        }

    def test_process_independent_result_returns_false_without_side_effects_on_store_failure(self) -> None:
        service, state = self._make_service()
        service.classify_partner_raid_arrival = MagicMock(
            return_value=("external_to_partner", "partner_lookup")
        )
        service.store_partner_raid_arrival = MagicMock(return_value=None)

        result = service.process_independent_partner_raid_arrival_result(
            to_broadcaster_id="9009",
            to_broadcaster_login="targetlogin",
            from_broadcaster_login="source_login",
            from_broadcaster_id=None,
            viewer_count=18,
            signal_type="channel.raid",
            correlation_status="independent_channel_raid",
        )

        self.assertFalse(result.processed)
        self.assertEqual(result.classification, "external_to_partner")
        self.assertIsNone(result.arrival_tracking_id)
        state["remember_recent"].assert_not_called()
        state["mark_manual"].assert_not_called()

    def test_process_independent_result_marks_manual_and_remembers_on_success(self) -> None:
        service, state = self._make_service()
        service.classify_partner_raid_arrival = MagicMock(
            return_value=("external_to_partner", "partner_lookup")
        )
        service.store_partner_raid_arrival = MagicMock(return_value=321)

        result = service.process_independent_partner_raid_arrival_result(
            to_broadcaster_id="9009",
            to_broadcaster_login="targetlogin",
            from_broadcaster_login="source_login",
            from_broadcaster_id=None,
            viewer_count=18,
            signal_type="channel.raid",
            correlation_status="independent_channel_raid",
        )

        self.assertTrue(result.processed)
        self.assertEqual(result.arrival_tracking_id, 321)
        state["mark_manual"].assert_called_once_with("resolved-source-id", 180.0)
        state["remember_recent"].assert_called_once()


if __name__ == "__main__":
    unittest.main()
