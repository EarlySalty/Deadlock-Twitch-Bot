from __future__ import annotations

import logging
from dataclasses import dataclass
from typing import Any, Awaitable, Protocol, Sequence

from .partner_resolution import normalize_broadcaster_login


log = logging.getLogger("TwitchStreams.RaidManager")


class LoadBlacklistRows(Protocol):
    def __call__(self) -> Sequence[Any] | list[Any] | tuple[Any, ...] | Any: ...


class IsBlacklisted(Protocol):
    def __call__(self, *, target_id: str, target_login: str) -> bool: ...


class StoreBlacklistEntry(Protocol):
    def __call__(self, *, target_id: str | None, target_login: str, reason: str) -> None: ...


class ScheduleExternalRecruitmentBlacklistPending(Protocol):
    def __call__(
        self,
        *,
        target_id: str,
        target_login: str,
        confirmed_raid_count: int,
        raid_flow_id: str | None,
        grace_seconds: int,
    ) -> None: ...


class DeleteExternalRecruitmentBlacklistPending(Protocol):
    def __call__(self, target_id: str) -> None: ...


class LoadDueExternalRecruitmentBlacklistPending(Protocol):
    def __call__(self) -> Sequence[Any] | list[Any] | tuple[Any, ...] | Any: ...


class IsTargetPartner(Protocol):
    def __call__(self, *, target_id: str, target_login: str) -> bool: ...


class ScheduleExternalTargetBanCheck(Protocol):
    def __call__(
        self,
        *,
        target_id: str | None,
        target_login: str,
        source: str,
        delay_seconds: int,
    ) -> None: ...


class DeleteExternalTargetBanCheckPending(Protocol):
    def __call__(self, target_id: str) -> None: ...


class RescheduleExternalTargetBanCheckPending(Protocol):
    def __call__(self, target_id: str, delay_seconds: int) -> None: ...


class LoadDueExternalTargetBanChecks(Protocol):
    def __call__(self) -> Sequence[Any] | list[Any] | tuple[Any, ...] | Any: ...


class GetChatBot(Protocol):
    def __call__(self) -> Any | None: ...


class PartChatChannels(Protocol):
    def __call__(self, chat_bot: Any, channels: Sequence[str]) -> Awaitable[None]: ...


class JoinChatChannel(Protocol):
    def __call__(self, chat_bot: Any, channel_login: str, channel_id: str | None) -> Awaitable[bool]: ...


@dataclass(slots=True, frozen=True)
class RaidBlacklistConfig:
    external_recruitment_raid_limit: int = 4
    external_recruitment_blacklist_grace_seconds: int = 48 * 3600
    external_target_ban_check_delay_seconds: int = 3600
    external_target_ban_check_reschedule_seconds: int = 900


@dataclass(slots=True, frozen=True)
class RaidBlacklistDependencies:
    load_blacklist_rows: LoadBlacklistRows
    is_blacklisted: IsBlacklisted
    store_blacklist_entry: StoreBlacklistEntry
    load_due_external_recruitment_blacklist_pending: LoadDueExternalRecruitmentBlacklistPending
    schedule_external_recruitment_blacklist_pending: ScheduleExternalRecruitmentBlacklistPending
    delete_external_recruitment_blacklist_pending: DeleteExternalRecruitmentBlacklistPending
    is_target_partner: IsTargetPartner
    load_due_external_target_ban_checks: LoadDueExternalTargetBanChecks
    schedule_external_target_ban_check: ScheduleExternalTargetBanCheck
    delete_external_target_ban_check_pending: DeleteExternalTargetBanCheckPending
    reschedule_external_target_ban_check_pending: RescheduleExternalTargetBanCheckPending
    get_chat_bot: GetChatBot
    join_chat_channel: JoinChatChannel
    part_chat_channels: PartChatChannels | None = None


RaidBlacklistCallbacks = RaidBlacklistDependencies


def _normalize_target_id(raw_value: Any) -> str:
    return str(raw_value or "").strip()


def _row_value(row: Any, key: str, index: int, default: Any = None) -> Any:
    if row is None:
        return default
    try:
        return row[key]
    except Exception:
        pass
    try:
        return row[index]
    except Exception:
        pass
    try:
        return getattr(row, key)
    except Exception:
        return default


def _normalize_pending_target(row: Any, key: str, index: int) -> str:
    return normalize_broadcaster_login(_row_value(row, key, index))


class RaidBlacklistService:
    def __init__(
        self,
        dependencies: RaidBlacklistDependencies,
        config: RaidBlacklistConfig | None = None,
    ) -> None:
        self._deps = dependencies
        self._config = config or RaidBlacklistConfig()

    @property
    def config(self) -> RaidBlacklistConfig:
        return self._config

    def is_blacklisted(self, target_id: str, target_login: str) -> bool:
        normalized_id = _normalize_target_id(target_id)
        normalized_login = normalize_broadcaster_login(target_login)
        if not normalized_id and not normalized_login:
            return False
        try:
            return bool(
                self._deps.is_blacklisted(
                    target_id=normalized_id,
                    target_login=normalized_login,
                )
            )
        except Exception:
            log.error("Error checking blacklist", exc_info=True)
            return False

    def load_raid_blacklist(self) -> tuple[set[str], set[str]]:
        blacklisted_ids: set[str] = set()
        blacklisted_logins: set[str] = set()
        try:
            rows = self._deps.load_blacklist_rows() or []
        except Exception:
            log.error("Error loading raid blacklist", exc_info=True)
            return blacklisted_ids, blacklisted_logins

        for row in rows:
            target_id = _normalize_target_id(_row_value(row, "target_id", 0))
            target_login = normalize_broadcaster_login(_row_value(row, "target_login", 1))
            if target_id:
                blacklisted_ids.add(target_id)
            if target_login:
                blacklisted_logins.add(target_login)

        return blacklisted_ids, blacklisted_logins

    def add_to_blacklist(self, target_id: str, target_login: str, reason: str) -> None:
        normalized_id = _normalize_target_id(target_id) or None
        normalized_login = normalize_broadcaster_login(target_login)
        if not normalized_login:
            return
        try:
            self._deps.store_blacklist_entry(
                target_id=normalized_id,
                target_login=normalized_login,
                reason=str(reason or ""),
            )
            log.info(
                "Added %s (ID: %s) to raid blacklist. Reason: %s",
                normalized_login,
                normalized_id,
                reason,
            )
        except Exception:
            log.error("Error adding to blacklist", exc_info=True)

    def schedule_external_recruitment_blacklist_pending(
        self,
        *,
        target_id: str,
        target_login: str,
        confirmed_raid_count: int,
        raid_flow_id: str | None,
    ) -> None:
        normalized_id = _normalize_target_id(target_id)
        normalized_login = normalize_broadcaster_login(target_login)
        count = max(0, int(confirmed_raid_count or 0))
        if not normalized_id or not normalized_login:
            return
        if count < self._config.external_recruitment_raid_limit:
            return

        try:
            if self._safe_is_target_partner(normalized_id, normalized_login):
                self._deps.delete_external_recruitment_blacklist_pending(normalized_id)
                return
        except Exception:
            log.debug(
                "Partner check before delayed external recruitment blacklist failed for %s (%s)",
                normalized_login,
                normalized_id,
                exc_info=True,
            )

        try:
            self._deps.schedule_external_recruitment_blacklist_pending(
                target_id=normalized_id,
                target_login=normalized_login,
                confirmed_raid_count=count,
                raid_flow_id=str(raid_flow_id or "").strip() or None,
                grace_seconds=self._config.external_recruitment_blacklist_grace_seconds,
            )
        except Exception:
            log.exception(
                "Failed to schedule delayed external recruitment blacklist for %s (%s)",
                normalized_login,
                normalized_id,
            )

    def delete_external_recruitment_blacklist_pending(self, target_id: str) -> None:
        normalized_id = _normalize_target_id(target_id)
        if not normalized_id:
            return
        try:
            self._deps.delete_external_recruitment_blacklist_pending(normalized_id)
        except Exception:
            log.debug(
                "Failed to delete delayed external recruitment blacklist for %s",
                normalized_id,
                exc_info=True,
            )

    def process_due_external_recruitment_blacklist_pending(self) -> None:
        try:
            rows = self._deps.load_due_external_recruitment_blacklist_pending() or []
        except Exception:
            log.debug("Failed to load due delayed external recruitment blacklists", exc_info=True)
            return

        for row in rows:
            target_id = _normalize_target_id(_row_value(row, "target_id", 0))
            target_login = _normalize_pending_target(row, "target_login", 1)
            confirmed_raid_count = self._safe_int(_row_value(row, "confirmed_raid_count", 2), 0)
            threshold_reached_at = str(_row_value(row, "threshold_reached_at", 3) or "").strip()
            if not target_id or not target_login:
                continue

            if self.is_blacklisted(target_id, target_login):
                self.delete_external_recruitment_blacklist_pending(target_id)
                continue

            if self._safe_is_target_partner(target_id, target_login):
                self.delete_external_recruitment_blacklist_pending(target_id)
                continue

            reason = (
                "confirmed_external_recruitment_limit_grace_expired:"
                f" count={confirmed_raid_count}"
                f" limit={self._config.external_recruitment_raid_limit}"
                f" threshold_reached_at={threshold_reached_at or '-'}"
            )
            self.add_to_blacklist(target_id, target_login, reason)
            self.delete_external_recruitment_blacklist_pending(target_id)

    def schedule_external_target_ban_check(
        self,
        *,
        target_id: str | None,
        target_login: str,
        source: str,
    ) -> None:
        normalized_id = _normalize_target_id(target_id)
        normalized_login = normalize_broadcaster_login(target_login)
        normalized_source = str(source or "").strip().lower()
        if not normalized_id or not normalized_login or not normalized_source:
            return
        try:
            self._deps.schedule_external_target_ban_check(
                target_id=normalized_id,
                target_login=normalized_login,
                source=normalized_source,
                delay_seconds=self._config.external_target_ban_check_delay_seconds,
            )
        except Exception:
            log.exception(
                "Failed to schedule delayed external bot ban check for %s (%s)",
                normalized_login,
                normalized_id,
            )

    def delete_external_target_ban_check_pending(self, target_id: str) -> None:
        normalized_id = _normalize_target_id(target_id)
        if not normalized_id:
            return
        try:
            self._deps.delete_external_target_ban_check_pending(normalized_id)
        except Exception:
            log.debug(
                "Failed to delete delayed external bot ban check for %s",
                normalized_id,
                exc_info=True,
            )

    def reschedule_external_target_ban_check_pending(
        self,
        target_id: str,
        delay_seconds: int = 900,
    ) -> None:
        normalized_id = _normalize_target_id(target_id)
        if not normalized_id:
            return
        try:
            self._deps.reschedule_external_target_ban_check_pending(
                normalized_id,
                int(max(60, delay_seconds)),
            )
        except Exception:
            log.debug(
                "Failed to reschedule delayed external bot ban check for %s",
                normalized_id,
                exc_info=True,
            )

    async def process_due_external_target_ban_checks(self) -> None:
        try:
            rows = self._deps.load_due_external_target_ban_checks() or []
        except Exception:
            log.debug("Failed to load due external bot ban checks", exc_info=True)
            return

        for row in rows:
            target_id = _normalize_target_id(_row_value(row, "target_id", 0))
            target_login = normalize_broadcaster_login(_row_value(row, "target_login", 1))
            source = str(_row_value(row, "source", 2) or "").strip().lower() or "recruitment"
            if not target_id or not target_login:
                continue

            if self.is_blacklisted(target_id, target_login):
                self.delete_external_target_ban_check_pending(target_id)
                continue

            if self._safe_is_target_partner(target_id, target_login):
                self.delete_external_target_ban_check_pending(target_id)
                continue

            chat_bot = self._deps.get_chat_bot()
            if not chat_bot:
                log.debug(
                    "Skipping due external bot ban check for %s: chat bot unavailable",
                    target_login,
                )
                self.reschedule_external_target_ban_check_pending(target_id)
                continue

            if self._deps.part_chat_channels is not None:
                try:
                    await self._deps.part_chat_channels(chat_bot, [target_login])
                except Exception:
                    log.debug(
                        "External bot ban check could not part %s before rejoin probe",
                        target_login,
                        exc_info=True,
                    )

            try:
                joined = await self._deps.join_chat_channel(chat_bot, target_login, target_id)
            except Exception:
                log.debug(
                    "External bot ban check failed for %s (source=%s)",
                    target_login,
                    source,
                    exc_info=True,
                )
                self.reschedule_external_target_ban_check_pending(target_id)
                continue

            if joined:
                log.info(
                    "External bot ban check completed for %s (source=%s)",
                    target_login,
                    source,
                )
                self.delete_external_target_ban_check_pending(target_id)
            else:
                log.warning(
                    "External bot ban check could not confirm chat access for %s (source=%s)",
                    target_login,
                    source,
                )
                if self.is_blacklisted(target_id, target_login):
                    self.delete_external_target_ban_check_pending(target_id)
                else:
                    self.reschedule_external_target_ban_check_pending(target_id)

    def _safe_is_target_partner(self, target_id: str, target_login: str) -> bool:
        try:
            return bool(
                self._deps.is_target_partner(
                    target_id=_normalize_target_id(target_id),
                    target_login=normalize_broadcaster_login(target_login),
                )
            )
        except Exception:
            log.debug(
                "Partner lookup failed for %s (%s)",
                normalize_broadcaster_login(target_login),
                _normalize_target_id(target_id),
                exc_info=True,
            )
            return False

    @staticmethod
    def _safe_int(value: Any, default: int) -> int:
        try:
            if value is None:
                return default
            return int(value)
        except (TypeError, ValueError):
            return default


def build_raid_blacklist_service(
    dependencies: RaidBlacklistDependencies,
    *,
    config: RaidBlacklistConfig | None = None,
) -> RaidBlacklistService:
    return RaidBlacklistService(dependencies, config=config)


__all__ = [
    "DeleteExternalRecruitmentBlacklistPending",
    "DeleteExternalTargetBanCheckPending",
    "GetChatBot",
    "IsBlacklisted",
    "IsTargetPartner",
    "JoinChatChannel",
    "LoadBlacklistRows",
    "LoadDueExternalRecruitmentBlacklistPending",
    "LoadDueExternalTargetBanChecks",
    "PartChatChannels",
    "RaidBlacklistConfig",
    "RaidBlacklistCallbacks",
    "RaidBlacklistDependencies",
    "RaidBlacklistService",
    "RescheduleExternalTargetBanCheckPending",
    "ScheduleExternalRecruitmentBlacklistPending",
    "ScheduleExternalTargetBanCheck",
    "StoreBlacklistEntry",
    "build_raid_blacklist_service",
]
