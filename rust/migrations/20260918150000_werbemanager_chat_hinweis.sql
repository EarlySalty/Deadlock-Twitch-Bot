ALTER TABLE twitch_ad_manager_settings
    ADD COLUMN IF NOT EXISTS chat_notice_before_ad BOOLEAN NOT NULL DEFAULT TRUE;

ALTER TABLE twitch_ad_manager_state
    ADD COLUMN IF NOT EXISTS last_hint_key TEXT,
    ADD COLUMN IF NOT EXISTS last_hint_variant SMALLINT,
    ADD COLUMN IF NOT EXISTS last_hint_at TIMESTAMPTZ;
