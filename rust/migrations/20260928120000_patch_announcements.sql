CREATE TABLE twitch_patch_announcements (
    event_id TEXT PRIMARY KEY,
    article_url TEXT NOT NULL UNIQUE,
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

CREATE TABLE twitch_patch_feed_state (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    bootstrapped_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE twitch_patch_feed_observations (
    patch_id BIGINT PRIMARY KEY CHECK (patch_id > 0),
    observed_at TIMESTAMPTZ NOT NULL,
    status TEXT NOT NULL CHECK (status IN (
        'historical', 'pending', 'processed', 'expired_timeout',
        'expired_missing_from_index', 'expired_unavailable'
    )),
    finalized_at TIMESTAMPTZ
);

DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT, INSERT ON twitch_patch_announcements TO twitchbot;
        GRANT SELECT, INSERT, UPDATE ON twitch_patch_announcement_deliveries TO twitchbot;
        GRANT SELECT, INSERT ON twitch_patch_feed_state TO twitchbot;
        GRANT SELECT, INSERT ON twitch_patch_feed_observations TO twitchbot;
        GRANT UPDATE (status, finalized_at) ON twitch_patch_feed_observations TO twitchbot;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT ON twitch_patch_announcements, twitch_patch_announcement_deliveries,
            twitch_patch_feed_state, twitch_patch_feed_observations TO twitchdash;
    END IF;
END $$;
