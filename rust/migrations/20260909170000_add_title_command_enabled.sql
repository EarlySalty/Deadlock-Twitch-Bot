ALTER TABLE streamer_plans
    ADD COLUMN IF NOT EXISTS title_command_enabled integer DEFAULT 1 NOT NULL;
