-- Event snapshot and at-most-once delivery receipts. Never replay historical patches.
CREATE TABLE twitch_patch_announcements (
    event_id TEXT PRIMARY KEY,
    source_url TEXT NOT NULL UNIQUE,
    detected_at TIMESTAMPTZ NOT NULL,
    message TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE twitch_patch_announcement_deliveries (
    event_id TEXT NOT NULL REFERENCES twitch_patch_announcements(event_id),
    broadcaster_id TEXT NOT NULL,
    stream_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'attempted', 'sent', 'skipped', 'dropped', 'uncertain')),
    attempted_at TIMESTAMPTZ,
    PRIMARY KEY (event_id, broadcaster_id)
);
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT, INSERT ON twitch_patch_announcements TO twitchbot;
        GRANT SELECT, INSERT, UPDATE ON twitch_patch_announcement_deliveries TO twitchbot;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT ON twitch_patch_announcements, twitch_patch_announcement_deliveries TO twitchdash;
    END IF;
END $$;
