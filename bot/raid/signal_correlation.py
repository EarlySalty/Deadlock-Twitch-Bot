from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Literal, Mapping

from .pending_raids import PendingRaid, normalize_broadcaster_login


RaidSignalType = Literal[
    "channel.raid",
    "channel.chat.notification",
    "channel.chat.notification.unraid",
]

RaidSignalOutcome = Literal[
    "secondary_signal_handled",
    "pending_matched",
    "pending_mismatch",
    "orphan_chat_notification",
    "independent_manual_arrival",
    "pending_unraid_observed",
    "no_pending",
]

RaidSignalActionKind = Literal[
    "record_secondary_signal",
    "record_pending_observation",
    "store_pending_raid",
    "confirm_pending_raid",
    "store_orphan_chat_notification",
    "mark_manual_raid_started",
    "record_independent_raid_arrival",
]


@dataclass(slots=True, frozen=True)
class RaidSignalAction:
    kind: RaidSignalActionKind
    data: dict[str, Any] = field(default_factory=dict)


@dataclass(slots=True, frozen=True)
class RaidSignalPlan:
    signal_type: RaidSignalType
    outcome: RaidSignalOutcome
    from_broadcaster_login: str
    from_broadcaster_id: str | None
    to_broadcaster_login: str
    to_broadcaster_id: str
    viewer_count: int
    pending_raid: PendingRaid | None
    actions: tuple[RaidSignalAction, ...]
    reason: str | None = None

    @property
    def is_short_circuit(self) -> bool:
        return self.outcome == "secondary_signal_handled"


def _normalize_target_id(raw_value: str | None) -> str:
    return str(raw_value or "").strip()


def _normalize_detail(raw_value: object | None) -> str | None:
    text = str(raw_value or "").strip()
    return text or None


def _coerce_pending_raid(
    pending_raid: PendingRaid | Mapping[str, Any] | None,
    *,
    to_broadcaster_id: str,
    from_broadcaster_login: str,
) -> PendingRaid | None:
    return PendingRaid.from_payload(
        pending_raid,
        to_broadcaster_id=to_broadcaster_id,
        from_broadcaster_login=from_broadcaster_login,
    )


class RaidSignalCorrelationService:
    """Pure orchestration planner for raid signal correlation."""

    def plan_raid_arrival(
        self,
        *,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        from_broadcaster_login: str,
        from_broadcaster_id: str | None,
        viewer_count: int,
        pending_raid: PendingRaid | Mapping[str, Any] | None,
        recent_arrival_present: bool,
        independent_manual_detected: bool = False,
        manual_raid_source_key: str | None = None,
    ) -> RaidSignalPlan:
        normalized_from = normalize_broadcaster_login(from_broadcaster_login)
        normalized_to = normalize_broadcaster_login(to_broadcaster_login)
        target_id = _normalize_target_id(to_broadcaster_id)

        if recent_arrival_present:
            return self._secondary_signal_plan(
                signal_type="channel.raid",
                from_broadcaster_login=normalized_from,
                from_broadcaster_id=from_broadcaster_id,
                to_broadcaster_login=normalized_to,
                to_broadcaster_id=target_id,
                viewer_count=viewer_count,
            )

        pending = _coerce_pending_raid(
            pending_raid,
            to_broadcaster_id=target_id,
            from_broadcaster_login=normalized_from,
        )
        if pending is None:
            return self._independent_or_empty_plan(
                signal_type="channel.raid",
                from_broadcaster_login=normalized_from,
                from_broadcaster_id=from_broadcaster_id,
                to_broadcaster_login=normalized_to,
                to_broadcaster_id=target_id,
                viewer_count=viewer_count,
                independent_manual_detected=independent_manual_detected,
                manual_raid_source_key=manual_raid_source_key,
            )

        if pending.from_broadcaster_login != normalized_from:
            return RaidSignalPlan(
                signal_type="channel.raid",
                outcome="pending_mismatch",
                from_broadcaster_login=normalized_from,
                from_broadcaster_id=from_broadcaster_id,
                to_broadcaster_login=normalized_to,
                to_broadcaster_id=target_id,
                viewer_count=viewer_count,
                pending_raid=pending,
                actions=(
                    RaidSignalAction(
                        kind="record_pending_observation",
                        data={
                            "pending_raid": pending,
                            "signal_type": "channel.raid",
                            "status": "ignored",
                            "reason": "source_target_mismatch",
                            "detail": f"expected={pending.from_broadcaster_login} actual={normalized_from}",
                        },
                    ),
                    RaidSignalAction(kind="store_pending_raid", data={"pending_raid": pending}),
                ),
                reason="source_target_mismatch",
            )

        return RaidSignalPlan(
            signal_type="channel.raid",
            outcome="pending_matched",
            from_broadcaster_login=normalized_from,
            from_broadcaster_id=from_broadcaster_id,
            to_broadcaster_login=normalized_to,
            to_broadcaster_id=target_id,
            viewer_count=viewer_count,
            pending_raid=pending,
            actions=(
                RaidSignalAction(
                    kind="record_pending_observation",
                    data={
                        "pending_raid": pending,
                        "signal_type": "channel.raid",
                        "status": "matched_pending",
                    },
                ),
                RaidSignalAction(kind="store_pending_raid", data={"pending_raid": pending}),
                RaidSignalAction(
                    kind="confirm_pending_raid",
                    data={
                        "signal_type": "channel.raid",
                        "to_broadcaster_id": target_id,
                        "to_broadcaster_login": normalized_to,
                        "from_broadcaster_login": normalized_from,
                        "from_broadcaster_id": from_broadcaster_id,
                        "viewer_count": int(viewer_count or 0),
                    },
                ),
            ),
        )

    def plan_chat_notification(
        self,
        *,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        from_broadcaster_login: str,
        from_broadcaster_id: str | None,
        viewer_count: int,
        message_id: str | None,
        event_timestamp: str | None,
        pending_raid: PendingRaid | Mapping[str, Any] | None,
        recent_arrival_present: bool,
    ) -> RaidSignalPlan:
        normalized_from = normalize_broadcaster_login(from_broadcaster_login)
        normalized_to = normalize_broadcaster_login(to_broadcaster_login)
        target_id = _normalize_target_id(to_broadcaster_id)

        if recent_arrival_present:
            return self._secondary_signal_plan(
                signal_type="channel.chat.notification",
                from_broadcaster_login=normalized_from,
                from_broadcaster_id=from_broadcaster_id,
                to_broadcaster_login=normalized_to,
                to_broadcaster_id=target_id,
                viewer_count=viewer_count,
            )

        pending = _coerce_pending_raid(
            pending_raid,
            to_broadcaster_id=target_id,
            from_broadcaster_login=normalized_from,
        )
        if pending is None:
            return RaidSignalPlan(
                signal_type="channel.chat.notification",
                outcome="orphan_chat_notification",
                from_broadcaster_login=normalized_from,
                from_broadcaster_id=from_broadcaster_id,
                to_broadcaster_login=normalized_to,
                to_broadcaster_id=target_id,
                viewer_count=viewer_count,
                pending_raid=None,
                actions=(
                    RaidSignalAction(
                        kind="store_orphan_chat_notification",
                        data={
                            "payload": {
                                "to_broadcaster_id": target_id,
                                "to_broadcaster_login": normalized_to,
                                "from_broadcaster_id": str(from_broadcaster_id or "").strip() or None,
                                "from_broadcaster_login": normalized_from,
                                "viewer_count": int(viewer_count or 0),
                                "message_id": _normalize_detail(message_id),
                                "event_timestamp": _normalize_detail(event_timestamp),
                            }
                        },
                    ),
                ),
                reason="no_pending_raid",
            )

        if pending.from_broadcaster_login != normalized_from:
            return RaidSignalPlan(
                signal_type="channel.chat.notification",
                outcome="pending_mismatch",
                from_broadcaster_login=normalized_from,
                from_broadcaster_id=from_broadcaster_id,
                to_broadcaster_login=normalized_to,
                to_broadcaster_id=target_id,
                viewer_count=viewer_count,
                pending_raid=pending,
                actions=(
                    RaidSignalAction(
                        kind="record_pending_observation",
                        data={
                            "pending_raid": pending,
                            "signal_type": "channel.chat.notification",
                            "status": "ignored",
                            "reason": "source_target_mismatch",
                            "detail": f"expected={pending.from_broadcaster_login} actual={normalized_from}",
                        },
                    ),
                    RaidSignalAction(kind="store_pending_raid", data={"pending_raid": pending}),
                ),
                reason="source_target_mismatch",
            )

        return RaidSignalPlan(
            signal_type="channel.chat.notification",
            outcome="pending_matched",
            from_broadcaster_login=normalized_from,
            from_broadcaster_id=from_broadcaster_id,
            to_broadcaster_login=normalized_to,
            to_broadcaster_id=target_id,
            viewer_count=viewer_count,
            pending_raid=pending,
            actions=(
                RaidSignalAction(
                    kind="record_pending_observation",
                    data={
                        "pending_raid": pending,
                        "signal_type": "channel.chat.notification",
                        "status": "matched_pending",
                        "detail": _normalize_detail(message_id),
                    },
                ),
                RaidSignalAction(kind="store_pending_raid", data={"pending_raid": pending}),
                RaidSignalAction(
                    kind="confirm_pending_raid",
                    data={
                        "signal_type": "channel.chat.notification",
                        "to_broadcaster_id": target_id,
                        "to_broadcaster_login": normalized_to,
                        "from_broadcaster_login": normalized_from,
                        "from_broadcaster_id": from_broadcaster_id,
                        "viewer_count": int(viewer_count or 0),
                    },
                ),
            ),
        )

    def plan_chat_unraid(
        self,
        *,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        from_broadcaster_login: str,
        from_broadcaster_id: str | None,
        pending_raid: PendingRaid | Mapping[str, Any] | None,
        recent_arrival_present: bool,
        event_timestamp: str | None,
    ) -> RaidSignalPlan:
        normalized_from = normalize_broadcaster_login(from_broadcaster_login)
        normalized_to = normalize_broadcaster_login(to_broadcaster_login)
        target_id = _normalize_target_id(to_broadcaster_id)

        if recent_arrival_present:
            return self._secondary_signal_plan(
                signal_type="channel.chat.notification.unraid",
                from_broadcaster_login=normalized_from,
                from_broadcaster_id=from_broadcaster_id,
                to_broadcaster_login=normalized_to,
                to_broadcaster_id=target_id,
                viewer_count=0,
                unraid_seen=True,
            )

        pending = _coerce_pending_raid(
            pending_raid,
            to_broadcaster_id=target_id,
            from_broadcaster_login=normalized_from,
        )
        if pending is None:
            return RaidSignalPlan(
                signal_type="channel.chat.notification.unraid",
                outcome="no_pending",
                from_broadcaster_login=normalized_from,
                from_broadcaster_id=from_broadcaster_id,
                to_broadcaster_login=normalized_to,
                to_broadcaster_id=target_id,
                viewer_count=0,
                pending_raid=None,
                actions=(),
                reason=_normalize_detail(event_timestamp),
            )

        return RaidSignalPlan(
            signal_type="channel.chat.notification.unraid",
            outcome="pending_unraid_observed",
            from_broadcaster_login=normalized_from,
            from_broadcaster_id=from_broadcaster_id,
            to_broadcaster_login=normalized_to,
            to_broadcaster_id=target_id,
            viewer_count=0,
            pending_raid=pending,
            actions=(
                RaidSignalAction(
                    kind="record_pending_observation",
                    data={
                        "pending_raid": pending,
                        "signal_type": "channel.chat.notification.unraid",
                        "status": "diagnostic_only",
                        "reason": "unraid_does_not_confirm",
                        "detail": _normalize_detail(event_timestamp),
                    },
                ),
                RaidSignalAction(kind="store_pending_raid", data={"pending_raid": pending}),
            ),
            reason="unraid_does_not_confirm",
        )

    def _secondary_signal_plan(
        self,
        *,
        signal_type: RaidSignalType,
        from_broadcaster_login: str,
        from_broadcaster_id: str | None,
        to_broadcaster_login: str,
        to_broadcaster_id: str,
        viewer_count: int,
        unraid_seen: bool = False,
    ) -> RaidSignalPlan:
        return RaidSignalPlan(
            signal_type=signal_type,
            outcome="secondary_signal_handled",
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            viewer_count=int(viewer_count or 0),
            pending_raid=None,
            actions=(
                RaidSignalAction(
                    kind="record_secondary_signal",
                    data={
                        "signal_type": signal_type,
                        "from_broadcaster_login": from_broadcaster_login,
                        "from_broadcaster_id": from_broadcaster_id,
                        "to_broadcaster_login": to_broadcaster_login,
                        "to_broadcaster_id": to_broadcaster_id,
                        "viewer_count": int(viewer_count or 0),
                        "unraid_seen": bool(unraid_seen),
                    },
                ),
            ),
            reason="recent_arrival_present",
        )

    def _independent_or_empty_plan(
        self,
        *,
        signal_type: RaidSignalType,
        from_broadcaster_login: str,
        from_broadcaster_id: str | None,
        to_broadcaster_login: str,
        to_broadcaster_id: str,
        viewer_count: int,
        independent_manual_detected: bool,
        manual_raid_source_key: str | None,
    ) -> RaidSignalPlan:
        if not independent_manual_detected:
            return RaidSignalPlan(
                signal_type=signal_type,
                outcome="no_pending",
                from_broadcaster_login=from_broadcaster_login,
                from_broadcaster_id=from_broadcaster_id,
                to_broadcaster_login=to_broadcaster_login,
                to_broadcaster_id=to_broadcaster_id,
                viewer_count=int(viewer_count or 0),
                pending_raid=None,
                actions=(),
                reason="no_pending_raid",
            )

        actions: tuple[RaidSignalAction, ...]
        if manual_raid_source_key:
            actions = (
                RaidSignalAction(
                    kind="mark_manual_raid_started",
                    data={
                        "source_key": str(manual_raid_source_key or "").strip(),
                        "ttl_seconds": 180.0,
                    },
                ),
                RaidSignalAction(
                    kind="record_independent_raid_arrival",
                    data={
                        "signal_type": signal_type,
                        "from_broadcaster_login": from_broadcaster_login,
                        "from_broadcaster_id": from_broadcaster_id,
                        "to_broadcaster_login": to_broadcaster_login,
                        "to_broadcaster_id": to_broadcaster_id,
                        "viewer_count": int(viewer_count or 0),
                    },
                ),
            )
        else:
            actions = (
                RaidSignalAction(
                    kind="record_independent_raid_arrival",
                    data={
                        "signal_type": signal_type,
                        "from_broadcaster_login": from_broadcaster_login,
                        "from_broadcaster_id": from_broadcaster_id,
                        "to_broadcaster_login": to_broadcaster_login,
                        "to_broadcaster_id": to_broadcaster_id,
                        "viewer_count": int(viewer_count or 0),
                    },
                ),
            )

        return RaidSignalPlan(
            signal_type=signal_type,
            outcome="independent_manual_arrival",
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            viewer_count=int(viewer_count or 0),
            pending_raid=None,
            actions=actions,
            reason="independent_or_manual_raid_detected",
        )


__all__ = [
    "RaidSignalAction",
    "RaidSignalActionKind",
    "RaidSignalCorrelationService",
    "RaidSignalOutcome",
    "RaidSignalPlan",
    "RaidSignalType",
]
