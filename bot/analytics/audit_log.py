"""Aggregate admin audit log entries across dashboard storage sources."""

from __future__ import annotations

import json
from datetime import UTC, datetime
from typing import Any

from ..dashboard.admin.legal_mixin import LEGAL_PAGE_SLUGS, load_legal_page_document
from ..dashboard.pages import load_roadmap_document
from ..promo_mode import load_global_promo_mode
from ..storage import pg as storage

_AUDIT_LOG_DEFAULT_LIMIT = 100
_AUDIT_LOG_MAX_LIMIT = 500


def _row_get_value(row: Any, key: str, index: int, default: Any = None) -> Any:
    if row is None:
        return default
    if hasattr(row, "get"):
        return row.get(key, default)
    values = tuple(row)
    return values[index] if index < len(values) else default


def _coerce_datetime(value: Any) -> datetime | None:
    if value is None:
        return None
    if isinstance(value, datetime):
        parsed = value
    else:
        text = str(value).strip()
        if not text:
            return None
        normalized = f"{text[:-1]}+00:00" if text.endswith("Z") else text
        try:
            parsed = datetime.fromisoformat(normalized)
        except ValueError:
            return None
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=UTC)
    return parsed.astimezone(UTC)


def _coerce_iso_datetime(value: Any) -> str | None:
    parsed = _coerce_datetime(value)
    return parsed.isoformat() if parsed is not None else None


def _matches_since(timestamp: str | None, since: datetime | None) -> bool:
    if since is None:
        return True
    parsed = _coerce_datetime(timestamp)
    return parsed is not None and parsed >= since


def _normalize_source_filters(source: str | list[str] | tuple[str, ...] | None) -> set[str]:
    if source is None:
        return set()
    if isinstance(source, str):
        raw_values = [source]
    else:
        raw_values = [str(entry or "") for entry in source]
    normalized: set[str] = set()
    for raw_value in raw_values:
        for part in str(raw_value or "").split(","):
            value = part.strip().lower()
            if value:
                normalized.add(value)
    return normalized


def _clamp_limit(limit: Any) -> int:
    try:
        parsed = int(limit)
    except (TypeError, ValueError):
        return _AUDIT_LOG_DEFAULT_LIMIT
    return max(1, min(_AUDIT_LOG_MAX_LIMIT, parsed))


def _is_missing_schema_error(exc: Exception) -> bool:
    normalized = str(exc).strip().lower()
    return any(
        marker in normalized
        for marker in (
            "does not exist",
            "no such table",
            "undefined table",
            "no such column",
            "undefined column",
        )
    )


def _table_exists(conn: Any, table_name: str) -> bool:
    try:
        row = conn.execute("SELECT to_regclass(%s)", (table_name,)).fetchone()
        resolved = _row_get_value(row, "to_regclass", 0, None)
        if resolved is not None:
            return bool(str(resolved).strip())
    except Exception:
        pass

    try:
        row = conn.execute(
            "SELECT name FROM sqlite_master WHERE type IN ('table', 'view') AND name = ?",
            (table_name,),
        ).fetchone()
        return row is not None
    except Exception:
        return False


def _make_entry(
    *,
    entry_id: str,
    source: str,
    action: str,
    actor: str | None,
    target: str | None,
    timestamp: Any,
    description: str,
    metadata: dict[str, Any] | None = None,
) -> dict[str, Any] | None:
    iso_timestamp = _coerce_iso_datetime(timestamp)
    if not iso_timestamp:
        return None
    return {
        "id": entry_id,
        "source": source,
        "action": action,
        "actor": str(actor).strip() or None if actor is not None else None,
        "target": str(target).strip() or None if target is not None else None,
        "timestamp": iso_timestamp,
        "description": description,
        "metadata": metadata or None,
    }


def _load_streamer_history_entries(conn: Any) -> list[dict[str, Any]]:
    if not _table_exists(conn, "twitch_partners"):
        return []

    rows = conn.execute(
        """
        SELECT
            p.id,
            p.twitch_user_id,
            p.twitch_login,
            p.added_by,
            p.manual_verified_at,
            p.partnered_at,
            p.admin_archived_at,
            p.departnered_at,
            p.status,
            p.technical_pause_reason
        FROM twitch_partners p
        ORDER BY
            COALESCE(p.partnered_at, p.departnered_at, p.admin_archived_at, p.manual_verified_at, '') ASC,
            p.id ASC
        """
    ).fetchall()

    prior_inactive_by_identity: set[str] = set()
    entries: list[dict[str, Any]] = []
    for row in rows or []:
        row_id = str(_row_get_value(row, "id", 0, "") or "").strip()
        twitch_user_id = str(_row_get_value(row, "twitch_user_id", 1, "") or "").strip()
        twitch_login = str(_row_get_value(row, "twitch_login", 2, "") or "").strip()
        identity = twitch_user_id or twitch_login.lower()
        added_by = str(_row_get_value(row, "added_by", 3, "") or "").strip() or None
        manual_verified_at = _row_get_value(row, "manual_verified_at", 4, None)
        partnered_at = _row_get_value(row, "partnered_at", 5, None)
        admin_archived_at = _row_get_value(row, "admin_archived_at", 6, None)
        departnered_at = _row_get_value(row, "departnered_at", 7, None)
        status = str(_row_get_value(row, "status", 8, "") or "").strip().lower()
        pause_reason = str(_row_get_value(row, "technical_pause_reason", 9, "") or "").strip().lower()

        if partnered_at:
            action = "restore" if identity and identity in prior_inactive_by_identity else "added"
            description = (
                f"Streamer {twitch_login or twitch_user_id} wurde wieder aktiviert."
                if action == "restore"
                else f"Streamer {twitch_login or twitch_user_id} wurde hinzugefuegt."
            )
            entry = _make_entry(
                entry_id=f"streamer_history:{row_id}:{action}",
                source="streamer_history",
                action=action,
                actor=added_by,
                target=twitch_login or twitch_user_id or None,
                timestamp=partnered_at,
                description=description,
                metadata={
                    "partnerId": row_id,
                    "status": status or None,
                    "twitchUserId": twitch_user_id or None,
                },
            )
            if entry is not None:
                entries.append(entry)

        if manual_verified_at:
            entry = _make_entry(
                entry_id=f"streamer_history:{row_id}:verify",
                source="streamer_history",
                action="verify",
                actor=None,
                target=twitch_login or twitch_user_id or None,
                timestamp=manual_verified_at,
                description=f"Streamer {twitch_login or twitch_user_id} wurde verifiziert.",
                metadata={
                    "partnerId": row_id,
                    "twitchUserId": twitch_user_id or None,
                },
            )
            if entry is not None:
                entries.append(entry)

        if admin_archived_at:
            entry = _make_entry(
                entry_id=f"streamer_history:{row_id}:archive",
                source="streamer_history",
                action="archive",
                actor=None,
                target=twitch_login or twitch_user_id or None,
                timestamp=admin_archived_at,
                description=f"Streamer {twitch_login or twitch_user_id} wurde archiviert.",
                metadata={
                    "partnerId": row_id,
                    "twitchUserId": twitch_user_id or None,
                    "status": status or None,
                },
            )
            if entry is not None:
                entries.append(entry)

        if departnered_at and status == "departnered":
            entry = _make_entry(
                entry_id=f"streamer_history:{row_id}:remove",
                source="streamer_history",
                action="remove",
                actor=None,
                target=twitch_login or twitch_user_id or None,
                timestamp=departnered_at,
                description=f"Streamer {twitch_login or twitch_user_id} wurde entfernt oder departnert.",
                metadata={
                    "partnerId": row_id,
                    "twitchUserId": twitch_user_id or None,
                },
            )
            if entry is not None:
                entries.append(entry)

        if identity and (status != "active" or pause_reason):
            prior_inactive_by_identity.add(identity)

    return entries


def _load_manual_plan_entries(conn: Any) -> list[dict[str, Any]]:
    if not _table_exists(conn, "streamer_plans"):
        return []

    rows = conn.execute(
        """
        SELECT
            twitch_user_id,
            twitch_login,
            manual_plan_id,
            manual_plan_expires_at,
            manual_plan_notes,
            manual_plan_updated_at
        FROM streamer_plans
        WHERE manual_plan_updated_at IS NOT NULL
        ORDER BY manual_plan_updated_at DESC
        """
    ).fetchall()

    entries: list[dict[str, Any]] = []
    for row in rows or []:
        twitch_user_id = str(_row_get_value(row, "twitch_user_id", 0, "") or "").strip()
        twitch_login = str(_row_get_value(row, "twitch_login", 1, "") or "").strip()
        manual_plan_id = str(_row_get_value(row, "manual_plan_id", 2, "") or "").strip()
        expires_at = _row_get_value(row, "manual_plan_expires_at", 3, None)
        notes = str(_row_get_value(row, "manual_plan_notes", 4, "") or "").strip()
        updated_at = _row_get_value(row, "manual_plan_updated_at", 5, None)

        if manual_plan_id:
            action = "plan_override"
            description = f"Manueller Plan fuer {twitch_login or twitch_user_id} auf {manual_plan_id} gesetzt."
        else:
            action = "plan_override_cleared"
            description = f"Manueller Plan-Override fuer {twitch_login or twitch_user_id} entfernt."

        entry = _make_entry(
            entry_id=f"manual_plan:{twitch_user_id or twitch_login}:{updated_at}",
            source="manual_plan",
            action=action,
            actor=None,
            target=twitch_login or twitch_user_id or None,
            timestamp=updated_at,
            description=description,
            metadata={
                "planId": manual_plan_id or None,
                "expiresAt": _coerce_iso_datetime(expires_at),
                "notes": notes or None,
                "twitchUserId": twitch_user_id or None,
            },
        )
        if entry is not None:
            entries.append(entry)
    return entries


def _extract_billing_target(event_payload: dict[str, Any], object_id: str) -> tuple[str | None, dict[str, Any]]:
    data = event_payload.get("data")
    object_payload = data.get("object") if isinstance(data, dict) else {}
    object_record = object_payload if isinstance(object_payload, dict) else {}
    metadata_value = object_record.get("metadata")
    metadata = metadata_value if isinstance(metadata_value, dict) else {}

    customer_reference = str(
        metadata.get("customer_reference")
        or object_record.get("client_reference_id")
        or object_record.get("customer_email")
        or ""
    ).strip()
    subscription_id = str(
        object_record.get("subscription") or object_record.get("id") or object_id or ""
    ).strip()
    plan_id = str(metadata.get("plan_id") or "").strip()
    status = str(object_record.get("status") or "").strip().lower()
    details = {
        "customerReference": customer_reference or None,
        "subscriptionId": subscription_id or None,
        "planId": plan_id or None,
        "status": status or None,
    }
    return (customer_reference or subscription_id or object_id or None), details


def _map_billing_action(event_type: str) -> tuple[str, str]:
    normalized = str(event_type or "").strip().lower()
    mapping = {
        "checkout.session.completed": (
            "checkout_completed",
            "Stripe-Checkout fuer ein Abo abgeschlossen.",
        ),
        "customer.subscription.created": (
            "subscription_created",
            "Stripe-Abo erstellt.",
        ),
        "customer.subscription.updated": (
            "subscription_updated",
            "Stripe-Abo aktualisiert.",
        ),
        "customer.subscription.deleted": (
            "subscription_canceled",
            "Stripe-Abo beendet.",
        ),
        "invoice.payment_succeeded": (
            "invoice_paid",
            "Abo-Zahlung erfolgreich verbucht.",
        ),
        "invoice.payment_failed": (
            "invoice_failed",
            "Abo-Zahlung fehlgeschlagen.",
        ),
    }
    if normalized in mapping:
        return mapping[normalized]
    fallback = normalized.replace(".", "_") or "billing_event"
    return fallback, f"Billing-Event {normalized or 'unknown'} verarbeitet."


def _load_billing_event_entries(conn: Any) -> list[dict[str, Any]]:
    if not _table_exists(conn, "twitch_billing_events"):
        return []

    rows = conn.execute(
        """
        SELECT
            stripe_event_id,
            event_type,
            object_id,
            received_at,
            livemode,
            payload
        FROM twitch_billing_events
        ORDER BY received_at DESC
        """
    ).fetchall()

    entries: list[dict[str, Any]] = []
    for row in rows or []:
        event_id = str(_row_get_value(row, "stripe_event_id", 0, "") or "").strip()
        event_type = str(_row_get_value(row, "event_type", 1, "") or "").strip()
        object_id = str(_row_get_value(row, "object_id", 2, "") or "").strip()
        received_at = _row_get_value(row, "received_at", 3, None)
        livemode = bool(_row_get_value(row, "livemode", 4, 0))
        payload_text = str(_row_get_value(row, "payload", 5, "") or "").strip()
        try:
            event_payload = json.loads(payload_text) if payload_text else {}
        except json.JSONDecodeError:
            event_payload = {}

        target, metadata = _extract_billing_target(
            event_payload if isinstance(event_payload, dict) else {},
            object_id,
        )
        action, description = _map_billing_action(event_type)
        entry = _make_entry(
            entry_id=f"billing:{event_id or object_id}",
            source="billing",
            action=action,
            actor=None,
            target=target,
            timestamp=received_at,
            description=description,
            metadata={
                "eventType": event_type or None,
                "objectId": object_id or None,
                "livemode": livemode,
                **metadata,
            },
        )
        if entry is not None:
            entries.append(entry)
    return entries


def _load_billing_subscription_entries(conn: Any) -> list[dict[str, Any]]:
    if not _table_exists(conn, "twitch_billing_subscriptions"):
        return []

    rows = conn.execute(
        """
        SELECT
            stripe_subscription_id,
            customer_reference,
            status,
            plan_id,
            current_period_end,
            canceled_at,
            ended_at,
            updated_at
        FROM twitch_billing_subscriptions
        WHERE updated_at IS NOT NULL
        ORDER BY updated_at DESC
        """
    ).fetchall()

    entries: list[dict[str, Any]] = []
    for row in rows or []:
        subscription_id = str(_row_get_value(row, "stripe_subscription_id", 0, "") or "").strip()
        customer_reference = str(_row_get_value(row, "customer_reference", 1, "") or "").strip()
        status = str(_row_get_value(row, "status", 2, "") or "").strip().lower()
        plan_id = str(_row_get_value(row, "plan_id", 3, "") or "").strip()
        current_period_end = _row_get_value(row, "current_period_end", 4, None)
        canceled_at = _row_get_value(row, "canceled_at", 5, None)
        ended_at = _row_get_value(row, "ended_at", 6, None)
        updated_at = _row_get_value(row, "updated_at", 7, None)

        action = "subscription_updated"
        description = f"Abo-Status fuer {customer_reference or subscription_id} auf {status or 'unknown'} aktualisiert."
        if ended_at or canceled_at or status in {"canceled", "cancelled", "incomplete_expired"}:
            action = "subscription_canceled"
            description = f"Abo fuer {customer_reference or subscription_id} beendet oder gekuendigt."

        entry = _make_entry(
            entry_id=f"billing:{subscription_id}:{updated_at}",
            source="billing",
            action=action,
            actor=None,
            target=customer_reference or subscription_id or None,
            timestamp=updated_at,
            description=description,
            metadata={
                "subscriptionId": subscription_id or None,
                "customerReference": customer_reference or None,
                "status": status or None,
                "planId": plan_id or None,
                "currentPeriodEnd": _coerce_iso_datetime(current_period_end),
                "canceledAt": _coerce_iso_datetime(canceled_at),
                "endedAt": _coerce_iso_datetime(ended_at),
            },
        )
        if entry is not None:
            entries.append(entry)
    return entries


def _load_promo_entries(conn: Any) -> list[dict[str, Any]]:
    try:
        config = load_global_promo_mode(conn)
    except Exception as exc:
        if _is_missing_schema_error(exc):
            return []
        raise

    updated_at = config.get("updated_at")
    if not updated_at:
        return []

    mode = str(config.get("mode") or "").strip() or "standard"
    is_enabled = bool(config.get("is_enabled"))
    description = (
        "Announcements-Konfiguration aktualisiert."
        if config.get("custom_message")
        else "Promo-Modus-Konfiguration aktualisiert."
    )
    entry = _make_entry(
        entry_id=f"promo:global:{updated_at}",
        source="promo",
        action="announcement_update",
        actor=str(config.get("updated_by") or "").strip() or None,
        target="global",
        timestamp=updated_at,
        description=description,
        metadata={
            "mode": mode,
            "isEnabled": is_enabled,
            "startsAt": _coerce_iso_datetime(config.get("starts_at")),
            "endsAt": _coerce_iso_datetime(config.get("ends_at")),
        },
    )
    return [entry] if entry is not None else []


def _load_roadmap_entries() -> list[dict[str, Any]]:
    document = load_roadmap_document()
    updated_at = document.get("lastUpdatedAt")
    if not updated_at:
        return []
    entry = _make_entry(
        entry_id=f"roadmap:main:{updated_at}",
        source="roadmap",
        action="content_edit",
        actor=str(document.get("lastUpdatedBy") or "").strip() or None,
        target="roadmap",
        timestamp=updated_at,
        description="Roadmap-Inhalt aktualisiert.",
        metadata=None,
    )
    return [entry] if entry is not None else []


def _load_legal_entries() -> list[dict[str, Any]]:
    entries: list[dict[str, Any]] = []
    for slug in sorted(LEGAL_PAGE_SLUGS):
        document = load_legal_page_document(slug)
        updated_at = document.get("lastUpdatedAt")
        if not updated_at:
            continue
        entry = _make_entry(
            entry_id=f"legal:{slug}:{updated_at}",
            source="legal",
            action="content_edit",
            actor=str(document.get("lastUpdatedBy") or "").strip() or None,
            target=slug,
            timestamp=updated_at,
            description=f"Legal-Seite {slug} aktualisiert.",
            metadata={
                "slug": slug,
                "title": str(document.get("title") or "").strip() or None,
            },
        )
        if entry is not None:
            entries.append(entry)
    return entries


def load_admin_audit_log(
    *,
    since: datetime | None = None,
    limit: int | None = None,
    source: str | list[str] | tuple[str, ...] | None = None,
) -> dict[str, Any]:
    entries: list[dict[str, Any]] = []
    with storage.readonly_connection() as conn:
        try:
            entries.extend(_load_streamer_history_entries(conn))
        except Exception as exc:
            if not _is_missing_schema_error(exc):
                raise

        try:
            entries.extend(_load_manual_plan_entries(conn))
        except Exception as exc:
            if not _is_missing_schema_error(exc):
                raise

        try:
            billing_entries = _load_billing_event_entries(conn)
        except Exception as exc:
            if not _is_missing_schema_error(exc):
                raise
            billing_entries = []
        if billing_entries:
            entries.extend(billing_entries)
        else:
            try:
                entries.extend(_load_billing_subscription_entries(conn))
            except Exception as exc:
                if not _is_missing_schema_error(exc):
                    raise

        try:
            entries.extend(_load_promo_entries(conn))
        except Exception as exc:
            if not _is_missing_schema_error(exc):
                raise

    entries.extend(_load_roadmap_entries())
    entries.extend(_load_legal_entries())

    since_filtered = [entry for entry in entries if _matches_since(entry.get("timestamp"), since)]
    since_filtered.sort(key=lambda item: item.get("timestamp") or "", reverse=True)
    all_sources = sorted({str(entry.get("source") or "").strip() for entry in since_filtered if entry.get("source")})

    source_filters = _normalize_source_filters(source)
    filtered_entries = (
        [entry for entry in since_filtered if str(entry.get("source") or "").strip().lower() in source_filters]
        if source_filters
        else since_filtered
    )

    resolved_limit = _clamp_limit(limit)
    limited_entries = filtered_entries[:resolved_limit]
    return {
        "entries": limited_entries,
        "sources": all_sources,
        "totalCount": len(filtered_entries),
        "hasMore": len(filtered_entries) > resolved_limit,
    }

