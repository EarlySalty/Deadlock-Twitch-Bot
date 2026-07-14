CREATE TABLE IF NOT EXISTS twitch_scout_pitch_ledger (
    id BIGSERIAL PRIMARY KEY,
    streamer_login TEXT NOT NULL,
    trigger_type TEXT NOT NULL,
    judge_input_excerpt TEXT,
    judge_verdict TEXT NOT NULL,
    confidence REAL,
    action TEXT NOT NULL,
    detail TEXT,
    discord_message_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_twitch_scout_pitch_ledger_stream_trigger_created
    ON twitch_scout_pitch_ledger (LOWER(streamer_login), trigger_type, created_at DESC);

CREATE TABLE IF NOT EXISTS twitch_scout_pitch_blacklist (
    streamer_login TEXT PRIMARY KEY,
    reason TEXT,
    created_at TIMESTAMPTZ DEFAULT NOW()
);
