-- Public partner pages are opt-in. A disconnect hides them through the live
-- partner-state join; content is retained for a later return, never auto-purged.
CREATE TABLE IF NOT EXISTS twitch_partner_profiles (
    twitch_user_id TEXT PRIMARY KEY,
    published BOOLEAN NOT NULL DEFAULT FALSE,
    content JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(content) = 'object'),
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_partner_profiles_published
    ON twitch_partner_profiles (twitch_user_id) WHERE published;
-- The dashboard owns its settings, while collectors only read them.
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT, INSERT, UPDATE ON twitch_partner_profiles TO twitchdash;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT ON twitch_partner_profiles TO twitchbot;
    END IF;
END $$;
