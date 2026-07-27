CREATE TABLE twitch_outreach_shadow_sessions (
    id UUID PRIMARY KEY,
    channel_login TEXT NOT NULL,
    streamer_user_id TEXT NOT NULL,
    started_at TIMESTAMPTZ NOT NULL,
    ended_at TIMESTAMPTZ,
    end_reason TEXT,
    stage TEXT NOT NULL DEFAULT 'watch'
        CHECK (stage IN ('watch', 'smalltalk', 'qualify', 'offer')),
    current_cycle_id UUID,
    processor_claim_id UUID,
    processor_claim_until TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK ((ended_at IS NULL) = (end_reason IS NULL)),
    CHECK ((processor_claim_id IS NULL) = (processor_claim_until IS NULL))
);

CREATE UNIQUE INDEX twitch_outreach_shadow_one_open_session_idx
    ON twitch_outreach_shadow_sessions ((TRUE))
    WHERE ended_at IS NULL;

CREATE INDEX twitch_outreach_shadow_channel_started_idx
    ON twitch_outreach_shadow_sessions (channel_login, started_at DESC);

CREATE TABLE twitch_outreach_shadow_events (
    id BIGSERIAL PRIMARY KEY,
    session_id UUID NOT NULL
        REFERENCES twitch_outreach_shadow_sessions(id) ON DELETE CASCADE,
    cycle_id UUID NOT NULL UNIQUE,
    channel_login TEXT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN (
        'hook',
        'silent',
        'parser_error',
        'timeout',
        'provider_error',
        'whisper_error'
    )),
    stage TEXT NOT NULL
        CHECK (stage IN ('watch', 'smalltalk', 'qualify', 'offer')),
    transcript TEXT,
    decision JSONB,
    static_recruitment_text TEXT,
    error_class TEXT,
    provider TEXT,
    model TEXT,
    discord_claim_id UUID,
    discord_claim_until TIMESTAMPTZ,
    discord_attempts INTEGER NOT NULL DEFAULT 0 CHECK (discord_attempts >= 0),
    discord_next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    discord_message_id TEXT,
    discord_last_error TEXT,
    discord_delete_attempts INTEGER NOT NULL DEFAULT 0
        CHECK (discord_delete_attempts >= 0),
    discord_delete_next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    discord_last_delete_error TEXT,
    content_tombstoned_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL DEFAULT (NOW() + INTERVAL '6 months'),
    CHECK (decision IS NULL OR jsonb_typeof(decision) = 'object'),
    CHECK ((discord_claim_id IS NULL) = (discord_claim_until IS NULL))
);

CREATE INDEX twitch_outreach_shadow_events_discord_pending_idx
    ON twitch_outreach_shadow_events (discord_next_attempt_at, id)
    WHERE discord_message_id IS NULL
      AND discord_attempts < 3;

CREATE INDEX twitch_outreach_shadow_events_session_time_idx
    ON twitch_outreach_shadow_events (session_id, occurred_at);

CREATE INDEX twitch_outreach_shadow_events_expires_idx
    ON twitch_outreach_shadow_events (expires_at, id);
