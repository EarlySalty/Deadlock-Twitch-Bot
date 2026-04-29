from __future__ import annotations

import json
import logging
import uuid
from collections.abc import Callable
from typing import Any, ContextManager

from bot.storage import transaction

log = logging.getLogger("TwitchStreams.VoiceReaction.AuditLog")

TransactionFactory = Callable[[], ContextManager[Any]]


def audit(
    streamer_login: str,
    event_kind: str,
    payload: dict | None = None,
    *,
    correlation_id: str | None = None,
    transaction_factory: TransactionFactory | None = None,
) -> int | None:
    """Schreibt best-effort einen Audit-Log-Eintrag."""
    normalized = str(streamer_login or "").strip().lower()
    if not normalized:
        return None

    payload_json = json.dumps(payload or {}, default=str, separators=(",", ":"))
    factory = transaction_factory or transaction
    try:
        with factory() as conn:
            if correlation_id is None:
                cursor = conn.execute(
                    """
                    INSERT INTO twitch_partner_outreach_audit
                        (streamer_login, event_kind, payload_json, correlation_id)
                    VALUES (%s, %s, %s::jsonb, NULL)
                    RETURNING id
                    """,
                    (normalized, event_kind, payload_json),
                )
            else:
                cursor = conn.execute(
                    """
                    INSERT INTO twitch_partner_outreach_audit
                        (streamer_login, event_kind, payload_json, correlation_id)
                    VALUES (%s, %s, %s::jsonb, %s::uuid)
                    RETURNING id
                    """,
                    (normalized, event_kind, payload_json, correlation_id),
                )
            row = cursor.fetchone()
            conn.commit()
            if row is None:
                return None
            if hasattr(row, "keys"):
                return int(row["id"])
            return int(row[0])
    except Exception:
        log.warning(
            "AuditLog: konnte event=%s für %s nicht schreiben",
            event_kind,
            normalized,
            exc_info=True,
        )
        return None


def new_correlation_id() -> str:
    """Erzeugt eine neue Korrelations-ID."""
    return uuid.uuid4().hex


__all__ = ["audit", "new_correlation_id"]
