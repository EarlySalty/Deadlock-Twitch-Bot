"""Compatibility facade for the split runtime contracts."""

from __future__ import annotations

from collections.abc import Awaitable, Callable
from dataclasses import dataclass
from typing import Any

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
    DashboardRuntime,
    DashboardRuntimeConfig,
    DashboardRuntimeContainer,
    DashboardRuntimeServices,
    DashboardRuntimeState,
    build_dashboard_runtime,
    build_runtime_state as build_dashboard_runtime_state,
)
from .shared_config import SharedRuntimeConfig


@dataclass(slots=True)
class DashboardBotService:
    """Dashboard-safe view onto bot-owned services."""

    raid_bot: Any = None
    twitch_api: Any = None
    clip_manager: Any = None
    eventsub_webhook_handler: Any = None
    twitch_chat_bot: Any = None
    bot_token_manager: Any = None
    reload_cb: Callable[[], Awaitable[str]] | None = None
    schedule_background: Callable[[Awaitable[Any], str], Any] | None = None

    @classmethod
    def from_cog(
        cls,
        cog: Any,
        *,
        reload_cb: Callable[[], Awaitable[str]] | None = None,
    ) -> "DashboardBotService":
        return cls(
            raid_bot=getattr(cog, "_raid_bot", None),
            twitch_api=getattr(cog, "api", None),
            clip_manager=getattr(cog, "clip_manager", None),
            eventsub_webhook_handler=getattr(cog, "_eventsub_webhook_handler", None),
            twitch_chat_bot=getattr(cog, "_twitch_chat_bot", None),
            bot_token_manager=getattr(cog, "_bot_token_manager", None),
            reload_cb=reload_cb,
            schedule_background=getattr(cog, "_spawn_bg_task", None),
        )

    def auth_manager(self) -> Any | None:
        return getattr(self.raid_bot, "auth_manager", None) if self.raid_bot is not None else None

    def discord_bot(self) -> Any | None:
        auth_manager = self.auth_manager()
        discord_bot = getattr(auth_manager, "_discord_bot", None) if auth_manager else None
        if discord_bot is not None:
            return discord_bot
        return getattr(self.raid_bot, "_discord_bot", None) if self.raid_bot is not None else None

    def chat_bot(self) -> Any | None:
        if self.twitch_chat_bot is not None:
            return self.twitch_chat_bot
        if self.raid_bot is None:
            return None
        chat_bot = getattr(self.raid_bot, "chat_bot", None)
        if chat_bot is not None:
            return chat_bot
        return getattr(getattr(self.raid_bot, "_cog", None), "_twitch_chat_bot", None)

    def token_manager(self) -> Any | None:
        chat_bot = self.chat_bot()
        token_manager = getattr(chat_bot, "_token_manager", None) if chat_bot is not None else None
        if token_manager is not None:
            return token_manager
        return self.bot_token_manager


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

