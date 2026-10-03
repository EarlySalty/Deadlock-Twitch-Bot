UPDATE social_media_partner_access AS access
SET twitch_user_id = streamer.twitch_user_id
FROM (
    SELECT LOWER(twitch_login) AS login, MIN(twitch_user_id) AS twitch_user_id
    FROM twitch_streamers
    WHERE twitch_user_id ~ '^[0-9]+$'
    GROUP BY LOWER(twitch_login)
    HAVING COUNT(DISTINCT twitch_user_id) = 1
) AS streamer
WHERE access.twitch_user_id IS NULL
  AND LOWER(access.streamer_login) = streamer.login
  AND streamer.twitch_user_id ~ '^[0-9]+$';

CREATE TABLE social_media_partner_access_unresolved (
    streamer_login TEXT PRIMARY KEY,
    twitch_user_id TEXT,
    granted BOOLEAN NOT NULL,
    granted_by TEXT,
    granted_at TIMESTAMPTZ NOT NULL,
    reported_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO social_media_partner_access_unresolved
    (streamer_login, twitch_user_id, granted, granted_by, granted_at)
SELECT streamer_login, twitch_user_id, granted, granted_by, granted_at
FROM social_media_partner_access
WHERE twitch_user_id IS NULL OR twitch_user_id !~ '^[0-9]+$';

DO $$
DECLARE unresolved TEXT;
BEGIN
    SELECT STRING_AGG(streamer_login, ', ' ORDER BY streamer_login)
    INTO unresolved FROM social_media_partner_access_unresolved;
    IF unresolved IS NOT NULL THEN
        RAISE WARNING 'Unresolved Social Media grants preserved in social_media_partner_access_unresolved: %', unresolved;
    END IF;
END $$;

DELETE FROM social_media_partner_access
WHERE twitch_user_id IS NULL OR twitch_user_id !~ '^[0-9]+$';

ALTER TABLE social_media_partner_access
    DROP CONSTRAINT social_media_partner_access_streamer_login_fkey,
    DROP CONSTRAINT social_media_partner_access_pkey,
    ALTER COLUMN twitch_user_id SET NOT NULL,
    ADD PRIMARY KEY (twitch_user_id),
    ADD CONSTRAINT social_media_partner_access_id_valid CHECK (twitch_user_id ~ '^[0-9]+$');

DROP INDEX social_media_partner_access_identity;

DO $$
DECLARE target TEXT;
BEGIN
    FOREACH target IN ARRAY ARRAY['clip_templates_streamer', 'clip_last_hashtags', 'social_media_streamer_layout', 'social_media_platform_auth'] LOOP
        EXECUTE FORMAT('UPDATE %I AS target SET twitch_user_id = source.twitch_user_id FROM (SELECT LOWER(twitch_login) AS login, MIN(twitch_user_id) AS twitch_user_id FROM twitch_streamers WHERE twitch_user_id ~ ''^[0-9]+$'' GROUP BY LOWER(twitch_login) HAVING COUNT(DISTINCT twitch_user_id) = 1) AS source WHERE target.twitch_user_id IS NULL AND LOWER(target.streamer_login) = source.login', target);
    END LOOP;
END $$;

CREATE UNIQUE INDEX clip_templates_streamer_identity_name
    ON clip_templates_streamer (twitch_user_id, template_name)
    WHERE twitch_user_id IS NOT NULL;
CREATE UNIQUE INDEX clip_last_hashtags_identity
    ON clip_last_hashtags (twitch_user_id)
    WHERE twitch_user_id IS NOT NULL;

ALTER TABLE clip_templates_streamer DROP CONSTRAINT clip_templates_streamer_streamer_login_template_name_key;
ALTER TABLE clip_last_hashtags DROP CONSTRAINT clip_last_hashtags_pkey;

CREATE UNIQUE INDEX social_media_streamer_layout_identity
    ON social_media_streamer_layout (twitch_user_id)
    WHERE twitch_user_id IS NOT NULL;
ALTER TABLE social_media_streamer_layout
    DROP CONSTRAINT social_media_streamer_layout_streamer_login_fkey,
    DROP CONSTRAINT social_media_streamer_layout_pkey;

DO $$
DECLARE target TEXT; relation_constraint RECORD;
BEGIN
    FOREACH target IN ARRAY ARRAY['social_media_streamer_settings', 'social_media_platform_schedule', 'social_media_category_settings', 'social_media_vod_archive'] LOOP
        EXECUTE FORMAT('ALTER TABLE %I ADD COLUMN twitch_user_id TEXT', target);
        EXECUTE FORMAT('UPDATE %I AS target SET twitch_user_id = source.twitch_user_id FROM (SELECT LOWER(twitch_login) AS login, MIN(twitch_user_id) AS twitch_user_id FROM twitch_streamers WHERE twitch_user_id ~ ''^[0-9]+$'' GROUP BY LOWER(twitch_login) HAVING COUNT(DISTINCT twitch_user_id) = 1) AS source WHERE LOWER(target.streamer_login) = source.login', target);
        FOR relation_constraint IN
            SELECT c.conname FROM pg_constraint c
            WHERE c.conrelid = TO_REGCLASS(target)
              AND (c.contype = 'p' OR (c.contype = 'f' AND c.conkey = ARRAY[(SELECT attnum FROM pg_attribute WHERE attrelid = c.conrelid AND attname = 'streamer_login')]::SMALLINT[]))
        LOOP
            EXECUTE FORMAT('ALTER TABLE %I DROP CONSTRAINT %I', target, relation_constraint.conname);
        END LOOP;
    END LOOP;
END $$;

CREATE UNIQUE INDEX social_media_streamer_settings_identity ON social_media_streamer_settings (twitch_user_id) WHERE twitch_user_id IS NOT NULL;
CREATE UNIQUE INDEX social_media_platform_schedule_identity ON social_media_platform_schedule (twitch_user_id, platform) WHERE twitch_user_id IS NOT NULL;
CREATE UNIQUE INDEX social_media_category_settings_identity ON social_media_category_settings (twitch_user_id, category_key) WHERE twitch_user_id IS NOT NULL;
CREATE UNIQUE INDEX social_media_vod_archive_identity ON social_media_vod_archive (twitch_user_id) WHERE twitch_user_id IS NOT NULL;

ALTER TABLE twitch_vod_archive_vods ADD COLUMN twitch_user_id TEXT;
UPDATE twitch_vod_archive_vods AS target SET twitch_user_id = source.twitch_user_id
FROM (
    SELECT LOWER(twitch_login) AS login, MIN(twitch_user_id) AS twitch_user_id
    FROM twitch_streamers WHERE twitch_user_id ~ '^[0-9]+$'
    GROUP BY LOWER(twitch_login) HAVING COUNT(DISTINCT twitch_user_id) = 1
) AS source WHERE LOWER(target.streamer_login) = source.login;
CREATE INDEX twitch_vod_archive_vods_owner ON twitch_vod_archive_vods (twitch_user_id);

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON social_media_partner_access_unresolved TO twitchbot;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchcollector') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON social_media_partner_access_unresolved TO twitchcollector;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT ON social_media_partner_access_unresolved TO twitchdash;
    END IF;
END $$;
