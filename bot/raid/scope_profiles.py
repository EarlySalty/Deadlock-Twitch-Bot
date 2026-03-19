"""Shared Twitch OAuth scope profiles for streamer authorization."""

from __future__ import annotations

from typing import Any

BASE_SCOPE_PROFILE = "base"
DASHBOARD_REAUTH_SCOPE_PROFILE = "dashboard_reauth"
AUTO_SCOPE_PROFILE = "auto"

VALID_SCOPE_PROFILES = frozenset(
    {
        BASE_SCOPE_PROFILE,
        DASHBOARD_REAUTH_SCOPE_PROFILE,
        AUTO_SCOPE_PROFILE,
    }
)

BASE_STREAMER_SCOPES: tuple[str, ...] = (
    "channel:manage:raids",
    "channel:manage:moderators",
    "channel:bot",
    "clips:edit",
    "channel:read:ads",
    "bits:read",
    "channel:read:redemptions",
)

DASHBOARD_UPGRADE_SCOPES: tuple[str, ...] = (
    "channel:read:subscriptions",
    "channel:read:hype_train",
)

FULL_STREAMER_SCOPES: tuple[str, ...] = BASE_STREAMER_SCOPES + DASHBOARD_UPGRADE_SCOPES

BASE_CRITICAL_STREAMER_SCOPES: frozenset[str] = frozenset(
    {
        "bits:read",
        "channel:read:redemptions",
    }
)


def normalize_scope_profile(raw_value: str | None) -> str:
    normalized = str(raw_value or "").strip().lower()
    if normalized in {BASE_SCOPE_PROFILE, DASHBOARD_REAUTH_SCOPE_PROFILE}:
        return normalized
    return AUTO_SCOPE_PROFILE if normalized == AUTO_SCOPE_PROFILE else BASE_SCOPE_PROFILE


def scopes_for_profile(scope_profile: str | None) -> tuple[str, ...]:
    normalized = normalize_scope_profile(scope_profile)
    if normalized == DASHBOARD_REAUTH_SCOPE_PROFILE:
        return FULL_STREAMER_SCOPES
    return BASE_STREAMER_SCOPES


def serialize_scope_profile_meta(
    scope_profile: str | None,
    *,
    discord_user_id: str | None = None,
) -> str:
    parts = [f"scope_profile:{normalize_scope_profile(scope_profile)}"]
    normalized_discord_user_id = str(discord_user_id or "").strip()
    if normalized_discord_user_id:
        parts.append(f"discord_user_id:{normalized_discord_user_id}")
    return "|".join(parts)


def parse_scope_profile_meta_details(raw_value: str | None) -> tuple[str, str | None]:
    raw_text = str(raw_value or "").strip()
    if not raw_text:
        return BASE_SCOPE_PROFILE, None

    scope_profile = BASE_SCOPE_PROFILE
    discord_user_id: str | None = None

    for part in raw_text.split("|"):
        key, sep, value = part.partition(":")
        if not sep:
            continue
        normalized_key = key.strip().lower()
        normalized_value = value.strip()
        if normalized_key == "scope_profile":
            scope_profile = normalize_scope_profile(normalized_value)
        elif normalized_key == "discord_user_id" and normalized_value:
            discord_user_id = normalized_value

    if scope_profile == BASE_SCOPE_PROFILE and raw_text.startswith("scope_profile:"):
        # Legacy single-token format.
        scope_profile = normalize_scope_profile(raw_text.split(":", 1)[1])
    return scope_profile, discord_user_id


def parse_scope_profile_meta(raw_value: str | None) -> str:
    scope_profile, _discord_user_id = parse_scope_profile_meta_details(raw_value)
    return scope_profile
