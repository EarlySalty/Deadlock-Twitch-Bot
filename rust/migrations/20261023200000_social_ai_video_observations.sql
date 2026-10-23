CREATE TABLE IF NOT EXISTS public.social_ai_video_observations (
    id BIGSERIAL PRIMARY KEY,
    twitch_user_id BIGINT NOT NULL CHECK (twitch_user_id > 0),
    job_namespace TEXT NOT NULL CHECK (job_namespace ~ '^(manual|production):[0-9]+$'),
    twitch_clip_id TEXT NOT NULL CHECK (length(twitch_clip_id) BETWEEN 1 AND 256),
    source_sha256 TEXT NOT NULL CHECK (length(source_sha256) = 64),
    source_bytes BIGINT NOT NULL CHECK (source_bytes > 0),
    observer TEXT NOT NULL CHECK (length(observer) BETWEEN 1 AND 128),
    prompt_version TEXT NOT NULL CHECK (length(prompt_version) BETWEEN 1 AND 64),
    outcome TEXT NOT NULL CHECK (outcome IN ('answered', 'failed')),
    failure_code TEXT CHECK (length(failure_code) <= 64),
    answer TEXT CHECK (octet_length(answer) <= 65536),
    elapsed_ms BIGINT NOT NULL CHECK (elapsed_ms >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT social_ai_video_observations_outcome CHECK (
        (outcome = 'answered' AND answer IS NOT NULL AND failure_code IS NULL)
        OR (outcome = 'failed' AND answer IS NULL AND failure_code IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS social_ai_video_observations_clip
    ON public.social_ai_video_observations (twitch_clip_id, created_at DESC);
CREATE INDEX IF NOT EXISTS social_ai_video_observations_source
    ON public.social_ai_video_observations (source_sha256);
CREATE INDEX IF NOT EXISTS social_ai_video_observations_job
    ON public.social_ai_video_observations (job_namespace);

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT, INSERT ON public.social_ai_video_observations TO twitchbot;
        GRANT USAGE ON SEQUENCE public.social_ai_video_observations_id_seq TO twitchbot;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT, INSERT ON public.social_ai_video_observations TO twitchdash;
        GRANT USAGE ON SEQUENCE public.social_ai_video_observations_id_seq TO twitchdash;
    END IF;
END
$$;
