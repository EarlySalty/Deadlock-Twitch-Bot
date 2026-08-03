ALTER TABLE public.streamer_plans
    ADD COLUMN IF NOT EXISTS greeting_reply_enabled integer DEFAULT 1 NOT NULL;
