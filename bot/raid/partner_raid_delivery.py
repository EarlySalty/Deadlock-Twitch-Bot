from __future__ import annotations

from dataclasses import dataclass
from typing import Literal


PartnerRaidDeliveryStatus = Literal["ready", "blocked"]
PartnerRaidViewerWord = Literal["Viewer", "Viewern"]


@dataclass(slots=True, frozen=True)
class PartnerRaidDeliveryConfig:
    delay_seconds: float = 5.0


@dataclass(slots=True, frozen=True)
class PartnerRaidDeliveryRequest:
    from_broadcaster_login: str
    to_broadcaster_login: str
    to_broadcaster_id: str | None
    viewer_count: int
    received_raid_count: int
    chat_bot_available: bool = True
    outbound_chat_suppressed: bool = False


@dataclass(slots=True, frozen=True)
class PartnerRaidDeliveryPlan:
    status: PartnerRaidDeliveryStatus
    reason: str | None
    delay_seconds: float
    target_id: str | None
    target_login: str
    from_login: str
    viewer_count: int
    viewer_word: PartnerRaidViewerWord | None
    received_raid_count: int
    message: str | None
    prerequisites: tuple[str, ...]

    @property
    def should_deliver(self) -> bool:
        return self.status == "ready"


class PartnerRaidDeliveryPlanner:
    def __init__(self, config: PartnerRaidDeliveryConfig | None = None) -> None:
        self._config = config or PartnerRaidDeliveryConfig()

    @property
    def config(self) -> PartnerRaidDeliveryConfig:
        return self._config

    def plan(self, request: PartnerRaidDeliveryRequest) -> PartnerRaidDeliveryPlan:
        target_id = str(request.to_broadcaster_id or "").strip() or None
        target_login = str(request.to_broadcaster_login or "").strip().lower()
        from_login = str(request.from_broadcaster_login or "").strip().lower()
        viewer_count = max(0, int(request.viewer_count or 0))
        received_raid_count = max(0, int(request.received_raid_count or 0))

        if not request.chat_bot_available:
            return self._blocked(
                reason="chat_bot_unavailable",
                target_id=target_id,
                target_login=target_login,
                from_login=from_login,
                viewer_count=viewer_count,
                received_raid_count=received_raid_count,
                prerequisites=("chat_bot_available",),
            )

        if not target_id:
            return self._blocked(
                reason="target_id_unresolved",
                target_id=None,
                target_login=target_login,
                from_login=from_login,
                viewer_count=viewer_count,
                received_raid_count=received_raid_count,
                prerequisites=("target_id_resolved",),
            )

        if request.outbound_chat_suppressed:
            return self._blocked(
                reason="outbound_chat_suppressed",
                target_id=target_id,
                target_login=target_login,
                from_login=from_login,
                viewer_count=viewer_count,
                received_raid_count=received_raid_count,
                prerequisites=("outbound_chat_unsuppressed",),
            )

        viewer_word = self._viewer_word(viewer_count)
        message = (
            f"Hey @{target_login}! 🎮 "
            f"@{from_login} hat dich gerade mit {viewer_count} {viewer_word} geraidet. "
            f"Das ist dein Raid Nr. {received_raid_count} aus dem Deadlock Streamer-Netzwerk. ❤️"
        )
        return PartnerRaidDeliveryPlan(
            status="ready",
            reason=None,
            delay_seconds=float(self._config.delay_seconds),
            target_id=target_id,
            target_login=target_login,
            from_login=from_login,
            viewer_count=viewer_count,
            viewer_word=viewer_word,
            received_raid_count=received_raid_count,
            message=message,
            prerequisites=(
                "chat_bot_available",
                "target_id_resolved",
                "outbound_chat_unsuppressed",
                "delay_elapsed",
            ),
        )

    def _blocked(
        self,
        *,
        reason: str,
        target_id: str | None,
        target_login: str,
        from_login: str,
        viewer_count: int,
        received_raid_count: int,
        prerequisites: tuple[str, ...],
    ) -> PartnerRaidDeliveryPlan:
        return PartnerRaidDeliveryPlan(
            status="blocked",
            reason=reason,
            delay_seconds=float(self._config.delay_seconds),
            target_id=target_id,
            target_login=target_login,
            from_login=from_login,
            viewer_count=viewer_count,
            viewer_word=None,
            received_raid_count=received_raid_count,
            message=None,
            prerequisites=prerequisites,
        )

    @staticmethod
    def _viewer_word(viewer_count: int) -> PartnerRaidViewerWord:
        return "Viewer" if int(viewer_count or 0) == 1 else "Viewern"


def plan_partner_raid_delivery(
    request: PartnerRaidDeliveryRequest,
    *,
    config: PartnerRaidDeliveryConfig | None = None,
) -> PartnerRaidDeliveryPlan:
    return PartnerRaidDeliveryPlanner(config).plan(request)


__all__ = [
    "PartnerRaidDeliveryConfig",
    "PartnerRaidDeliveryPlan",
    "PartnerRaidDeliveryPlanner",
    "PartnerRaidDeliveryRequest",
    "PartnerRaidDeliveryStatus",
    "PartnerRaidViewerWord",
    "plan_partner_raid_delivery",
]
