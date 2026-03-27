from __future__ import annotations

import logging
import unittest
from dataclasses import replace
from unittest.mock import AsyncMock, MagicMock, patch

from bot.raid.bot import RaidBot
from bot.raid.partner_arrival_tracking import (
    PartnerArrivalTrackingDependencies,
    PartnerArrivalTrackingService,
)
from bot.raid.pending_raids import PendingRaid
from bot.raid.raid_dependencies import build_default_raid_runtime_deps


class PartnerArrivalTrackingServiceRegressionTests(unittest.TestCase):
    def _make_service(self) -> tuple[PartnerArrivalTrackingService, list[tuple[str, float]], list[dict[str, object]]]:
        manual_calls: list[tuple[str, float]] = []
        remember_calls: list[dict[str, object]] = []
        logger = MagicMock(spec=logging.Logger)
        deps = PartnerArrivalTrackingDependencies(
            readonly_connection=MagicMock(),
            transaction=MagicMock(),
            load_active_partner=MagicMock(return_value={"twitch_user_id": "9009"}),
            load_streamer_identity=MagicMock(return_value=None),
            resolve_streamer_id_by_login=lambda login: f"resolved-{login}",
            mark_manual_raid_started=lambda broadcaster_id, ttl_seconds: manual_calls.append(
                (broadcaster_id, ttl_seconds)
            ),
            remember_recent_raid_arrival=lambda **kwargs: remember_calls.append(dict(kwargs)),
            logger=logger,
        )
        return PartnerArrivalTrackingService(deps), manual_calls, remember_calls

    def test_process_independent_partner_raid_arrival_returns_false_when_persist_fails(self) -> None:
        service, manual_calls, remember_calls = self._make_service()

        with patch.object(
            PartnerArrivalTrackingService,
            "store_partner_raid_arrival",
            return_value=None,
        ):
            processed = service.process_independent_partner_raid_arrival(
                to_broadcaster_id="9009",
                to_broadcaster_login="targetlogin",
                from_broadcaster_login="source_login",
                from_broadcaster_id=None,
                viewer_count=42,
                signal_type="channel.raid",
                correlation_status="independent_channel_raid",
            )

        self.assertFalse(processed)
        self.assertEqual(manual_calls, [])
        self.assertEqual(remember_calls, [])


class RaidBotRuntimeWiringRegressionTests(unittest.IsolatedAsyncioTestCase):
    def _build_raid_bot(self) -> RaidBot:
        raid_bot = RaidBot.__new__(RaidBot)
        raid_bot._pending_raids = {}
        raid_bot._recent_raid_arrivals = {}
        raid_bot._orphan_chat_raid_notifications = {}
        raid_bot._manual_raid_suppression = {}
        raid_bot._raid_readiness_by_flow_id = {}
        raid_bot._raid_observability_counter_store = {}
        raid_bot._user_scope_fallback_warned = set()
        raid_bot.chat_bot = None
        raid_bot._bot_id = None
        raid_bot._cog = None
        raid_bot._session = None
        raid_bot._refresh_partner_score_cache_if_available = AsyncMock()
        raid_bot._send_partner_raid_message = AsyncMock()
        raid_bot._send_recruitment_message_now = AsyncMock()
        return raid_bot

    async def test_confirm_pending_raid_arrival_uses_wrapper_history_loader(self) -> None:
        raid_bot = self._build_raid_bot()
        raid_bot._store_pending_raid(
            PendingRaid(
                from_broadcaster_login="source_login",
                to_broadcaster_id="9009",
                target_stream_data={"_partner_score": {"final_score": 1.1}},
                registered_ts=1.0,
                is_partner_raid=True,
                registered_viewer_count=42,
                raid_flow_id="raid-flow-wrapper-history",
            )
        )

        with patch.object(
            raid_bot,
            "_load_recent_raid_history_reference",
            return_value=(123, "2026-03-27T12:00:00+00:00"),
        ) as history_mock, patch.object(
            raid_bot,
            "_store_partner_raid_arrival",
            return_value=321,
        ), patch.object(
            raid_bot,
            "_classify_partner_raid_arrival",
            return_value=("ours_to_partner", "known_source_network"),
        ), patch.object(
            raid_bot,
            "_lookup_silent_raid_enabled",
            return_value=True,
        ):
            await raid_bot._confirm_pending_raid_arrival(
                signal_type="channel.chat.notification",
                to_broadcaster_id="9009",
                to_broadcaster_login="targetlogin",
                from_broadcaster_login="source_login",
                from_broadcaster_id="1001",
                viewer_count=42,
            )

        history_mock.assert_called_once_with(
            from_broadcaster_login="source_login",
            to_broadcaster_id="9009",
        )

    def test_runtime_deps_override_state_store_config(self) -> None:
        deps = replace(
            build_default_raid_runtime_deps(),
            recent_raid_arrival_ttl_seconds=123.0,
            raid_readiness_max_entries=9,
        )
        raid_bot = RaidBot(
            client_id="client",
            client_secret="secret",
            redirect_uri="https://example.test/callback",
            session=None,
            deps=deps,
        )

        config = raid_bot._raid_state_store_config()

        self.assertIs(raid_bot._runtime_deps(), deps)
        self.assertEqual(config.recent_raid_arrival_ttl_seconds, 123.0)
        self.assertEqual(config.raid_readiness_max_entries, 9)


if __name__ == "__main__":
    unittest.main()
