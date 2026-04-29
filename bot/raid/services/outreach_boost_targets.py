"""Loader und CAS-Marker für Outreach-Boost-Raid-Ziele.

Channels, die der Discovery-Outreach in `bot.community.partner_recruit` kürzlich
angeschrieben hat, sollen pro Empfänger genau einmal als Auto-Raid-Ziel
priorisiert werden ("Lock-Angebot"). Sobald der Boost-Raid erfolgreich
ausgelöst wurde, wird `raid_used_at` gesetzt und der Channel fällt zurück in
die normale Selection-Logik.
"""

from __future__ import annotations

import logging
from collections.abc import Callable
from typing import Any, ContextManager

from bot.storage import readonly_connection, transaction

log = logging.getLogger(__name__)

ReadonlyConnectionFactory = Callable[[], ContextManager[Any]]
TransactionFactory = Callable[[], ContextManager[Any]]


def load_outreach_boost_logins(
    *,
    lookback_hours: int = 48,
    connection_factory: ReadonlyConnectionFactory | None = None,
) -> dict[str, dict[str, Any]]:
    """Liefert {login_lower: {streamer_user_id, contacted_at}} aller frischen,
    noch nicht boost-geraidten Outreach-Empfänger."""
    if lookback_hours <= 0:
        return {}

    factory = connection_factory or readonly_connection
    try:
        with factory() as conn:
            rows = conn.execute(
                """
                SELECT streamer_login, streamer_user_id, contacted_at
                FROM twitch_partner_outreach
                WHERE status = 'sent'
                  AND raid_used_at IS NULL
                  AND contacted_at IS NOT NULL
                  AND contacted_at::timestamptz >= NOW() - (%s || ' hours')::interval
                """,
                (str(int(lookback_hours)),),
            ).fetchall()
    except Exception:
        log.debug("OutreachBoost: Loader-Query fehlgeschlagen", exc_info=True)
        return {}

    out: dict[str, dict[str, Any]] = {}
    for row in rows:
        if hasattr(row, "keys"):
            login = str(row["streamer_login"] or "").strip().lower()
            user_id = str(row["streamer_user_id"] or "").strip()
            contacted_at = row["contacted_at"]
        else:
            login = str(row[0] or "").strip().lower()
            user_id = str(row[1] or "").strip()
            contacted_at = row[2]
        if not login:
            continue
        out[login] = {
            "streamer_user_id": user_id or None,
            "contacted_at": contacted_at,
        }
    return out


def mark_outreach_boost_used(
    login: str,
    *,
    transaction_factory: TransactionFactory | None = None,
) -> bool:
    """Markiert den Outreach-Boost als verbraucht.

    Returnt True, wenn ein Row tatsächlich aktualisiert wurde (CAS auf
    `raid_used_at IS NULL`). Damit wird sichergestellt, dass jeder
    Empfänger höchstens einen Boost-Raid bekommt — auch bei parallelen
    Pipelines.
    """
    normalized = str(login or "").strip().lower()
    if not normalized:
        return False

    factory = transaction_factory or transaction
    try:
        with factory() as conn:
            cursor = conn.execute(
                """
                UPDATE twitch_partner_outreach
                   SET raid_used_at = NOW()
                 WHERE streamer_login = %s
                   AND raid_used_at IS NULL
                """,
                (normalized,),
            )
            updated = getattr(cursor, "rowcount", 0) or 0
            conn.commit()
            return int(updated) > 0
    except Exception:
        log.debug("OutreachBoost: Markierung fehlgeschlagen für %s", normalized, exc_info=True)
        return False


__all__ = [
    "load_outreach_boost_logins",
    "mark_outreach_boost_used",
]
