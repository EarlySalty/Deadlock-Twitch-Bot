"""Bot runtime contract for the Twitch worker process."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass, field
from typing import Any

from .shared_config import SharedRuntimeConfig


@dataclass(frozen=True, slots=True)
class LegacyRuntimeFieldSpec:
    legacy_name: str
    section: str
    field_name: str


LEGACY_RUNTIME_ATTRIBUTE_TABLE: tuple[LegacyRuntimeFieldSpec, ...] = (
    LegacyRuntimeFieldSpec("client_id", "config", "client_id"),
    LegacyRuntimeFieldSpec("client_secret", "config", "client_secret"),
    LegacyRuntimeFieldSpec("_twitch_bot_client_id", "config", "twitch_bot_client_id"),
    LegacyRuntimeFieldSpec("_twitch_bot_secret", "config", "twitch_bot_secret"),
    LegacyRuntimeFieldSpec("_language_filters", "config", "language_filters"),
    LegacyRuntimeFieldSpec("_log_every_n", "config", "log_every_n"),
    LegacyRuntimeFieldSpec("_category_sample_limit", "config", "category_sample_limit"),
    LegacyRuntimeFieldSpec("_poll_interval_seconds", "config", "poll_interval_seconds"),
    LegacyRuntimeFieldSpec(
        "_poll_interval_resync_interval_seconds",
        "config",
        "poll_interval_resync_interval_seconds",
    ),
    LegacyRuntimeFieldSpec("_poll_interval_settings_table", "config", "poll_interval_settings_table"),
    LegacyRuntimeFieldSpec("_poll_interval_settings_key", "config", "poll_interval_settings_key"),
    LegacyRuntimeFieldSpec("_notify_channel_id", "config", "notify_channel_id"),
    LegacyRuntimeFieldSpec("_alert_channel_id", "config", "alert_channel_id"),
    LegacyRuntimeFieldSpec("_alert_mention", "config", "alert_mention"),
    LegacyRuntimeFieldSpec("_target_game_name", "config", "target_game_name"),
    LegacyRuntimeFieldSpec("_target_game_lower", "config", "target_game_lower"),
    LegacyRuntimeFieldSpec("_internal_api_token", "config", "internal_api_token"),
    LegacyRuntimeFieldSpec("_internal_api_host", "config", "internal_api_host"),
    LegacyRuntimeFieldSpec("_internal_api_port", "config", "internal_api_port"),
    LegacyRuntimeFieldSpec("_raid_redirect_uri", "config", "raid_redirect_uri"),
    LegacyRuntimeFieldSpec(
        "_experimental_irc_lurker_channels",
        "config",
        "experimental_irc_lurker_channels",
    ),
    LegacyRuntimeFieldSpec("api", "services", "api"),
    LegacyRuntimeFieldSpec("partner_raid_score_service", "services", "partner_raid_score_service"),
    LegacyRuntimeFieldSpec("_internal_api_runner", "services", "internal_api_runner"),
    LegacyRuntimeFieldSpec("_raid_bot", "services", "raid_bot"),
    LegacyRuntimeFieldSpec("_twitch_chat_bot", "services", "twitch_chat_bot"),
    LegacyRuntimeFieldSpec("_bot_token_manager", "services", "bot_token_manager"),
    LegacyRuntimeFieldSpec("_reload_manager", "services", "reload_manager"),
    LegacyRuntimeFieldSpec("_eventsub_webhook_handler", "services", "eventsub_webhook_handler"),
    LegacyRuntimeFieldSpec("_webhook_base_url", "services", "webhook_base_url"),
    LegacyRuntimeFieldSpec("_webhook_secret", "services", "webhook_secret"),
    LegacyRuntimeFieldSpec("clip_manager", "services", "clip_manager"),
    LegacyRuntimeFieldSpec("clip_fetcher", "services", "clip_fetcher"),
    LegacyRuntimeFieldSpec("upload_worker", "services", "upload_worker"),
    LegacyRuntimeFieldSpec("social_media_retention_worker", "services", "social_media_retention_worker"),
    LegacyRuntimeFieldSpec("_twl_command", "services", "twl_command"),
    LegacyRuntimeFieldSpec("_twitch_bot_token", "services", "twitch_bot_token"),
    LegacyRuntimeFieldSpec("_twitch_bot_refresh_token", "services", "twitch_bot_refresh_token"),
    LegacyRuntimeFieldSpec("_category_id", "state", "category_id"),
    LegacyRuntimeFieldSpec("_tick_count", "state", "tick_count"),
    LegacyRuntimeFieldSpec(
        "_admin_polling_interval_seconds",
        "state",
        "admin_polling_interval_seconds",
    ),
    LegacyRuntimeFieldSpec("_active_sessions", "state", "active_sessions"),
    LegacyRuntimeFieldSpec("_invite_codes", "state", "invite_codes"),
    LegacyRuntimeFieldSpec("_runtime_started", "state", "runtime_started"),
    LegacyRuntimeFieldSpec("_runtime_start_lock", "state", "runtime_start_lock"),
    LegacyRuntimeFieldSpec("_runtime_stop_lock", "state", "runtime_stop_lock"),
    LegacyRuntimeFieldSpec(
        "_poll_interval_last_sync_monotonic",
        "state",
        "poll_interval_last_sync_monotonic",
    ),
    LegacyRuntimeFieldSpec(
        "_poll_interval_last_error_log_at",
        "state",
        "poll_interval_last_error_log_at",
    ),
    LegacyRuntimeFieldSpec(
        "_poll_interval_last_invalid_value",
        "state",
        "poll_interval_last_invalid_value",
    ),
    LegacyRuntimeFieldSpec(
        "_experimental_irc_lurker_enabled",
        "state",
        "experimental_irc_lurker_enabled",
    ),
    LegacyRuntimeFieldSpec("_irc_lurker_tracker", "state", "irc_lurker_tracker"),
    LegacyRuntimeFieldSpec("_managed_bg_tasks", "state", "managed_bg_tasks"),
    LegacyRuntimeFieldSpec(
        "_periodic_channel_join_task",
        "state",
        "periodic_channel_join_task",
    ),
)

LEGACY_RUNTIME_ATTRIBUTE_LOOKUP: dict[str, LegacyRuntimeFieldSpec] = {
    spec.legacy_name: spec for spec in LEGACY_RUNTIME_ATTRIBUTE_TABLE
}


@dataclass(slots=True)
class BotRuntimeConfig(SharedRuntimeConfig):
    internal_api_token: str | None = None
    internal_api_host: str = "127.0.0.1"
    internal_api_port: int = 0
    language_filters: list[str] | None = None
    log_every_n: int = 1
    category_sample_limit: int = 400
    poll_interval_seconds: int = 15
    poll_interval_resync_interval_seconds: float = 60.0
    poll_interval_settings_table: str = "twitch_global_settings"
    poll_interval_settings_key: str = "poll_interval_seconds"
    notify_channel_id: int = 0
    alert_channel_id: int = 0
    alert_mention: str = ""
    target_game_name: str = ""
    target_game_lower: str = ""
    raid_redirect_uri: str = ""
    experimental_irc_lurker_channels: set[str] = field(default_factory=set)


@dataclass(slots=True)
class BotRuntimeServices:
    api: Any = None
    partner_raid_score_service: Any = None
    internal_api_runner: Any = None
    raid_bot: Any = None
    twitch_chat_bot: Any = None
    bot_token_manager: Any = None
    reload_manager: Any = None
    eventsub_webhook_handler: Any = None
    webhook_base_url: str | None = None
    webhook_secret: str | None = None
    clip_manager: Any = None
    clip_fetcher: Any = None
    upload_worker: Any = None
    social_media_retention_worker: Any = None
    twl_command: Any = None
    twitch_bot_token: str | None = None
    twitch_bot_refresh_token: str | None = None


@dataclass(slots=True)
class BotRuntimeState:
    category_id: str | None = None
    tick_count: int = 0
    admin_polling_interval_seconds: int = 0
    active_sessions: dict[str, Any] = field(default_factory=dict)
    invite_codes: dict[str, Any] = field(default_factory=dict)
    runtime_started: bool = False
    runtime_start_lock: asyncio.Lock = field(default_factory=asyncio.Lock)
    runtime_stop_lock: asyncio.Lock | None = None
    poll_interval_last_sync_monotonic: float = 0.0
    poll_interval_last_error_log_at: float = 0.0
    poll_interval_last_invalid_value: object | None = None
    experimental_irc_lurker_enabled: bool = False
    irc_lurker_tracker: Any = None
    managed_bg_tasks: set[Any] = field(default_factory=set)
    periodic_channel_join_task: Any = None


@dataclass(slots=True)
class BotRuntimeContainer:
    config: BotRuntimeConfig = field(default_factory=BotRuntimeConfig)
    state: BotRuntimeState = field(default_factory=BotRuntimeState)
    services: BotRuntimeServices = field(default_factory=BotRuntimeServices)

    def assign(self, **values: Any) -> None:
        for legacy_name, value in values.items():
            spec = LEGACY_RUNTIME_ATTRIBUTE_LOOKUP.get(legacy_name)
            if spec is None:
                raise KeyError(f"Unknown runtime field: {legacy_name}")
            section = getattr(self, spec.section)
            setattr(section, spec.field_name, value)

    def get(self, legacy_name: str) -> Any:
        spec = LEGACY_RUNTIME_ATTRIBUTE_LOOKUP.get(legacy_name)
        if spec is None:
            raise KeyError(f"Unknown runtime field: {legacy_name}")
        section = getattr(self, spec.section)
        return getattr(section, spec.field_name)

    def delete(self, legacy_name: str) -> None:
        spec = LEGACY_RUNTIME_ATTRIBUTE_LOOKUP.get(legacy_name)
        if spec is None:
            raise KeyError(f"Unknown runtime field: {legacy_name}")
        section = getattr(self, spec.section)
        field_type = type(section)
        default_value = field_type()
        setattr(section, spec.field_name, getattr(default_value, spec.field_name))

    def legacy_snapshot(self) -> dict[str, Any]:
        return {spec.legacy_name: self.get(spec.legacy_name) for spec in LEGACY_RUNTIME_ATTRIBUTE_TABLE}


def ensure_bot_runtime_container(owner: Any) -> BotRuntimeContainer:
    owner_dict = getattr(owner, "__dict__", None)
    runtime = owner_dict.get("_runtime_state") if isinstance(owner_dict, dict) else None
    if runtime is None and isinstance(owner_dict, dict):
        runtime = owner_dict.get("runtime_state")
    if runtime is None:
        runtime = BotRuntimeContainer()
        if isinstance(owner_dict, dict):
            owner_dict["_runtime_state"] = runtime
        else:
            setattr(owner, "runtime_state", runtime)
        return runtime
    if not isinstance(runtime, BotRuntimeContainer):
        raise TypeError("owner.runtime_state must be a BotRuntimeContainer")
    return runtime


def build_runtime_state(owner: Any) -> BotRuntimeContainer:
    del owner
    return BotRuntimeContainer()


RUNTIME_STATE_FIELDS: tuple[str, ...] = tuple(spec.legacy_name for spec in LEGACY_RUNTIME_ATTRIBUTE_TABLE)
RUNTIME_STATE_ALIASES: dict[str, str] = {spec.legacy_name: spec.legacy_name for spec in LEGACY_RUNTIME_ATTRIBUTE_TABLE}


BotRuntime = BotRuntimeContainer
TwitchRuntimeConfig = BotRuntimeConfig
TwitchRuntimeServices = BotRuntimeServices
TwitchRuntimeState = BotRuntimeState
TwitchRuntimeContainer = BotRuntimeContainer
ensure_bot_runtime = ensure_bot_runtime_container
ensure_runtime_container = ensure_bot_runtime_container


__all__ = [
    "BotRuntime",
    "BotRuntimeConfig",
    "BotRuntimeContainer",
    "BotRuntimeServices",
    "BotRuntimeState",
    "LEGACY_RUNTIME_ATTRIBUTE_LOOKUP",
    "LEGACY_RUNTIME_ATTRIBUTE_TABLE",
    "LegacyRuntimeFieldSpec",
    "RUNTIME_STATE_ALIASES",
    "RUNTIME_STATE_FIELDS",
    "TwitchRuntimeConfig",
    "TwitchRuntimeContainer",
    "TwitchRuntimeServices",
    "TwitchRuntimeState",
    "build_runtime_state",
    "ensure_bot_runtime",
    "ensure_bot_runtime_container",
    "ensure_runtime_container",
]
