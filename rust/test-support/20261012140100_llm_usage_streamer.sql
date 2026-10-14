ALTER TABLE public.llm_usage
    ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;

CREATE INDEX IF NOT EXISTS idx_llm_usage_streamer
    ON public.llm_usage (twitch_user_id, ts)
    WHERE twitch_user_id IS NOT NULL;
