ALTER TABLE twitch_engagement_settings
    DROP CONSTRAINT IF EXISTS twitch_engagement_settings_output_mode_chk;

ALTER TABLE twitch_engagement_settings
    ADD CONSTRAINT twitch_engagement_settings_output_mode_chk
    CHECK (output_mode IN ('off', 'shadow', 'live', 'test'));

CREATE TABLE twitch_smalltalk_sessions (
    id UUID PRIMARY KEY,
    channel_login TEXT NOT NULL,
    streamer_user_id TEXT NOT NULL,
    started_at TIMESTAMPTZ NOT NULL,
    ended_at TIMESTAMPTZ,
    end_reason TEXT,
    viewer_count INTEGER,
    settings_existed BOOLEAN NOT NULL,
    previous_enabled BOOLEAN NOT NULL,
    previous_irc_read BOOLEAN NOT NULL,
    previous_output_mode TEXT NOT NULL,
    provider_error_count INTEGER NOT NULL DEFAULT 0
        CHECK (provider_error_count >= 0),
    last_provider_error TEXT,
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
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL DEFAULT (NOW() + INTERVAL '6 months'),
    CHECK ((ended_at IS NULL) = (end_reason IS NULL)),
    CHECK ((discord_claim_id IS NULL) = (discord_claim_until IS NULL))
);

CREATE UNIQUE INDEX twitch_smalltalk_one_open_session_idx
    ON twitch_smalltalk_sessions ((TRUE))
    WHERE ended_at IS NULL;

CREATE INDEX twitch_smalltalk_channel_started_idx
    ON twitch_smalltalk_sessions (channel_login, started_at DESC);

CREATE INDEX twitch_smalltalk_discord_pending_idx
    ON twitch_smalltalk_sessions (discord_next_attempt_at, started_at)
    WHERE ended_at IS NOT NULL
      AND discord_message_id IS NULL
      AND discord_attempts < 3;

CREATE INDEX twitch_smalltalk_expires_idx
    ON twitch_smalltalk_sessions (expires_at, id);

CREATE TABLE twitch_smalltalk_messages (
    id BIGSERIAL PRIMARY KEY,
    session_id UUID NOT NULL
        REFERENCES twitch_smalltalk_sessions(id) ON DELETE CASCADE,
    channel_login TEXT NOT NULL,
    generated_at TIMESTAMPTZ NOT NULL,
    generated_text TEXT NOT NULL,
    trigger_text TEXT NOT NULL,
    triggered_by_msg_id TEXT,
    outcome TEXT NOT NULL CHECK (outcome IN ('would_send', 'rejected')),
    reject_reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (
        (outcome = 'would_send' AND reject_reason IS NULL)
        OR (outcome = 'rejected' AND reject_reason IS NOT NULL)
    )
);

CREATE UNIQUE INDEX twitch_smalltalk_message_source_once_idx
    ON twitch_smalltalk_messages (session_id, triggered_by_msg_id)
    WHERE triggered_by_msg_id IS NOT NULL;

CREATE INDEX twitch_smalltalk_messages_session_time_idx
    ON twitch_smalltalk_messages (session_id, generated_at, id);
