from __future__ import annotations

import logging
from typing import Any

log = logging.getLogger("TwitchStreams.SocialMediaStorage")


SOCIAL_MEDIA_SEQUENCE_TARGETS: tuple[tuple[str, str], ...] = (
    ("twitch_clips_social_media", "id"),
    ("twitch_clips_social_analytics", "id"),
    ("twitch_clips_upload_queue", "id"),
    ("clip_templates_global", "id"),
    ("clip_templates_streamer", "id"),
    ("clip_fetch_history", "id"),
    ("social_media_platform_auth", "id"),
    ("social_media_reports", "id"),
)


def apply_phase0_stabilization(conn: Any) -> None:
    """Apply the Phase 0 stabilization DDL for social-media tables."""
    _ensure_oauth_state_tokens_hardening(conn)
    _ensure_reauth_notifications_table(conn)
    repair_social_media_sequences(conn)


def apply_phase1_layout_and_uploads(conn: Any) -> None:
    """Apply the Phase 1 DDL for layout storage, uploads and retention."""
    _ensure_streamer_layout_table(conn)
    _ensure_clip_layout_and_retention_columns(conn)
    repair_social_media_sequences(conn)


def apply_phase2_enrichment(conn: Any) -> None:
    """Apply the Phase 2 DDL for vocabulary and clip enrichment."""
    _ensure_deadlock_vocab_table(conn)
    _ensure_clip_enrichment_table(conn)
    _ensure_social_media_settings_table(conn)
    repair_social_media_sequences(conn)


def apply_phase4_approval(conn: Any) -> None:
    """Apply the Phase 4 DDL for approval workflow and auto-approve settings."""
    _ensure_social_media_settings_table(conn)
    _ensure_clip_approval_table(conn)
    _ensure_auto_approve_settings(conn)
    repair_social_media_sequences(conn)


def apply_phase3_analytics(conn: Any) -> None:
    """Apply the Phase 3 DDL for analytics tracking and report storage."""
    _ensure_social_media_settings_table(conn)
    _ensure_phase3_social_analytics_columns(conn)
    _ensure_social_media_reports_table(conn)
    repair_social_media_sequences(conn)


def repair_social_media_sequences(conn: Any) -> None:
    """Repair known social-media SERIAL/IDENTITY sequences idempotently."""
    for table, column in SOCIAL_MEDIA_SEQUENCE_TARGETS:
        _repair_sequence(conn, table, column)


def _ensure_oauth_state_tokens_hardening(conn: Any) -> None:
    conn.execute(
        "ALTER TABLE oauth_state_tokens ADD COLUMN IF NOT EXISTS consumed_at TIMESTAMPTZ"
    )
    conn.execute(
        """
        ALTER TABLE oauth_state_tokens
        ALTER COLUMN created_at TYPE TIMESTAMPTZ
        USING CASE
            WHEN created_at IS NULL OR BTRIM(created_at::text) = '' THEN NULL
            ELSE created_at::timestamptz
        END
        """
    )
    conn.execute(
        """
        ALTER TABLE oauth_state_tokens
        ALTER COLUMN expires_at TYPE TIMESTAMPTZ
        USING CASE
            WHEN expires_at IS NULL OR BTRIM(expires_at::text) = '' THEN NULL
            ELSE expires_at::timestamptz
        END
        """
    )
    conn.execute(
        "ALTER TABLE oauth_state_tokens ALTER COLUMN created_at SET DEFAULT CURRENT_TIMESTAMP"
    )
    conn.execute("ALTER TABLE oauth_state_tokens ALTER COLUMN expires_at SET NOT NULL")
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_oauth_state_platform_expires "
        "ON oauth_state_tokens(platform, expires_at)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_oauth_state_consumed_at "
        "ON oauth_state_tokens(consumed_at)"
    )


def _ensure_reauth_notifications_table(conn: Any) -> None:
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS social_media_reauth_notifications (
            streamer_login TEXT NOT NULL,
            platform       TEXT NOT NULL,
            error_kind     TEXT NOT NULL,
            last_sent_at   TIMESTAMPTZ NOT NULL,
            PRIMARY KEY (streamer_login, platform, error_kind)
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_social_media_reauth_notifications_last_sent "
        "ON social_media_reauth_notifications(last_sent_at DESC)"
    )


def _ensure_streamer_layout_table(conn: Any) -> None:
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS social_media_streamer_layout (
            streamer_login TEXT PRIMARY KEY REFERENCES twitch_streamers(twitch_login) ON DELETE CASCADE,
            layout_json    JSONB NOT NULL,
            cam_enabled    BOOLEAN NOT NULL DEFAULT TRUE,
            mode           TEXT NOT NULL DEFAULT 'pip',
            updated_at     TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_by     TEXT,
            CONSTRAINT social_media_layout_mode_chk CHECK (mode IN ('pip', 'stacked'))
        )
        """
    )


def _ensure_clip_layout_and_retention_columns(conn: Any) -> None:
    conn.execute(
        """
        ALTER TABLE twitch_clips_social_media
          ADD COLUMN IF NOT EXISTS layout_override_json JSONB,
          ADD COLUMN IF NOT EXISTS source_kind TEXT NOT NULL DEFAULT 'twitch',
          ADD COLUMN IF NOT EXISTS upload_local_path TEXT,
          ADD COLUMN IF NOT EXISTS retention_until TIMESTAMPTZ,
          ADD COLUMN IF NOT EXISTS discarded_at TIMESTAMPTZ
        """
    )
    conn.execute(
        "ALTER TABLE twitch_clips_social_media DROP CONSTRAINT IF EXISTS twitch_clips_source_kind_chk"
    )
    conn.execute(
        """
        ALTER TABLE twitch_clips_social_media
          ADD CONSTRAINT twitch_clips_source_kind_chk
          CHECK (source_kind IN ('twitch', 'manual_upload'))
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_clips_social_media_retention "
        "ON twitch_clips_social_media(retention_until)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_clips_social_media_discarded_at "
        "ON twitch_clips_social_media(discarded_at)"
    )
    conn.execute(
        """
        CREATE OR REPLACE FUNCTION social_media_set_retention_until()
        RETURNS trigger
        LANGUAGE plpgsql
        AS $$
        BEGIN
            IF NEW.created_at IS NULL OR BTRIM(NEW.created_at::text) = '' THEN
                RETURN NEW;
            END IF;
            NEW.retention_until := (NEW.created_at::timestamptz + INTERVAL '14 days');
            RETURN NEW;
        END;
        $$;
        """
    )
    conn.execute("DROP TRIGGER IF EXISTS social_media_retention_until_tg ON twitch_clips_social_media")
    conn.execute(
        """
        CREATE TRIGGER social_media_retention_until_tg
        BEFORE INSERT OR UPDATE OF created_at
        ON twitch_clips_social_media
        FOR EACH ROW
        EXECUTE FUNCTION social_media_set_retention_until()
        """
    )
    conn.execute(
        """
        UPDATE twitch_clips_social_media
           SET retention_until = created_at::timestamptz + INTERVAL '14 days'
         WHERE created_at IS NOT NULL
           AND BTRIM(created_at::text) <> ''
           AND retention_until IS NULL
        """
    )


def _ensure_deadlock_vocab_table(conn: Any) -> None:
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS deadlock_vocab (
            term       TEXT PRIMARY KEY,
            canonical  TEXT NOT NULL,
            category   TEXT NOT NULL,
            source     TEXT NOT NULL DEFAULT 'manual',
            aliases    JSONB NOT NULL DEFAULT '[]'::JSONB,
            weight     INTEGER NOT NULL DEFAULT 1,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
            CONSTRAINT deadlock_vocab_category_chk
                CHECK (category IN ('hero', 'item', 'ability', 'slang')),
            CONSTRAINT deadlock_vocab_source_chk
                CHECK (source IN ('deadlock_api', 'manual'))
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_deadlock_vocab_category "
        "ON deadlock_vocab(category)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_deadlock_vocab_canonical "
        "ON deadlock_vocab(canonical)"
    )


def _ensure_social_media_settings_table(conn: Any) -> None:
    """Key/value-Settings-Tabelle (z.B. fuer externe-LLM-Consent)."""
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS social_media_settings (
            key        TEXT PRIMARY KEY,
            value      JSONB NOT NULL,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_by TEXT
        )
        """
    )


def _ensure_phase3_social_analytics_columns(conn: Any) -> None:
    table = "twitch_clips_social_analytics"
    _ensure_column(conn, table, "bucket", "TEXT")
    _ensure_column(conn, table, "watch_time_seconds", "INTEGER")
    _ensure_column(conn, table, "ctr_percent", "NUMERIC(5,2)")
    _ensure_column(conn, table, "provider", "TEXT")
    _ensure_column(conn, table, "next_pull_at", "TIMESTAMPTZ")
    if _has_column(conn, table, "engagement_rate"):
        _coerce_phase3_numeric_column(conn, table, "engagement_rate")
    else:
        _ensure_column(conn, table, "engagement_rate", "NUMERIC(5,2)")
    conn.execute(
        """
        UPDATE twitch_clips_social_analytics
           SET bucket = '30d'
         WHERE bucket IS NULL
            OR BTRIM(bucket) = ''
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_clips_social_analytics_bucket "
        "ON twitch_clips_social_analytics(clip_id, platform, bucket)"
    )


def _ensure_social_media_reports_table(conn: Any) -> None:
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS social_media_reports (
            id             SERIAL PRIMARY KEY,
            kind           TEXT NOT NULL,
            streamer_login TEXT,
            period_start   TIMESTAMPTZ NOT NULL,
            period_end     TIMESTAMPTZ NOT NULL,
            content_md     TEXT NOT NULL,
            model          TEXT,
            created_at     TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_social_media_reports_kind_period "
        "ON social_media_reports(kind, period_end DESC)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_social_media_reports_streamer_period "
        "ON social_media_reports(streamer_login, period_end DESC)"
    )


def _ensure_clip_enrichment_table(conn: Any) -> None:
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS social_media_clip_enrichment (
            clip_db_id            INTEGER PRIMARY KEY
                REFERENCES twitch_clips_social_media(id) ON DELETE CASCADE,
            transcript_raw        TEXT,
            transcript_corrected  TEXT,
            transcript_segments   JSONB,
            transcript_lang       TEXT,
            detected_terms        JSONB NOT NULL DEFAULT '[]'::JSONB,
            title_youtube         TEXT,
            title_tiktok          TEXT,
            title_instagram       TEXT,
            description_youtube   TEXT,
            description_tiktok    TEXT,
            description_instagram TEXT,
            hashtags_youtube      JSONB NOT NULL DEFAULT '[]'::JSONB,
            hashtags_tiktok       JSONB NOT NULL DEFAULT '[]'::JSONB,
            hashtags_instagram    JSONB NOT NULL DEFAULT '[]'::JSONB,
            llm_provider          TEXT,
            llm_model             TEXT,
            cost_usd_estimate     NUMERIC(10, 6),
            status                TEXT NOT NULL DEFAULT 'pending',
            error_message         TEXT,
            started_at            TIMESTAMPTZ,
            completed_at          TIMESTAMPTZ,
            edited_by             TEXT,
            updated_at            TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
            CONSTRAINT social_media_clip_enrichment_status_chk
                CHECK (status IN (
                    'pending', 'transcribing', 'correcting', 'llm',
                    'done', 'failed', 'skipped_no_key'
                ))
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_social_media_clip_enrichment_status "
        "ON social_media_clip_enrichment(status)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_social_media_clip_enrichment_updated_at "
        "ON social_media_clip_enrichment(updated_at DESC)"
    )


def _ensure_clip_approval_table(conn: Any) -> None:
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS social_media_clip_approval (
            clip_db_id          INTEGER PRIMARY KEY
                REFERENCES twitch_clips_social_media(id) ON DELETE CASCADE,
            state               TEXT NOT NULL DEFAULT 'awaiting_approval',
            approved_platforms  JSONB NOT NULL DEFAULT '[]'::JSONB,
            approver_user_id    TEXT,
            decided_at          TIMESTAMPTZ,
            dm_message_id       TEXT,
            dm_channel_id       TEXT,
            last_sent_at        TIMESTAMPTZ,
            CONSTRAINT social_media_clip_approval_state_chk
                CHECK (state IN ('awaiting_approval', 'approved', 'skipped', 'editing'))
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_social_media_clip_approval_state "
        "ON social_media_clip_approval(state)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_social_media_clip_approval_last_sent_at "
        "ON social_media_clip_approval(last_sent_at DESC)"
    )


def _ensure_auto_approve_settings(conn: Any) -> None:
    for key in (
        "auto_approve_youtube",
        "auto_approve_tiktok",
        "auto_approve_instagram",
    ):
        conn.execute(
            """
            INSERT INTO social_media_settings (key, value, updated_at, updated_by)
            VALUES (%s, 'false', CURRENT_TIMESTAMP, 'phase4_migration')
            ON CONFLICT (key) DO NOTHING
            """,
            (key,),
        )


def _repair_sequence(conn: Any, table: str, column: str) -> None:
    try:
        row = conn.execute("SELECT to_regclass(%s)", (table,)).fetchone()
    except Exception:
        return
    regclass_value = row["to_regclass"] if row and hasattr(row, "keys") else (row[0] if row else None)
    if regclass_value is None:
        return

    try:
        sequence_row = conn.execute(
            "SELECT pg_get_serial_sequence(%s, %s)",
            (table, column),
        ).fetchone()
    except Exception:
        return
    sequence_name = (
        sequence_row["pg_get_serial_sequence"]
        if sequence_row and hasattr(sequence_row, "keys")
        else (sequence_row[0] if sequence_row else None)
    )
    if not sequence_name:
        return

    conn.execute(
        f"""
        SELECT setval(
            pg_get_serial_sequence(%s, %s),
            GREATEST(COALESCE((SELECT MAX({column}) FROM {table}), 0) + 1, 1),
            false
        )
        """,
        (table, column),
    )


def _has_column(conn: Any, table: str, column: str) -> bool:
    # information_schema-Lookup zuerst (Postgres und sqlite-DSN-Adapter, der
    # diese View ebenfalls liefert). Den sqlite-PRAGMA-Pfad behalten wir nur
    # als Fallback fuer reine sqlite-Verbindungen, isoliert per SAVEPOINT,
    # damit ein Syntax-Fehler die Postgres-Transaktion nicht abortet.
    try:
        result = conn.execute(
            """
            SELECT 1
              FROM information_schema.columns
             WHERE table_name = %s
               AND column_name = %s
             LIMIT 1
            """,
            (table, column),
        ).fetchone()
        if result:
            return True
        return False
    except Exception:
        # Transaktion koennte nun aborted sein -> SAVEPOINT-Try fuer sqlite
        pass

    try:
        savepoint_used = False
        try:
            conn.execute("SAVEPOINT _has_column_pragma")
            savepoint_used = True
        except Exception:
            savepoint_used = False
        try:
            rows = conn.execute(f"PRAGMA table_info({table})").fetchall()
        except Exception:
            rows = []
        if savepoint_used:
            try:
                conn.execute("RELEASE SAVEPOINT _has_column_pragma")
            except Exception:
                try:
                    conn.execute("ROLLBACK TO SAVEPOINT _has_column_pragma")
                except Exception:
                    pass
        for row in rows:
            name = row["name"] if hasattr(row, "keys") else row[1]
            if str(name).strip().lower() == column.lower():
                return True
    except Exception:
        return False
    return False


def _ensure_column(conn: Any, table: str, column: str, ddl: str) -> None:
    if _has_column(conn, table, column):
        return
    conn.execute(f"ALTER TABLE {table} ADD COLUMN {column} {ddl}")


def _coerce_phase3_numeric_column(conn: Any, table: str, column: str) -> None:
    # Bestehende Spalten koennen bereits ein numerischer Typ
    # (NUMERIC/DOUBLE PRECISION/REAL/INTEGER) sein. In dem Fall ist nichts zu tun;
    # ein erzwungener TYPE-Cast wuerde u. a. ueber die Skala hinaus floaten und
    # die laufende Transaktion abortieren. Wir pruefen den aktuellen Datentyp
    # und ueberspringen den Cast, wenn die Spalte numerisch ist.
    try:
        result = conn.execute(
            """
            SELECT data_type
              FROM information_schema.columns
             WHERE table_name = %s
               AND column_name = %s
             LIMIT 1
            """,
            (table, column),
        ).fetchone()
    except Exception:
        return
    if result is None:
        return
    data_type = (
        result["data_type"] if hasattr(result, "keys") else result[0]
    )
    numeric_types = {
        "numeric",
        "double precision",
        "real",
        "integer",
        "bigint",
        "smallint",
    }
    if str(data_type or "").strip().lower() in numeric_types:
        return
    try:
        savepoint_used = False
        try:
            conn.execute("SAVEPOINT _phase3_coerce")
            savepoint_used = True
        except Exception:
            savepoint_used = False
        try:
            conn.execute(
                f"""
                ALTER TABLE {table}
                ALTER COLUMN {column} TYPE NUMERIC(5,2)
                USING CASE
                    WHEN {column} IS NULL OR BTRIM({column}::text) = '' THEN NULL
                    ELSE ROUND(({column})::numeric, 2)
                END
                """
            )
            if savepoint_used:
                conn.execute("RELEASE SAVEPOINT _phase3_coerce")
        except Exception:
            if savepoint_used:
                try:
                    conn.execute("ROLLBACK TO SAVEPOINT _phase3_coerce")
                except Exception:
                    pass
                try:
                    conn.execute("RELEASE SAVEPOINT _phase3_coerce")
                except Exception:
                    pass
            return
    except Exception:
        return
