#!/usr/bin/env python3
"""
Einmaliges Schema-Migration-Skript: Fügt die 3 Experimental-Tabellen
(exp_sessions, exp_snapshots, exp_game_transitions) zur bestehenden
PostgreSQL/TimescaleDB-Datenbank hinzu.

Idempotent: verwendet CREATE TABLE IF NOT EXISTS / CREATE INDEX IF NOT EXISTS.

Usage:
    python bot/migrations/exp_tables_migrate.py
    python bot/migrations/exp_tables_migrate.py --dsn "postgresql://..."
"""

from __future__ import annotations

import argparse
import os
import sys


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Fügt exp_sessions, exp_snapshots, exp_game_transitions zur DB hinzu."
    )
    parser.add_argument(
        "--dsn",
        default=os.environ.get("TWITCH_ANALYTICS_DSN"),
        help="Postgres DSN (Env: TWITCH_ANALYTICS_DSN)",
    )
    return parser.parse_args()


DDL = """
CREATE TABLE IF NOT EXISTS exp_sessions (
  id              BIGSERIAL PRIMARY KEY,
  streamer        TEXT    NOT NULL,
  stream_id       TEXT,
  started_at      TEXT    NOT NULL,
  ended_at        TEXT,
  game_name       TEXT,
  stream_title    TEXT,
  peak_viewers    INTEGER DEFAULT 0,
  avg_viewers     REAL    DEFAULT 0,
  samples         INTEGER DEFAULT 0,
  follower_delta  INTEGER,
  duration_min    REAL
);
CREATE INDEX IF NOT EXISTS idx_exp_sessions_streamer ON exp_sessions(streamer, started_at);
CREATE UNIQUE INDEX IF NOT EXISTS idx_exp_sessions_stream_id ON exp_sessions(stream_id) WHERE stream_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS exp_snapshots (
  id                 BIGSERIAL PRIMARY KEY,
  exp_session_id     BIGINT  NOT NULL,
  ts_utc             TEXT    NOT NULL,
  viewer_count       INTEGER,
  minutes_from_start REAL
);
CREATE INDEX IF NOT EXISTS idx_exp_snapshots_session ON exp_snapshots(exp_session_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_exp_snapshots_session_ts
  ON exp_snapshots(exp_session_id, ts_utc);

CREATE TABLE IF NOT EXISTS exp_game_transitions (
  id              BIGSERIAL PRIMARY KEY,
  exp_session_id  BIGINT  NOT NULL,
  streamer        TEXT    NOT NULL,
  ts_utc          TEXT    NOT NULL,
  from_game       TEXT,
  to_game         TEXT,
  viewer_count    INTEGER
);
CREATE INDEX IF NOT EXISTS idx_exp_transitions_streamer ON exp_game_transitions(streamer, ts_utc);
"""


def main() -> int:
    args = parse_args()
    if not args.dsn:
        # Fallback: try Windows Credential Manager via keyring
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
        print("Fertig. Tabellen exp_sessions, exp_snapshots, exp_game_transitions erstellt (falls noch nicht vorhanden).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
