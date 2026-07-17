CREATE TABLE twitch_crew_review_events (
    id BIGSERIAL PRIMARY KEY,
    review_session_id UUID NOT NULL,
    channel_login TEXT NOT NULL,
    subject_twitch_user_id TEXT NOT NULL,
    event_kind TEXT NOT NULL CHECK (event_kind IN (
        'session_started',
        'ricky_message',
        'streamer_transcript',
        'ai_decision',
        'ai_draft',
        'provider_error',
        'session_ended'
    )),
    source_message_id TEXT,
    occurred_at TIMESTAMPTZ NOT NULL,
    content TEXT,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb
        CHECK (jsonb_typeof(metadata) = 'object'),
    provider TEXT,
    model TEXT,
    confidence DOUBLE PRECISION
        CHECK (confidence IS NULL OR confidence BETWEEN 0.0 AND 1.0),
    discord_message_id TEXT,
    discord_deleted_at TIMESTAMPTZ,
    last_delete_error TEXT,
    tombstoned_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL DEFAULT (NOW() + INTERVAL '6 months')
);

CREATE UNIQUE INDEX twitch_crew_review_events_ricky_source_uidx
    ON twitch_crew_review_events (subject_twitch_user_id, source_message_id)
    WHERE event_kind = 'ricky_message'
      AND source_message_id IS NOT NULL
      AND btrim(source_message_id) <> '';

CREATE INDEX twitch_crew_review_events_session_occurred_idx
    ON twitch_crew_review_events (review_session_id, occurred_at);

CREATE INDEX twitch_crew_review_events_channel_occurred_idx
    ON twitch_crew_review_events (channel_login, occurred_at DESC);

CREATE INDEX twitch_crew_review_events_expires_idx
    ON twitch_crew_review_events (expires_at);
