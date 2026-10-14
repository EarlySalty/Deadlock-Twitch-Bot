ALTER TABLE public.social_ai_processing_control
    ADD COLUMN monthly_cap_micro_usd BIGINT CHECK (monthly_cap_micro_usd BETWEEN 0 AND 2000000);

ALTER TABLE public.social_ai_user_limits
    ADD COLUMN monthly_ai_jobs BIGINT CHECK (monthly_ai_jobs BETWEEN 0 AND 9007199254740991),
    ADD COLUMN monthly_revisions BIGINT CHECK (monthly_revisions BETWEEN 0 AND 9007199254740991);
