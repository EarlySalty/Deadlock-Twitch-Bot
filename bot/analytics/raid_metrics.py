"""
Shared raid metric helpers.

These helpers recalculate chat-derived raid metrics in bulk to avoid N+1
queries per raid.
"""

from __future__ import annotations

import json
from typing import Any

from ..core.chat_bots import build_known_chat_bot_not_in_clause

DEFAULT_RECALC_BATCH_SIZE = 500


def raid_identity_key(raid_id: Any, executed_at: Any) -> tuple[int, str] | None:
    try:
        normalized_raid_id = int(raid_id)
    except Exception:
        return None
    return normalized_raid_id, str(_to_iso(executed_at) or "")


def _row_get(row: Any, key: str, index: int) -> Any:
    try:
        return row[key]
    except Exception:
        try:
            return row[index]
        except Exception:
            return None


def _to_iso(value: Any) -> str | None:
    if value is None:
        return None
    if hasattr(value, "isoformat"):
        try:
            return value.isoformat()
        except Exception:
            return str(value)
    return str(value)


def _recalculate_raid_chat_metrics_batch(
    conn,
    normalized_raids: list[dict[str, Any]],
) -> dict[tuple[int, str], dict[str, int]]:
    payload = json.dumps(normalized_raids)
    session_bot_clause, session_bot_params = build_known_chat_bot_not_in_clause(
        column_expr="sc.chatter_login",
        placeholder="%s",
    )
    rollup_bot_clause, rollup_bot_params = build_known_chat_bot_not_in_clause(
        column_expr="cr.chatter_login",
        placeholder="%s",
    )

    metrics: dict[tuple[int, str], dict[str, int]] = {
        key: {
            "plus5m": 0,
            "plus15m": 0,
            "plus30m": 0,
            "known_from_raider": 0,
            "new_chatters": 0,
        }
        for raid in normalized_raids
        if (key := raid_identity_key(raid.get("raid_id"), raid.get("executed_at_key"))) is not None
    }

    retention_rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
        f"""
        WITH raid_inputs AS (
            SELECT
                CAST(r.raid_id AS BIGINT) AS raid_id,
                COALESCE(r.executed_at_key, '') AS executed_at_key,
                CAST(r.target_session_id AS BIGINT) AS target_session_id,
                CAST(r.executed_at AS TIMESTAMPTZ) AS executed_at
            FROM json_to_recordset(%s::json) AS r(
                raid_id TEXT,
                executed_at_key TEXT,
                target_session_id TEXT,
                executed_at TEXT,
                from_login TEXT,
                to_login TEXT
            )
        )
        SELECT
            ri.raid_id,
            ri.executed_at_key,
            COUNT(
                DISTINCT CASE
                    WHEN sc.last_seen_at <= ri.executed_at + INTERVAL '5 minutes'
                    THEN COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id)
                    ELSE NULL
                END
            ) AS plus5m,
            COUNT(
                DISTINCT CASE
                    WHEN sc.last_seen_at <= ri.executed_at + INTERVAL '15 minutes'
                    THEN COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id)
                    ELSE NULL
                END
            ) AS plus15m,
            COUNT(DISTINCT COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id)) AS plus30m
        FROM raid_inputs ri
        LEFT JOIN twitch_session_chatters sc
            ON sc.session_id = ri.target_session_id
           AND ri.executed_at IS NOT NULL
           AND sc.last_seen_at >= ri.executed_at
           AND sc.last_seen_at <= ri.executed_at + INTERVAL '30 minutes'
           AND {session_bot_clause}
        GROUP BY ri.raid_id, ri.executed_at_key
        """,
        (payload, *session_bot_params),
    ).fetchall()

    for row in retention_rows:
        key = raid_identity_key(_row_get(row, "raid_id", 0), _row_get(row, "executed_at_key", 1))
        if key is None or key not in metrics:
            continue
        metrics[key]["plus5m"] = int(_row_get(row, "plus5m", 2) or 0)
        metrics[key]["plus15m"] = int(_row_get(row, "plus15m", 3) or 0)
        metrics[key]["plus30m"] = int(_row_get(row, "plus30m", 4) or 0)

    known_rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
        f"""
        WITH raid_inputs AS (
            SELECT
                CAST(r.raid_id AS BIGINT) AS raid_id,
                COALESCE(r.executed_at_key, '') AS executed_at_key,
                CAST(r.target_session_id AS BIGINT) AS target_session_id,
                CAST(r.executed_at AS TIMESTAMPTZ) AS executed_at,
                LOWER(COALESCE(r.from_login, '')) AS from_login
            FROM json_to_recordset(%s::json) AS r(
                raid_id TEXT,
                executed_at_key TEXT,
                target_session_id TEXT,
                executed_at TEXT,
                from_login TEXT,
                to_login TEXT
            )
        )
        SELECT
            ri.raid_id,
            ri.executed_at_key,
            COUNT(DISTINCT LOWER(sc.chatter_login)) AS known
        FROM raid_inputs ri
        JOIN twitch_session_chatters sc
            ON sc.session_id = ri.target_session_id
           AND ri.executed_at IS NOT NULL
           AND sc.last_seen_at >= ri.executed_at
           AND sc.chatter_login IS NOT NULL
           AND sc.chatter_login <> ''
           AND {session_bot_clause}
        JOIN twitch_chatter_rollup cr
            ON LOWER(cr.chatter_login) = LOWER(sc.chatter_login)
           AND LOWER(cr.streamer_login) = ri.from_login
           AND cr.first_seen_at < ri.executed_at
           AND {rollup_bot_clause}
        GROUP BY ri.raid_id, ri.executed_at_key
        """,
        (payload, *session_bot_params, *rollup_bot_params),
    ).fetchall()

    for row in known_rows:
        key = raid_identity_key(_row_get(row, "raid_id", 0), _row_get(row, "executed_at_key", 1))
        if key is None or key not in metrics:
            continue
        metrics[key]["known_from_raider"] = int(_row_get(row, "known", 2) or 0)

    new_rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
        f"""
        WITH raid_inputs AS (
            SELECT
                CAST(r.raid_id AS BIGINT) AS raid_id,
                COALESCE(r.executed_at_key, '') AS executed_at_key,
                CAST(r.target_session_id AS BIGINT) AS target_session_id,
                CAST(r.executed_at AS TIMESTAMPTZ) AS executed_at,
                LOWER(COALESCE(r.to_login, '')) AS to_login
            FROM json_to_recordset(%s::json) AS r(
                raid_id TEXT,
                executed_at_key TEXT,
                target_session_id TEXT,
                executed_at TEXT,
                from_login TEXT,
                to_login TEXT
            )
        )
        SELECT
            ri.raid_id,
            ri.executed_at_key,
            COUNT(DISTINCT COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id)) AS new_chatters
        FROM raid_inputs ri
        JOIN twitch_session_chatters sc
            ON sc.session_id = ri.target_session_id
           AND ri.executed_at IS NOT NULL
           AND sc.first_message_at >= ri.executed_at
           AND sc.messages > 0
           AND {session_bot_clause}
        LEFT JOIN twitch_chatter_rollup cr
            ON LOWER(cr.chatter_login) = LOWER(sc.chatter_login)
           AND LOWER(cr.streamer_login) = ri.to_login
           AND cr.first_seen_at < ri.executed_at
           AND {rollup_bot_clause}
        WHERE sc.chatter_login IS NULL
           OR sc.chatter_login = ''
           OR cr.chatter_login IS NULL
        GROUP BY ri.raid_id, ri.executed_at_key
        """,
        (payload, *session_bot_params, *rollup_bot_params),
    ).fetchall()

    for row in new_rows:
        key = raid_identity_key(_row_get(row, "raid_id", 0), _row_get(row, "executed_at_key", 1))
        if key is None or key not in metrics:
            continue
        metrics[key]["new_chatters"] = int(_row_get(row, "new_chatters", 2) or 0)

    return metrics


def recalculate_raid_chat_metrics(
    conn,
    raids: list[dict[str, Any]],
    *,
    batch_size: int = DEFAULT_RECALC_BATCH_SIZE,
) -> dict[tuple[int, str], dict[str, int]]:
    """
    Recalculate retention/new/known metrics for many raids.

    Expects each raid dict to contain:
    - raid_id
    - target_session_id
    - executed_at
    - from_login
    - to_login
    """
    normalized: list[dict[str, Any]] = []
    for raid in raids:
        try:
            raid_id = int(raid.get("raid_id"))
            target_session_id = int(raid.get("target_session_id"))
        except Exception:
            continue
        normalized.append(
            {
                "raid_id": raid_id,
                "executed_at_key": str(_to_iso(raid.get("executed_at")) or ""),
                "target_session_id": target_session_id,
                "executed_at": _to_iso(raid.get("executed_at")),
                "from_login": str(raid.get("from_login") or "").lower(),
                "to_login": str(raid.get("to_login") or "").lower(),
            }
        )

    if not normalized:
        return {}

    safe_batch_size = max(1, int(batch_size or 1))
    if len(normalized) <= safe_batch_size:
        return _recalculate_raid_chat_metrics_batch(conn, normalized)

    merged: dict[tuple[int, str], dict[str, int]] = {}
    for start in range(0, len(normalized), safe_batch_size):
        chunk = normalized[start : start + safe_batch_size]
        merged.update(_recalculate_raid_chat_metrics_batch(conn, chunk))
    return merged
