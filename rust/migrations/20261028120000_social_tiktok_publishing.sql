ALTER TABLE social_media_autopilot_settings
    ADD COLUMN IF NOT EXISTS tiktok_publishing JSONB NOT NULL DEFAULT '{}'
        CHECK (jsonb_typeof(tiktok_publishing) = 'object');
