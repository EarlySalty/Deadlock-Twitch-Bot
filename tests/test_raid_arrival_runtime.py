from __future__ import annotations

import logging
import unittest
from unittest.mock import AsyncMock, MagicMock

from bot.raid.arrival_confirmation import ArrivalConfirmationService
from bot.raid.pending_raids import PendingRaid
from bot.raid.raid_arrival_runtime import (
    RaidArrivalRuntime,
    RaidArrivalRuntimeDependencies,
)
from bot.raid.signal_correlation import RaidSignalAction, RaidSignalCorrelationService
from bot.raid.signal_correlation import RaidSignalPlan


class _PartnerLookup:
    def __init__(self, row: object | None) -> None:
        self.row = row

    def __call__(
        self,
        *,
        twitch_user_id: str | None = None,
        twitch_login: str | None = None,
    ) -> object | None:
        del twitch_user_id, twitch_login
        return self.row


class _KnownStreamerLookup:
    def __init__(self, row: object | None) -> None:
        self.row = row

    def __call__(
        self,
        *,
        broadcaster_id: str | None = None,
        broadcaster_login: str | None = None,
    ) -> object | None:
        del broadcaster_id, broadcaster_login
        return self.row


def _make_pending(*, is_partner_raid: bool, from_login: str = "source_login") -> PendingRaid:
    return PendingRaid(
        from_broadcaster_login=from_login,
        to_broadcaster_id="9009",
        target_stream_data={"user_login": "targetlogin"},
        registered_ts=123.0,
        is_partner_raid=is_partner_raid,
        registered_viewer_count=42,
        offline_trigger_ts=111.0,
        raid_flow_id="raid-flow-1",
    ).normalize()


class RaidArrivalRuntimeTests(unittest.IsolatedAsyncioTestCase):
    def _make_runtime(
        self,
        *,
        pending_raid: PendingRaid | None = None,
        partner_lookup_row: object | None = None,
    ) -> tuple[RaidArrivalRuntime, dict[str, object]]:
        pending_store: dict[tuple[str, str], PendingRaid] = {}
        recent_arrivals: dict[tuple[str, str], dict[str, object]] = {}
        orphan_notifications: list[dict[str, object]] = []
        observation_calls: list[tuple[PendingRaid, str, str, str | None, str | None]] = []
        store_pending_calls: list[PendingRaid] = []
        partner_arrival_calls: list[dict[str, object]] = []
        independent_calls: list[dict[str, object]] = []
        manual_calls: list[tuple[str, float]] = []
        event_calls: list[dict[str, object]] = []
        counter_calls: list[tuple[str, int]] = []
        cancel_mock = MagicMock(return_value=0)

        if pending_raid is not None:
            pending_store[pending_raid.key] = pending_raid

        logger = MagicMock(spec=logging.Logger)
        logger.info = MagicMock()
        logger.warning = MagicMock()
        logger.debug = MagicMock()

        deps = RaidArrivalRuntimeDependencies(
            arrival_confirmation_service=ArrivalConfirmationService(
                partner_lookup=_PartnerLookup(partner_lookup_row),
                known_streamer_lookup=_KnownStreamerLookup(None),
            ),
            signal_correlation_service=RaidSignalCorrelationService(),
            get_pending_raid=lambda to_broadcaster_id, from_broadcaster_login: pending_store.get(
                (str(to_broadcaster_id or "").strip(), str(from_broadcaster_login or "").strip().lower())
            ),
            store_pending_raid=lambda raid: store_pending_calls.append(raid),
            pop_pending_raid=lambda to_broadcaster_id, from_broadcaster_login: pending_store.pop(
                (
                    str(to_broadcaster_id or "").strip(),
                    str(from_broadcaster_login or "").strip().lower(),
                ),
                None,
            ),
            record_pending_signal_observation=lambda pending, signal_type, status, reason, detail: observation_calls.append(
                (pending, signal_type, status, reason, detail)
            ),
            store_orphan_chat_raid_notification=lambda payload: orphan_notifications.append(payload),
            lookup_recent_raid_arrival=lambda to_broadcaster_id, from_broadcaster_login: recent_arrivals.get(
                (str(to_broadcaster_id or "").strip(), str(from_broadcaster_login or "").strip().lower())
            ),
            remember_recent_raid_arrival=lambda **kwargs: recent_arrivals.__setitem__(
                (
                    str(kwargs["to_broadcaster_id"] or "").strip(),
                    str(kwargs["from_broadcaster_login"] or "").strip().lower(),
                ),
                dict(kwargs),
            ),
            update_partner_raid_arrival=lambda arrival_tracking_id, confirmation_signals, unraid_seen: partner_arrival_calls.append(
                {
                    "arrival_tracking_id": arrival_tracking_id,
                    "confirmation_signals": set(confirmation_signals),
                    "unraid_seen": unraid_seen,
                }
            ),
            store_partner_raid_arrival=lambda **kwargs: (
                partner_arrival_calls.append(dict(kwargs)) or 321
            ),
            load_recent_raid_history_reference=lambda **_kwargs: (222, "2026-03-27T12:00:00+00:00"),
            process_independent_partner_raid_arrival=lambda **kwargs: independent_calls.append(dict(kwargs)) or False,
            cancel_pending_raids_for_source_unraid=cancel_mock,
            resolve_streamer_id_by_login=lambda login: "resolved-" + login,
            mark_manual_raid_started=lambda broadcaster_id, ttl_seconds=300.0: manual_calls.append(
                (broadcaster_id, ttl_seconds)
            ),
            lookup_silent_raid_enabled=lambda _login: False,
            refresh_partner_score_cache_if_available=AsyncMock(),
            track_confirmed_partner_raid=MagicMock(),
            delete_external_recruitment_blacklist_pending=MagicMock(),
            record_confirmed_external_recruitment_raid=MagicMock(return_value=7),
            maybe_schedule_external_recruitment_blacklist_pending=MagicMock(),
            send_partner_raid_message=AsyncMock(),
            send_recruitment_message=AsyncMock(),
            increment_raid_observability_counter=lambda name, amount=1: counter_calls.append((name, amount)) or 1,
            log_raid_observability_event=lambda **kwargs: event_calls.append(dict(kwargs)),
            next_raid_observability_flow_id=lambda prefix: f"{prefix}-flow",
            logger=logger,
            now=lambda: 1000.0,
        )
        runtime = RaidArrivalRuntime(deps)
        return runtime, {
            "logger": logger,
            "pending_store": pending_store,
            "recent_arrivals": recent_arrivals,
            "orphan_notifications": orphan_notifications,
            "observation_calls": observation_calls,
            "store_pending_calls": store_pending_calls,
            "partner_arrival_calls": partner_arrival_calls,
            "independent_calls": independent_calls,
            "manual_calls": manual_calls,
            "event_calls": event_calls,
            "counter_calls": counter_calls,
            "cancel_mock": cancel_mock,
            "deps": deps,
        }

    async def test_execute_signal_plan_actions_dispatches_all_supported_actions(self) -> None:
        runtime, state = self._make_runtime()
        pending = _make_pending(is_partner_raid=False)

        await runtime._execute_signal_plan_actions(
            (
                RaidSignalAction(
                    kind="record_pending_observation",
                    data={
                        "pending_raid": pending,
                        "signal_type": "channel.raid",
                        "status": "matched_pending",
                        "reason": None,
                        "detail": "msg-1",
                    },
                ),
                RaidSignalAction(kind="store_pending_raid", data={"pending_raid": pending}),
                RaidSignalAction(
                    kind="store_orphan_chat_notification",
                    data={"payload": {"to_broadcaster_id": "9009", "message_id": "msg-2"}},
                ),
                RaidSignalAction(kind="mark_manual_raid_started", data={"source_key": "1001", "ttl_seconds": 180.0}),
                RaidSignalAction(
                    kind="record_independent_raid_arrival",
                    data={
                        "signal_type": "channel.raid",
                        "to_broadcaster_id": "9009",
                        "to_broadcaster_login": "targetlogin",
                        "from_broadcaster_login": "source_login",
                        "from_broadcaster_id": "1001",
                        "viewer_count": 42,
                    },
                ),
            )
        )

        self.assertEqual(len(state["observation_calls"]), 1)
        self.assertEqual(len(state["store_pending_calls"]), 1)
        self.assertEqual(len(state["orphan_notifications"]), 1)
        self.assertEqual(state["manual_calls"], [("1001", 180.0)])
        self.assertEqual(len(state["independent_calls"]), 1)

    async def test_handle_secondary_confirmed_signal_updates_recent_arrival(self) -> None:
        runtime, state = self._make_runtime()
        state["recent_arrivals"][("9009", "source_login")] = {
            "arrival_tracking_id": 77,
            "confirmation_signals": {"channel.chat.notification"},
            "viewer_count": 12,
            "raid_flow_id": "raid-flow-1",
        }

        handled = runtime._handle_secondary_confirmed_signal(
            signal_type="channel.raid",
            to_broadcaster_id="9009",
            to_broadcaster_login="targetlogin",
            from_broadcaster_login="source_login",
            viewer_count=20,
        )

        self.assertTrue(handled)
        self.assertEqual(len(state["event_calls"]), 1)
        self.assertEqual(state["partner_arrival_calls"][0]["arrival_tracking_id"], 77)
        self.assertEqual(state["partner_arrival_calls"][0]["unraid_seen"], False)

    async def test_on_raid_arrival_confirmed_partner_sends_partner_message(self) -> None:
        runtime, state = self._make_runtime(
            pending_raid=_make_pending(is_partner_raid=True),
            partner_lookup_row={"twitch_user_id": "9009"},
        )

        await runtime.on_raid_arrival(
            to_broadcaster_id="9009",
            to_broadcaster_login="TargetLogin",
            from_broadcaster_login="Source_Login",
            viewer_count=42,
            from_broadcaster_id="1001",
        )

        deps = state["deps"]
        deps.send_partner_raid_message.assert_awaited_once()
        deps.send_recruitment_message.assert_not_awaited()
        deps.refresh_partner_score_cache_if_available.assert_awaited_once_with(
            "9009",
            reason="incoming_partner_raid_confirmed",
        )
        deps.track_confirmed_partner_raid.assert_called_once()

    async def test_on_raid_arrival_non_partner_sends_recruitment_message(self) -> None:
        runtime, state = self._make_runtime(
            pending_raid=_make_pending(is_partner_raid=False),
            partner_lookup_row=None,
        )

        await runtime.on_raid_arrival(
            to_broadcaster_id="9009",
            to_broadcaster_login="TargetLogin",
            from_broadcaster_login="Source_Login",
            viewer_count=42,
            from_broadcaster_id="1001",
        )

        deps = state["deps"]
        deps.send_partner_raid_message.assert_not_awaited()
        deps.send_recruitment_message.assert_awaited_once()
        deps.record_confirmed_external_recruitment_raid.assert_called_once()
        deps.maybe_schedule_external_recruitment_blacklist_pending.assert_called_once()

    async def test_on_raid_arrival_independent_manual_path_processes_only_once(self) -> None:
        runtime, state = self._make_runtime(partner_lookup_row={"twitch_user_id": "9009"})
        object.__setattr__(
            state["deps"],
            "process_independent_partner_raid_arrival",
            lambda **kwargs: state["independent_calls"].append(dict(kwargs)) or True,
        )

        await runtime.on_raid_arrival(
            to_broadcaster_id="9009",
            to_broadcaster_login="TargetLogin",
            from_broadcaster_login="Source_Login",
            viewer_count=42,
            from_broadcaster_id="1001",
        )

        self.assertEqual(len(state["independent_calls"]), 1)
        self.assertEqual(state["manual_calls"], [])

    async def test_on_chat_raid_notification_orphan_is_stored(self) -> None:
        runtime, state = self._make_runtime()

        await runtime.on_chat_raid_notification(
            to_broadcaster_id="9009",
            to_broadcaster_login="TargetLogin",
            from_broadcaster_login="Source_Login",
            viewer_count=42,
            from_broadcaster_id="1001",
            message_id="msg-1",
            event_timestamp="2026-03-27T12:00:00+00:00",
        )

        self.assertEqual(len(state["orphan_notifications"]), 1)
        self.assertEqual(state["counter_calls"][0], ("raid_orphan_chat_notification_total", 1))

    async def test_on_chat_unraid_notification_records_diagnostic_pending(self) -> None:
        runtime, state = self._make_runtime(pending_raid=_make_pending(is_partner_raid=False))

        await runtime.on_chat_unraid_notification(
            to_broadcaster_id="9009",
            to_broadcaster_login="TargetLogin",
            from_broadcaster_login="Source_Login",
            from_broadcaster_id="1001",
            event_timestamp="2026-03-27T12:00:00+00:00",
        )

        self.assertEqual(len(state["observation_calls"]), 1)
        self.assertEqual(state["observation_calls"][0][2], "diagnostic_only")
        self.assertEqual(len(state["store_pending_calls"]), 1)

    async def test_on_raid_arrival_executes_signal_plan_actions(self) -> None:
        runtime, state = self._make_runtime(pending_raid=_make_pending(is_partner_raid=False))
        pending = state["pending_store"][("9009", "source_login")]
        custom_plan = RaidSignalPlan(
            signal_type="channel.raid",
            outcome="pending_mismatch",
            from_broadcaster_login="source_login",
            from_broadcaster_id="1001",
            to_broadcaster_login="targetlogin",
            to_broadcaster_id="9009",
            viewer_count=42,
            pending_raid=pending,
            actions=(
                RaidSignalAction(
                    kind="record_pending_observation",
                    data={
                        "pending_raid": pending,
                        "signal_type": "channel.raid",
                        "status": "ignored",
                        "reason": "source_target_mismatch",
                        "detail": "expected=source_login actual=source_login",
                    },
                ),
                RaidSignalAction(kind="store_pending_raid", data={"pending_raid": pending}),
            ),
            reason="source_target_mismatch",
        )
        runtime._deps.signal_correlation_service.plan_raid_arrival = MagicMock(
            return_value=custom_plan
        )
        runtime._execute_signal_plan_actions = AsyncMock()

        await runtime.on_raid_arrival(
            to_broadcaster_id="9009",
            to_broadcaster_login="TargetLogin",
            from_broadcaster_login="Source_Login",
            viewer_count=42,
            from_broadcaster_id="1001",
        )

        runtime._execute_signal_plan_actions.assert_awaited_once_with(custom_plan.actions)

    async def test_on_source_self_unraid_notification_short_circuits_when_canceled(self) -> None:
        runtime, state = self._make_runtime()
        state["cancel_mock"].return_value = 1

        await runtime.on_source_self_unraid_notification(
            broadcaster_id="1001",
            broadcaster_login="Source_Login",
            message_id="msg-1",
            event_timestamp="2026-03-27T12:00:00+00:00",
        )

        state["cancel_mock"].assert_called_once()
        state["logger"].info.assert_not_called()


if __name__ == "__main__":
    unittest.main()
