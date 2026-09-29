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
    drop_code TEXT,
    http_status SMALLINT,
    uncertainty_reason TEXT,
    PRIMARY KEY (event_id, broadcaster_id)
);

CREATE TABLE twitch_patch_feed_state (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    bootstrapped_at TIMESTAMPTZ,
    last_successful_index_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE twitch_patch_feed_observations (
    patch_id BIGINT PRIMARY KEY CHECK (patch_id > 0),
    observed_at TIMESTAMPTZ NOT NULL,
    status TEXT NOT NULL CHECK (status IN (
        'historical', 'pending', 'processed', 'expired_timeout',
        'expired_missing_from_index', 'expired_unavailable', 'missed_during_outage'
    )),
    finalized_at TIMESTAMPTZ
);

CREATE TABLE twitch_patch_announcement_recipients (
    patch_id BIGINT NOT NULL REFERENCES twitch_patch_feed_observations(patch_id),
    broadcaster_id TEXT NOT NULL,
    stream_id TEXT NOT NULL,
    PRIMARY KEY (patch_id, broadcaster_id)
);

CREATE FUNCTION twitch_patch_snapshot_recipients() RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    candidate RECORD;
    last_seen TIMESTAMPTZ;
BEGIN
    IF NEW.status <> 'pending' THEN
        RETURN NEW;
    END IF;

    FOR candidate IN
        SELECT p.twitch_user_id, l.last_stream_id, l.last_seen_at
        FROM twitch_streamers_partner_state p
        JOIN twitch_live_state l ON l.twitch_user_id = p.twitch_user_id
        WHERE p.is_partner_active = 1
          AND COALESCE(p.manual_partner_opt_out, 0) = 0
          AND l.is_live = 1
          AND l.last_game = 'Deadlock'
          AND COALESCE(p.twitch_user_id, '') <> ''
          AND COALESCE(l.last_stream_id, '') <> ''
          AND EXISTS (
              SELECT 1 FROM twitch_raid_auth ra
              WHERE ra.twitch_user_id = p.twitch_user_id
                AND ra.needs_reauth IS FALSE
                AND 'channel:bot' = ANY(regexp_split_to_array(
                    COALESCE(ra.scopes, ''), '[[:space:],]+'))
          )
    LOOP
        BEGIN
            last_seen := candidate.last_seen_at::timestamptz;
        EXCEPTION
            WHEN data_exception THEN
                CONTINUE;
        END;

        IF last_seen IS NULL
           OR last_seen < NEW.observed_at - INTERVAL '120 seconds'
           OR last_seen > NEW.observed_at THEN
            CONTINUE;
        END IF;

        INSERT INTO twitch_patch_announcement_recipients
            (patch_id, broadcaster_id, stream_id)
        VALUES (NEW.patch_id, candidate.twitch_user_id, candidate.last_stream_id)
        ON CONFLICT DO NOTHING;
    END LOOP;

    RETURN NEW;
END
$$;

CREATE TRIGGER twitch_patch_snapshot_recipients_after_observation
AFTER INSERT ON twitch_patch_feed_observations
FOR EACH ROW EXECUTE FUNCTION twitch_patch_snapshot_recipients();

DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT, INSERT ON twitch_patch_announcements TO twitchbot;
        GRANT SELECT, INSERT ON twitch_patch_announcement_deliveries TO twitchbot;
        GRANT UPDATE (status, attempted_at, drop_code, http_status, uncertainty_reason)
            ON twitch_patch_announcement_deliveries TO twitchbot;
        GRANT SELECT, INSERT ON twitch_patch_feed_state TO twitchbot;
        GRANT UPDATE (singleton, bootstrapped_at, last_successful_index_at)
            ON twitch_patch_feed_state TO twitchbot;
        GRANT SELECT, INSERT ON twitch_patch_feed_observations TO twitchbot;
        GRANT UPDATE (status, finalized_at) ON twitch_patch_feed_observations TO twitchbot;
        GRANT SELECT, INSERT ON twitch_patch_announcement_recipients TO twitchbot;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT ON twitch_patch_announcements, twitch_patch_announcement_deliveries,
            twitch_patch_feed_state, twitch_patch_feed_observations,
            twitch_patch_announcement_recipients TO twitchdash;
    END IF;
END $$;
