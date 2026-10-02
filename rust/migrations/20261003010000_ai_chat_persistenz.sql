-- Analyse-Folgechats bleiben höchstens 24 Stunden ab dem ursprünglichen Beginn nutzbar.
CREATE TABLE twitch_ai_chat_sessions (
    twitch_user_id TEXT NOT NULL CHECK (twitch_user_id ~ '^[0-9]+$'),
    analysis_id BIGINT NOT NULL REFERENCES ai_analyses(id) ON DELETE CASCADE,
    session_json JSONB NOT NULL,
    history JSONB NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(history) = 'array'),
    follow_up_count BIGINT NOT NULL DEFAULT 0 CHECK (follow_up_count >= 0),
    created_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (twitch_user_id, analysis_id),
    CHECK (expires_at = created_at + INTERVAL '24 hours')
);
CREATE INDEX twitch_ai_chat_sessions_expiry ON twitch_ai_chat_sessions(expires_at);
CREATE INDEX twitch_ai_chat_sessions_analysis ON twitch_ai_chat_sessions(analysis_id);
CREATE TABLE twitch_ai_chat_hourly (
    twitch_user_id TEXT PRIMARY KEY CHECK (twitch_user_id ~ '^[0-9]+$'),
    window_start TIMESTAMPTZ NOT NULL,
    consumed BIGINT NOT NULL DEFAULT 0 CHECK (consumed BETWEEN 0 AND 10)
);
CREATE TABLE twitch_ai_chat_reservations (
    operation_id UUID PRIMARY KEY,
    twitch_user_id TEXT NOT NULL,
    analysis_id BIGINT NOT NULL,
    model_kind TEXT NOT NULL CHECK (model_kind IN ('llm', 'opus')),
    state TEXT NOT NULL CHECK (state IN ('running', 'unknown', 'done', 'failed')),
    backend_pid INTEGER NOT NULL,
    lock_key BIGINT NOT NULL,
    capacity_expires_at TIMESTAMPTZ NOT NULL,
    FOREIGN KEY (twitch_user_id, analysis_id)
        REFERENCES twitch_ai_chat_sessions(twitch_user_id, analysis_id) ON DELETE CASCADE
);
CREATE INDEX twitch_ai_chat_reservations_capacity
    ON twitch_ai_chat_reservations(twitch_user_id, state, capacity_expires_at);
CREATE INDEX twitch_ai_chat_reservations_session ON twitch_ai_chat_reservations(twitch_user_id, analysis_id);
CREATE INDEX twitch_ai_chat_reservations_expiry ON twitch_ai_chat_reservations(capacity_expires_at) WHERE state = 'unknown';
CREATE INDEX twitch_ai_chat_reservations_running ON twitch_ai_chat_reservations(backend_pid) WHERE state = 'running';
CREATE INDEX twitch_ai_chat_hourly_expiry ON twitch_ai_chat_hourly(window_start);
DO $$
DECLARE
    role_name TEXT;
BEGIN
    REVOKE ALL ON twitch_ai_chat_sessions, twitch_ai_chat_hourly, twitch_ai_chat_reservations FROM PUBLIC;
    FOREACH role_name IN ARRAY ARRAY['twitchbot', 'twitchdash', 'twitchlegacy', 'twitchcontest', 'twitchcollector'] LOOP
        IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = role_name) THEN
            EXECUTE format('REVOKE ALL ON twitch_ai_chat_sessions, twitch_ai_chat_hourly, twitch_ai_chat_reservations FROM %I', role_name);
        END IF;
    END LOOP;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchanalysis') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON twitch_ai_chat_sessions,
            twitch_ai_chat_hourly, twitch_ai_chat_reservations TO twitchanalysis;
    END IF;
END $$;
