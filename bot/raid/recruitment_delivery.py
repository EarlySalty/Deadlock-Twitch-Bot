from __future__ import annotations

from dataclasses import dataclass
from typing import Literal


RecruitmentDeliveryStatus = Literal["ready", "blocked"]
RecruitmentMessageVariant = Literal["intro", "second", "hattrick", "support"]
RecruitmentInviteVariant = Literal["direct", "standard"]


@dataclass(slots=True, frozen=True)
class RecruitmentDeliveryConfig:
    delay_seconds: float = 15.0
    recent_raid_threshold: int = 2
    max_recruitment_messages: int = 4
    direct_invite_max_followers: int = 120


@dataclass(slots=True, frozen=True)
class RecruitmentDeliveryRequest:
    from_broadcaster_login: str
    to_broadcaster_login: str
    target_id: str | None
    recent_raid_count: int
    total_recruitment_raid_count: int | None
    followers_total: int | None = None
    chat_bot_available: bool = True
    outbound_chat_suppressed: bool = False


@dataclass(slots=True, frozen=True)
class RecruitmentDeliveryPlan:
    status: RecruitmentDeliveryStatus
    reason: str | None
    delay_seconds: float
    target_id: str | None
    target_login: str
    recent_raid_count: int
    total_recruitment_raid_count: int | None
    message_variant: RecruitmentMessageVariant | None
    invite_variant: RecruitmentInviteVariant | None
    prerequisites: tuple[str, ...]

    @property
    def should_deliver(self) -> bool:
        return self.status == "ready"


class RecruitmentDeliveryPlanner:
    def __init__(self, config: RecruitmentDeliveryConfig | None = None) -> None:
        self._config = config or RecruitmentDeliveryConfig()

    @property
    def config(self) -> RecruitmentDeliveryConfig:
        return self._config

    def plan(self, request: RecruitmentDeliveryRequest) -> RecruitmentDeliveryPlan:
        target_id = str(request.target_id or "").strip() or None
        target_login = str(request.to_broadcaster_login or "").strip().lower()
        recent_raid_count = max(0, int(request.recent_raid_count or 0))
        total_recruitment_raid_count = (
            int(request.total_recruitment_raid_count)
            if request.total_recruitment_raid_count is not None
            else None
        )
        followers_total = (
            int(request.followers_total)
            if request.followers_total is not None
            else None
        )

        if not request.chat_bot_available:
            return self._blocked(
                reason="chat_bot_unavailable",
                target_id=target_id,
                target_login=target_login,
                recent_raid_count=recent_raid_count,
                total_recruitment_raid_count=total_recruitment_raid_count,
                prerequisites=("chat_bot_available",),
            )

        if not target_id:
            return self._blocked(
                reason="target_id_unresolved",
                target_id=None,
                target_login=target_login,
                recent_raid_count=recent_raid_count,
                total_recruitment_raid_count=total_recruitment_raid_count,
                prerequisites=("target_id_resolved",),
            )

        if request.outbound_chat_suppressed:
            return self._blocked(
                reason="outbound_chat_suppressed",
                target_id=target_id,
                target_login=target_login,
                recent_raid_count=recent_raid_count,
                total_recruitment_raid_count=total_recruitment_raid_count,
                prerequisites=("outbound_chat_unsuppressed",),
            )

        if recent_raid_count > self._config.recent_raid_threshold:
            return self._blocked(
                reason="recent_raids_exceed_threshold",
                target_id=target_id,
                target_login=target_login,
                recent_raid_count=recent_raid_count,
                total_recruitment_raid_count=total_recruitment_raid_count,
                prerequisites=("recent_raid_count_within_threshold",),
            )

        if total_recruitment_raid_count is None:
            return self._blocked(
                reason="total_recruitment_raid_count_unresolved",
                target_id=target_id,
                target_login=target_login,
                recent_raid_count=recent_raid_count,
                total_recruitment_raid_count=None,
                prerequisites=("total_recruitment_raid_count_resolved",),
            )

        if total_recruitment_raid_count > self._config.max_recruitment_messages:
            return self._blocked(
                reason="max_recruitment_messages_reached",
                target_id=target_id,
                target_login=target_login,
                recent_raid_count=recent_raid_count,
                total_recruitment_raid_count=total_recruitment_raid_count,
                prerequisites=("recruitment_message_budget_available",),
            )

        message_variant = self._message_variant(total_recruitment_raid_count)
        invite_variant = self._invite_variant(followers_total)
        return RecruitmentDeliveryPlan(
            status="ready",
            reason=None,
            delay_seconds=float(self._config.delay_seconds),
            target_id=target_id,
            target_login=target_login,
            recent_raid_count=recent_raid_count,
            total_recruitment_raid_count=total_recruitment_raid_count,
            message_variant=message_variant,
            invite_variant=invite_variant,
            prerequisites=(
                "target_id_resolved",
                "outbound_chat_unsuppressed",
                "recent_raid_count_within_threshold",
                "total_recruitment_raid_count_resolved",
                "delay_elapsed",
            ),
        )

    def _blocked(
        self,
        *,
        reason: str,
        target_id: str | None,
        target_login: str,
        recent_raid_count: int,
        total_recruitment_raid_count: int | None,
        prerequisites: tuple[str, ...],
    ) -> RecruitmentDeliveryPlan:
        return RecruitmentDeliveryPlan(
            status="blocked",
            reason=reason,
            delay_seconds=float(self._config.delay_seconds),
            target_id=target_id,
            target_login=target_login,
            recent_raid_count=recent_raid_count,
            total_recruitment_raid_count=total_recruitment_raid_count,
            message_variant=None,
            invite_variant=None,
            prerequisites=prerequisites,
        )

    def _message_variant(self, total_recruitment_raid_count: int) -> RecruitmentMessageVariant:
        if total_recruitment_raid_count <= 1:
            return "intro"
        if total_recruitment_raid_count == 2:
            return "second"
        if total_recruitment_raid_count == 3:
            return "hattrick"
        return "support"

    def _invite_variant(self, followers_total: int | None) -> RecruitmentInviteVariant:
        if followers_total is not None and followers_total <= self._config.direct_invite_max_followers:
            return "direct"
        return "standard"


def plan_recruitment_delivery(
    request: RecruitmentDeliveryRequest,
    *,
    config: RecruitmentDeliveryConfig | None = None,
) -> RecruitmentDeliveryPlan:
    return RecruitmentDeliveryPlanner(config).plan(request)


__all__ = [
    "RecruitmentDeliveryConfig",
    "RecruitmentDeliveryPlan",
    "RecruitmentDeliveryPlanner",
    "RecruitmentDeliveryRequest",
    "RecruitmentDeliveryStatus",
    "RecruitmentInviteVariant",
    "RecruitmentMessageVariant",
    "plan_recruitment_delivery",
]
