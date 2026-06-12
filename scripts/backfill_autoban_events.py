#!/usr/bin/env python3
"""Backfill: trägt historische Auto-Bans aus der Review-Logdatei in die
Tabelle ``twitch_ban_events`` nach, damit die öffentliche ``recent-bans``-
Statistik die echte Spam-/Viewer-Bot-Moderation rückwirkend widerspiegelt.

Quelle: ``logs/twitch_autobans.log`` (tab-getrennt:
``ts \\t [STATUS] \\t channel_login \\t chatter_login \\t chatter_id \\t reason \\t content``).
Nur ``[BANNED]``-Zeilen werden übernommen; als Feed-Grund dient der Spam-Inhalt
(``content``), wie beim Live-Pfad in ``bot/chat/moderation.py``.

Idempotent: ein Eintrag wird über ``(target_id, received_at)`` dedupliziert,
mehrfaches Ausführen erzeugt keine Duplikate. Muss mit DB-Env (Infisical)
laufen, z. B. ``scripts/run_with_infisical.sh .venv/bin/python scripts/backfill_autoban_events.py``.
"""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from bot.storage import readonly_connection, transaction  # noqa: E402
from bot.storage.pg import prepare_runtime_storage  # noqa: E402

LOG_PATH = ROOT / "logs" / "twitch_autobans.log"


def _load_login_to_user_id() -> dict[str, str]:
    mapping: dict[str, str] = {}
    with readonly_connection() as conn:
        rows = conn.execute(
            "SELECT LOWER(twitch_login) AS login, twitch_user_id "
            "FROM twitch_streamers WHERE twitch_login IS NOT NULL"
        ).fetchall()
    for row in rows:
        login = (row[0] if not hasattr(row, "keys") else row["login"]) or ""
        uid = (row[1] if not hasattr(row, "keys") else row["twitch_user_id"]) or ""
        if login and uid:
            mapping[str(login).strip().lower()] = str(uid).strip()
    return mapping


def main() -> int:
    if not LOG_PATH.exists():
        print(f"Logdatei fehlt: {LOG_PATH}", file=sys.stderr)
        return 1

    prepare_runtime_storage()
    login_to_uid = _load_login_to_user_id()
    with readonly_connection() as conn:
        existing_total = conn.execute(
            "SELECT COUNT(*) FROM twitch_ban_events WHERE event_type = 'ban'"
        ).fetchone()[0]
    print(f"Streamer-Mapping: {len(login_to_uid)} Logins | bereits {existing_total} Ban-Events in DB")

    inserted = 0
    skipped_parse = 0
    candidates: list[tuple] = []

    for raw in LOG_PATH.read_text(encoding="utf-8", errors="replace").splitlines():
        parts = raw.split("\t")
        if len(parts) < 7 or "BANNED" not in parts[1]:
            skipped_parse += 1
            continue
        ts = parts[0].strip()
        channel_login = parts[2].strip().lower()
        chatter_login = parts[3].strip().lower()
        chatter_id = parts[4].strip()
        content = parts[6].strip()
        if not ts or not chatter_id:
            skipped_parse += 1
            continue
        # twitch_user_id (Kanal-Owner) auflösen; Fallback = Kanal-Login (TEXT-Spalte).
        twitch_user_id = login_to_uid.get(channel_login, channel_login)
        candidates.append((twitch_user_id, chatter_login or None, chatter_id, content[:300] or None, ts))

    # Dedup DB-seitig per timestamptz-Vergleich (received_at ist in Prod TIMESTAMPTZ;
    # ein String-Vergleich von ISO-Text scheitert am T-vs-Leerzeichen-Format).
    with transaction() as conn:
        for twitch_user_id, chatter_login, chatter_id, reason, ts in candidates:
            cur = conn.execute(
                """
                INSERT INTO twitch_ban_events
                    (twitch_user_id, event_type, target_login, target_id, reason, received_at)
                SELECT %s, 'ban', %s, %s, %s, %s::timestamptz
                WHERE NOT EXISTS (
                    SELECT 1 FROM twitch_ban_events
                    WHERE event_type = 'ban'
                      AND target_id IS NOT DISTINCT FROM %s
                      AND received_at = %s::timestamptz
                )
                """,
                (twitch_user_id, chatter_login, chatter_id, reason, ts, chatter_id, ts),
            )
            inserted += cur.rowcount if cur.rowcount and cur.rowcount > 0 else 0

    print(
        f"Backfill fertig: {inserted} neu eingefügt, "
        f"{len(candidates) - inserted} bereits vorhanden (übersprungen), "
        f"{skipped_parse} Nicht-BANNED/Parse-Skips."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
