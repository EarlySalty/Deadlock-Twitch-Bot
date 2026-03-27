from __future__ import annotations

import asyncio
import inspect
import logging
from dataclasses import dataclass
from typing import Any, Awaitable, Literal, Protocol

from .chat_targets import make_chat_target


log = logging.getLogger("TwitchStreams.RaidManager")


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


class GetChatBot(Protocol):
    def __call__(self) -> Any | None: ...


class CountReceivedNetworkRaids(Protocol):
    def __call__(self, to_broadcaster_id: str) -> int: ...


class LookupOutboundChatSuppression(Protocol):
    def __call__(
        self,
        *,
        target_login: str,
        target_id: str | None,
        source: str,
    ) -> dict[str, Any] | None: ...


class JoinChatChannel(Protocol):
    def __call__(
        self,
        chat_bot: Any,
        channel_login: str,
        channel_id: str | None,
    ) -> Awaitable[Any] | Any: ...


class SendChatMessage(Protocol):
    def __call__(
        self,
        chat_bot: Any,
        channel: Any,
        message: str,
        source: str,
    ) -> Awaitable[bool | None] | bool | None: ...


class SleepFn(Protocol):
    def __call__(self, seconds: float) -> Awaitable[None]: ...


@dataclass(slots=True, frozen=True)
class PartnerRaidDeliveryDependencies:
    get_chat_bot: GetChatBot
    count_received_network_raids: CountReceivedNetworkRaids
    lookup_outbound_chat_suppression: LookupOutboundChatSuppression
    join_chat_channel: JoinChatChannel
    send_chat_message: SendChatMessage
    sleep: SleepFn = asyncio.sleep
    logger: logging.Logger = log


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


async def _maybe_await(value: object) -> object:
    if inspect.isawaitable(value):
        return await value
    return value


class PartnerRaidDeliveryService:
    def __init__(
        self,
        dependencies: PartnerRaidDeliveryDependencies,
        planner: PartnerRaidDeliveryPlanner | None = None,
    ) -> None:
        self._deps = dependencies
        self._planner = planner or PartnerRaidDeliveryPlanner()

    async def send_partner_raid_message(
        self,
        *,
        from_broadcaster_login: str,
        to_broadcaster_login: str,
        to_broadcaster_id: str,
        viewer_count: int,
    ) -> None:
        join_chat_bot = self._deps.get_chat_bot()
        if not join_chat_bot:
            self._deps.logger.debug("Chat bot not available for partner raid message")
            return

        try:
            suppression = self._deps.lookup_outbound_chat_suppression(
                target_login=to_broadcaster_login,
                target_id=to_broadcaster_id,
                source="partner_raid",
            )
            if suppression is not None:
                self._deps.logger.info(
                    "Skipping partner raid message to %s due stored chat suppression (code=%s, until=%s)",
                    to_broadcaster_login,
                    suppression.get("reason_code") or "unknown",
                    suppression.get("suppressed_until") or "-",
                )
                return

            received_raid_count = self._deps.count_received_network_raids(to_broadcaster_id)
            if received_raid_count <= 0:
                received_raid_count = 1

            plan = self._planner.plan(
                PartnerRaidDeliveryRequest(
                    from_broadcaster_login=from_broadcaster_login,
                    to_broadcaster_login=to_broadcaster_login,
                    to_broadcaster_id=to_broadcaster_id,
                    viewer_count=viewer_count,
                    received_raid_count=received_raid_count,
                    chat_bot_available=True,
                    outbound_chat_suppressed=False,
                )
            )
            if not plan.should_deliver or not plan.message:
                self._deps.logger.info(
                    "Skipping partner raid message to %s (%s)",
                    to_broadcaster_login,
                    plan.reason or "blocked",
                )
                return

            target_channel = make_chat_target(to_broadcaster_login, to_broadcaster_id)
            await _maybe_await(
                self._deps.join_chat_channel(
                    join_chat_bot,
                    to_broadcaster_login,
                    to_broadcaster_id,
                )
            )
            await self._deps.sleep(plan.delay_seconds)

            send_chat_bot = self._deps.get_chat_bot()
            if not send_chat_bot:
                self._deps.logger.debug(
                    "Chat bot not available anymore for partner raid message to %s",
                    to_broadcaster_login,
                )
                return

            if send_chat_bot is not join_chat_bot:
                await _maybe_await(
                    self._deps.join_chat_channel(
                        send_chat_bot,
                        to_broadcaster_login,
                        to_broadcaster_id,
                    )
                )

            success = await _maybe_await(
                self._deps.send_chat_message(
                    send_chat_bot,
                    target_channel,
                    plan.message,
                    "partner_raid",
                )
            )

            if success is None:
                self._deps.logger.debug(
                    "Chat bot does not have _send_chat_message method, skipping partner raid message to %s",
                    to_broadcaster_login,
                )
                return

            if success:
                self._deps.logger.info(
                    "✅ Sent partner raid message to %s (raided by %s with %d viewers, network_raid_no=%d)",
                    to_broadcaster_login,
                    from_broadcaster_login,
                    viewer_count,
                    received_raid_count,
                )
            else:
                self._deps.logger.warning(
                    "Failed to send partner raid message to %s",
                    to_broadcaster_login,
                )
        except Exception:
            self._deps.logger.exception(
                "Failed to send partner raid message to %s (raided by %s)",
                to_broadcaster_login,
                from_broadcaster_login,
            )


def plan_partner_raid_delivery(
    request: PartnerRaidDeliveryRequest,
    *,
    config: PartnerRaidDeliveryConfig | None = None,
) -> PartnerRaidDeliveryPlan:
    return PartnerRaidDeliveryPlanner(config).plan(request)


__all__ = [
    "PartnerRaidDeliveryConfig",
    "PartnerRaidDeliveryPlan",
    "PartnerRaidDeliveryDependencies",
    "PartnerRaidDeliveryService",
    "PartnerRaidDeliveryPlanner",
    "PartnerRaidDeliveryRequest",
    "PartnerRaidDeliveryStatus",
    "PartnerRaidViewerWord",
    "plan_partner_raid_delivery",
]
