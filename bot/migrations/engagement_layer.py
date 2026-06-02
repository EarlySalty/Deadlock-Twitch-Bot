#!/usr/bin/env python3
"""
Engagement-Layer Migration für den Twitch-Bot.

Legt die Tabellen für den AI-Engagement-Layer (MiniMax-M3-Stammgast) an
und seedet EarlySalty als super_mod, falls bereits in twitch_streamers
registriert.

Tabellen:
- twitch_engagement_settings
- twitch_engagement_conversation
- twitch_user_profile
- twitch_user_threads
- twitch_user_engagement_optout
- twitch_channel_match_state
- twitch_engagement_log
- twitch_admin_roles

Idempotent: Re-Runs sind sicher (CREATE TABLE IF NOT EXISTS, ON CONFLICT).

Usage:
    python bot/migrations/engagement_layer.py
    python bot/migrations/engagement_layer.py --dsn "postgresql://..."
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Wendet die Engagement-Layer-Migration für den Twitch-Bot an."
    )
    parser.add_argument(
        "--dsn",
        default=os.environ.get("TWITCH_ANALYTICS_DSN"),
        help="Postgres DSN (Env: TWITCH_ANALYTICS_DSN)",
    )
    return parser.parse_args()


def _resolve_dsn(explicit_dsn: str | None) -> str | None:
    if explicit_dsn:
        return explicit_dsn
    try:
        import keyring

        return keyring.get_password("DeadlockBot", "TWITCH_ANALYTICS_DSN")
    except Exception:
        return None


def main() -> int:
    args = parse_args()
    dsn = _resolve_dsn(args.dsn)
    if not dsn:
        print("Fehlender DSN: setze --dsn oder Env TWITCH_ANALYTICS_DSN", file=sys.stderr)
        return 1

    sql_path = Path(__file__).with_suffix(".sql")
    if not sql_path.exists():
        print(f"SQL-File nicht gefunden: {sql_path}", file=sys.stderr)
        return 1

    try:
        import psycopg
    except ImportError:
        print("psycopg nicht installiert: pip install psycopg[binary]", file=sys.stderr)
        return 1

    sql = sql_path.read_text(encoding="utf-8")

    print("Verbinde mit Postgres …")
    with psycopg.connect(dsn) as conn:
        print(f"Wende Engagement-Layer-Migration an aus {sql_path.name} …")
        with conn.cursor() as cur:
            cur.execute(sql)
        conn.commit()

        with conn.cursor() as cur:
            cur.execute(
                """
                SELECT table_name
                FROM information_schema.tables
                WHERE table_schema = 'public'
                  AND table_name IN (
                      'twitch_engagement_settings',
                      'twitch_engagement_conversation',
                      'twitch_user_profile',
                      'twitch_user_threads',
                      'twitch_user_engagement_optout',
                      'twitch_channel_match_state',
                      'twitch_engagement_log',
                      'twitch_admin_roles'
                  )
                ORDER BY table_name;
                """
            )
            created = [row[0] for row in cur.fetchall()]
            print(f"Verifiziert: {len(created)}/8 Tabellen vorhanden.")
            for name in created:
                print(f"  - {name}")

            cur.execute(
                """
                SELECT twitch_user_id
                FROM twitch_admin_roles
                WHERE role = 'super_mod';
                """
            )
            super_mods = [row[0] for row in cur.fetchall()]
            if super_mods:
                print(f"super_mod IDs: {super_mods}")
            else:
                print("Hinweis: kein super_mod geseedet (EarlySalty noch nicht in twitch_streamers?).")

    print("Fertig.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
