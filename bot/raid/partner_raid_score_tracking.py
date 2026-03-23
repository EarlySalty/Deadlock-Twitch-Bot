"""Track confirmed partner raid score snapshots and post-raid Deadlock duration."""

from __future__ import annotations

import logging
from datetime import UTC, datetime, timedelta
from typing import Any

from ..core.constants import TWITCH_TARGET_GAME_NAME
from ..storage import transaction

log = logging.getLogger("TwitchStreams.PartnerRaidScoreTracking")


def _parse_dt(value: object) -> datetime | None:
    text = str(value or "").strip()
    if not text:
        return None
    normalized = text.replace("Z", "+00:00")
    try:
        parsed = datetime.fromisoformat(normalized)
    except ValueError:
        return None
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=UTC)
    return parsed.astimezone(UTC)


def _iso_utc(value: datetime) -> str:
    return value.astimezone(UTC).isoformat(timespec="seconds")


def _row_value(row: Any, key: str, index: int, default: object = None) -> object:
    if row is None:
        return default
    if hasattr(row, "keys"):
        try:
            return row[key]
        except Exception:
            return default
    try:
        return row[index]
    except Exception:
        return default


def _safe_int(value: object, default: int = 0) -> int:
    try:
        if value is None:
            return default
        return int(value)
    except (TypeError, ValueError):
        return default


def _safe_float(value: object, default: float = 0.0) -> float:
    try:
        if value is None:
            return default
        return float(value)
    except (TypeError, ValueError):
        return default


def _target_game_lower() -> str:
    return (TWITCH_TARGET_GAME_NAME or "").strip().lower()


def _score_payload(score_snapshot: dict | None) -> dict[str, object]:
    snapshot = score_snapshot if isinstance(score_snapshot, dict) else {}
    return {
        "final_score": _safe_float(snapshot.get("final_score"), 0.0),
        "base_score": _safe_float(snapshot.get("base_score"), 0.0),
        "duration_score": _safe_float(snapshot.get("duration_score"), 0.5),
        "time_pattern_score": _safe_float(snapshot.get("time_pattern_score"), 0.5),
        "new_partner_multiplier": _safe_float(snapshot.get("new_partner_multiplier"), 1.0),
        "raid_boost_multiplier": _safe_float(snapshot.get("raid_boost_multiplier"), 1.0),
        "today_received_raids": _safe_int(snapshot.get("today_received_raids"), 0),
        "score_last_computed_at": str(snapshot.get("last_computed_at") or "").strip() or None,
    }


def _lookup_open_session_id(
    conn,
    *,
    streamer_login: str,
    target_stream_started_at: str | None,
) -> int | None:
    login_lower = str(streamer_login or "").strip().lower()
    if not login_lower:
        return None
    row = conn.execute(
        """
        SELECT id
        FROM twitch_stream_sessions
        WHERE LOWER(streamer_login) = LOWER(%s)
          AND ended_at IS NULL
        ORDER BY
            CASE WHEN COALESCE(started_at, '') = COALESCE(%s, '') THEN 0 ELSE 1 END,
            started_at DESC,
            id DESC
        LIMIT 1
        """,
        (login_lower, target_stream_started_at),
    ).fetchone()
    return _safe_int(_row_value(row, "id", 0), 0) or None


def _load_cached_score_snapshot(conn, twitch_user_id: str) -> dict[str, object]:
    row = conn.execute(
        """
        SELECT final_score, base_score, duration_score, time_pattern_score,
               new_partner_multiplier, raid_boost_multiplier,
               today_received_raids, last_computed_at
        FROM twitch_partner_raid_scores
        WHERE twitch_user_id = %s
        """,
        (twitch_user_id,),
    ).fetchone()
    if row is None:
        return {}
    return {
        "final_score": _safe_float(_row_value(row, "final_score", 0), 0.0),
        "base_score": _safe_float(_row_value(row, "base_score", 1), 0.0),
        "duration_score": _safe_float(_row_value(row, "duration_score", 2), 0.5),
        "time_pattern_score": _safe_float(_row_value(row, "time_pattern_score", 3), 0.5),
        "new_partner_multiplier": _safe_float(_row_value(row, "new_partner_multiplier", 4), 1.0),
        "raid_boost_multiplier": _safe_float(_row_value(row, "raid_boost_multiplier", 5), 1.0),
        "today_received_raids": _safe_int(_row_value(row, "today_received_raids", 6), 0),
        "last_computed_at": str(_row_value(row, "last_computed_at", 7) or "").strip() or None,
    }


def _load_raid_history_reference(
    conn,
    *,
    target_id: str,
    target_login: str,
    source_login: str,
    source_id: str | None,
    confirmed_at: datetime,
) -> tuple[int | None, str | None]:
    confirmed_upper_bound = _iso_utc(confirmed_at.astimezone(UTC) + timedelta(minutes=10))

    def _extract_reference(row: Any) -> tuple[int | None, str | None]:
        raid_history_id = _safe_int(_row_value(row, "id", 0), 0) or None
        executed_at = _parse_dt(_row_value(row, "executed_at", 1))
        return raid_history_id, (_iso_utc(executed_at) if executed_at is not None else None)

    if source_id:
        row = conn.execute(
            """
            SELECT id, executed_at
            FROM twitch_raid_history
            WHERE to_broadcaster_id = %s
              AND LOWER(to_broadcaster_login) = LOWER(%s)
              AND from_broadcaster_id = %s
              AND LOWER(from_broadcaster_login) = LOWER(%s)
              AND COALESCE(success, FALSE) IS TRUE
              AND executed_at <= %s
            ORDER BY executed_at DESC, id DESC
            LIMIT 1
            """,
            (target_id, target_login, source_id, source_login, confirmed_upper_bound),
        ).fetchone()
        raid_history_reference = _extract_reference(row)
        if raid_history_reference[0] is not None:
            return raid_history_reference
    row = conn.execute(
        """
        SELECT id, executed_at
        FROM twitch_raid_history
        WHERE to_broadcaster_id = %s
          AND LOWER(to_broadcaster_login) = LOWER(%s)
          AND LOWER(from_broadcaster_login) = LOWER(%s)
          AND COALESCE(success, FALSE) IS TRUE
          AND executed_at <= %s
        ORDER BY executed_at DESC, id DESC
        LIMIT 1
        """,
        (target_id, target_login, source_login, confirmed_upper_bound),
    ).fetchone()
    return _extract_reference(row)


def _load_session_started_at(conn, session_id: int) -> datetime | None:
    row = conn.execute(
        """
        SELECT started_at
        FROM twitch_stream_sessions
        WHERE id = %s
        LIMIT 1
        """,
        (int(session_id),),
    ).fetchone()
    return _parse_dt(_row_value(row, "started_at", 0))


def _load_unresolved_tracking_rows_for_session(
    conn,
    *,
    session_id: int,
    target_id: str,
    login_lower: str,
    session_started_at: datetime | None,
    session_ended_at: datetime,
) -> list[Any]:
    rows = conn.execute(
        """
        SELECT id, confirmed_at, to_broadcaster_id, was_deadlock_at_raid
        FROM twitch_partner_raid_score_tracking
        WHERE target_session_id = %s
          AND resolved_at IS NULL
        ORDER BY confirmed_at ASC, id ASC
        """,
        (int(session_id),),
    ).fetchall()
    if session_started_at is None:
        return rows

    target_identifier_sql = ""
    params: list[object] = []
    if target_id:
        target_identifier_sql = "to_broadcaster_id = %s"
        params.append(target_id)
    elif login_lower:
        target_identifier_sql = "LOWER(to_broadcaster_login) = LOWER(%s)"
        params.append(login_lower)
    else:
        return []

    params.extend(
        [
            _iso_utc(session_started_at),
            _iso_utc(session_ended_at),
            _iso_utc(session_started_at),
        ]
    )
    fallback_rows = conn.execute(
        f"""
        SELECT id, confirmed_at, to_broadcaster_id, was_deadlock_at_raid
        FROM twitch_partner_raid_score_tracking
        WHERE target_session_id IS NULL
          AND resolved_at IS NULL
          AND {target_identifier_sql}
          AND confirmed_at >= %s
          AND confirmed_at <= %s
          AND (
              target_stream_started_at IS NULL
              OR target_stream_started_at = %s
          )
        ORDER BY confirmed_at ASC, id ASC
        """,
        tuple(params),
    ).fetchall()

    if not fallback_rows:
        return rows

    combined: dict[int, Any] = {}
    for row in rows:
        row_id = _safe_int(_row_value(row, "id", 0), 0)
        if row_id:
            combined[row_id] = row
    for row in fallback_rows:
        row_id = _safe_int(_row_value(row, "id", 0), 0)
        if row_id and row_id not in combined:
            combined[row_id] = row

    return sorted(
        combined.values(),
        key=lambda row: (
            _parse_dt(_row_value(row, "confirmed_at", 1)) or datetime.min.replace(tzinfo=UTC),
            _safe_int(_row_value(row, "id", 0), 0),
        ),
    )


def track_confirmed_partner_raid(
    *,
    to_broadcaster_id: str,
    to_broadcaster_login: str,
    from_broadcaster_login: str,
    from_broadcaster_id: str | None = None,
    viewer_count: int = 0,
    score_snapshot: dict | None = None,
    confirmed_at: datetime | None = None,
) -> int | None:
    target_id = str(to_broadcaster_id or "").strip()
    if not target_id:
        return None

    confirmed_dt = (confirmed_at or datetime.now(UTC)).astimezone(UTC)
    confirmed_at_iso = _iso_utc(confirmed_dt)
    target_login = str(to_broadcaster_login or "").strip().lower()
    source_login = str(from_broadcaster_login or "").strip().lower()
    source_id = str(from_broadcaster_id or "").strip() or None

    try:
        with transaction() as conn:
            live_state = conn.execute(
                """
                SELECT active_session_id, last_started_at, last_game, streamer_login
                FROM twitch_live_state
                WHERE twitch_user_id = %s
                """,
                (target_id,),
            ).fetchone()
            if score_snapshot:
                score_payload = _score_payload(score_snapshot)
            else:
                score_payload = _score_payload(_load_cached_score_snapshot(conn, target_id))

            active_session_id = _row_value(live_state, "active_session_id", 0, None)
            target_stream_started_at = (
                str(_row_value(live_state, "last_started_at", 1) or "").strip() or None
            )
            last_game = str(_row_value(live_state, "last_game", 2) or "").strip().lower()
            streamer_login = str(
                _row_value(live_state, "streamer_login", 3, target_login) or target_login
            ).strip().lower()
            active_session_id_value = _safe_int(active_session_id, 0) or None
            if active_session_id_value is None:
                active_session_id_value = _lookup_open_session_id(
                    conn,
                    streamer_login=streamer_login,
                    target_stream_started_at=target_stream_started_at,
                )
            was_deadlock_at_raid = bool(last_game and last_game == _target_game_lower())

            raid_history_id, raid_history_executed_at = _load_raid_history_reference(
                conn,
                target_id=target_id,
                target_login=target_login,
                source_login=source_login,
                source_id=source_id,
                confirmed_at=confirmed_dt,
            )

            deadlock_continued_until = None if was_deadlock_at_raid else confirmed_at_iso
            deadlock_continued_sec = None if was_deadlock_at_raid else 0
            resolved_at = None if was_deadlock_at_raid else confirmed_at_iso
            resolution_reason = None if was_deadlock_at_raid else "not_deadlock_at_raid"

            cur = conn.execute(
                """
                INSERT INTO twitch_partner_raid_score_tracking (
                    raid_history_id,
                    raid_history_executed_at,
                    from_broadcaster_id,
                    from_broadcaster_login,
                    to_broadcaster_id,
                    to_broadcaster_login,
                    viewer_count,
                    confirmed_at,
                    target_session_id,
                    target_stream_started_at,
                    score_last_computed_at,
                    final_score,
                    base_score,
                    duration_score,
                    time_pattern_score,
                    new_partner_multiplier,
                    raid_boost_multiplier,
                    today_received_raids,
                    was_deadlock_at_raid,
                    deadlock_continued_until,
                    deadlock_continued_sec,
                    resolved_at,
                    resolution_reason
                ) VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s)
                """,
                (
                    raid_history_id,
                    raid_history_executed_at,
                    source_id,
                    source_login,
                    target_id,
                    target_login,
                    _safe_int(viewer_count, 0),
                    confirmed_at_iso,
                    active_session_id_value,
                    target_stream_started_at,
                    score_payload.get("score_last_computed_at"),
                    score_payload.get("final_score"),
                    score_payload.get("base_score"),
                    score_payload.get("duration_score"),
                    score_payload.get("time_pattern_score"),
                    score_payload.get("new_partner_multiplier"),
                    score_payload.get("raid_boost_multiplier"),
                    score_payload.get("today_received_raids"),
                    int(was_deadlock_at_raid),
                    deadlock_continued_until,
                    deadlock_continued_sec,
                    resolved_at,
                    resolution_reason,
                ),
            )
            tracking_id = getattr(cur, "lastrowid", None)
            if tracking_id is None and raid_history_id is not None:
                row = conn.execute(
                    """
                    SELECT id
                    FROM twitch_partner_raid_score_tracking
                    WHERE raid_history_id = %s
                      AND (
                          raid_history_executed_at = %s
                          OR (%s IS NULL AND raid_history_executed_at IS NULL)
                      )
                    ORDER BY id DESC
                    LIMIT 1
                    """,
                    (raid_history_id, raid_history_executed_at, raid_history_executed_at),
                ).fetchone()
                tracking_id = _safe_int(_row_value(row, "id", 0), 0) or None

        log.info(
            "Partner raid score tracking stored for %s -> %s (raid_history_id=%s, raid_history_executed_at=%s, deadlock_at_raid=%s)",
            source_login or source_id or "<unknown>",
            target_login or target_id,
            raid_history_id,
            raid_history_executed_at,
            was_deadlock_at_raid,
        )
        return _safe_int(tracking_id, 0) or None
    except Exception:
        log.debug(
            "Partner raid score tracking store failed for %s -> %s",
            source_login or source_id or "<unknown>",
            target_login or target_id,
            exc_info=True,
        )
        return None


def resolve_partner_raid_tracking_for_session(
    *,
    twitch_user_id: str | None,
    streamer_login: str,
    session_id: int | None,
    session_ended_at: datetime | str | None,
) -> int:
    resolved = 0
    target_id = str(twitch_user_id or "").strip()
    login_lower = str(streamer_login or "").strip().lower()
    if session_id is None:
        return 0

    ended_at_dt = _parse_dt(session_ended_at)
    if ended_at_dt is None:
        return 0

    try:
        with transaction() as conn:
            session_started_at = _load_session_started_at(conn, int(session_id))
            rows = _load_unresolved_tracking_rows_for_session(
                conn,
                session_id=int(session_id),
                target_id=target_id,
                login_lower=login_lower,
                session_started_at=session_started_at,
                session_ended_at=ended_at_dt,
            )
            if not rows:
                return 0

            for row in rows:
                tracking_id = _safe_int(_row_value(row, "id", 0), 0)
                confirmed_at_dt = _parse_dt(_row_value(row, "confirmed_at", 1))
                tracked_user_id = str(
                    _row_value(row, "to_broadcaster_id", 2, target_id) or target_id
                ).strip()
                was_deadlock_at_raid = bool(_safe_int(_row_value(row, "was_deadlock_at_raid", 3), 0))
                if not tracking_id or confirmed_at_dt is None:
                    continue

                resolution_dt = ended_at_dt
                resolution_reason = "session_ended"
                if was_deadlock_at_raid:
                    update_rows = conn.execute(
                        """
                        SELECT game_name, recorded_at
                        FROM twitch_channel_updates
                        WHERE twitch_user_id = %s
                          AND recorded_at >= %s
                          AND recorded_at <= %s
                        ORDER BY recorded_at ASC
                        """,
                        (
                            tracked_user_id or target_id,
                            _iso_utc(confirmed_at_dt),
                            _iso_utc(ended_at_dt),
                        ),
                    ).fetchall()
                    for update_row in update_rows:
                        game_name = str(_row_value(update_row, "game_name", 0) or "").strip().lower()
                        recorded_at_dt = _parse_dt(_row_value(update_row, "recorded_at", 1))
                        if not game_name or recorded_at_dt is None:
                            continue
                        if game_name != _target_game_lower():
                            resolution_dt = recorded_at_dt
                            resolution_reason = "channel_update_non_deadlock"
                            break
                else:
                    resolution_reason = "not_deadlock_at_raid"

                duration_sec = max(0, int((resolution_dt - confirmed_at_dt).total_seconds()))
                conn.execute(
                    """
                    UPDATE twitch_partner_raid_score_tracking
                    SET deadlock_continued_until = %s,
                        deadlock_continued_sec = %s,
                        resolved_at = %s,
                        resolution_reason = %s
                    WHERE id = %s
                    """,
                    (
                        _iso_utc(resolution_dt),
                        duration_sec,
                        _iso_utc(ended_at_dt),
                        resolution_reason,
                        tracking_id,
                    ),
                )
                resolved += 1

        if resolved:
            log.info(
                "Partner raid score tracking resolved for %s session=%s rows=%d",
                login_lower or target_id,
                session_id,
                resolved,
            )
    except Exception:
        log.debug(
            "Partner raid score tracking resolve failed for %s session=%s",
            login_lower or target_id,
            session_id,
            exc_info=True,
        )
        return 0

    return resolved


__all__ = [
    "resolve_partner_raid_tracking_for_session",
    "track_confirmed_partner_raid",
]
