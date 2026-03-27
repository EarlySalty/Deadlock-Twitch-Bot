from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Callable, Literal, Protocol

from .partner_resolution import PartnerRaidArrivalResolution, classify_partner_raid_arrival
from .pending_raids import PendingRaid

FollowUpKind = Literal["partner", "external", "suppressed_external"]
_UNSET = object()


class PartnerLookup(Protocol):
    def __call__(
        self,
        *,
        twitch_user_id: str | None = None,
        twitch_login: str | None = None,
    ) -> Any: ...


class KnownStreamerLookup(Protocol):
    def __call__(
        self,
        *,
        broadcaster_id: str | None = None,
        broadcaster_login: str | None = None,
    ) -> Any: ...


@dataclass(slots=True, frozen=True)
class ArrivalConfirmationDecision:
    signal_type: str
    pending_raid: PendingRaid
    raw_resolution: PartnerRaidArrivalResolution
    classification: str | None
    source_resolution: str
    follow_up_kind: FollowUpKind
    target_is_partner: bool
    pending_is_partner_raid: bool
    should_load_recent_raid_history_reference: bool
    should_delete_external_recruitment_blacklist_pending: bool
    should_refresh_partner_score_cache: bool
    should_track_confirmed_partner_raid: bool
    should_send_partner_raid_message: bool
    should_persist_confirmed_external_recruitment_raid: bool
    should_schedule_external_recruitment_blacklist_pending: bool
    should_send_recruitment_message: bool
    suppression_reason: str | None = None


FollowUpHook = Callable[[ArrivalConfirmationDecision], Any]


class ArrivalConfirmationService:
    def __init__(
        self,
        *,
        partner_lookup: PartnerLookup,
        known_streamer_lookup: KnownStreamerLookup,
    ) -> None:
        self._partner_lookup = partner_lookup
        self._known_streamer_lookup = known_streamer_lookup

    def confirm_pending_raid_arrival(
        self,
        *,
        pending_raid: PendingRaid | dict[str, Any] | None,
        signal_type: str,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        from_broadcaster_login: str,
        viewer_count: int,
        from_broadcaster_id: str | None = None,
        classification_override: str | None | object = _UNSET,
        source_resolution_override: str | None | object = _UNSET,
        target_is_partner_override: bool | None = None,
        on_partner_follow_up: FollowUpHook | None = None,
        on_external_follow_up: FollowUpHook | None = None,
        on_suppressed_external_follow_up: FollowUpHook | None = None,
    ) -> ArrivalConfirmationDecision | None:
        raid = PendingRaid.from_payload(
            pending_raid,
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )
        if raid is None:
            return None

        raw_resolution = classify_partner_raid_arrival(
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            partner_lookup=self._partner_lookup,
            known_streamer_lookup=self._known_streamer_lookup,
        )

        classification = (
            raw_resolution.classification
            if classification_override is _UNSET
            else classification_override
        )
        source_resolution = (
            raw_resolution.source_resolution
            if source_resolution_override is _UNSET
            else source_resolution_override
        )
        target_is_partner = (
            raw_resolution.target_is_partner
            if target_is_partner_override is None
            else bool(target_is_partner_override)
        )

        if raid.is_partner_raid and target_is_partner and classification != "ours_to_partner":
            classification = "ours_to_partner"
            source_resolution = "pending_partner_raid"

        if classification == "ours_to_partner":
            follow_up_kind: FollowUpKind = "partner"
            suppression_reason = None
        elif target_is_partner:
            follow_up_kind = "suppressed_external"
            suppression_reason = "partner_target_without_our_raid_confirmation"
        elif not target_is_partner and not raid.is_partner_raid:
            follow_up_kind = "external"
            suppression_reason = None
        elif not target_is_partner and raid.is_partner_raid:
            follow_up_kind = "suppressed_external"
            suppression_reason = "pending_partner_raid_later_resolved_non_partner"

        should_load_recent_raid_history_reference = raid.is_partner_raid or classification == "ours_to_partner"
        should_delete_external_recruitment_blacklist_pending = target_is_partner
        should_refresh_partner_score_cache = classification == "ours_to_partner"
        should_track_confirmed_partner_raid = classification == "ours_to_partner"
        should_send_partner_raid_message = classification == "ours_to_partner"
        should_persist_confirmed_external_recruitment_raid = follow_up_kind == "external"
        should_schedule_external_recruitment_blacklist_pending = follow_up_kind == "external"
        should_send_recruitment_message = follow_up_kind == "external"

        decision = ArrivalConfirmationDecision(
            signal_type=str(signal_type or "").strip(),
            pending_raid=raid,
            raw_resolution=raw_resolution,
            classification=classification,
            source_resolution=source_resolution,
            follow_up_kind=follow_up_kind,
            target_is_partner=target_is_partner,
            pending_is_partner_raid=bool(raid.is_partner_raid),
            should_load_recent_raid_history_reference=should_load_recent_raid_history_reference,
            should_delete_external_recruitment_blacklist_pending=should_delete_external_recruitment_blacklist_pending,
            should_refresh_partner_score_cache=should_refresh_partner_score_cache,
            should_track_confirmed_partner_raid=should_track_confirmed_partner_raid,
            should_send_partner_raid_message=should_send_partner_raid_message,
            should_persist_confirmed_external_recruitment_raid=should_persist_confirmed_external_recruitment_raid,
            should_schedule_external_recruitment_blacklist_pending=should_schedule_external_recruitment_blacklist_pending,
            should_send_recruitment_message=should_send_recruitment_message,
            suppression_reason=suppression_reason,
        )
        if decision.follow_up_kind == "partner" and on_partner_follow_up is not None:
            on_partner_follow_up(decision)
        elif decision.follow_up_kind == "external" and on_external_follow_up is not None:
            on_external_follow_up(decision)
        elif (
            decision.follow_up_kind == "suppressed_external"
            and on_suppressed_external_follow_up is not None
        ):
            on_suppressed_external_follow_up(decision)
        return decision


__all__ = [
    "ArrivalConfirmationDecision",
    "ArrivalConfirmationService",
    "FollowUpHook",
    "FollowUpKind",
    "KnownStreamerLookup",
    "PartnerLookup",
]
