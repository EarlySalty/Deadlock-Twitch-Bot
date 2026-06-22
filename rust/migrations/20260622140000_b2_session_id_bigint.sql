-- B2: Korrektur der fehlerhaften 20260622120000-Migration.
-- active_session_id und twitch_session_chatters.session_id muessen BIGINT sein,
-- weil Rust sie als i64 liest und sie fachlich auf twitch_stream_sessions.id zeigen.
--
-- Guarded/idempotent: Live wurde bereits manuell repariert; dort ist das ein No-op.

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
          FROM information_schema.columns
         WHERE table_schema = 'public'
           AND table_name = 'twitch_live_state'
           AND column_name = 'active_session_id'
           AND data_type = 'integer'
    ) THEN
        ALTER TABLE public.twitch_live_state
            ALTER COLUMN active_session_id DROP DEFAULT,
            ALTER COLUMN active_session_id TYPE bigint USING active_session_id::bigint;
    END IF;

    IF EXISTS (
        SELECT 1
          FROM information_schema.columns
         WHERE table_schema = 'public'
           AND table_name = 'twitch_session_chatters'
           AND column_name = 'session_id'
           AND data_type = 'integer'
    ) THEN
        ALTER TABLE public.twitch_session_chatters
            ALTER COLUMN session_id DROP DEFAULT,
            ALTER COLUMN session_id TYPE bigint USING session_id::bigint;
    END IF;
END $$;
