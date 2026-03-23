#!/usr/bin/env python3
"""
Einmaliges, idempotentes Backfill-Skript.

Liest bestehende twitch_stream_sessions und twitch_session_viewers und
befüllt daraus exp_sessions + exp_snapshots.

Idempotent: ON CONFLICT (stream_id) DO NOTHING verhindert Doppel-Einträge.
Sessions ohne stream_id werden übersprungen (können nicht eindeutig identifiziert werden).

Usage:
    python bot/migrations/exp_backfill.py
    python bot/migrations/exp_backfill.py --dsn "postgresql://..." --batch 500
"""

from __future__ import annotations

import argparse
import contextlib
import os
import sys


def _executemany(conn, sql: str, params_seq) -> None:
    executemany = getattr(conn, "executemany", None)
    if callable(executemany):
        executemany(sql, params_seq)
        return
    with contextlib.closing(conn.cursor()) as cur:
        cur.executemany(sql, params_seq)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Backfill exp_sessions + exp_snapshots aus twitch_stream_sessions."
    )
    parser.add_argument(
        "--dsn",
        default=os.environ.get("TWITCH_ANALYTICS_DSN"),
        help="Postgres DSN (Env: TWITCH_ANALYTICS_DSN)",
    )
    parser.add_argument(
        "--batch",
        type=int,
        default=200,
        help="Sessions pro Batch (default: 200)",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Nur lesen, nichts schreiben",
    )
    return parser.parse_args()


def _get_dsn(args: argparse.Namespace) -> str | None:
    if args.dsn:
        return args.dsn
    try:
        import keyring
        return keyring.get_password("DeadlockBot", "TWITCH_ANALYTICS_DSN")
    except Exception:
        return None


def _inspect_source_tables(conn) -> dict[str, list[str]]:
    """Bestehende Spalten lesen und ausgeben."""
    result = {}
    for table in ("twitch_stream_sessions", "twitch_session_viewers"):
        rows = conn.execute(
            """
            SELECT column_name
            FROM information_schema.columns
            WHERE table_schema = 'public' AND table_name = %s
            ORDER BY ordinal_position
            """,
            (table,),
        ).fetchall()
        cols = [r[0] for r in rows]
        result[table] = cols
        print(f"  {table}: {cols}")
    return result


def main() -> int:
    args = parse_args()
    dsn = _get_dsn(args)
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
        # 1) Quell-Schema inspizieren
        print("\nQuell-Tabellen-Spalten:")
        cols = _inspect_source_tables(conn)

        session_cols = cols.get("twitch_stream_sessions", [])
        required_session = {"id", "streamer_login", "stream_id", "started_at", "ended_at",
                            "game_name", "stream_title", "peak_viewers", "avg_viewers",
                            "samples", "follower_delta", "duration_seconds"}
        missing = required_session - set(session_cols)
        if missing:
            print(f"\nWARNING: Folgende Spalten fehlen in twitch_stream_sessions: {missing}")
            print("Das Backfill wird trotzdem durchgeführt mit COALESCE-Fallbacks.")

        # 2) Bestehende exp_sessions stream_ids laden (für Idempotenz)
        existing_ids: set[str] = set()
        try:
            rows = conn.execute("SELECT stream_id FROM exp_sessions WHERE stream_id IS NOT NULL").fetchall()
            existing_ids = {r[0] for r in rows}
            print(f"\nBereits in exp_sessions: {len(existing_ids)} Sessions mit stream_id")
        except Exception as exc:
            print(f"  Konnte exp_sessions nicht lesen: {exc}")
            print("  Bitte zuerst exp_tables_migrate.py ausführen!")
            return 1

        # 3) Quell-Sessions laden (nur mit stream_id, in Batches)
        offset = 0
        total_inserted = 0
        total_snapshots = 0
        batch_size = args.batch

        print(f"\nStarte Backfill (batch={batch_size}, dry_run={args.dry_run}) …")

        while True:
            source_rows = conn.execute(
                """
                SELECT
                    id,
                    streamer_login,
                    stream_id,
                    started_at,
                    ended_at,
                    COALESCE(game_name, '') AS game_name,
                    COALESCE(stream_title, '') AS stream_title,
                    COALESCE(peak_viewers, 0) AS peak_viewers,
                    COALESCE(avg_viewers, 0.0) AS avg_viewers,
                    COALESCE(samples, 0) AS samples,
                    follower_delta,
                    COALESCE(duration_seconds, 0) AS duration_seconds
                FROM twitch_stream_sessions
                WHERE stream_id IS NOT NULL
                  AND stream_id <> ''
                ORDER BY id
                LIMIT %s OFFSET %s
                """,
                (batch_size, offset),
            ).fetchall()

            if not source_rows:
                break

            offset += batch_size
            batch_inserted = 0

            for row in source_rows:
                (
                    src_id, streamer_login, stream_id, started_at, ended_at,
                    game_name, stream_title, peak_viewers, avg_viewers,
                    samples, follower_delta, duration_seconds,
                ) = row

                # Idempotenz: überspringen wenn bereits vorhanden
                if stream_id in existing_ids:
                    continue

                duration_min = float(duration_seconds) / 60.0 if duration_seconds else None

                if args.dry_run:
                    print(f"  [dry-run] würde einfügen: streamer={streamer_login} stream_id={stream_id}")
                    batch_inserted += 1
                    continue

                # exp_sessions einfügen
                result = conn.execute(
                    """
                    INSERT INTO exp_sessions (
                        streamer, stream_id, started_at, ended_at,
                        game_name, stream_title, peak_viewers, avg_viewers,
                        samples, follower_delta, duration_min
                    ) VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s)
                    ON CONFLICT (stream_id) WHERE stream_id IS NOT NULL DO NOTHING
                    RETURNING id
                    """,
                    (
                        streamer_login, stream_id,
                        str(started_at) if started_at else None,
                        str(ended_at) if ended_at else None,
                        game_name or None, stream_title or None,
                        int(peak_viewers or 0), float(avg_viewers or 0.0),
                        int(samples or 0), follower_delta, duration_min,
                    ),
                ).fetchone()

                if not result:
                    # ON CONFLICT DO NOTHING — schon vorhanden
                    existing_ids.add(stream_id)
                    continue

                exp_session_id = result[0]
                existing_ids.add(stream_id)
                batch_inserted += 1

                # exp_snapshots aus twitch_session_viewers befüllen
                viewer_rows = conn.execute(
                    """
                    SELECT ts_utc, viewer_count, minutes_from_start
                    FROM twitch_session_viewers
                    WHERE session_id = %s
                    ORDER BY ts_utc
                    """,
                    (src_id,),
                ).fetchall()

                if viewer_rows:
                    snapshot_data = [
                        (
                            exp_session_id,
                            str(vr[0]),
                            int(vr[1]) if vr[1] is not None else None,
                            float(vr[2]) if vr[2] is not None else None,
                        )
                        for vr in viewer_rows
                    ]
                    _executemany(
                        conn,
                        """
                        INSERT INTO exp_snapshots (exp_session_id, ts_utc, viewer_count, minutes_from_start)
                        VALUES (%s, %s, %s, %s)
                        ON CONFLICT DO NOTHING
                        """,
                        snapshot_data,
                    )
                    total_snapshots += len(snapshot_data)

            if not args.dry_run:
                conn.commit()

            total_inserted += batch_inserted
            print(f"  Batch offset={offset - batch_size}: {batch_inserted} Sessions eingefügt")

        print(f"\nFertig!")
        print(f"  exp_sessions eingefügt: {total_inserted}")
        print(f"  exp_snapshots eingefügt: {total_snapshots}")
        if args.dry_run:
            print("  [dry-run] – keine Änderungen geschrieben")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
