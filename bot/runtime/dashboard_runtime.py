"""Dashboard runtime contract for standalone and embedded dashboard services."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

from .shared_config import SharedRuntimeConfig


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
    bot_api_client: Any = None
    internal_api_client: Any = None
    auth_service: Any = None
    template_service: Any = None
    bot_service: Any = None
    eventsub_webhook_handler: Any = None


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
    "DashboardRuntime",
    "DashboardRuntimeConfig",
    "DashboardRuntimeContainer",
    "DashboardRuntimeServices",
    "DashboardRuntimeState",
    "build_dashboard_runtime",
    "build_runtime_state",
]
