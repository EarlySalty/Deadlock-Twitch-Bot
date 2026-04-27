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


def _repair_sequence(conn: Any, table: str, column: str) -> None:
    row = conn.execute("SELECT to_regclass(%s)", (table,)).fetchone()
    regclass_value = row["to_regclass"] if row and hasattr(row, "keys") else (row[0] if row else None)
    if regclass_value is None:
        return

    sequence_row = conn.execute("SELECT pg_get_serial_sequence(%s, %s)", (table, column)).fetchone()
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
