"""Shared plan and entitlement helpers."""

from .catalog import (
    KNOWN_PLAN_IDS,
    legacy_plan_name_has_entitlement,
    normalize_plan_id_from_legacy_name,
    plan_display_name,
    plan_entitlements,
    plan_has_entitlement,
    plan_is_extended,
    plan_tier,
)
from .resolver import (
    resolve_plan_snapshot_for_login,
    resolve_plan_snapshot_for_refs,
)

__all__ = [
    "KNOWN_PLAN_IDS",
    "legacy_plan_name_has_entitlement",
    "normalize_plan_id_from_legacy_name",
    "plan_display_name",
    "plan_entitlements",
    "plan_has_entitlement",
    "plan_is_extended",
    "plan_tier",
    "resolve_plan_snapshot_for_login",
    "resolve_plan_snapshot_for_refs",
]
