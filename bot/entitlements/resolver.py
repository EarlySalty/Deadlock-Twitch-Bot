"""Resolve effective plan and entitlements from persisted billing state."""

from __future__ import annotations
from datetime import UTC, datetime
from typing import Any

from ..storage import pg as storage
from .catalog import (
    KNOWN_PLAN_IDS,
    normalize_plan_id,
    plan_display_name,
    plan_entitlements,
    plan_is_extended,
    plan_tier,
)

_ACTIVE_BILLING_STATUSES = ("active", "trialing", "past_due")


def _is_missing_current_period_end_error(exc: Exception) -> bool:
    message = str(exc).strip().lower()
    return (
        "current_period_end" in message
        and ("no such column" in message or "does not exist" in message)
    )


def _is_missing_manual_override_metadata_error(exc: Exception) -> bool:
    message = str(exc).strip().lower()
    if "no such column" not in message and "does not exist" not in message:
        return False
    return "manual_plan_notes" in message or "manual_plan_updated_at" in message


def _row_value(row: Any, key: str, index: int, default: Any = None) -> Any:
    if row is None:
        return default
    if hasattr(row, "get"):
        return row.get(key, default)
    values = tuple(row)
    return values[index] if index < len(values) else default


def _parse_datetime_value(raw_value: Any) -> datetime | None:
    if raw_value is None:
        return None
    if isinstance(raw_value, datetime):
        parsed = raw_value
    else:
        text = str(raw_value).strip()
        if not text:
            return None
        if len(text) == 10 and text[4] == "-" and text[7] == "-":
            text = f"{text}T23:59:59+00:00"
        normalized = text.replace("Z", "+00:00")
        try:
            parsed = datetime.fromisoformat(normalized)
        except ValueError:
            return None
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=UTC)
    return parsed.astimezone(UTC)


def _normalize_candidate_refs(refs: list[str] | tuple[str, ...]) -> list[str]:
    normalized: list[str] = []
    seen: set[str] = set()
    for ref in refs:
        value = str(ref or "").strip()
        if not value:
            continue
        key = value.lower()
        if key in seen:
            continue
        seen.add(key)
        normalized.append(value)
    return normalized


def _manual_override_from_row(row: Any) -> dict[str, Any] | None:
    plan_id = str(_row_value(row, "manual_plan_id", 2, "") or "").strip()
    if plan_id not in KNOWN_PLAN_IDS:
        return None
    expires_at = _parse_datetime_value(_row_value(row, "manual_plan_expires_at", 3, None))
    is_active = not bool(expires_at and expires_at < datetime.now(UTC))
    return {
        "twitch_user_id": str(_row_value(row, "twitch_user_id", 0, "") or "").strip(),
        "twitch_login": str(_row_value(row, "twitch_login", 1, "") or "").strip(),
        "plan_id": plan_id,
        "expires_at": expires_at.isoformat() if expires_at else None,
        "notes": str(_row_value(row, "manual_plan_notes", 4, "") or "").strip(),
        "updated_at": str(_row_value(row, "manual_plan_updated_at", 5, "") or "").strip() or None,
        "is_active": is_active,
        "is_expired": not is_active,
    }


def _load_manual_override(conn: Any, refs: list[str]) -> dict[str, Any] | None:
    for ref in refs:
        try:
            row = conn.execute(
                """
                SELECT
                    twitch_user_id,
                    twitch_login,
                    manual_plan_id,
                    manual_plan_expires_at,
                    manual_plan_notes,
                    manual_plan_updated_at
                FROM streamer_plans
                WHERE TRIM(COALESCE(twitch_user_id, '')) = %s
                   OR LOWER(COALESCE(twitch_login, '')) = LOWER(%s)
                ORDER BY
                    CASE WHEN TRIM(COALESCE(twitch_user_id, '')) = %s THEN 0 ELSE 1 END,
                    manual_plan_updated_at DESC
                LIMIT 1
                """,
                (ref, ref, ref),
            ).fetchone()
        except Exception as exc:
            if not _is_missing_manual_override_metadata_error(exc):
                raise
            row = conn.execute(
                """
                SELECT
                    twitch_user_id,
                    twitch_login,
                    manual_plan_id,
                    manual_plan_expires_at,
                    '' AS manual_plan_notes,
                    NULL AS manual_plan_updated_at
                FROM streamer_plans
                WHERE TRIM(COALESCE(twitch_user_id, '')) = %s
                   OR LOWER(COALESCE(twitch_login, '')) = LOWER(%s)
                ORDER BY
                    CASE WHEN TRIM(COALESCE(twitch_user_id, '')) = %s THEN 0 ELSE 1 END
                LIMIT 1
                """,
                (ref, ref, ref),
            ).fetchone()
        payload = _manual_override_from_row(row)
        if payload:
            return payload
    return None


def _load_billing_subscription(conn: Any, refs: list[str]) -> dict[str, Any] | None:
    for ref in refs:
        try:
            row = conn.execute(
                """
                SELECT customer_reference, plan_id, status, current_period_end, updated_at
                FROM twitch_billing_subscriptions
                WHERE LOWER(customer_reference) = LOWER(%s)
                  AND status IN (%s, %s, %s)
                ORDER BY updated_at DESC
                LIMIT 1
                """,
                (ref, *_ACTIVE_BILLING_STATUSES),
            ).fetchone()
            current_period_end = _row_value(row, "current_period_end", 3, None)
            updated_at = _row_value(row, "updated_at", 4, "")
        except Exception as exc:
            if not _is_missing_current_period_end_error(exc):
                raise
            row = conn.execute(
                """
                SELECT customer_reference, plan_id, status, updated_at
                FROM twitch_billing_subscriptions
                WHERE LOWER(customer_reference) = LOWER(%s)
                  AND status IN (%s, %s, %s)
                ORDER BY updated_at DESC
                LIMIT 1
                """,
                (ref, *_ACTIVE_BILLING_STATUSES),
            ).fetchone()
            current_period_end = None
            updated_at = _row_value(row, "updated_at", 3, "")
        if not row:
            continue
        status = str(_row_value(row, "status", 2, "") or "").strip().lower()
        if status not in _ACTIVE_BILLING_STATUSES:
            continue
        return {
            "customer_reference": str(_row_value(row, "customer_reference", 0, ref) or ref).strip(),
            "plan_id": normalize_plan_id(_row_value(row, "plan_id", 1, "raid_free")),
            "status": status,
            "current_period_end": _parse_datetime_value(current_period_end),
            "updated_at": str(updated_at or "").strip() or None,
        }
    return None


def _build_plan_snapshot(
    *,
    manual_override: dict[str, Any] | None,
    billing_subscription: dict[str, Any] | None,
    fallback_ref: str,
) -> dict[str, Any]:
    plan_id = "raid_free"
    source = "default_basic"
    status = "active"
    customer_reference = str(fallback_ref or "").strip()
    expires_at: str | None = None
    if manual_override and bool(manual_override.get("is_active")):
        plan_id = normalize_plan_id(manual_override.get("plan_id"))
        source = "manual_override"
        customer_reference = str(
            manual_override.get("twitch_login") or manual_override.get("twitch_user_id") or fallback_ref or ""
        ).strip()
        expires_at = str(manual_override.get("expires_at") or "").strip() or None
    elif billing_subscription:
        plan_id = normalize_plan_id(billing_subscription.get("plan_id"))
        source = "billing_subscription"
        status = str(billing_subscription.get("status") or "active").strip() or "active"
        customer_reference = str(
            billing_subscription.get("customer_reference") or fallback_ref or ""
        ).strip()
        current_period_end = billing_subscription.get("current_period_end")
        expires_at = (
            current_period_end.isoformat()
            if hasattr(current_period_end, "isoformat")
            else str(current_period_end or "").strip() or None
        )

    return {
        "plan_id": plan_id,
        "plan_name": plan_display_name(plan_id),
        "tier": plan_tier(plan_id),
        "is_extended": plan_is_extended(plan_id),
        "entitlements": list(plan_entitlements(plan_id)),
        "status": status,
        "expires_at": expires_at,
        "source": source,
        "customer_reference": customer_reference,
        "manual_override": manual_override,
        "billing_subscription": billing_subscription,
    }


def resolve_plan_snapshot_for_refs(
    refs: list[str] | tuple[str, ...],
    *,
    conn: Any | None = None,
    fallback_ref: str = "",
) -> dict[str, Any]:
    normalized_refs = _normalize_candidate_refs(refs)
    if not normalized_refs:
        return _build_plan_snapshot(
            manual_override=None,
            billing_subscription=None,
            fallback_ref=fallback_ref,
        )

    if conn is not None:
        manual_override = _load_manual_override(conn, normalized_refs)
        billing_subscription = _load_billing_subscription(conn, normalized_refs)
        return _build_plan_snapshot(
            manual_override=manual_override,
            billing_subscription=billing_subscription,
            fallback_ref=fallback_ref or normalized_refs[0],
        )

    with storage.readonly_connection() as local_conn:
        manual_override = _load_manual_override(local_conn, normalized_refs)
        billing_subscription = _load_billing_subscription(local_conn, normalized_refs)
    return _build_plan_snapshot(
        manual_override=manual_override,
        billing_subscription=billing_subscription,
        fallback_ref=fallback_ref or normalized_refs[0],
    )


def resolve_plan_snapshot_for_login(login: str, *, conn: Any | None = None) -> dict[str, Any]:
    normalized_login = str(login or "").strip().lower()
    if not normalized_login:
        return resolve_plan_snapshot_for_refs((), conn=conn, fallback_ref="")
    return resolve_plan_snapshot_for_refs((normalized_login,), conn=conn, fallback_ref=normalized_login)
