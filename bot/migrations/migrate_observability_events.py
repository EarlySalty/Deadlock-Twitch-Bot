#!/usr/bin/env python3
"""
Migration für twitch_observability_events:
1. Fehlgeschlagene Analytics-Events löschen (terminal_decision + failed)
2. Primärschlüssel ändern auf (id, created_at) für TimescaleDB-Kompatibilität
3. Zur hypertable konvertieren
4. Komprimierung und Retention-Policy aktivieren

Usage:
    python bot/migrations/migrate_observability_events.py
    python bot/migrations/migrate_observability_events.py --dsn "postgresql://..."
"""
from __future__ import annotations

import argparse
import os
import sys
import time


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Migriert twitch_observability_events.")
    parser.add_argument(
        "--dsn",
        default=os.environ.get("TWITCH_ANALYTICS_DSN"),
        help="Postgres DSN (Env: TWITCH_ANALYTICS_DSN)",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Nur zählen, nicht löschen",
    )
    parser.add_argument(
        "--retention-days",
        type=int,
        default=7,
        help="Retention in Tagen (default: 7)",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if not args.dsn:
        print("Fehlender DSN: setze --dsn oder Env TWITCH_ANALYTICS_DSN", file=sys.stderr)
        return 1

    try:
        import psycopg
    except ImportError:
        print("psycopg nicht installiert: pip install psycopg[binary]", file=sys.stderr)
        return 1

    print("Verbinde mit Postgres …")
    conn = psycopg.connect(args.dsn)

    # 1. Zählen was gelöscht wird
    print("\n[1] Zähle fehlgeschlagene Events …")
    cur = conn.execute("""
        SELECT COUNT(*)
        FROM twitch_observability_events
        WHERE flow_type = 'analytics'
        AND step = 'terminal_decision'
        AND decision = 'failed'
    """)
    failed_count = cur.fetchone()[0]
    print(f"  Fehlgeschlagene Analytics-Events: {failed_count:,}")

    cur = conn.execute("SELECT COUNT(*) FROM twitch_observability_events")
    total = cur.fetchone()[0]
    print(f"  Gesamtevents: {total:,}")

    if args.dry_run:
        print("\n[DRY RUN] Keine Änderungen vorgenommen.")
        conn.close()
        return 0

    # 2. Löschen
    if failed_count > 0:
        print(f"\n[2] Lösche {failed_count:,} fehlgeschlagene Events …")
        start = time.time()
        cur = conn.execute("""
            DELETE FROM twitch_observability_events
            WHERE flow_type = 'analytics'
            AND step = 'terminal_decision'
            AND decision = 'failed'
        """)
        conn.commit()
        elapsed = time.time() - start
        print(f"  Gelöscht in {elapsed:.1f}s")

    # 3. Primärschlüssel ändern (TimescaleDB braucht partitioning key im PK)
    print("\n[3] Ändere Primärschlüssel auf (id, created_at) …")
    try:
        # Erst den alten PK droppen
        conn.execute("ALTER TABLE twitch_observability_events DROP CONSTRAINT IF EXISTS twitch_observability_events_pkey")
        # Dann neuen mit created_at
        conn.execute("ALTER TABLE twitch_observability_events ADD PRIMARY KEY (id, created_at)")
        conn.commit()
        print("  Primärschlüssel geändert.")
    except Exception as exc:
        print(f"  Warnung beim Ändern PK: {exc}")
        conn.rollback()

    # 4. Zur hypertable konvertieren
    print("\n[4] Konvertiere zu hypertable …")
    try:
        cur = conn.execute("""
            SELECT create_hypertable(
                'twitch_observability_events',
                'created_at',
                if_not_exists => TRUE,
                migrate_data => TRUE,
                chunk_time_interval => INTERVAL '7 days'
            )
        """)
        conn.commit()
        print("  Hypertable erstellt.")
    except Exception as exc:
        print(f"  Hypertable-Fehler: {exc}")
        conn.rollback()

    # 5. Komprimierung aktivieren
    print("\n[5] Aktiviere Komprimierung …")
    try:
        conn.execute("""
            ALTER TABLE twitch_observability_events SET (
                timescaledb.compress,
                timescaledb.compress_segmentby = 'flow_type,flow_id',
                timescaledb.compress_orderby = 'created_at DESC'
            )
        """)
        conn.commit()
        print("  Komprimierung aktiviert.")
    except Exception as exc:
        print(f"  Komprimierung-Fehler: {exc}")
        conn.rollback()

    # 6. Compression Policy
    print(f"\n[6] Setze Kompressions-Policy ({args.retention_days} Tage) …")
    try:
        conn.execute(f"""
            SELECT add_compression_policy(
                'twitch_observability_events',
                INTERVAL '{args.retention_days} days',
                if_not_exists => TRUE
            )
        """)
        conn.commit()
        print("  Kompressions-Policy gesetzt.")
    except Exception as exc:
        print(f"  Compression Policy-Fehler: {exc}")
        conn.rollback()

    # 7. Retention Policy
    print(f"\n[7] Setze Retention-Policy ({args.retention_days} Tage) …")
    try:
        conn.execute(f"""
            SELECT add_retention_policy(
                'twitch_observability_events',
                INTERVAL '{args.retention_days} days',
                if_not_exists => TRUE
            )
        """)
        conn.commit()
        print("  Retention-Policy gesetzt.")
    except Exception as exc:
        print(f"  Retention Policy-Fehler: {exc}")
        conn.rollback()

    # 8. Ergebnis
    print("\n[8] Ergebnis:")
    cur = conn.execute("SELECT pg_size_pretty(pg_total_relation_size('twitch_observability_events')) as size")
    print(f"  Neue Größe: {cur.fetchone()[0]}")
    cur = conn.execute("SELECT COUNT(*) FROM twitch_observability_events")
    print(f"  Verbleibende Events: {cur.fetchone()[0]:,}")

    try:
        cur = conn.execute("SELECT hypertable_name FROM timescaledb_information.hypertables WHERE hypertable_name = 'twitch_observability_events'")
        ht = cur.fetchone()
        print(f"  Hypertable: {'JA' if ht else 'NEIN'}")
    except Exception:
        pass

    conn.close()
    print("\nFertig.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())