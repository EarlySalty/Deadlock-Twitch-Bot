"""Billing plan definitions, catalog builder and ID-mapping helpers."""

from __future__ import annotations

import json
from typing import Any

from ...entitlements.catalog import (
    plan_display_name,
    plan_entitlements,
    plan_has_entitlement,
    plan_tier,
)

BILLING_STRIPE_QUICKSTART_URL = "https://docs.stripe.com/billing/quickstart"

BILLING_CYCLE_DISCOUNTS: dict[int, int] = {1: 0, 12: 0}

BILLING_PLANS: tuple[dict[str, Any], ...] = (
    {
        "id": "raid_free",
        "name": "Raid Free",
        "tier": plan_tier("raid_free"),
        "badge": "free",
        "description": "Starte kostenlos mit automatischen Raids in die Community.",
        "monthly_net_cents": 0,
        "recommended": False,
        "entitlements": list(plan_entitlements("raid_free")),
        "features": [
            "Auto-Raid Grundfunktion bleibt aktiv",
            "Keine monatlichen Kosten für Basis-Raids",
            "Upgrade auf Raid Boost jederzeit moeglich",
        ],
    },
    {
        "id": "chat_quiet",
        "name": "Werbefrei",
        "tier": plan_tier("chat_quiet"),
        "badge": "quiet",
        "description": "Discord-Werbung im eigenen Chat dauerhaft aus \u2014 kein Boost, keine Analytics.",
        "monthly_net_cents": 399,
        "recommended": False,
        "entitlements": list(plan_entitlements("chat_quiet")),
        "features": [
            "Chat-Werbung des Bots dauerhaft deaktiviert",
            "Greift auch bei aktiven Admin-Promo-Events",
            "Jederzeit monatlich k\u00fcndbar",
        ],
    },
    {
        "id": "raid_boost",
        "name": "Raid Boost",
        "tier": plan_tier("raid_boost"),
        "badge": "raids",
        "description": "Dein Kanal wird bevorzugt als Raid-Ziel vorgeschlagen \u2014 mehr eingehende Zuschauer.",
        "monthly_net_cents": 399,
        "recommended": False,
        "entitlements": list(plan_entitlements("raid_boost")),
        "features": [
            "Bevorzugte Platzierung im Raid-Netzwerk",
            "Sichtbarkeit auch bei deiner Inaktivit\u00e4t",
            "Lurker Steuer Erinnerungen f\u00fcr bekannte Lurker",
            "Kein Setup n\u00f6tig \u2014 l\u00e4uft automatisch",
        ],
    },
    {
        "id": "bundle_chat_quiet_raid_boost",
        "name": "Werbefrei + Raid Boost",
        "tier": plan_tier("bundle_chat_quiet_raid_boost"),
        "badge": "bundle",
        "description": "Werbefrei + bevorzugte Raid-Platzierung im Paket \u2014 g\u00fcnstiger als einzeln.",
        "monthly_net_cents": 599,
        "recommended": False,
        "entitlements": list(plan_entitlements("bundle_chat_quiet_raid_boost")),
        "features": [
            "Chat-Werbung dauerhaft aus",
            "Bevorzugte Platzierung im Raid-Netzwerk",
            "Lurker Steuer Erinnerungen f\u00fcr bekannte Lurker",
            "Spart 2 EUR gegen\u00fcber Einzelkauf",
        ],
    },
    {
        "id": "analysis_dashboard",
        "name": "Analyse Dashboard",
        "tier": plan_tier("analysis_dashboard"),
        "badge": "analytics",
        "description": "Vollst\u00e4ndiges Analytics-Dashboard mit Stream-Statistiken, Viewer-Kurven und Wachstumsvergleichen.",
        "monthly_net_cents": 849,
        "recommended": True,
        "entitlements": list(plan_entitlements("analysis_dashboard")),
        "features": [
            "Viewer-Verlauf & Peak-Analyse pro Stream",
            "Zeitraumvergleiche und Wachstumstrends",
            "Lurker Steuer Erinnerungen f\u00fcr bekannte Lurker",
            "Follower- und Retention-\u00dcbersichten",
        ],
    },
    {
        "id": "bundle_werbefrei_analyse",
        "name": "Werbefrei + Analyse",
        "tier": plan_tier("bundle_werbefrei_analyse"),
        "badge": "bundle",
        "description": "Chat-Werbung dauerhaft aus + volles Analytics-Dashboard — günstiger als einzeln.",
        "monthly_net_cents": 1149,
        "recommended": False,
        "entitlements": list(plan_entitlements("bundle_werbefrei_analyse")),
        "features": [
            "Chat-Werbung dauerhaft deaktiviert",
            "Vollständiges Analytics-Dashboard",
            "KI-Coaching & Viewer-Analyse",
            "Spart gegenüber Einzelkauf",
        ],
    },
    {
        "id": "bundle_komplett",
        "name": "Alles drin",
        "tier": plan_tier("bundle_komplett"),
        "badge": "bundle",
        "description": "Werbefrei + Raid Boost + Analytics — das komplette Paket zum besten Preis.",
        "monthly_net_cents": 1399,
        "recommended": False,
        "entitlements": list(plan_entitlements("bundle_komplett")),
        "features": [
            "Alle Features aus allen Plänen",
            "Bevorzugte Raid-Platzierung aktiv",
            "Volles Analytics + KI-Coaching",
            "Beste Ersparnis gegenüber Einzelkauf",
        ],
    },
    {
        "id": "bundle_analysis_raid_boost",
        "name": "Bundle: Analyse + Raid Boost",
        "tier": plan_tier("bundle_analysis_raid_boost"),
        "badge": "bundle",
        "description": "Analyse Dashboard + Raid Boost im Paket \u2014 g\u00fcnstiger als einzeln.",
        "monthly_net_cents": 1149,
        "recommended": False,
        "entitlements": list(plan_entitlements("bundle_analysis_raid_boost")),
        "features": [
            "Alle Analytics-Features inklusive",
            "Bevorzugte Raid-Platzierung aktiv",
            "Lurker Steuer Erinnerungen f\u00fcr bekannte Lurker",
            "Spare gegen\u00fcber Einzelbuchung",
        ],
    },
)

PAID_PLAN_IDS: frozenset[str] = frozenset(
    str(plan.get("id") or "").strip()
    for plan in BILLING_PLANS
    if int(plan.get("monthly_net_cents") or 0) > 0 and str(plan.get("id") or "").strip()
)


def normalize_billing_cycle(raw_cycle: int | str | None) -> int:
    try:
        cycle = int(raw_cycle or 1)
    except (TypeError, ValueError):
        cycle = 1
    if cycle not in BILLING_CYCLE_DISCOUNTS:
        return 1
    return cycle


def billing_cycle_label(months: int) -> str:
    if months == 1:
        return "30 Tage"
    return f"{months} Monate"


def format_eur_cents(cents: int) -> str:
    euros, remainder = divmod(max(int(cents), 0), 100)
    return f"{euros},{remainder:02d} EUR"


def billing_payment_state_from_readiness(readiness: dict[str, Any] | None) -> dict[str, Any]:
    payload = dict(readiness or {})
    checkout_ready = bool(payload.get("checkout_ready"))
    price_map_ready = bool(payload.get("price_map_ready"))
    integration_state = str(payload.get("integration_state") or "").strip()
    if not integration_state:
        integration_state = "live" if (checkout_ready and price_map_ready) else "planned"
    return {
        "integration_state": integration_state,
        "checkout_enabled": bool(checkout_ready and price_map_ready),
    }


def build_billing_catalog(
    cycle_months: int | str | None,
    *,
    readiness: dict[str, Any] | None = None,
) -> dict[str, Any]:
    cycle = normalize_billing_cycle(cycle_months)
    cycle_discount = int(BILLING_CYCLE_DISCOUNTS.get(cycle, 0))
    cycle_label = billing_cycle_label(cycle)
    payment_state = billing_payment_state_from_readiness(readiness)
    plans: list[dict[str, Any]] = []
    for blueprint in BILLING_PLANS:
        monthly_net_cents = int(blueprint["monthly_net_cents"])
        subtotal_net_cents = monthly_net_cents * cycle
        discount_percent = cycle_discount if cycle > 1 and subtotal_net_cents > 0 else 0
        discount_cents = (
            (subtotal_net_cents * discount_percent + 50) // 100
            if discount_percent > 0
            else 0
        )
        total_net_cents = subtotal_net_cents - discount_cents
        effective_monthly_net_cents = (
            (total_net_cents + cycle // 2) // cycle if cycle > 0 else total_net_cents
        )
        plans.append(
            {
                "id": blueprint["id"],
                "name": blueprint["name"],
                "tier": blueprint["tier"],
                "badge": blueprint["badge"],
                "description": blueprint["description"],
                "recommended": bool(blueprint.get("recommended")),
                "monthly_net_cents": monthly_net_cents,
                "entitlements": list(blueprint.get("entitlements", [])),
                "features": list(blueprint.get("features", [])),
                "price": {
                    "cycle_months": cycle,
                    "cycle_label": cycle_label,
                    "subtotal_net_cents": subtotal_net_cents,
                    "discount_percent": discount_percent,
                    "discount_cents": discount_cents,
                    "total_net_cents": total_net_cents,
                    "effective_monthly_net_cents": effective_monthly_net_cents,
                    "subtotal_net_label": format_eur_cents(subtotal_net_cents),
                    "total_net_label": format_eur_cents(total_net_cents),
                    "effective_monthly_net_label": format_eur_cents(effective_monthly_net_cents),
                },
            }
        )
    return {
        "currency": "EUR",
        "tax_mode": "net_only",
        "gross_available": False,
        "cycle_months": cycle,
        "cycle_label": cycle_label,
        "discount_percent": cycle_discount if cycle > 1 else 0,
        "plans": plans,
        "payment": {
            "provider": "stripe",
            "integration_state": payment_state["integration_state"],
            "checkout_enabled": payment_state["checkout_enabled"],
            "checkout_preview_enabled": True,
            "catalog_path": "/twitch/api/billing/catalog",
            "checkout_preview_path": "/twitch/api/billing/checkout-preview",
            "checkout_session_path": "/twitch/api/billing/checkout-session",
            "readiness_path": "/twitch/api/billing/readiness",
            "webhook_path": "/twitch/api/billing/stripe/webhook",
            "quickstart_url": BILLING_STRIPE_QUICKSTART_URL,
            "supported_methods_planned": [
                "card",
                "sepa_debit",
                "paypal_via_wallet_if_enabled",
            ],
        },
    }


def billing_value_preview(raw_value: str | None, *, secret: bool) -> str:
    value = str(raw_value or "").strip()
    if not value:
        return ""
    if not secret:
        return value
    if len(value) <= 8:
        return "*" * len(value)
    return f"{value[:4]}...{value[-4:]}"


def billing_parse_cycle_key(raw_cycle: Any) -> int | None:
    try:
        cycle = int(raw_cycle)
    except (TypeError, ValueError):
        return None
    if cycle not in BILLING_CYCLE_DISCOUNTS:
        return None
    return cycle


def billing_parse_price_id_mapping(raw_mapping: Any) -> dict[str, dict[int, str]]:
    payload: Any = raw_mapping
    if isinstance(raw_mapping, str):
        raw = raw_mapping.strip()
        if not raw:
            return {}
        try:
            payload = json.loads(raw)
        except Exception:
            return {}
    if not isinstance(payload, dict):
        return {}
    normalized: dict[str, dict[int, str]] = {}
    for raw_plan_id, raw_cycle_map in payload.items():
        plan_id = str(raw_plan_id or "").strip()
        if not plan_id or not isinstance(raw_cycle_map, dict):
            continue
        cycle_map: dict[int, str] = {}
        for raw_cycle, raw_price_id in raw_cycle_map.items():
            cycle = billing_parse_cycle_key(raw_cycle)
            if cycle is None:
                continue
            price_id = str(raw_price_id or "").strip()
            if price_id:
                cycle_map[cycle] = price_id
        if cycle_map:
            normalized[plan_id] = cycle_map
    return normalized


def billing_parse_product_id_mapping(raw_mapping: Any) -> dict[str, str]:
    payload: Any = raw_mapping
    if isinstance(raw_mapping, str):
        raw = raw_mapping.strip()
        if not raw:
            return {}
        try:
            payload = json.loads(raw)
        except Exception:
            return {}
    if not isinstance(payload, dict):
        return {}
    normalized: dict[str, str] = {}
    for raw_plan_id, raw_product_id in payload.items():
        plan_id = str(raw_plan_id or "").strip()
        product_id = str(raw_product_id or "").strip()
        if plan_id and product_id:
            normalized[plan_id] = product_id
    return normalized


def billing_dump_price_id_mapping(mapping: dict[str, dict[int, str]]) -> str:
    payload: dict[str, dict[str, str]] = {}
    for plan_id in sorted(mapping.keys()):
        cycle_map = mapping.get(plan_id) or {}
        normalized_cycle_map: dict[str, str] = {}
        for cycle in sorted(cycle_map.keys()):
            price_id = str(cycle_map.get(cycle) or "").strip()
            if price_id:
                normalized_cycle_map[str(cycle)] = price_id
        if normalized_cycle_map:
            payload[plan_id] = normalized_cycle_map
    return json.dumps(payload, ensure_ascii=True, separators=(",", ":"))


def billing_dump_product_id_mapping(mapping: dict[str, str]) -> str:
    payload: dict[str, str] = {}
    for plan_id in sorted(mapping.keys()):
        product_id = str(mapping.get(plan_id) or "").strip()
        if product_id:
            payload[plan_id] = product_id
    return json.dumps(payload, ensure_ascii=True, separators=(",", ":"))


# Known Stripe IDs committed to source — not secrets, safe to push.
# The vault env vars are merged on top (vault wins on conflict).
STRIPE_PRICE_ID_DEFAULTS: dict[str, dict[int, str]] = {
    "chat_quiet":                   {1: "price_1TZDfQ0yU8I2yGJ07EtjKXvc",  12: "price_1TZDvw0yU8I2yGJ0q6ZrcLB2"},
    "raid_boost":                   {12: "price_1TZDvx0yU8I2yGJ0E6pN53qe"},
    "analysis_dashboard":           {12: "price_1TZDvy0yU8I2yGJ0iPJcramq"},
    "bundle_chat_quiet_raid_boost": {1: "price_1TZDfR0yU8I2yGJ03ribVvev",  12: "price_1TZDvy0yU8I2yGJ0zZcKzob2"},
    "bundle_werbefrei_analyse":     {1: "price_1TZD6U0yU8I2yGJ0fq8MZaqg",  12: "price_1TZDvz0yU8I2yGJ0wKyC8W9W"},
    "bundle_komplett":              {1: "price_1TZD6W0yU8I2yGJ0JQzboooa",  12: "price_1TZDw00yU8I2yGJ0lQ1sliPd"},
    "bundle_analysis_raid_boost":   {12: "price_1TZDw00yU8I2yGJ0yYj926cP"},
}

STRIPE_PRODUCT_ID_DEFAULTS: dict[str, str] = {
    "chat_quiet":                  "prod_UYKKvIg1sbjVrl",
    "bundle_chat_quiet_raid_boost": "prod_UYKKwFHm0ozy5w",
    "bundle_werbefrei_analyse":    "prod_UYJjXXe90gt8WO",
    "bundle_komplett":             "prod_UYJjhWpzqyNqr0",
}


def billing_merge_price_id_defaults(mapping: dict[str, dict[int, str]]) -> dict[str, dict[int, str]]:
    """Return mapping with STRIPE_PRICE_ID_DEFAULTS filled in for any missing slots."""
    result: dict[str, dict[int, str]] = {}
    for plan_id, cycle_map in STRIPE_PRICE_ID_DEFAULTS.items():
        merged = dict(cycle_map)
        merged.update(mapping.get(plan_id) or {})
        result[plan_id] = merged
    for plan_id, cycle_map in mapping.items():
        if plan_id not in result:
            result[plan_id] = dict(cycle_map)
    return result


def billing_merge_product_id_defaults(mapping: dict[str, str]) -> dict[str, str]:
    """Return mapping with STRIPE_PRODUCT_ID_DEFAULTS filled in for any missing entries."""
    result = dict(STRIPE_PRODUCT_ID_DEFAULTS)
    result.update(mapping)
    return result


def billing_is_paid_plan_id(plan_id: str | None) -> bool:
    return str(plan_id or "").strip() in PAID_PLAN_IDS


def billing_is_paid_plan(plan: dict[str, Any] | str | None) -> bool:
    if isinstance(plan, dict):
        plan_id = str(plan.get("id") or "").strip()
        if plan_id:
            return billing_is_paid_plan_id(plan_id)
        return int(plan.get("monthly_net_cents") or 0) > 0
    return billing_is_paid_plan_id(str(plan or "").strip())


def billing_plan_tier(plan_id: str | None) -> str:
    return plan_tier(plan_id)


def billing_plan_entitlements(plan_id: str | None) -> tuple[str, ...]:
    return plan_entitlements(plan_id)


def billing_plan_has_entitlement(plan_id: str | None, entitlement: str | None) -> bool:
    return plan_has_entitlement(plan_id, entitlement)


def billing_plan_display_name(plan_id: str | None) -> str:
    normalized_plan_id = str(plan_id or "").strip()
    for blueprint in BILLING_PLANS:
        if str(blueprint.get("id") or "").strip() == normalized_plan_id:
            return str(blueprint.get("name") or "").strip() or plan_display_name(normalized_plan_id)
    return plan_display_name(normalized_plan_id)


# ---------------------------------------------------------------------------
# Back-compat aliases — old underscore names still importable during migration
# ---------------------------------------------------------------------------
_BILLING_CYCLE_DISCOUNTS = BILLING_CYCLE_DISCOUNTS
_BILLING_PLANS = BILLING_PLANS
_PAID_PLAN_IDS = PAID_PLAN_IDS
_build_billing_catalog = build_billing_catalog
_billing_cycle_label = billing_cycle_label
_normalize_billing_cycle = normalize_billing_cycle
_format_eur_cents = format_eur_cents
_billing_value_preview = billing_value_preview
_billing_parse_cycle_key = billing_parse_cycle_key
_billing_parse_price_id_mapping = billing_parse_price_id_mapping
_billing_parse_product_id_mapping = billing_parse_product_id_mapping
_billing_dump_price_id_mapping = billing_dump_price_id_mapping
_billing_dump_product_id_mapping = billing_dump_product_id_mapping
_billing_is_paid_plan_id = billing_is_paid_plan_id
_billing_is_paid_plan = billing_is_paid_plan
_billing_plan_tier = billing_plan_tier
_billing_plan_entitlements = billing_plan_entitlements
_billing_plan_has_entitlement = billing_plan_has_entitlement
_billing_plan_display_name = billing_plan_display_name
