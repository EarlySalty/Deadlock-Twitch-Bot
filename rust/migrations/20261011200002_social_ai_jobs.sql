CREATE TABLE public.social_ai_jobs (
    id BIGSERIAL PRIMARY KEY,
    twitch_user_id BIGINT NOT NULL CHECK (twitch_user_id > 0),
    clip_db_id BIGINT NOT NULL REFERENCES public.twitch_clips_social_media(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'queued' CHECK (status IN ('queued', 'preparing', 'transcribing', 'analyzing', 'planning', 'rendering', 'validating', 'review', 'failed')),
    stage TEXT NOT NULL DEFAULT 'preparing' CHECK (stage IN ('preparing', 'transcribing', 'analyzing', 'planning', 'rendering', 'validating')),
    attempts JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(attempts) = 'object'),
    checkpoints JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(checkpoints) = 'object' AND octet_length(checkpoints::text) <= 400000),
    highlights JSONB NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(highlights) = 'array'),
    decisions JSONB NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(decisions) = 'array'),
    uncertainty JSONB NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(uncertainty) = 'array'),
    review_reasons JSONB NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(review_reasons) = 'array'),
    provider TEXT,
    model TEXT,
    original_duration_ms BIGINT CHECK (original_duration_ms >= 0),
    output_duration_ms BIGINT CHECK (output_duration_ms >= 0),
    processing_ms BIGINT CHECK (processing_ms >= 0),
    rendering_ms BIGINT CHECK (rendering_ms >= 0),
    input_tokens BIGINT CHECK (input_tokens >= 0),
    output_tokens BIGINT CHECK (output_tokens >= 0),
    estimated_provider_cost_usd DOUBLE PRECISION CHECK (estimated_provider_cost_usd >= 0 AND estimated_provider_cost_usd < 'Infinity'::double precision),
    measured_provider_cost_usd DOUBLE PRECISION CHECK (measured_provider_cost_usd >= 0 AND measured_provider_cost_usd < 'Infinity'::double precision),
    produced_video_id BIGINT,
    failure_code TEXT,
    failure_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    CHECK (status <> 'review' OR (stage = 'validating' AND checkpoints ? 'validating' AND produced_video_id IS NOT NULL))
);

CREATE UNIQUE INDEX social_ai_jobs_active_clip_uq
    ON public.social_ai_jobs (twitch_user_id, clip_db_id)
    WHERE status IN ('queued', 'preparing', 'transcribing', 'analyzing', 'planning', 'rendering', 'validating');

CREATE INDEX social_ai_jobs_owner_recent_idx
    ON public.social_ai_jobs (twitch_user_id, id DESC);
