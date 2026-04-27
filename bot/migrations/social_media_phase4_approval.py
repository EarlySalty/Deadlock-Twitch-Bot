#!/usr/bin/env python3
"""
Social Media Phase 4 migration.

Enthaelt:
- Discord-Approval-Storage (`social_media_clip_approval`)
- Auto-Approve-Settings pro Plattform

Usage:
    python bot/migrations/social_media_phase4_approval.py
    python bot/migrations/social_media_phase4_approval.py --dsn "postgresql://..."
"""

from __future__ import annotations

import argparse
import os
import sys

from bot.social_media.storage import apply_phase4_approval


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Wendet die Social-Media-Phase-4-Migration an."
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
        print("Wende Social-Media-Phase-4-Migration an …")
        apply_phase4_approval(conn)
        conn.commit()
        print("Fertig. Social-Media-Phase-4-Migration angewendet.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
