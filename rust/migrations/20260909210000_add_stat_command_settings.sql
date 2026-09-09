ALTER TABLE streamer_plans
    ADD COLUMN IF NOT EXISTS stat_command_settings jsonb NOT NULL DEFAULT '{}'::jsonb
    CHECK (jsonb_typeof(stat_command_settings) = 'object');
