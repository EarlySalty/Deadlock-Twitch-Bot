ALTER TABLE streamer_plans
    ADD COLUMN IF NOT EXISTS command_name_overrides jsonb NOT NULL DEFAULT '{}'::jsonb
    CHECK (jsonb_typeof(command_name_overrides) = 'object');
