"""Shared helpers for raw-chat availability and ingestion-gap detection."""

from __future__ import annotations

from datetime import UTC, datetime
from typing import Any


def _coerce_timestamp(value: Any) -> datetime | None:
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


def _iso_or_none(value: Any) -> str | None:
    parsed = _coerce_timestamp(value)
    return parsed.isoformat() if parsed else None


def _query_scope_presence_stats(
    conn,
    *,
    streamer_login: str,
    since_date: str | None = None,
    session_ids: list[int] | None = None,
) -> dict[str, Any]:
    if session_ids is not None:
        if not session_ids:
            return {
                "presenceRows": 0,
                "sessionsWithPresence": 0,
                "gapStart": None,
            }
        placeholders = ",".join("%s" for _ in session_ids)
        presence_row = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
            f"""
            SELECT
                COUNT(*) AS presence_rows,
                COUNT(DISTINCT sc.session_id) AS sessions_with_presence
            FROM twitch_session_chatters sc
            WHERE LOWER(sc.streamer_login) = %s
              AND sc.session_id IN ({placeholders})
            """,
            (streamer_login, *session_ids),
        ).fetchone()
        gap_row = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
            f"""
            SELECT MIN(s.started_at) AS gap_start
            FROM twitch_stream_sessions s
            WHERE LOWER(s.streamer_login) = %s
              AND s.id IN ({placeholders})
              AND EXISTS (
                  SELECT 1
                  FROM twitch_session_chatters sc
                  WHERE sc.session_id = s.id
              )
              AND NOT EXISTS (
                  SELECT 1
                  FROM twitch_chat_messages m
                  WHERE m.session_id = s.id
              )
            """,
            (streamer_login, *session_ids),
        ).fetchone()
    else:
        presence_row = conn.execute(
            """
            SELECT
                COUNT(*) AS presence_rows,
                COUNT(DISTINCT sc.session_id) AS sessions_with_presence
            FROM twitch_session_chatters sc
            JOIN twitch_stream_sessions s ON s.id = sc.session_id
            WHERE LOWER(s.streamer_login) = %s
              AND s.started_at >= %s
            """,
            (streamer_login, since_date),
        ).fetchone()
        gap_row = conn.execute(
            """
            SELECT MIN(s.started_at) AS gap_start
            FROM twitch_stream_sessions s
            WHERE LOWER(s.streamer_login) = %s
              AND s.started_at >= %s
              AND EXISTS (
                  SELECT 1
                  FROM twitch_session_chatters sc
                  WHERE sc.session_id = s.id
              )
              AND NOT EXISTS (
                  SELECT 1
                  FROM twitch_chat_messages m
                  WHERE m.session_id = s.id
              )
            """,
            (streamer_login, since_date),
        ).fetchone()

    return {
        "presenceRows": int((presence_row[0] if presence_row else 0) or 0),
        "sessionsWithPresence": int((presence_row[1] if presence_row else 0) or 0),
        "gapStart": _iso_or_none(gap_row[0] if gap_row else None),
    }


def _query_scope_raw_stats(
    conn,
    *,
    streamer_login: str,
    since_date: str | None = None,
    session_ids: list[int] | None = None,
) -> dict[str, Any]:
    if session_ids is not None:
        if not session_ids:
            return {
                "rawRows": 0,
                "sessionsWithRaw": 0,
                "lastMessageAt": None,
            }
        placeholders = ",".join("%s" for _ in session_ids)
        row = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
            f"""
            SELECT
                COUNT(*) AS raw_rows,
                COUNT(DISTINCT m.session_id) AS sessions_with_raw,
                MAX(m.message_ts) AS last_message_at
            FROM twitch_chat_messages m
            WHERE LOWER(m.streamer_login) = %s
              AND m.session_id IN ({placeholders})
            """,
            (streamer_login, *session_ids),
        ).fetchone()
    else:
        row = conn.execute(
            """
            SELECT
                COUNT(*) AS raw_rows,
                COUNT(DISTINCT m.session_id) AS sessions_with_raw,
                MAX(m.message_ts) AS last_message_at
            FROM twitch_chat_messages m
            WHERE LOWER(m.streamer_login) = %s
              AND m.message_ts >= %s
            """,
            (streamer_login, since_date),
        ).fetchone()

    return {
        "rawRows": int((row[0] if row else 0) or 0),
        "sessionsWithRaw": int((row[1] if row else 0) or 0),
        "lastMessageAt": _iso_or_none(row[2] if row else None),
    }


def build_raw_chat_status(
    conn,
    streamer_login: str,
    *,
    since_date: str | None = None,
    session_ids: list[int] | None = None,
) -> dict[str, Any]:
    normalized_streamer = str(streamer_login or "").strip().lower()
    if not normalized_streamer:
        return {
            "available": False,
            "lastMessageAt": None,
            "gapStart": None,
            "suspectedIngestionIssue": False,
            "backfillState": "not_needed",
            "note": None,
        }

    health_row = None
    latest_backfill_row = None
    last_message_at = None
    try:
        health_row = conn.execute(
            """
            SELECT
                last_raw_chat_message_at,
                last_raw_chat_insert_ok_at,
                last_raw_chat_insert_error_at,
                last_raw_chat_error
            FROM twitch_raw_chat_ingest_health
            WHERE LOWER(streamer_login) = %s
            LIMIT 1
            """,
            (normalized_streamer,),
        ).fetchone()
    except Exception:
        health_row = None

    try:
        fallback_row = conn.execute(
            """
            SELECT MAX(message_ts) AS last_message_at
            FROM twitch_chat_messages
            WHERE LOWER(streamer_login) = %s
            """,
            (normalized_streamer,),
        ).fetchone()
        last_message_at = _iso_or_none(fallback_row[0] if fallback_row else None)
    except Exception:
        last_message_at = None

    health_last_message_at = _iso_or_none(health_row[0] if health_row else None)
    health_last_insert_ok_at = _iso_or_none(health_row[1] if health_row else None)
    health_last_insert_error_at = _iso_or_none(health_row[2] if health_row else None)
    health_last_error = str((health_row[3] if health_row else "") or "").strip() or None

    scope_presence = _query_scope_presence_stats(
        conn,
        streamer_login=normalized_streamer,
        since_date=since_date,
        session_ids=session_ids,
    )
    scope_raw = _query_scope_raw_stats(
        conn,
        streamer_login=normalized_streamer,
        since_date=since_date,
        session_ids=session_ids,
    )

    suspected_issue = False
    gap_start = scope_presence["gapStart"]
    if scope_presence["presenceRows"] > 0 and scope_raw["rawRows"] == 0:
        suspected_issue = True
    elif scope_presence["sessionsWithPresence"] > scope_raw["sessionsWithRaw"] > 0:
        suspected_issue = True

    backfill_state = "not_needed"
    try:
        latest_backfill_row = conn.execute(
            """
            SELECT status, note
            FROM twitch_raw_chat_backfill_runs
            WHERE LOWER(streamer_login) = %s
            ORDER BY COALESCE(finished_at, started_at) DESC
            LIMIT 1
            """,
            (normalized_streamer,),
        ).fetchone()
    except Exception:
        latest_backfill_row = None

    if latest_backfill_row:
        backfill_state = str(latest_backfill_row[0] or "").strip() or "not_started"
    elif suspected_issue:
        backfill_state = "not_started"

    note = None
    if suspected_issue and scope_raw["rawRows"] == 0:
        note = (
            "Presence-/Rollup-Daten vorhanden, aber keine Roh-Chat-Nachrichten "
            "im gewählten Zeitraum."
        )
    elif suspected_issue:
        note = (
            "Roh-Chat-Nachrichten sind im gewählten Zeitraum nur teilweise vorhanden; "
            "message-basierte KPIs sind unvollständig."
        )
    elif scope_raw["rawRows"] == 0:
        note = "Keine Roh-Chat-Nachrichten im gewählten Zeitraum."

    if not note and health_last_error and health_last_insert_error_at:
        note = f"Letzter Roh-Chat-Insert-Fehler: {health_last_error}"

    return {
        "available": scope_raw["rawRows"] > 0,
        "lastMessageAt": scope_raw["lastMessageAt"] or health_last_message_at or last_message_at,
        "gapStart": gap_start,
        "suspectedIngestionIssue": suspected_issue,
        "backfillState": backfill_state,
        "note": note,
        "lastInsertOkAt": health_last_insert_ok_at,
        "lastInsertErrorAt": health_last_insert_error_at,
    }


def build_viewer_window_metadata(
    conn,
    streamer_login: str,
    logins: list[str],
    *,
    since_date: str,
) -> dict[str, dict[str, Any]]:
    normalized_streamer = str(streamer_login or "").strip().lower()
    normalized_logins = sorted(
        {
            str(login or "").strip().lower()
            for login in logins
            if str(login or "").strip()
        }
    )
    if not normalized_streamer or not normalized_logins:
        return {}

    placeholders = ",".join("%s" for _ in normalized_logins)

    presence_rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
        f"""
        SELECT
            LOWER(sc.chatter_login) AS chatter_login,
            COUNT(DISTINCT sc.session_id) AS window_sessions,
            COALESCE(SUM(sc.messages), 0) AS window_presence_messages
        FROM twitch_session_chatters sc
        JOIN twitch_stream_sessions s ON s.id = sc.session_id
        WHERE LOWER(s.streamer_login) = %s
          AND s.started_at >= %s
          AND LOWER(sc.chatter_login) IN ({placeholders})
        GROUP BY LOWER(sc.chatter_login)
        """,
        (normalized_streamer, since_date, *normalized_logins),
    ).fetchall()

    raw_rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
        f"""
        SELECT
            LOWER(m.chatter_login) AS chatter_login,
            COUNT(*) AS raw_messages
        FROM twitch_chat_messages m
        WHERE LOWER(m.streamer_login) = %s
          AND m.message_ts >= %s
          AND LOWER(m.chatter_login) IN ({placeholders})
        GROUP BY LOWER(m.chatter_login)
        """,
        (normalized_streamer, since_date, *normalized_logins),
    ).fetchall()

    result: dict[str, dict[str, Any]] = {
        login: {
            "windowPresenceSessions": 0,
            "windowPresenceMessages": 0,
            "windowRawMessages": 0,
            "hasRawMessages": False,
            "presenceOnlyInWindow": False,
            "messageGapNote": None,
        }
        for login in normalized_logins
    }

    for row in presence_rows:
        login = str(row[0] or "").strip().lower()
        if not login:
            continue
        result.setdefault(login, {})
        result[login]["windowPresenceSessions"] = int(row[1] or 0)
        result[login]["windowPresenceMessages"] = int(row[2] or 0)

    for row in raw_rows:
        login = str(row[0] or "").strip().lower()
        if not login:
            continue
        result.setdefault(login, {})
        raw_messages = int(row[1] or 0)
        result[login]["windowRawMessages"] = raw_messages
        result[login]["hasRawMessages"] = raw_messages > 0

    for login, meta in result.items():
        presence_only = (
            int(meta.get("windowPresenceSessions") or 0) > 0
            and int(meta.get("windowRawMessages") or 0) == 0
        )
        meta["presenceOnlyInWindow"] = presence_only
        if presence_only:
            meta["messageGapNote"] = (
                "Nur Presence-/Rollup-Daten im gewählten Zeitraum; "
                "keine Roh-Chat-Nachrichten vorhanden."
            )

    return result


__all__ = ["build_raw_chat_status", "build_viewer_window_metadata"]
