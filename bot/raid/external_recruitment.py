from __future__ import annotations

from dataclasses import dataclass
from typing import Literal, Protocol

from .partner_resolution import normalize_broadcaster_login


class PersistConfirmedExternalRecruitmentRaid(Protocol):
    def __call__(
        self,
        *,
        raid_flow_id: str | None,
        from_broadcaster_id: str | None,
        from_broadcaster_login: str,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        viewer_count: int,
        confirmation_signal: str,
    ) -> int | None: ...


class CountConfirmedExternalRecruitmentRaids(Protocol):
    def __call__(self, to_broadcaster_id: str) -> int: ...


class SchedulePendingExternalRecruitmentBlacklist(Protocol):
    def __call__(
        self,
        *,
        target_id: str,
        target_login: str,
        confirmed_raid_count: int,
        raid_flow_id: str | None,
    ) -> None: ...


class DeletePendingExternalRecruitmentBlacklist(Protocol):
    def __call__(self, target_id: str) -> None: ...


class TargetPartnerLookup(Protocol):
    def __call__(self, *, target_id: str, target_login: str) -> bool: ...


BlacklistDecisionAction = Literal["noop", "scheduled", "cleared"]


@dataclass(slots=True, frozen=True)
class ExternalRecruitmentPolicy:
    raid_threshold: int = 4
    blacklist_grace_seconds: int = 48 * 3600


@dataclass(slots=True, frozen=True)
class ExternalRecruitmentRaidRecord:
    raid_flow_id: str | None
    from_broadcaster_id: str | None
    from_broadcaster_login: str
    to_broadcaster_id: str
    to_broadcaster_login: str
    viewer_count: int
    confirmation_signal: str


@dataclass(slots=True, frozen=True)
class ExternalRecruitmentRaidRecordResult:
    record: ExternalRecruitmentRaidRecord
    persisted_count: int | None
    persisted: bool
    used_count_fallback: bool
    error: str | None = None


@dataclass(slots=True, frozen=True)
class ExternalRecruitmentBlacklistDecision:
    action: BlacklistDecisionAction
    reason: str
    target_id: str
    target_login: str
    confirmed_raid_count: int
    threshold: int
    raid_flow_id: str | None = None

    @property
    def should_schedule(self) -> bool:
        return self.action == "scheduled"

    @property
    def should_clear(self) -> bool:
        return self.action == "cleared"


class ExternalRecruitmentService:
    def __init__(
        self,
        *,
        persist_confirmed_raid: PersistConfirmedExternalRecruitmentRaid,
        count_confirmed_raids: CountConfirmedExternalRecruitmentRaids | None,
        schedule_pending_blacklist: SchedulePendingExternalRecruitmentBlacklist,
        delete_pending_blacklist: DeletePendingExternalRecruitmentBlacklist,
        is_target_partner: TargetPartnerLookup,
        policy: ExternalRecruitmentPolicy | None = None,
    ) -> None:
        self._persist_confirmed_raid = persist_confirmed_raid
        self._count_confirmed_raids = count_confirmed_raids
        self._schedule_pending_blacklist = schedule_pending_blacklist
        self._delete_pending_blacklist = delete_pending_blacklist
        self._is_target_partner = is_target_partner
        self._policy = policy or ExternalRecruitmentPolicy()

    def record_confirmed_raid(
        self,
        *,
        raid_flow_id: str | None,
        from_broadcaster_id: str | None,
        from_broadcaster_login: str,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        viewer_count: int,
        confirmation_signal: str,
    ) -> ExternalRecruitmentRaidRecordResult:
        record = ExternalRecruitmentRaidRecord(
            raid_flow_id=str(raid_flow_id or "").strip() or None,
            from_broadcaster_id=str(from_broadcaster_id or "").strip() or None,
            from_broadcaster_login=normalize_broadcaster_login(from_broadcaster_login),
            to_broadcaster_id=str(to_broadcaster_id or "").strip(),
            to_broadcaster_login=normalize_broadcaster_login(to_broadcaster_login),
            viewer_count=int(viewer_count or 0),
            confirmation_signal=str(confirmation_signal or "").strip(),
        )
        if not record.to_broadcaster_id or not record.to_broadcaster_login:
            return ExternalRecruitmentRaidRecordResult(
                record=record,
                persisted_count=None,
                persisted=False,
                used_count_fallback=False,
                error="missing_target_identity",
            )

        persisted_count: int | None = None
        used_count_fallback = False
        error: str | None = None
        try:
            persisted_count = self._persist_confirmed_raid(
                raid_flow_id=record.raid_flow_id,
                from_broadcaster_id=record.from_broadcaster_id,
                from_broadcaster_login=record.from_broadcaster_login,
                to_broadcaster_id=record.to_broadcaster_id,
                to_broadcaster_login=record.to_broadcaster_login,
                viewer_count=record.viewer_count,
                confirmation_signal=record.confirmation_signal,
            )
        except Exception:
            error = "persist_failed"

        if persisted_count is None and self._count_confirmed_raids is not None:
            try:
                persisted_count = int(self._count_confirmed_raids(record.to_broadcaster_id))
                used_count_fallback = True
            except Exception:
                if error is None:
                    error = "count_failed"

        return ExternalRecruitmentRaidRecordResult(
            record=record,
            persisted_count=persisted_count,
            persisted=persisted_count is not None,
            used_count_fallback=used_count_fallback,
            error=error,
        )

    def maybe_schedule_blacklist(
        self,
        *,
        target_id: str,
        target_login: str,
        confirmed_raid_count: int,
        raid_flow_id: str | None,
    ) -> ExternalRecruitmentBlacklistDecision:
        normalized_id = str(target_id or "").strip()
        normalized_login = normalize_broadcaster_login(target_login)
        count = int(confirmed_raid_count or 0)
        if not normalized_id or not normalized_login:
            return ExternalRecruitmentBlacklistDecision(
                action="noop",
                reason="missing_target_identity",
                target_id=normalized_id,
                target_login=normalized_login,
                confirmed_raid_count=count,
                threshold=self._policy.raid_threshold,
                raid_flow_id=str(raid_flow_id or "").strip() or None,
            )

        if count < self._policy.raid_threshold:
            return ExternalRecruitmentBlacklistDecision(
                action="noop",
                reason="threshold_not_reached",
                target_id=normalized_id,
                target_login=normalized_login,
                confirmed_raid_count=count,
                threshold=self._policy.raid_threshold,
                raid_flow_id=str(raid_flow_id or "").strip() or None,
            )

        if self._is_target_partner(target_id=normalized_id, target_login=normalized_login):
            self._delete_pending_blacklist(normalized_id)
            return ExternalRecruitmentBlacklistDecision(
                action="cleared",
                reason="target_is_partner",
                target_id=normalized_id,
                target_login=normalized_login,
                confirmed_raid_count=count,
                threshold=self._policy.raid_threshold,
                raid_flow_id=str(raid_flow_id or "").strip() or None,
            )

        self._schedule_pending_blacklist(
            target_id=normalized_id,
            target_login=normalized_login,
            confirmed_raid_count=count,
            raid_flow_id=str(raid_flow_id or "").strip() or None,
        )
        return ExternalRecruitmentBlacklistDecision(
            action="scheduled",
            reason="threshold_reached",
            target_id=normalized_id,
            target_login=normalized_login,
            confirmed_raid_count=count,
            threshold=self._policy.raid_threshold,
            raid_flow_id=str(raid_flow_id or "").strip() or None,
        )

    def clear_pending_blacklist(
        self,
        *,
        target_id: str,
        target_login: str,
    ) -> ExternalRecruitmentBlacklistDecision:
        normalized_id = str(target_id or "").strip()
        normalized_login = normalize_broadcaster_login(target_login)
        if not normalized_id or not normalized_login:
            return ExternalRecruitmentBlacklistDecision(
                action="noop",
                reason="missing_target_identity",
                target_id=normalized_id,
                target_login=normalized_login,
                confirmed_raid_count=0,
                threshold=self._policy.raid_threshold,
            )

        if not self._is_target_partner(target_id=normalized_id, target_login=normalized_login):
            return ExternalRecruitmentBlacklistDecision(
                action="noop",
                reason="target_not_partner",
                target_id=normalized_id,
                target_login=normalized_login,
                confirmed_raid_count=0,
                threshold=self._policy.raid_threshold,
            )

        self._delete_pending_blacklist(normalized_id)
        return ExternalRecruitmentBlacklistDecision(
            action="cleared",
            reason="target_is_partner",
            target_id=normalized_id,
            target_login=normalized_login,
            confirmed_raid_count=0,
            threshold=self._policy.raid_threshold,
        )


__all__ = [
    "BlacklistDecisionAction",
    "CountConfirmedExternalRecruitmentRaids",
    "DeletePendingExternalRecruitmentBlacklist",
    "ExternalRecruitmentBlacklistDecision",
    "ExternalRecruitmentPolicy",
    "ExternalRecruitmentRaidRecord",
    "ExternalRecruitmentRaidRecordResult",
    "ExternalRecruitmentService",
    "PersistConfirmedExternalRecruitmentRaid",
    "SchedulePendingExternalRecruitmentBlacklist",
    "TargetPartnerLookup",
]
