"""Explicit runtime contract objects for the dashboard server."""

from __future__ import annotations

from collections.abc import Awaitable, Callable
from dataclasses import dataclass, field
from typing import Any

from ..runtime.contracts import DashboardBotService


@dataclass(slots=True)
class DashboardRuntimeServices:
    """Service bundle used to wire the dashboard server without Cog references."""

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
    bot_service: DashboardBotService = field(default_factory=DashboardBotService)


@dataclass(slots=True)
class DashboardRuntimeConfig:
    """Static dashboard runtime settings."""

    app_token: str | None = None
    noauth: bool = False
    partner_token: str | None = None
    oauth_client_id: str | None = None
    oauth_client_secret: str | None = None
    oauth_redirect_uri: str | None = None
    session_ttl_seconds: int = 6 * 3600
    legacy_stats_url: str | None = None


@dataclass(slots=True)
class DashboardRuntimeState:
    """Mutable dashboard runtime state."""

    web_runner: Any | None = None
    web_app: Any | None = None


__all__ = [
    "DashboardRuntimeConfig",
    "DashboardRuntimeServices",
    "DashboardRuntimeState",
]
