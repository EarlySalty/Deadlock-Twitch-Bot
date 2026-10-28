ALTER TABLE social_media_autopilot_settings
    ADD COLUMN IF NOT EXISTS ai_frame_format TEXT NOT NULL DEFAULT 'layout'
        CHECK (ai_frame_format IN ('layout', 'fill', 'full'));
