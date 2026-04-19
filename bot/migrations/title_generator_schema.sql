-- bot/migrations/title_generator_schema.sql

CREATE TABLE IF NOT EXISTS title_generator_knowledge (
    id              SERIAL PRIMARY KEY,
    title           TEXT NOT NULL,
    keywords        TEXT[] DEFAULT '{}',
    game_context    TEXT NOT NULL DEFAULT 'deadlock',
    relative_perf   FLOAT NOT NULL,
    engagement_rate FLOAT NOT NULL,
    history_weight  FLOAT NOT NULL DEFAULT 1.0,
    normalized_score FLOAT NOT NULL,
    streamer_size   TEXT CHECK (streamer_size IN ('small','medium','large')),
    source_streamer TEXT,
    quality_tier    SMALLINT NOT NULL DEFAULT 1 CHECK (quality_tier IN (1,2,3)),
    added_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (title, game_context)
);
CREATE INDEX IF NOT EXISTS idx_tgk_score ON title_generator_knowledge (normalized_score DESC);
CREATE INDEX IF NOT EXISTS idx_tgk_keywords ON title_generator_knowledge USING GIN (keywords);

CREATE TABLE IF NOT EXISTS title_generator_insights (
    id              SERIAL PRIMARY KEY,
    streamer_id     TEXT NOT NULL,
    generated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    period_start    TIMESTAMPTZ NOT NULL,
    period_end      TIMESTAMPTZ NOT NULL,
    strengths       TEXT,
    weaknesses      TEXT,
    patterns        TEXT,
    recommendations TEXT,
    raw_response    JSONB
);
CREATE INDEX IF NOT EXISTS idx_tgi_streamer ON title_generator_insights (streamer_id, generated_at DESC);
