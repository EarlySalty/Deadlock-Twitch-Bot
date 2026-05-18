"""Canonical plan metadata and derived entitlements."""

from __future__ import annotations

from typing import Final

# Trial Plan
ANALYTICS_TRIAL_PLAN_ID = "analytics_trial"
TRIAL_DURATION_DAYS = 45

KNOWN_PLAN_IDS: Final[frozenset[str]] = frozenset(
    {
        "raid_free",
        "chat_quiet",
        "raid_boost",
        "bundle_chat_quiet_raid_boost",
        "analysis_dashboard",
        "bundle_analysis_raid_boost",
        ANALYTICS_TRIAL_PLAN_ID,  # 45-day free trial for new users
    }
)

LEGACY_PLAN_NAME_TO_ID_MAP: Final[dict[str, str]] = {
    "free": "raid_free",
    "raid_free": "raid_free",
    "werbefrei": "chat_quiet",
    "quiet": "chat_quiet",
    "chat_quiet": "chat_quiet",
    "raid_boost": "raid_boost",
    "chat_quiet_bundle": "bundle_chat_quiet_raid_boost",
    "bundle_chat_quiet_raid_boost": "bundle_chat_quiet_raid_boost",
    "analysis": "analysis_dashboard",
    "analysis_dashboard": "analysis_dashboard",
    "bundle": "bundle_analysis_raid_boost",
    "bundle_analysis_raid_boost": "bundle_analysis_raid_boost",
}

PLAN_TIER_MAP: Final[dict[str, str]] = {
    "raid_free": "free",
    "chat_quiet": "basic",
    "raid_boost": "basic",
    "bundle_chat_quiet_raid_boost": "basic",
    "analysis_dashboard": "extended",
    "bundle_analysis_raid_boost": "extended",
    ANALYTICS_TRIAL_PLAN_ID: "extended",  # Trial gives extended access
}

PLAN_DISPLAY_NAME_MAP: Final[dict[str, str]] = {
    "raid_free": "Free",
    "chat_quiet": "Werbefrei",
    "raid_boost": "Basic",
    "bundle_chat_quiet_raid_boost": "Werbefrei + Raid Boost",
    "analysis_dashboard": "Erweitert",
    "bundle_analysis_raid_boost": "Erweitert (Bundle)",
    ANALYTICS_TRIAL_PLAN_ID: "Trial",
}

PLAN_ENTITLEMENTS_MAP: Final[dict[str, frozenset[str]]] = {
    "raid_free": frozenset(),
    "chat_quiet": frozenset({"chat.promos.disable"}),
    "raid_boost": frozenset(
        {
            "analytics.ai_mini",
            "analytics.basic",
            "chat.lurker_tax",
            "raid.priority",
        }
    ),
    "bundle_chat_quiet_raid_boost": frozenset(
        {
            "analytics.ai_mini",
            "analytics.basic",
            "chat.lurker_tax",
            "chat.promos.disable",
            "raid.priority",
        }
    ),
    "analysis_dashboard": frozenset(
        {
            "analytics.basic",
            "analytics.ai_full",
            "analytics.extended",
            "chat.lurker_tax",
        }
    ),
    "bundle_analysis_raid_boost": frozenset(
        {
            "analytics.basic",
            "analytics.ai_full",
            "analytics.extended",
            "chat.lurker_tax",
            "chat.promos.disable",
            "raid.priority",
        }
    ),
    ANALYTICS_TRIAL_PLAN_ID: frozenset(
        {
            "analytics.ai_mini",
            "analytics.basic",
            "analytics.extended",
            "chat.lurker_tax",
        }
    ),
}


def normalize_plan_id(raw_plan_id: str | None) -> str:
    plan_id = str(raw_plan_id or "").strip()
    return plan_id if plan_id in KNOWN_PLAN_IDS else "raid_free"


def plan_tier(plan_id: str | None) -> str:
    normalized = normalize_plan_id(plan_id)
    return PLAN_TIER_MAP.get(normalized, "free")


def normalize_plan_id_from_legacy_name(raw_plan_name: str | None) -> str:
    plan_name = str(raw_plan_name or "").strip().lower()
    return normalize_plan_id(LEGACY_PLAN_NAME_TO_ID_MAP.get(plan_name))


def plan_display_name(plan_id: str | None) -> str:
    normalized = normalize_plan_id(plan_id)
    return PLAN_DISPLAY_NAME_MAP.get(normalized, "Free")


def plan_entitlements(plan_id: str | None) -> tuple[str, ...]:
    normalized = normalize_plan_id(plan_id)
    return tuple(sorted(PLAN_ENTITLEMENTS_MAP.get(normalized, frozenset())))


def plan_has_entitlement(plan_id: str | None, entitlement: str | None) -> bool:
    required = str(entitlement or "").strip()
    if not required:
        return True
    normalized = normalize_plan_id(plan_id)
    return required in PLAN_ENTITLEMENTS_MAP.get(normalized, frozenset())


def legacy_plan_name_has_entitlement(raw_plan_name: str | None, entitlement: str | None) -> bool:
    return plan_has_entitlement(normalize_plan_id_from_legacy_name(raw_plan_name), entitlement)


def plan_is_extended(plan_id: str | None) -> bool:
    normalized = normalize_plan_id(plan_id)
    return plan_has_entitlement(normalized, "analytics.extended")
