#!/usr/bin/env python3
"""
Phase-0-Stabilisierung fuer das Social-Media-Modul.

Enthaelt:
- Sequence-Repair fuer drift-anfaellige Social-Media-Tabellen
- OAuth-State-Haertung (`consumed_at`, TIMESTAMPTZ)
- Reauth-Notification-Tabelle fuer Refresh-Fehler

Usage:
    python bot/migrations/social_media_phase0_stabilization.py
    python bot/migrations/social_media_phase0_stabilization.py --dsn "postgresql://..."
"""

from __future__ import annotations

import argparse
import os
import sys

from bot.social_media.storage import apply_phase0_stabilization


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Wendet die Phase-0-Stabilisierung fuer Social-Media-Tabellen an."
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

    try:
        import psycopg
    except ImportError:
        print("psycopg nicht installiert: pip install psycopg[binary]", file=sys.stderr)
        return 1

    print("Verbinde mit Postgres …")
    with psycopg.connect(dsn) as conn:
        print("Wende Social-Media-Phase-0-Stabilisierung an …")
        apply_phase0_stabilization(conn)
        conn.commit()
        print("Fertig. Social-Media-Phase-0-Stabilisierung angewendet.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
