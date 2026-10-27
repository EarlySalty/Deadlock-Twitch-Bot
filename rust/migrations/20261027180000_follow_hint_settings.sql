ALTER TABLE streamer_plans
    ADD COLUMN IF NOT EXISTS follow_hint_enabled integer DEFAULT 0 NOT NULL,
    ADD COLUMN IF NOT EXISTS follow_hint_message text;
