"""Shared contracts and constants for the internal API surface."""

from __future__ import annotations

import asyncio
from collections.abc import Awaitable, Callable
from dataclasses import dataclass
from typing import Any

INTERNAL_API_BASE_PATH = "/internal/twitch/v1"
INTERNAL_TOKEN_HEADER = "X-Internal-Token"
IDEMPOTENCY_KEY_HEADER = "Idempotency-Key"
PUBLIC_WEBSITE_ONBOARDING_LOGIN = "public:website_onboarding"

AddStreamerCallback = Callable[[str, bool], Awaitable[str]]
RemoveStreamerCallback = Callable[[str], Awaitable[str]]
StreamersCallback = Callable[[], Awaitable[list[dict[str, Any]]]]
StatsCallback = Callable[..., Awaitable[dict[str, Any]]]
VerifyStreamerCallback = Callable[[str, str], Awaitable[str]]
ArchiveStreamerCallback = Callable[[str, str], Awaitable[str]]
DiscordFlagCallback = Callable[[str, bool], Awaitable[str]]
DiscordProfileCallback = Callable[..., Awaitable[str]]
StreamerAnalyticsCallback = Callable[[str, int], Awaitable[dict[str, Any]]]
ComparisonCallback = Callable[[int], Awaitable[dict[str, Any]]]
SessionCallback = Callable[[int], Awaitable[dict[str, Any]]]
RaidAuthUrlCallback = Callable[..., Awaitable[str]]
RaidAuthStateCallback = Callable[[str], Awaitable[dict[str, Any]]]
RaidBlockStateCallback = Callable[..., Awaitable[dict[str, Any]]]
RaidGoUrlCallback = Callable[[str], Awaitable[str | None]]
RaidRequirementsCallback = Callable[[str], Awaitable[str]]
RaidOauthCallback = Callable[..., Awaitable[dict[str, Any]]]
LiveActiveAnnouncementsCallback = Callable[[], Awaitable[list[dict[str, Any]]]]
LiveLinkClickCallback = Callable[..., Awaitable[dict[str, Any] | None]]
ObservabilitySnapshotCallback = Callable[[], Awaitable[dict[str, Any]]]
ChattersDebugCallback = Callable[[str], Awaitable[dict[str, Any]]]


@dataclass(slots=True, frozen=True)
class InternalApiCallbacks:
    """Typed callback bundle for the internal API surface."""

    add: AddStreamerCallback | None = None
    remove: RemoveStreamerCallback | None = None
    streamers: StreamersCallback | None = None
    stats: StatsCallback | None = None
    verify: VerifyStreamerCallback | None = None
    archive: ArchiveStreamerCallback | None = None
    discord_flag: DiscordFlagCallback | None = None
    discord_profile: DiscordProfileCallback | None = None
    streamer_analytics: StreamerAnalyticsCallback | None = None
    comparison: ComparisonCallback | None = None
    session: SessionCallback | None = None
    raid_auth_url: RaidAuthUrlCallback | None = None
    raid_auth_state: RaidAuthStateCallback | None = None
    raid_block_state: RaidBlockStateCallback | None = None
    raid_go_url: RaidGoUrlCallback | None = None
    raid_requirements: RaidRequirementsCallback | None = None
    raid_oauth_callback: RaidOauthCallback | None = None
    live_active_announcements: LiveActiveAnnouncementsCallback | None = None
    live_link_click: LiveLinkClickCallback | None = None
    observability_snapshot: ObservabilitySnapshotCallback | None = None
    chatters_debug: ChattersDebugCallback | None = None

    @classmethod
    def coalesce(
        cls,
        callbacks: "InternalApiCallbacks | None" = None,
        *,
        add_cb: AddStreamerCallback | None = None,
        remove_cb: RemoveStreamerCallback | None = None,
        list_cb: StreamersCallback | None = None,
        stats_cb: StatsCallback | None = None,
        verify_cb: VerifyStreamerCallback | None = None,
        archive_cb: ArchiveStreamerCallback | None = None,
        discord_flag_cb: DiscordFlagCallback | None = None,
        discord_profile_cb: DiscordProfileCallback | None = None,
        streamer_analytics_cb: StreamerAnalyticsCallback | None = None,
        comparison_cb: ComparisonCallback | None = None,
        session_cb: SessionCallback | None = None,
        raid_auth_url_cb: RaidAuthUrlCallback | None = None,
        raid_auth_state_cb: RaidAuthStateCallback | None = None,
        raid_block_state_cb: RaidBlockStateCallback | None = None,
        raid_go_url_cb: RaidGoUrlCallback | None = None,
        raid_requirements_cb: RaidRequirementsCallback | None = None,
        raid_oauth_callback_cb: RaidOauthCallback | None = None,
        live_active_announcements_cb: LiveActiveAnnouncementsCallback | None = None,
        live_link_click_cb: LiveLinkClickCallback | None = None,
        observability_snapshot_cb: ObservabilitySnapshotCallback | None = None,
        chatters_debug_cb: ChattersDebugCallback | None = None,
    ) -> "InternalApiCallbacks":
        base = callbacks or cls()
        return cls(
            add=add_cb if add_cb is not None else base.add,
            remove=remove_cb if remove_cb is not None else base.remove,
            streamers=list_cb if list_cb is not None else base.streamers,
            stats=stats_cb if stats_cb is not None else base.stats,
            verify=verify_cb if verify_cb is not None else base.verify,
            archive=archive_cb if archive_cb is not None else base.archive,
            discord_flag=discord_flag_cb if discord_flag_cb is not None else base.discord_flag,
            discord_profile=(
                discord_profile_cb if discord_profile_cb is not None else base.discord_profile
            ),
            streamer_analytics=(
                streamer_analytics_cb
                if streamer_analytics_cb is not None
                else base.streamer_analytics
            ),
            comparison=comparison_cb if comparison_cb is not None else base.comparison,
            session=session_cb if session_cb is not None else base.session,
            raid_auth_url=raid_auth_url_cb if raid_auth_url_cb is not None else base.raid_auth_url,
            raid_auth_state=(
                raid_auth_state_cb if raid_auth_state_cb is not None else base.raid_auth_state
            ),
            raid_block_state=(
                raid_block_state_cb if raid_block_state_cb is not None else base.raid_block_state
            ),
            raid_go_url=raid_go_url_cb if raid_go_url_cb is not None else base.raid_go_url,
            raid_requirements=(
                raid_requirements_cb if raid_requirements_cb is not None else base.raid_requirements
            ),
            raid_oauth_callback=(
                raid_oauth_callback_cb
                if raid_oauth_callback_cb is not None
                else base.raid_oauth_callback
            ),
            live_active_announcements=(
                live_active_announcements_cb
                if live_active_announcements_cb is not None
                else base.live_active_announcements
            ),
            live_link_click=(
                live_link_click_cb if live_link_click_cb is not None else base.live_link_click
            ),
            observability_snapshot=(
                observability_snapshot_cb
                if observability_snapshot_cb is not None
                else base.observability_snapshot
            ),
            chatters_debug=chatters_debug_cb if chatters_debug_cb is not None else base.chatters_debug,
        )


@dataclass(slots=True)
class IdempotencyInFlight:
    fingerprint: str
    future: asyncio.Future[tuple[int, Any]]
    created_at: float


__all__ = [
    "AddStreamerCallback",
    "ArchiveStreamerCallback",
    "ChattersDebugCallback",
    "ComparisonCallback",
    "DiscordFlagCallback",
    "DiscordProfileCallback",
    "IDEMPOTENCY_KEY_HEADER",
    "INTERNAL_API_BASE_PATH",
    "INTERNAL_TOKEN_HEADER",
    "IdempotencyInFlight",
    "InternalApiCallbacks",
    "LiveActiveAnnouncementsCallback",
    "LiveLinkClickCallback",
    "ObservabilitySnapshotCallback",
    "PUBLIC_WEBSITE_ONBOARDING_LOGIN",
    "RaidAuthStateCallback",
    "RaidAuthUrlCallback",
    "RaidBlockStateCallback",
    "RaidGoUrlCallback",
    "RaidOauthCallback",
    "RaidRequirementsCallback",
    "RemoveStreamerCallback",
    "SessionCallback",
    "StatsCallback",
    "StreamerAnalyticsCallback",
    "StreamersCallback",
    "VerifyStreamerCallback",
]
