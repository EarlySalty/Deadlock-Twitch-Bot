from __future__ import annotations

import importlib
from dataclasses import dataclass
from typing import Any, Callable

from ... import storage as _storage

try:
    from ..partner_scores import (
        load_partner_raid_score_map,
        refresh_partner_raid_score_async,
    )
except Exception:  # pragma: no cover - best effort if helper is unavailable during partial deploys
    load_partner_raid_score_map = None  # type: ignore[assignment]
    refresh_partner_raid_score_async = None  # type: ignore[assignment]

try:
    from ..partner_raid_score_tracking import track_confirmed_partner_raid
except Exception:  # pragma: no cover - best effort if helper is unavailable during partial deploys
    track_confirmed_partner_raid = None  # type: ignore[assignment]

insert_observability_event = _storage.insert_observability_event
load_active_partner = _storage.load_active_partner
load_offline_auto_raid_eligibility = _storage.load_offline_auto_raid_eligibility
load_streamer_identity = _storage.load_streamer_identity
readonly_connection = _storage.readonly_connection
transaction = _storage.transaction


def _bot_module():
    return importlib.import_module("bot.raid.bot")


def _lookup_bot_symbol(name: str) -> Any:
    return getattr(_bot_module(), name)


class DynamicCallable:
    def __init__(self, symbol_name: str) -> None:
        self._symbol_name = str(symbol_name or "").strip()

    def __call__(self, *args: Any, **kwargs: Any) -> Any:
        target = _lookup_bot_symbol(self._symbol_name)
        if not callable(target):
            raise LookupError(f"bot.raid.bot.{self._symbol_name} is not callable")
        return target(*args, **kwargs)


class OptionalDynamicCallable(DynamicCallable):
    def __bool__(self) -> bool:
        return callable(_lookup_bot_symbol(self._symbol_name))


@dataclass(slots=True, frozen=True)
class RaidRuntimeDeps:
    readonly_connection_factory: Callable[..., Any]
    transaction_factory: Callable[..., Any]
    load_active_partner_fn: Callable[..., Any]
    load_offline_auto_raid_eligibility_fn: Callable[..., Any]
    load_streamer_identity_fn: Callable[..., Any]
    insert_observability_event_fn: Callable[..., Any]
    load_partner_raid_score_map_fn: Callable[..., Any]
    refresh_partner_raid_score_async_fn: Callable[..., Any]
    track_confirmed_partner_raid_fn: Callable[..., Any] | None
    utcnow: Callable[[], Any]
    raid_target_cooldown_days: int
    pending_chat_notification_grace_seconds: float
    recent_raid_arrival_ttl_seconds: float
    orphan_chat_notification_retention_seconds: float
    raid_readiness_ttl_seconds: float
    raid_readiness_max_entries: int
    external_recruitment_raid_limit: int
    external_target_ban_check_delay_seconds: int
    external_recruitment_blacklist_grace_seconds: int
    twitch_api_base: str
    mask_log_identifier: Callable[..., str]


def build_default_raid_runtime_deps() -> RaidRuntimeDeps:
    return RaidRuntimeDeps(
        readonly_connection_factory=DynamicCallable("readonly_connection"),
        transaction_factory=DynamicCallable("transaction"),
        load_active_partner_fn=load_active_partner,
        load_offline_auto_raid_eligibility_fn=load_offline_auto_raid_eligibility,
        load_streamer_identity_fn=load_streamer_identity,
        insert_observability_event_fn=insert_observability_event,
        load_partner_raid_score_map_fn=OptionalDynamicCallable("load_partner_raid_score_map"),
        refresh_partner_raid_score_async_fn=OptionalDynamicCallable(
            "refresh_partner_raid_score_async"
        ),
        track_confirmed_partner_raid_fn=OptionalDynamicCallable(
            "track_confirmed_partner_raid"
        ),
        utcnow=lambda: _lookup_bot_symbol("datetime").now(_lookup_bot_symbol("UTC")),
        raid_target_cooldown_days=int(_lookup_bot_symbol("RAID_TARGET_COOLDOWN_DAYS")),
        pending_chat_notification_grace_seconds=float(
            _lookup_bot_symbol("_PENDING_CHAT_NOTIFICATION_GRACE_SECONDS")
        ),
        recent_raid_arrival_ttl_seconds=float(
            _lookup_bot_symbol("_RECENT_RAID_ARRIVAL_TTL_SECONDS")
        ),
        orphan_chat_notification_retention_seconds=float(
            _lookup_bot_symbol("_ORPHAN_CHAT_NOTIFICATION_RETENTION_SECONDS")
        ),
        raid_readiness_ttl_seconds=float(_lookup_bot_symbol("_RAID_READINESS_TTL_SECONDS")),
        raid_readiness_max_entries=int(_lookup_bot_symbol("_RAID_READINESS_MAX_ENTRIES")),
        external_recruitment_raid_limit=int(
            _lookup_bot_symbol("_EXTERNAL_RECRUITMENT_RAID_LIMIT")
        ),
        external_target_ban_check_delay_seconds=int(
            _lookup_bot_symbol("_EXTERNAL_BAN_CHECK_DELAY_SECONDS")
        ),
        external_recruitment_blacklist_grace_seconds=int(
            _lookup_bot_symbol("_EXTERNAL_RECRUITMENT_BLACKLIST_GRACE_SECONDS")
        ),
        twitch_api_base=str(_lookup_bot_symbol("TWITCH_API_BASE")),
        mask_log_identifier=lambda value, *, visible_prefix=3, visible_suffix=2: (
            _lookup_bot_symbol("_mask_log_identifier")(
                value,
                visible_prefix=visible_prefix,
                visible_suffix=visible_suffix,
            )
        ),
    )
