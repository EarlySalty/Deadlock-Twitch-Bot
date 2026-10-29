ALTER TABLE social_media_autopilot_settings
    ADD COLUMN IF NOT EXISTS ai_auto_cam BOOLEAN NOT NULL DEFAULT TRUE;
