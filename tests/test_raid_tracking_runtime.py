from __future__ import annotations

import logging
import unittest
from unittest.mock import AsyncMock, MagicMock

from bot.raid.pending_raids import PendingRaid, PendingRaidStore
from bot.raid.raid_tracking_runtime import (
    RaidTrackingRuntimeDependencies,
    RaidTrackingRuntimeService,
    RaidTrackingRuntimeState,
)


class RaidTrackingRuntimeTests(unittest.IsolatedAsyncioTestCase):
    def _make_service(self, **overrides) -> tuple[RaidTrackingRuntimeService, MagicMock, list[dict[str, object]]]:
        logger = MagicMock(spec=logging.Logger)
        events: list[dict[str, object]] = []
        deps = RaidTrackingRuntimeDependencies(
            logger=logger,
            state=RaidTrackingRuntimeState(),
            snapshot_chat_notification_subscription=lambda _login: ("subscribed", "chat ready"),
            get_cog=lambda: object(),
            eventsub_has_sub=lambda _cog, _sub_type, _broadcaster_id: True,
            ensure_raid_target_dynamic_ready=AsyncMock(return_value=(True, "ready")),
            subscribe_raid_target_dynamic=AsyncMock(return_value=True),
            orphan_chat_raid_notification_handler=AsyncMock(),
            next_raid_observability_flow_id=lambda prefix: f"{prefix}-1",
            increment_raid_observability_counter=MagicMock(return_value=1),
            log_raid_observability_event=lambda **payload: events.append(payload),
            monotonic=lambda: 200.0,
            now=lambda: 1000.0,
        )
        for key, value in overrides.items():
            setattr(deps, key, value)
        return RaidTrackingRuntimeService(deps), logger, events

    def test_cleanup_stale_pending_raids_removes_expired_entries(self) -> None:
        service, logger, events = self._make_service()
        service.state.pending_store.store(
            {
                "from_broadcaster_login": "source_login",
                "to_broadcaster_id": "old-target",
                "registered_ts": 10.0,
                "offline_trigger_ts": 5.0,
                "raid_flow_id": "flow-old",
            }
        )
        service.state.pending_store.store(
            {
                "from_broadcaster_login": "source_login",
                "to_broadcaster_id": "fresh-target",
                "registered_ts": 900.0,
            }
        )

        removed = service.cleanup_stale_pending_raids(now=400.0, timeout_seconds=300.0)

        self.assertEqual(len(removed), 1)
        self.assertEqual(removed[0].to_broadcaster_id, "old-target")
        self.assertIsNone(service.state.pending_store.get(to_broadcaster_id="old-target", from_broadcaster_login="source_login"))
        self.assertIsNotNone(service.state.pending_store.get(to_broadcaster_id="fresh-target", from_broadcaster_login="source_login"))
        logger.warning.assert_called_once()
        self.assertEqual(events[-1]["decision"], "timeout")

    def test_clear_superseded_pending_raids_removes_older_targets_only(self) -> None:
        service, logger, events = self._make_service()
        service.state.pending_store.store(
            {
                "from_broadcaster_login": "Source_Login",
                "to_broadcaster_id": "old-target",
                "registered_ts": 10.0,
            }
        )
        service.state.pending_store.store(
            {
                "from_broadcaster_login": "Source_Login",
                "to_broadcaster_id": "new-target",
                "registered_ts": 11.0,
            }
        )
        service.state.pending_store.store(
            {
                "from_broadcaster_login": "other_source",
                "to_broadcaster_id": "other-target",
                "registered_ts": 12.0,
            }
        )

        removed = service.clear_superseded_pending_raids(
            from_broadcaster_login="source_login",
            current_target_id="new-target",
        )

        self.assertEqual([raid.to_broadcaster_id for raid in removed], ["old-target"])
        self.assertIsNone(service.state.pending_store.get(to_broadcaster_id="old-target", from_broadcaster_login="source_login"))
        self.assertIsNotNone(service.state.pending_store.get(to_broadcaster_id="new-target", from_broadcaster_login="source_login"))
        self.assertIsNotNone(service.state.pending_store.get(to_broadcaster_id="other-target", from_broadcaster_login="other_source"))
        logger.info.assert_called_once()
        self.assertEqual(events[-1]["decision"], "superseded")

    def test_cancel_pending_raids_for_source_unraid_removes_matching_entries(self) -> None:
        service, logger, events = self._make_service()
        first = service.state.pending_store.store(
            {
                "from_broadcaster_login": "source_login",
                "to_broadcaster_id": "target-a",
                "registered_ts": 10.0,
            }
        )
        second = service.state.pending_store.store(
            {
                "from_broadcaster_login": "source_login",
                "to_broadcaster_id": "target-b",
                "registered_ts": 11.0,
            }
        )
        service.state.pending_store.store(
            {
                "from_broadcaster_login": "other_source",
                "to_broadcaster_id": "target-c",
                "registered_ts": 12.0,
            }
        )

        canceled = service.cancel_pending_raids_for_source_unraid(
            from_broadcaster_login="SOURCE_LOGIN",
            from_broadcaster_id="source-id",
            message_id="msg-1",
            event_timestamp="2026-03-27T12:00:00Z",
        )

        self.assertEqual(canceled, 2)
        self.assertIsNone(service.state.pending_store.get(to_broadcaster_id=first.to_broadcaster_id, from_broadcaster_login="source_login"))
        self.assertIsNone(service.state.pending_store.get(to_broadcaster_id=second.to_broadcaster_id, from_broadcaster_login="source_login"))
        self.assertIsNotNone(service.state.pending_store.get(to_broadcaster_id="target-c", from_broadcaster_login="other_source"))
        self.assertEqual(service.state.pending_store.get(to_broadcaster_id="target-c", from_broadcaster_login="other_source").registered_ts, 12.0)
        self.assertEqual(logger.info.call_count, 2)
        self.assertGreaterEqual(len(events), 2)

    async def test_ensure_raid_arrival_subscription_ready_without_cog_is_best_effort(self) -> None:
        service, logger, events = self._make_service(get_cog=lambda: None)

        ready = await service.ensure_raid_arrival_subscription_ready(
            to_broadcaster_id="9009",
            to_broadcaster_login="targetlogin",
        )

        self.assertTrue(ready)
        self.assertEqual(events[-1]["decision"], "no_cog_best_effort")
        logger.warning.assert_not_called()

    async def test_ensure_raid_arrival_subscription_ready_tracks_local_only_false(self) -> None:
        service, logger, events = self._make_service(
            eventsub_has_sub=lambda _cog, _sub_type, _broadcaster_id: True,
            ensure_raid_target_dynamic_ready=AsyncMock(return_value=(False, "not ready")),
        )

        ready = await service.ensure_raid_arrival_subscription_ready(
            to_broadcaster_id="9009",
            to_broadcaster_login="targetlogin",
            raid_flow_id="raid-ready-7",
        )

        self.assertFalse(ready)
        self.assertEqual(service.state.readiness_states["raid-ready-7"]["ready"], False)
        self.assertIn("raid_eventsub_ready_false_total", service._deps.increment_raid_observability_counter.call_args_list[0].args[0])
        self.assertEqual(events[-1]["decision"], "not_ready")
        logger.warning.assert_called_once()

    async def test_register_pending_raid_stores_record_and_handles_orphan_match(self) -> None:
        orphan_handler = AsyncMock()
        service, logger, events = self._make_service(orphan_chat_raid_notification_handler=orphan_handler)
        service.state.pending_store.store(
            {
                "from_broadcaster_login": "source_login",
                "to_broadcaster_id": "old-target",
                "registered_ts": 10.0,
            }
        )
        service.store_orphan_chat_raid_notification(
            {
                "to_broadcaster_id": "9009",
                "to_broadcaster_login": "targetlogin",
                "from_broadcaster_login": "source_login",
                "viewer_count": 42,
                "message_id": "msg-1",
                "event_timestamp": "2026-03-27T12:00:00Z",
            }
        )
        service._remember_readiness_state(
            flow_id="flow-1",
            ready=True,
            detail="ready detail",
            locally_tracked=False,
        )

        pending = await service.register_pending_raid(
            from_broadcaster_login="Source_Login",
            to_broadcaster_id="9009",
            to_broadcaster_login="TargetLogin",
            target_stream_data={"user_login": "targetlogin"},
            is_partner_raid=True,
            viewer_count=77,
            offline_trigger_ts=123.5,
            raid_flow_id="flow-1",
            channel_raid_ready=False,
        )

        self.assertIsInstance(pending, PendingRaid)
        assert pending is not None
        self.assertEqual(pending.from_broadcaster_login, "source_login")
        self.assertEqual(pending.to_broadcaster_id, "9009")
        self.assertEqual(pending.channel_raid_ready_detail, "ready detail")
        self.assertEqual(pending.chat_notification_state, "subscribed")
        self.assertIsNotNone(service.state.pending_store.get(to_broadcaster_id="9009", from_broadcaster_login="source_login"))
        self.assertIsNone(service.state.pending_store.get(to_broadcaster_id="old-target", from_broadcaster_login="source_login"))
        self.assertEqual(orphan_handler.await_count, 1)
        self.assertEqual(events[-1]["step"], "pending_orphan_notification_match")
        self.assertIn("raid_pending_registered_total", service._deps.increment_raid_observability_counter.call_args_list[0].args[0])
        logger.info.assert_called()

    def test_build_pending_timeout_detail_prefers_signal_observations(self) -> None:
        service, _, _ = self._make_service()
        pending = PendingRaid(
            from_broadcaster_login="source_login",
            to_broadcaster_id="9009",
            registered_ts=10.0,
            is_partner_raid=True,
            registered_viewer_count=42,
        )
        pending.record_signal_observation(
            signal_type="channel.raid",
            status="matched_pending",
            reason="confirmed",
            detail="raid event",
        )
        pending.record_signal_observation(
            signal_type="channel.chat.notification",
            status="matched_pending",
        )

        detail = service.build_pending_timeout_detail(pending)

        self.assertIn("channel.raid:matched_pending (confirmed) [raid event]", detail)
        self.assertIn("channel.chat.notification:matched_pending", detail)


if __name__ == "__main__":
    unittest.main()
