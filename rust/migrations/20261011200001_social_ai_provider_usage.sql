CREATE TABLE public.social_ai_user_consent (
    twitch_user_id TEXT PRIMARY KEY,
    external_ai_consent BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by TEXT
);

ALTER TABLE public.llm_usage
    ADD COLUMN social_ai_twitch_user_id TEXT,
    ADD COLUMN social_ai_job_id TEXT,
    ADD COLUMN social_ai_stage TEXT,
    ADD COLUMN reserved_micro_usd BIGINT,
    ADD COLUMN estimated_micro_usd BIGINT,
    ADD COLUMN cost_basis TEXT,
    ADD COLUMN cache_creation_input_tokens BIGINT,
    ADD COLUMN cache_read_input_tokens BIGINT;

ALTER TABLE public.llm_usage
    ADD CONSTRAINT llm_usage_social_ai_cost_check CHECK (
        (reserved_micro_usd IS NULL OR reserved_micro_usd >= 0)
        AND (estimated_micro_usd IS NULL OR estimated_micro_usd >= 0)
        AND (cost_basis IS NULL OR cost_basis IN ('reserved_upper_bound', 'usage_priced_estimate'))
    );

CREATE INDEX llm_usage_social_ai_budget_idx ON public.llm_usage (purpose, provider)
    WHERE purpose = 'social_editor' AND provider = 'anthropic';

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchanalysis') THEN
        GRANT SELECT, INSERT, UPDATE ON public.social_ai_user_consent TO twitchanalysis;
        GRANT SELECT, INSERT, UPDATE ON public.llm_usage TO twitchanalysis;
        GRANT USAGE, SELECT ON SEQUENCE public.llm_usage_id_seq TO twitchanalysis;
    END IF;
END $$;

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchaudit') THEN
        GRANT SELECT ON public.social_ai_user_consent TO twitchaudit;
        GRANT SELECT, INSERT, UPDATE ON public.llm_usage TO twitchaudit;
        GRANT USAGE, SELECT ON SEQUENCE public.llm_usage_id_seq TO twitchaudit;
    END IF;
END $$;
