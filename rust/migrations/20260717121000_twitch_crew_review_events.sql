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
    model_claim_id UUID,
    model_claim_until TIMESTAMPTZ,
    discord_claim_id UUID,
    discord_claim_until TIMESTAMPTZ,
    discord_channel_id BIGINT,
    discord_message_id TEXT,
    discord_deleted_at TIMESTAMPTZ,
    last_delete_error TEXT,
    tombstoned_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL DEFAULT (NOW() + INTERVAL '6 months'),
    CHECK ((model_claim_id IS NULL) = (model_claim_until IS NULL)),
    CHECK (model_claim_until IS NULL OR model_claim_until < expires_at),
    CHECK ((discord_claim_id IS NULL) = (discord_claim_until IS NULL)),
    CHECK (discord_claim_until IS NULL OR discord_claim_until < expires_at),
    CHECK ((discord_channel_id IS NULL) = (discord_message_id IS NULL)),
    CHECK (discord_channel_id IS NULL OR discord_channel_id > 0)
);

CREATE UNIQUE INDEX twitch_crew_review_events_ricky_source_uidx
    ON twitch_crew_review_events (subject_twitch_user_id, source_message_id)
    WHERE event_kind = 'ricky_message'
      AND source_message_id IS NOT NULL
      AND btrim(source_message_id) <> '';

CREATE INDEX twitch_crew_review_events_session_occurred_idx
    ON twitch_crew_review_events (review_session_id, occurred_at);

CREATE INDEX twitch_crew_review_events_terminal_cycle_idx
    ON twitch_crew_review_events (review_session_id, (metadata->>'cycle_id'))
    WHERE event_kind IN ('ai_decision', 'provider_error')
      AND metadata ? 'cycle_id'
      AND jsonb_typeof(metadata->'cycle_id') = 'string'
      AND NULLIF(btrim(metadata->>'cycle_id'), '') IS NOT NULL;

CREATE INDEX twitch_crew_review_events_channel_occurred_idx
    ON twitch_crew_review_events (channel_login, occurred_at DESC);

CREATE INDEX twitch_crew_review_events_expires_idx
    ON twitch_crew_review_events (expires_at);
