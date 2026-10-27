ALTER TABLE public.llm_usage DROP CONSTRAINT llm_usage_social_ai_cost_check;

ALTER TABLE public.llm_usage
    ADD CONSTRAINT llm_usage_social_ai_cost_check CHECK (
        (reserved_micro_usd IS NULL OR reserved_micro_usd >= 0)
        AND (estimated_micro_usd IS NULL OR estimated_micro_usd >= 0)
        AND (cost_basis IS NULL OR cost_basis IN ('reserved_upper_bound', 'usage_priced_estimate', 'provider_rejected_unbilled'))
    );

UPDATE public.llm_usage
SET estimated_micro_usd = 0,
    cost_basis = 'provider_rejected_unbilled'
WHERE purpose = 'social_editor'
  AND provider = 'anthropic'
  AND attempt_state = 'failed'
  AND http_status BETWEEN 400 AND 499
  AND http_status NOT IN (408, 429)
  AND error_code LIKE 'Provider/%'
  AND split_part(error_code, '/', 2) NOT IN ('rate_limit_error', 'api_error', 'overloaded_error')
  AND tokens_in IS NULL
  AND tokens_out IS NULL
  AND total IS NULL
  AND cache_creation_input_tokens IS NULL
  AND cache_read_input_tokens IS NULL;

ALTER TABLE public.social_ai_processing_control DROP CONSTRAINT social_ai_processing_control_monthly_cap_micro_usd_check;
ALTER TABLE public.social_ai_processing_control
    ADD CONSTRAINT social_ai_processing_control_monthly_cap_micro_usd_check CHECK (monthly_cap_micro_usd BETWEEN 0 AND 20000000);
ALTER TABLE public.social_ai_user_limits DROP CONSTRAINT social_ai_user_limits_monthly_cap_micro_usd_check;
ALTER TABLE public.social_ai_user_limits
    ADD CONSTRAINT social_ai_user_limits_monthly_cap_micro_usd_check CHECK (monthly_cap_micro_usd BETWEEN 0 AND 20000000);
ALTER TABLE public.social_ai_plan_limit_drafts DROP CONSTRAINT social_ai_plan_limit_drafts_monthly_cap_micro_usd_check;
ALTER TABLE public.social_ai_plan_limit_drafts
    ADD CONSTRAINT social_ai_plan_limit_drafts_monthly_cap_micro_usd_check CHECK (monthly_cap_micro_usd BETWEEN 0 AND 20000000);
