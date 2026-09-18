-- Smalltalk is not outreach: identity, API evidence and cooldowns live here.
-- A failed refresh explicitly replaces old evidence with NULL/false.
CREATE TABLE twitch_smalltalk_candidate_state (
    twitch_user_id TEXT PRIMARY KEY CHECK (twitch_user_id ~ '^[0-9]+$'),
    channel_login TEXT NOT NULL CHECK (channel_login ~ '^[a-z0-9_]+$'),
    follower_count INTEGER CHECK (follower_count >= 0),
    checked_at TIMESTAMPTZ,
    source TEXT CHECK (source = 'helix'),
    live_deadlock BOOLEAN NOT NULL DEFAULT FALSE,
    live_checked_at TIMESTAMPTZ,
    next_refresh_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    cooldown_until TIMESTAMPTZ,
    last_error TEXT
);
ALTER TABLE twitch_smalltalk_sessions
    ADD COLUMN live_test BOOLEAN NOT NULL DEFAULT FALSE;
CREATE INDEX twitch_smalltalk_sessions_identity_cooldown
    ON twitch_smalltalk_sessions (streamer_user_id, ended_at DESC);
