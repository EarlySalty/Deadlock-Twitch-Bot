ALTER TABLE public.llm_usage DROP CONSTRAINT llm_usage_social_ai_cost_check;

ALTER TABLE public.llm_usage
    ADD CONSTRAINT llm_usage_social_ai_cost_check CHECK (
        (reserved_micro_usd IS NULL OR reserved_micro_usd >= 0)
        AND (estimated_micro_usd IS NULL OR estimated_micro_usd >= 0)
        AND (cost_basis IS NULL OR cost_basis IN ('reserved_upper_bound', 'usage_priced_estimate', 'provider_rejected_unbilled', 'subscription_included'))
    );
