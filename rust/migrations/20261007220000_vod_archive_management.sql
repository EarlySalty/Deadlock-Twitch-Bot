ALTER TABLE twitch_vod_archive_vods
    ADD COLUMN hidden_at TIMESTAMPTZ,
    ADD COLUMN drive_requested BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN drive_url TEXT,
    ADD COLUMN last_attempt_at TIMESTAMPTZ;

CREATE INDEX twitch_vod_archive_visible_channel
    ON twitch_vod_archive_vods (twitch_user_id, discovered_at DESC)
    WHERE hidden_at IS NULL;
