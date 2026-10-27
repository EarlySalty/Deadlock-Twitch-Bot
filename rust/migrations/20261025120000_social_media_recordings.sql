CREATE TABLE IF NOT EXISTS public.social_media_recording_consent (
    twitch_user_id TEXT PRIMARY KEY CHECK (twitch_user_id ~ '^[0-9]+$'),
    twitch_login TEXT NOT NULL,
    decision TEXT NOT NULL CHECK (decision IN ('granted', 'declined')),
    decided_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS public.social_media_recording_sessions (
    id UUID PRIMARY KEY,
    twitch_user_id TEXT NOT NULL CHECK (twitch_user_id ~ '^[0-9]+$'),
    twitch_login TEXT NOT NULL,
    started_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_chunk_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    user_agent TEXT NOT NULL DEFAULT '',
    viewport_width INTEGER,
    viewport_height INTEGER,
    total_bytes BIGINT NOT NULL DEFAULT 0,
    chunk_count INTEGER NOT NULL DEFAULT 0,
    deleted_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS social_media_recording_sessions_user_started_idx
    ON public.social_media_recording_sessions (twitch_user_id, started_at DESC);

CREATE INDEX IF NOT EXISTS social_media_recording_sessions_started_idx
    ON public.social_media_recording_sessions (started_at);

CREATE TABLE IF NOT EXISTS public.social_media_recording_chunks (
    session_id UUID NOT NULL REFERENCES public.social_media_recording_sessions (id) ON DELETE CASCADE,
    seq INTEGER NOT NULL CHECK (seq >= 0),
    payload JSONB NOT NULL,
    byte_size INTEGER NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (session_id, seq)
);

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.social_media_recording_consent TO twitchdash;
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.social_media_recording_sessions TO twitchdash;
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.social_media_recording_chunks TO twitchdash;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.social_media_recording_consent TO twitchbot;
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.social_media_recording_sessions TO twitchbot;
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.social_media_recording_chunks TO twitchbot;
    END IF;
END
$$;
