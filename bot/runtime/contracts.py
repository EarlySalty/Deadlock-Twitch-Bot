"""Compatibility facade for the split runtime contracts."""

from __future__ import annotations

from .bot_runtime import (
    BotRuntime,
    BotRuntimeConfig,
    BotRuntimeContainer,
    BotRuntimeServices,
    BotRuntimeState,
    LEGACY_RUNTIME_ATTRIBUTE_LOOKUP,
    LEGACY_RUNTIME_ATTRIBUTE_TABLE,
    LegacyRuntimeFieldSpec,
    RUNTIME_STATE_ALIASES,
    RUNTIME_STATE_FIELDS,
    TwitchRuntimeConfig,
    TwitchRuntimeContainer,
    TwitchRuntimeServices,
    TwitchRuntimeState,
    build_runtime_state,
    ensure_bot_runtime,
    ensure_bot_runtime_container,
    ensure_runtime_container,
)
from .dashboard_runtime import (
    DashboardBotService,
    DashboardRuntime,
    DashboardRuntimeConfig,
    DashboardRuntimeContainer,
    DashboardRuntimeServices,
    DashboardRuntimeState,
    build_dashboard_runtime,
    build_runtime_state as build_dashboard_runtime_state,
)
from .shared_config import SharedRuntimeConfig


__all__ = [
    "BotRuntime",
    "BotRuntimeConfig",
    "BotRuntimeContainer",
    "BotRuntimeServices",
    "BotRuntimeState",
    "DashboardBotService",
    "DashboardRuntime",
    "DashboardRuntimeConfig",
    "DashboardRuntimeContainer",
    "DashboardRuntimeServices",
    "DashboardRuntimeState",
    "LEGACY_RUNTIME_ATTRIBUTE_LOOKUP",
    "LEGACY_RUNTIME_ATTRIBUTE_TABLE",
    "LegacyRuntimeFieldSpec",
    "RUNTIME_STATE_ALIASES",
    "RUNTIME_STATE_FIELDS",
    "SharedRuntimeConfig",
    "TwitchRuntimeConfig",
    "TwitchRuntimeContainer",
    "TwitchRuntimeServices",
    "TwitchRuntimeState",
    "build_dashboard_runtime",
    "build_dashboard_runtime_state",
    "build_runtime_state",
    "ensure_bot_runtime",
    "ensure_bot_runtime_container",
    "ensure_runtime_container",
]
