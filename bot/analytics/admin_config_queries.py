"""Synchronous config and billing helpers for admin dashboard endpoints."""

from __future__ import annotations

from datetime import UTC, datetime
from typing import Any, Callable

from ..promo_mode import evaluate_global_promo_mode, save_global_promo_mode
from ..storage import pg as storage


def _row_get_value(row: Any, key: str, index: int, default: Any = None) -> Any:
    if row is None:
        return default
    if hasattr(row, "get"):
        return row.get(key, default)
    values = tuple(row)
    return values[index] if index < len(values) else default


def save_admin_promo_config(*, config: dict[str, Any], updated_by: str) -> dict[str, Any]:
    with storage.transaction() as conn:
        saved = save_global_promo_mode(conn, config=config, updated_by=updated_by)
        evaluation = evaluate_global_promo_mode(saved)
    return {"ok": True, "config": saved, "evaluation": evaluation}


def update_admin_raid_config(
    *,
    scope: str,
    raid_bot_enabled: bool,
    live_ping_enabled: bool,
    updated_by: str,
    load_streamer_config_snapshots: Callable[..., tuple[dict[str, Any], dict[str, Any]]],
) -> dict[str, Any]:
    updated_at = datetime.now(UTC).isoformat()
    with storage.transaction() as conn:
        target_count = storage.bulk_update_partner_flags(
            conn,
            scope=scope,
            raid_bot_enabled=raid_bot_enabled,
            live_ping_enabled=live_ping_enabled,
        )
        raid_snapshot, chat_snapshot = load_streamer_config_snapshots(conn, scope=scope)
    return {
        "ok": True,
        "scope": scope,
        "updatedAt": updated_at,
        "updatedBy": updated_by,
        "targetCount": target_count,
        "updatedCount": target_count,
        "raids": {
            **raid_snapshot,
            "raidBotEnabled": raid_bot_enabled,
            "livePingEnabled": live_ping_enabled,
        },
        "chat": chat_snapshot,
    }


def update_admin_chat_config(
    *,
    scope: str,
    silent_ban: bool,
    silent_raid: bool,
    updated_by: str,
    load_streamer_config_snapshots: Callable[..., tuple[dict[str, Any], dict[str, Any]]],
) -> dict[str, Any]:
    updated_at = datetime.now(UTC).isoformat()
    with storage.transaction() as conn:
        target_count = storage.bulk_update_partner_flags(
            conn,
            scope=scope,
            silent_ban=silent_ban,
            silent_raid=silent_raid,
        )
        raid_snapshot, chat_snapshot = load_streamer_config_snapshots(conn, scope=scope)
    return {
        "ok": True,
        "scope": scope,
        "updatedAt": updated_at,
        "updatedBy": updated_by,
        "targetCount": target_count,
        "updatedCount": target_count,
        "raids": raid_snapshot,
        "chat": {
            **chat_snapshot,
            "silentBan": silent_ban,
            "silentRaid": silent_raid,
        },
    }


def load_admin_billing_subscriptions() -> dict[str, Any]:
    with storage.readonly_connection() as conn:
        rows = conn.execute(
            """
            SELECT
                b.customer_reference,
                b.plan_id,
                b.status,
                b.current_period_start,
                b.current_period_end,
                b.updated_at,
                b.canceled_at,
                b.ended_at,
                sp.manual_plan_id,
                sp.manual_plan_expires_at
            FROM twitch_billing_subscriptions b
            LEFT JOIN streamer_plans sp
                ON LOWER(sp.twitch_login) = LOWER(b.customer_reference)
            ORDER BY b.updated_at DESC
            """
        ).fetchall()

    items = [
        {
            "login": str(_row_get_value(row, "customer_reference", 0, "") or "").strip().lower()
            or None,
            "customerReference": _row_get_value(row, "customer_reference", 0, None),
            "planId": _row_get_value(row, "plan_id", 1, None),
            "status": _row_get_value(row, "status", 2, None),
            "trialEndsAt": None,
            "currentPeriodEnd": _row_get_value(row, "current_period_end", 4, None),
            "updatedAt": _row_get_value(row, "updated_at", 5, None),
            "manualPlanId": _row_get_value(row, "manual_plan_id", 8, None),
            "manualPlanExpiresAt": _row_get_value(row, "manual_plan_expires_at", 9, None),
        }
        for row in rows
    ]
    return {"items": items, "count": len(items)}


def load_admin_billing_affiliates() -> dict[str, Any]:
    with storage.readonly_connection() as conn:
        rows = conn.execute(
            """
            SELECT
                twitch_login,
                email,
                stripe_account_id,
                stripe_connect_status,
                commission_rate,
                updated_at,
                created_at
            FROM affiliate_accounts
            ORDER BY COALESCE(updated_at, created_at) DESC
            """
        ).fetchall()

    items = [
        {
            "twitchLogin": _row_get_value(row, "twitch_login", 0, None),
            "stripeAccountId": _row_get_value(row, "stripe_account_id", 2, None),
            "status": _row_get_value(row, "stripe_connect_status", 3, None),
            "payoutEmail": _row_get_value(row, "email", 1, None),
            "commissionRate": _row_get_value(row, "commission_rate", 4, None),
            "updatedAt": _row_get_value(row, "updated_at", 5, None)
            or _row_get_value(row, "created_at", 6, None),
        }
        for row in rows
    ]
    return {"items": items, "count": len(items)}


__all__ = [
    "load_admin_billing_affiliates",
    "load_admin_billing_subscriptions",
    "save_admin_promo_config",
    "update_admin_chat_config",
    "update_admin_raid_config",
]
