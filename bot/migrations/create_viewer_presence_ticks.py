#!/usr/bin/env python3
"""
Migration: twitch_viewer_presence_ticks Tabelle anlegen.
Idempotent – verwendet CREATE TABLE IF NOT EXISTS.

Usage:
    python bot/migrations/create_viewer_presence_ticks.py
    python bot/migrations/create_viewer_presence_ticks.py --dsn "postgresql://..."
"""

from __future__ import annotations

import argparse
import os
import sys


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Legt twitch_viewer_presence_ticks Tabelle an.",
    )
    parser.add_argument(
        "--dsn",
        default=os.environ.get("TWITCH_ANALYTICS_DSN"),
        help="Postgres DSN (Env: TWITCH_ANALYTICS_DSN)",
    )
    return parser.parse_args()


DDL = """
CREATE TABLE IF NOT EXISTS twitch_viewer_presence_ticks (
    session_id     BIGINT       NOT NULL REFERENCES twitch_stream_sessions(id) ON DELETE CASCADE,
    streamer_login TEXT         NOT NULL,
    viewer_login   TEXT         NOT NULL,
    tick_at        TIMESTAMPTZ  NOT NULL,
    PRIMARY KEY (session_id, viewer_login, tick_at)
);
CREATE INDEX IF NOT EXISTS idx_viewer_presence_ticks_session
    ON twitch_viewer_presence_ticks(session_id, viewer_login, tick_at);
"""


def main() -> int:
    args = parse_args()
    if not args.dsn:
        try:
            import keyring
            dsn = keyring.get_password("DeadlockBot", "TWITCH_ANALYTICS_DSN")
            if not dsn:
                print(
                    "Fehlender DSN: setze --dsn oder Env TWITCH_ANALYTICS_DSN",
                    file=sys.stderr,
                )
                return 1
            args.dsn = dsn
        except Exception:
            print(
                "Fehlender DSN: setze --dsn oder Env TWITCH_ANALYTICS_DSN",
                file=sys.stderr,
            )
            return 1

    try:
        import psycopg
    except ImportError:
        print("psycopg nicht installiert: pip install psycopg[binary]", file=sys.stderr)
        return 1

    print("Verbinde mit Postgres …")
    with psycopg.connect(args.dsn) as conn:
        print("Führe DDL aus …")
        conn.execute(DDL)
        conn.commit()
        print("Fertig. Tabelle twitch_viewer_presence_ticks erstellt (falls noch nicht vorhanden).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())