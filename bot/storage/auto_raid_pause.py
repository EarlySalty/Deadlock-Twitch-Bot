"""Storage helpers for the admin-set auto-raid pause.

Eine Pause unterdrueckt ausschliesslich den automatischen Raid beim
Offline-Gehen (stream.offline). Das manuelle `!raid` bleibt unberuehrt.

Eine Pause gilt als *aktiv*, solange eine Zeile existiert und
``paused_until > now()``. Abgelaufene Zeilen werden bei der Abfrage
ignoriert; ein eigener Cleanup-Job ist nicht noetig.
"""

from __future__ import annotations

from typing import Any

# Obergrenze fuer eine einzelne Pause (24h), damit ein Tippfehler den
# Auto-Raid nicht unbegrenzt lahmlegt.
MAX_PAUSE_MINUTES = 24 * 60


def _clamp_minutes(minutes: Any) -> int:
    try:
        value = int(minutes)
    except (TypeError, ValueError):
        value = 60
    return max(1, min(value, MAX_PAUSE_MINUTES))


def set_auto_raid_pause(
    conn: Any,
    *,
    twitch_user_id: str,
    twitch_login: str | None,
    minutes: Any,
    reason: str | None = None,
    set_by: str | None = None,
) -> dict[str, Any]:
    """Setzt/erneuert die Auto-Raid-Pause und gibt die neue ``paused_until`` zurueck."""
    user_id = str(twitch_user_id or "").strip()
    if not user_id:
        raise ValueError("twitch_user_id required")
    login = str(twitch_login or "").strip().lower() or None
    mins = _clamp_minutes(minutes)
    row = conn.execute(
        """
        INSERT INTO twitch_auto_raid_pause
            (twitch_user_id, twitch_login, paused_until, reason, set_by,
             created_at, updated_at)
        VALUES
            (%s, %s, CURRENT_TIMESTAMP + make_interval(mins => %s), %s, %s,
             CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
        ON CONFLICT (twitch_user_id) DO UPDATE SET
            twitch_login = EXCLUDED.twitch_login,
            paused_until = EXCLUDED.paused_until,
            reason       = EXCLUDED.reason,
            set_by       = EXCLUDED.set_by,
            updated_at   = CURRENT_TIMESTAMP
        RETURNING twitch_user_id, twitch_login, paused_until, reason, set_by,
                  EXTRACT(EPOCH FROM (paused_until - CURRENT_TIMESTAMP)) AS remaining_seconds
        """,
        (user_id, login, mins, (reason or None), (set_by or None)),
    ).fetchone()
    return dict(row) if row is not None else {}


def clear_auto_raid_pause(conn: Any, *, twitch_user_id: str) -> bool:
    """Hebt die Pause auf. Gibt ``True`` zurueck, wenn eine Zeile entfernt wurde."""
    user_id = str(twitch_user_id or "").strip()
    if not user_id:
        return False
    row = conn.execute(
        "DELETE FROM twitch_auto_raid_pause WHERE twitch_user_id = %s RETURNING twitch_user_id",
        (user_id,),
    ).fetchone()
    return row is not None


def get_auto_raid_pause(conn: Any, *, twitch_user_id: str) -> dict[str, Any] | None:
    """Gibt die *aktive* Pause (paused_until > now) inkl. Restzeit zurueck, sonst ``None``."""
    user_id = str(twitch_user_id or "").strip()
    if not user_id:
        return None
    row = conn.execute(
        """
        SELECT twitch_user_id, twitch_login, paused_until, reason, set_by,
               created_at, updated_at,
               EXTRACT(EPOCH FROM (paused_until - CURRENT_TIMESTAMP)) AS remaining_seconds
        FROM twitch_auto_raid_pause
        WHERE twitch_user_id = %s AND paused_until > CURRENT_TIMESTAMP
        """,
        (user_id,),
    ).fetchone()
    return dict(row) if row is not None else None


def is_auto_raid_paused(conn: Any, *, twitch_user_id: str) -> bool:
    """``True``, wenn der Auto-Raid fuer diesen Kanal gerade aktiv pausiert ist."""
    user_id = str(twitch_user_id or "").strip()
    if not user_id:
        return False
    row = conn.execute(
        """
        SELECT 1
        FROM twitch_auto_raid_pause
        WHERE twitch_user_id = %s AND paused_until > CURRENT_TIMESTAMP
        """,
        (user_id,),
    ).fetchone()
    return row is not None


__all__ = [
    "MAX_PAUSE_MINUTES",
    "set_auto_raid_pause",
    "clear_auto_raid_pause",
    "get_auto_raid_pause",
    "is_auto_raid_paused",
]
