CREATE TABLE IF NOT EXISTS public.social_ai_observer_comparisons (
    id BIGSERIAL PRIMARY KEY,
    clip_db_id BIGINT NOT NULL CHECK (clip_db_id > 0),
    twitch_user_id BIGINT NOT NULL CHECK (twitch_user_id > 0),
    requested_by TEXT NOT NULL CHECK (length(requested_by) BETWEEN 1 AND 128),
    status TEXT NOT NULL DEFAULT 'queued' CHECK (status IN ('queued', 'expanded', 'failed')),
    failure_code TEXT CHECK (length(failure_code) <= 64),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS social_ai_observer_comparisons_status
    ON public.social_ai_observer_comparisons (status, id);

CREATE TABLE IF NOT EXISTS public.social_ai_observer_trials (
    id BIGSERIAL PRIMARY KEY,
    comparison_id BIGINT NOT NULL REFERENCES public.social_ai_observer_comparisons (id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position >= 0),
    observer TEXT NOT NULL CHECK (length(observer) BETWEEN 1 AND 128),
    model_item TEXT NOT NULL CHECK (length(model_item) BETWEEN 1 AND 80),
    thinking_item TEXT NOT NULL CHECK (length(thinking_item) BETWEEN 1 AND 80),
    active_label TEXT NOT NULL CHECK (length(active_label) BETWEEN 1 AND 80),
    prompt_version TEXT NOT NULL CHECK (length(prompt_version) BETWEEN 1 AND 64),
    status TEXT NOT NULL DEFAULT 'queued' CHECK (status IN ('queued', 'running', 'answered', 'failed')),
    failure_code TEXT CHECK (length(failure_code) <= 64),
    answer TEXT CHECK (octet_length(answer) <= 65536),
    source_sha256 TEXT CHECK (length(source_sha256) = 64),
    elapsed_ms BIGINT CHECK (elapsed_ms >= 0),
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ,
    rating SMALLINT CHECK (rating BETWEEN 1 AND 5),
    rating_note TEXT CHECK (length(rating_note) <= 1000),
    rated_by TEXT CHECK (length(rated_by) <= 128),
    rated_at TIMESTAMPTZ,
    UNIQUE (comparison_id, observer)
);

CREATE INDEX IF NOT EXISTS social_ai_observer_trials_queue
    ON public.social_ai_observer_trials (status, comparison_id, position);

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT, UPDATE ON public.social_ai_observer_comparisons TO twitchbot;
        GRANT SELECT, INSERT, UPDATE ON public.social_ai_observer_trials TO twitchbot;
        GRANT USAGE ON SEQUENCE public.social_ai_observer_trials_id_seq TO twitchbot;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT, INSERT ON public.social_ai_observer_comparisons TO twitchdash;
        GRANT USAGE ON SEQUENCE public.social_ai_observer_comparisons_id_seq TO twitchdash;
        GRANT SELECT, UPDATE ON public.social_ai_observer_trials TO twitchdash;
    END IF;
END
$$;
