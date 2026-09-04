CREATE TABLE IF NOT EXISTS public.twitch_dashboard_assistent_log (
    id BIGSERIAL PRIMARY KEY,
    twitch_user_id TEXT NOT NULL,
    page TEXT,
    language TEXT NOT NULL DEFAULT 'de',
    question TEXT NOT NULL,
    answer TEXT NOT NULL,
    grounded BOOLEAN NOT NULL DEFAULT FALSE,
    flagged_injection BOOLEAN NOT NULL DEFAULT FALSE,
    provider TEXT,
    model TEXT,
    latency_ms BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS twitch_dashboard_assistent_log_user_time_idx
    ON public.twitch_dashboard_assistent_log (twitch_user_id, created_at DESC);
