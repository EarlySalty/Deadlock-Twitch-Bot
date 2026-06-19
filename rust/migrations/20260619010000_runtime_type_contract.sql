-- Produktionsvertrag für Rust-Schreibpfade:
-- EventSub-Flags sind BOOLEAN, Clip-Zeitstempel sind TIMESTAMPTZ.

ALTER TABLE twitch_subscription_events
    ALTER COLUMN is_gift DROP DEFAULT,
    ALTER COLUMN is_gift TYPE BOOLEAN
        USING CASE
            WHEN is_gift IS NULL THEN FALSE
            WHEN is_gift::text IN ('1', 'true', 't', 'yes', 'on') THEN TRUE
            ELSE FALSE
        END,
    ALTER COLUMN is_gift SET DEFAULT FALSE;

ALTER TABLE twitch_ad_break_events
    ALTER COLUMN is_automatic DROP DEFAULT,
    ALTER COLUMN is_automatic TYPE BOOLEAN
        USING CASE
            WHEN is_automatic IS NULL THEN FALSE
            WHEN is_automatic::text IN ('1', 'true', 't', 'yes', 'on') THEN TRUE
            ELSE FALSE
        END,
    ALTER COLUMN is_automatic SET DEFAULT FALSE;

ALTER TABLE twitch_clips_social_media
    ALTER COLUMN created_at DROP DEFAULT,
    ALTER COLUMN created_at TYPE TIMESTAMPTZ
        USING CASE
            WHEN created_at IS NULL OR BTRIM(created_at::text) = '' THEN NOW()
            ELSE created_at::text::timestamptz
        END,
    ALTER COLUMN created_at SET DEFAULT NOW();
