"""Dashboard runtime contract for standalone and embedded dashboard services."""

from __future__ import annotations

from collections.abc import Awaitable, Callable
from dataclasses import dataclass, field
from typing import Any

from .shared_config import SharedRuntimeConfig


@dataclass(slots=True)
class DashboardBotService:
    """Dashboard-safe view onto bot-owned services."""

    _auth_manager: Any = None
    _discord_bot: Any = None
    _chat_bot: Any = None
    _token_manager: Any = None
    _clip_manager: Any = None
    _twitch_api: Any = None
    _eventsub_webhook_handler: Any = None
    _raid_complete_setup_cb: Callable[..., Awaitable[Any]] | None = None
    _raid_sync_partner_state_cb: Callable[..., Awaitable[Any]] | None = None
    _reload_cb: Callable[[], Awaitable[str]] | None = None
    _schedule_background: Callable[[Awaitable[Any], str], Any] | None = None

    def auth_manager(self) -> Any | None:
        return self._auth_manager

    def discord_bot(self) -> Any | None:
        return self._discord_bot

    def chat_bot(self) -> Any | None:
        return self._chat_bot

    def token_manager(self) -> Any | None:
        return self._token_manager

    def clip_manager(self) -> Any | None:
        return self._clip_manager

    def twitch_api(self) -> Any | None:
        return self._twitch_api

    def eventsub_webhook_handler(self) -> Any | None:
        return self._eventsub_webhook_handler

    def raid_complete_setup_cb(self) -> Callable[..., Awaitable[Any]] | None:
        return self._raid_complete_setup_cb

    def raid_sync_partner_state_cb(self) -> Callable[..., Awaitable[Any]] | None:
        return self._raid_sync_partner_state_cb

    def reload_cb(self) -> Callable[[], Awaitable[str]] | None:
        return self._reload_cb

    def schedule_background(self) -> Callable[[Awaitable[Any], str], Any] | None:
        return self._schedule_background


@dataclass(slots=True)
class DashboardRuntimeConfig:
    host: str = "127.0.0.1"
    port: int = 0
    noauth: bool = False
    token: str | None = None
    partner_token: str | None = None
    oauth_client_id: str | None = None
    oauth_client_secret: str | None = None
    oauth_redirect_uri: str | None = None
    session_ttl_seconds: int = 6 * 3600
    legacy_stats_url: str | None = None


@dataclass(slots=True)
class DashboardRuntimeServices:
    """Dashboard-facing callbacks and the optional bot-service bridge."""

    add_cb: Callable[[str, bool], Awaitable[str]] | None = None
    remove_cb: Callable[[str], Awaitable[str]] | None = None
    list_cb: Callable[[], Awaitable[list[dict[str, Any]]]] | None = None
    stats_cb: Callable[..., Awaitable[dict[str, Any]]] | None = None
    verify_cb: Callable[[str, str], Awaitable[str]] | None = None
    archive_cb: Callable[[str, str], Awaitable[str]] | None = None
    discord_flag_cb: Callable[[str, bool], Awaitable[str]] | None = None
    discord_profile_cb: Callable[[str, str | None, str | None, bool], Awaitable[str]] | None = None
    raid_history_cb: Callable[..., Awaitable[list[dict[str, Any]]]] | None = None
    raid_auth_url_cb: Callable[..., Awaitable[str]] | None = None
    raid_go_url_cb: Callable[[str], Awaitable[str | None]] | None = None
    raid_requirements_cb: Callable[[str], Awaitable[str]] | None = None
    raid_oauth_callback_cb: Callable[..., Awaitable[dict[str, Any]]] | None = None
    reload_cb: Callable[[], Awaitable[str]] | None = None
    eventsub_webhook_handler: Any | None = None
    social_media_clip_manager: Any | None = None
    social_media_twitch_api: Any | None = None
    bot_service: DashboardBotService | None = None

    def resolve_bot_service(self) -> DashboardBotService | None:
        """Return the normalized bot-service adapter when one is wired in."""

        bot_service = self.bot_service
        if isinstance(bot_service, DashboardBotService):
            return bot_service
        return None


@dataclass(slots=True)
class DashboardRuntimeState:
    web_app: Any = None
    web_runner: Any = None
    server: Any = None
    social_media_clip_manager: Any = None


@dataclass(slots=True)
class DashboardRuntimeContainer:
    shared_config: SharedRuntimeConfig = field(default_factory=SharedRuntimeConfig)
    config: DashboardRuntimeConfig = field(default_factory=DashboardRuntimeConfig)
    services: DashboardRuntimeServices = field(default_factory=DashboardRuntimeServices)
    state: DashboardRuntimeState = field(default_factory=DashboardRuntimeState)

    def resolve_bot_service(self) -> DashboardBotService | None:
        return self.services.resolve_bot_service()


def build_runtime_state(
    *,
    shared_config: SharedRuntimeConfig | None = None,
    config: DashboardRuntimeConfig | None = None,
    services: DashboardRuntimeServices | None = None,
    state: DashboardRuntimeState | None = None,
) -> DashboardRuntimeContainer:
    return DashboardRuntimeContainer(
        shared_config=shared_config or SharedRuntimeConfig(),
        config=config or DashboardRuntimeConfig(),
        services=services or DashboardRuntimeServices(),
        state=state or DashboardRuntimeState(),
    )


DashboardRuntime = DashboardRuntimeContainer
build_dashboard_runtime = build_runtime_state


__all__ = [
    "DashboardBotService",
    "DashboardRuntime",
    "DashboardRuntimeConfig",
    "DashboardRuntimeContainer",
    "DashboardRuntimeServices",
    "DashboardRuntimeState",
    "build_dashboard_runtime",
    "build_runtime_state",
]
