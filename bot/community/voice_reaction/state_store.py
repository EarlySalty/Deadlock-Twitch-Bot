from __future__ import annotations

import contextlib
import json
import logging
from collections.abc import Callable
from datetime import UTC, datetime
from typing import Any, ContextManager

from bot.storage import readonly_connection, transaction

log = logging.getLogger("TwitchStreams.VoiceReaction.StateStore")

ReadonlyConnectionFactory = Callable[[], ContextManager[Any]]
TransactionFactory = Callable[[], ContextManager[Any]]
_ROLE_TO_COLUMN = {
    "voice": "last_voice_capture_at",
    "streamer_chat": "last_streamer_signal_at",
    "bot_chat": "last_bot_message_at",
}


def open_conversation(
    *,
    streamer_login: str,
    streamer_user_id: str | None,
    source: str,
    initial_messages: list[dict] | None = None,
    transaction_factory: TransactionFactory | None = None,
) -> bool:
    """Legt eine neue Conversation-Row an und markiert Outreach als offen."""
    normalized = str(streamer_login or "").strip().lower()
    if not normalized:
        return False

    payload = json.dumps(initial_messages or [], default=str, separators=(",", ":"))
    factory = transaction_factory or transaction
    try:
        with factory() as conn:
            cursor = conn.execute(
                """
                INSERT INTO twitch_partner_outreach_conversations
                    (streamer_login, streamer_user_id, source, messages_json)
                VALUES (%s, %s, %s, %s::jsonb)
                ON CONFLICT (streamer_login) DO NOTHING
                """,
                (normalized, streamer_user_id, source, payload),
            )
            conn.execute(
                """
                UPDATE twitch_partner_outreach
                   SET conversation_status = 'open'
                 WHERE streamer_login = %s
                """,
                (normalized,),
            )
            conn.commit()
            return int(getattr(cursor, "rowcount", 0) or 0) > 0
    except Exception:
        log.debug("VoiceReaction: open_conversation fehlgeschlagen für %s", normalized, exc_info=True)
        return False


def append_message(
    *,
    streamer_login: str,
    role: str,
    text: str,
    meta: dict | None = None,
    transaction_factory: TransactionFactory | None = None,
) -> bool:
    """Appendiert eine Nachricht an die Conversation-History."""
    normalized = str(streamer_login or "").strip().lower()
    if not normalized:
        return False

    entry = {
        "role": role,
        "ts": datetime.now(UTC).isoformat(),
        "text": text,
        "meta": meta or {},
    }
    append_json = json.dumps([entry], default=str, separators=(",", ":"))
    last_column = _ROLE_TO_COLUMN.get(role)
    assignments = ["messages_json = messages_json || %s::jsonb", "updated_at = NOW()"]
    if last_column:
        assignments.append(f"{last_column} = NOW()")

    factory = transaction_factory or transaction
    try:
        with factory() as conn:
            cursor = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                f"""
                UPDATE twitch_partner_outreach_conversations
                   SET {", ".join(assignments)}
                 WHERE streamer_login = %s
                """,
                (append_json, normalized),
            )
            conn.commit()
            return int(getattr(cursor, "rowcount", 0) or 0) > 0
    except Exception:
        log.debug("VoiceReaction: append_message fehlgeschlagen für %s", normalized, exc_info=True)
        return False


def update_state(
    *,
    streamer_login: str,
    new_state: str,
    last_stance: str | None = None,
    last_confidence: float | None = None,
    error_kind: str | None = None,
    error_detail: str | None = None,
    transaction_factory: TransactionFactory | None = None,
) -> bool:
    """Setzt den Conversation-State und optionale Metadaten."""
    normalized = str(streamer_login or "").strip().lower()
    if not normalized:
        return False

    factory = transaction_factory or transaction
    try:
        with factory() as conn:
            cursor = conn.execute(
                """
                UPDATE twitch_partner_outreach_conversations
                   SET state = %s,
                       updated_at = NOW(),
                       last_stance = COALESCE(%s, last_stance),
                       last_confidence = COALESCE(%s, last_confidence),
                       error_kind = COALESCE(%s, error_kind),
                       error_detail = COALESCE(%s, error_detail)
                 WHERE streamer_login = %s
                """,
                (
                    new_state,
                    last_stance,
                    last_confidence,
                    error_kind,
                    error_detail,
                    normalized,
                ),
            )
            conn.commit()
            return int(getattr(cursor, "rowcount", 0) or 0) > 0
    except Exception:
        log.debug("VoiceReaction: update_state fehlgeschlagen für %s", normalized, exc_info=True)
        return False


def close_conversation(
    *,
    streamer_login: str,
    close_reason: str,
    extend_cooldown_days: int | None = None,
    transaction_factory: TransactionFactory | None = None,
) -> bool:
    """Schließt eine Conversation und markiert das Outreach als beendet."""
    normalized = str(streamer_login or "").strip().lower()
    if not normalized:
        return False

    closed_state = f"closed_{close_reason}"
    factory = transaction_factory or transaction
    try:
        with factory() as conn:
            cursor = conn.execute(
                """
                UPDATE twitch_partner_outreach_conversations
                   SET state = %s,
                       closed_at = NOW(),
                       updated_at = NOW()
                 WHERE streamer_login = %s
                """,
                (closed_state, normalized),
            )
            updated = int(getattr(cursor, "rowcount", 0) or 0)
            if updated <= 0:
                conn.commit()
                return False

            if extend_cooldown_days is None:
                conn.execute(
                    """
                    UPDATE twitch_partner_outreach
                       SET conversation_status = 'closed'
                     WHERE streamer_login = %s
                    """,
                    (normalized,),
                )
            else:
                conn.execute(
                    """
                    UPDATE twitch_partner_outreach
                       SET cooldown_until = GREATEST(
                             COALESCE(cooldown_until::timestamptz, NOW()),
                             NOW() + (%s || ' days')::interval
                           )::text,
                           conversation_status = 'closed'
                     WHERE streamer_login = %s
                    """,
                    (str(int(extend_cooldown_days)), normalized),
                )
            conn.commit()
            return True
    except Exception:
        log.debug("VoiceReaction: close_conversation fehlgeschlagen für %s", normalized, exc_info=True)
        return False


def get_conversation(
    *,
    streamer_login: str,
    connection_factory: ReadonlyConnectionFactory | None = None,
) -> dict[str, Any] | None:
    """Liest eine vollständige Conversation-Row."""
    normalized = str(streamer_login or "").strip().lower()
    if not normalized:
        return None

    factory = connection_factory or readonly_connection
    try:
        with factory() as conn:
            row = conn.execute(
                """
                SELECT *
                FROM twitch_partner_outreach_conversations
                WHERE streamer_login = %s
                """,
                (normalized,),
            ).fetchone()
    except Exception:
        log.debug("VoiceReaction: get_conversation fehlgeschlagen für %s", normalized, exc_info=True)
        return None

    if row is None:
        return None
    return _normalize_conversation_row(row)


def load_active_conversations(
    *,
    connection_factory: ReadonlyConnectionFactory | None = None,
) -> list[dict[str, Any]]:
    """Lädt alle aktiven Conversations."""
    factory = connection_factory or readonly_connection
    try:
        with factory() as conn:
            rows = conn.execute(
                """
                SELECT streamer_login,
                       streamer_user_id,
                       source,
                       state,
                       messages_json,
                       last_voice_capture_at,
                       last_streamer_signal_at,
                       last_bot_message_at,
                       last_stance
                FROM twitch_partner_outreach_conversations
                WHERE state IN ('open', 'listening', 'brain_pending')
                ORDER BY COALESCE(last_streamer_signal_at, created_at) DESC
                """
            ).fetchall()
    except Exception:
        log.debug("VoiceReaction: load_active_conversations fehlgeschlagen", exc_info=True)
        return []

    return [_normalize_conversation_row(row) for row in rows]


def has_active_conversation(
    streamer_login: str,
    *,
    connection_factory: ReadonlyConnectionFactory | None = None,
) -> bool:
    """Quick-Check, ob eine aktive Conversation existiert."""
    normalized = str(streamer_login or "").strip().lower()
    if not normalized:
        return False

    factory = connection_factory or readonly_connection
    try:
        with factory() as conn:
            row = conn.execute(
                """
                SELECT 1
                FROM twitch_partner_outreach_conversations
                WHERE streamer_login = %s
                  AND state IN ('open', 'listening', 'brain_pending')
                LIMIT 1
                """,
                (normalized,),
            ).fetchone()
    except Exception:
        log.debug(
            "VoiceReaction: has_active_conversation fehlgeschlagen für %s",
            normalized,
            exc_info=True,
        )
        return False
    return row is not None


def _normalize_conversation_row(row: Any) -> dict[str, Any]:
    raw = _row_to_dict(row)
    raw["messages_json"] = _parse_messages(raw.get("messages_json"))
    for key, value in list(raw.items()):
        if key == "messages_json":
            continue
        raw[key] = _to_iso_string(value)
    return raw


def _row_to_dict(row: Any) -> dict[str, Any]:
    if hasattr(row, "keys"):
        return {key: row[key] for key in row.keys()}
    return dict(row)


def _parse_messages(value: Any) -> list[dict[str, Any]]:
    if isinstance(value, list):
        return [entry for entry in value if isinstance(entry, dict)]
    if not value:
        return []
    with contextlib.suppress(TypeError, ValueError, json.JSONDecodeError):
        parsed = json.loads(value)
        if isinstance(parsed, list):
            return [entry for entry in parsed if isinstance(entry, dict)]
    return []


def _to_iso_string(value: Any) -> Any:
    if isinstance(value, datetime):
        return value.isoformat()
    return value


__all__ = [
    "open_conversation",
    "append_message",
    "update_state",
    "close_conversation",
    "get_conversation",
    "load_active_conversations",
    "has_active_conversation",
    "TransactionFactory",
    "ReadonlyConnectionFactory",
]
