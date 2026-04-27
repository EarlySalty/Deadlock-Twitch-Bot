#!/usr/bin/env python3
"""
Social Media Phase 1 migration.

Enthaelt:
- Streamer-Layout-Storage
- Clip-Layout-Overrides
- Upload-/Retention-Spalten inkl. 14-Tage-Trigger

Usage:
    python bot/migrations/social_media_phase1_layout_and_uploads.py
    python bot/migrations/social_media_phase1_layout_and_uploads.py --dsn "postgresql://..."
"""

from __future__ import annotations

import argparse
import os
import sys

from bot.social_media.storage import apply_phase1_layout_and_uploads


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Wendet die Social-Media-Phase-1-Migration an."
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
        print("Wende Social-Media-Phase-1-Migration an …")
        apply_phase1_layout_and_uploads(conn)
        conn.commit()
        print("Fertig. Social-Media-Phase-1-Migration angewendet.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
