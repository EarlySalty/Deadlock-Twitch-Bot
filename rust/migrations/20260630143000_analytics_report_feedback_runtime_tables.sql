-- Analytics/report feedback runtime-created tables now owned by migrations:
-- ai_analyses, twitch_stream_report_ratings, twitch_stream_report_ab_votes.

CREATE TABLE IF NOT EXISTS public.ai_analyses (
    id BIGSERIAL NOT NULL,
    streamer TEXT NOT NULL,
    days INTEGER NOT NULL,
    model TEXT NOT NULL,
    generated_at TIMESTAMPTZ DEFAULT now() NOT NULL,
    data_snapshot JSONB NOT NULL,
    points JSONB NOT NULL,
    CONSTRAINT ai_analyses_pkey PRIMARY KEY (id)
);

CREATE INDEX IF NOT EXISTS idx_ai_analyses_streamer_ts
    ON public.ai_analyses USING btree (streamer, generated_at DESC);

CREATE TABLE IF NOT EXISTS public.twitch_stream_report_ratings (
    id BIGSERIAL NOT NULL,
    session_id BIGINT NOT NULL,
    streamer_login TEXT NOT NULL,
    report_variant TEXT DEFAULT 'compact'::text NOT NULL,
    rating TEXT NOT NULL,
    comment TEXT,
    rated_by TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT now(),
    updated_at TIMESTAMPTZ DEFAULT now(),
    CONSTRAINT twitch_stream_report_ratings_rating_check
        CHECK ((rating = ANY (ARRAY['gut'::text, 'schlecht'::text, 'neutral'::text]))),
    CONSTRAINT twitch_stream_report_ratings_pkey PRIMARY KEY (id),
    CONSTRAINT twitch_stream_report_ratings_session_id_report_variant_rate_key
        UNIQUE (session_id, report_variant, rated_by)
);

CREATE TABLE IF NOT EXISTS public.twitch_stream_report_ab_votes (
    id BIGSERIAL NOT NULL,
    session_id BIGINT NOT NULL,
    streamer_login TEXT NOT NULL,
    winner TEXT NOT NULL,
    comment TEXT,
    voted_by TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT now(),
    updated_at TIMESTAMPTZ DEFAULT now(),
    CONSTRAINT twitch_stream_report_ab_votes_winner_check
        CHECK ((winner = ANY (ARRAY['compact'::text, 'full'::text, 'gleich'::text]))),
    CONSTRAINT twitch_stream_report_ab_votes_pkey PRIMARY KEY (id),
    CONSTRAINT twitch_stream_report_ab_votes_session_id_voted_by_key UNIQUE (session_id, voted_by)
);

CREATE INDEX IF NOT EXISTS idx_ab_votes_session
    ON public.twitch_stream_report_ab_votes USING btree (session_id);

CREATE INDEX IF NOT EXISTS idx_ab_votes_streamer
    ON public.twitch_stream_report_ab_votes USING btree (streamer_login);
