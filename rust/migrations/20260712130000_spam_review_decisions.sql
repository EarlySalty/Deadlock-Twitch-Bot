CREATE TABLE IF NOT EXISTS public.twitch_spam_review_decisions (
    id              BIGSERIAL PRIMARY KEY,
    channel_login   TEXT NOT NULL,
    chatter_login   TEXT NOT NULL,
    chatter_id      TEXT,
    source_message  TEXT NOT NULL,
    verdict         TEXT NOT NULL,
    confidence      REAL,
    reason          TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT twitch_spam_review_decisions_verdict_check CHECK (
        verdict IN (
            'spam', 'clean', 'unsure', 'skipped', 'timeout', 'provider_error', 'parse_error'
        )
    )
);
