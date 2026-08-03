ALTER TABLE public.streamer_plans
    ADD COLUMN IF NOT EXISTS clip_command_enabled integer DEFAULT 1 NOT NULL;
