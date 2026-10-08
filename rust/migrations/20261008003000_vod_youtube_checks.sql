CREATE TABLE twitch_vod_youtube_checks (
    vod_id BIGINT PRIMARY KEY REFERENCES twitch_vod_archive_vods(id) ON DELETE CASCADE,
    auth_id INTEGER NOT NULL,
    auth_revision TEXT NOT NULL,
    channel_id TEXT,
    state TEXT NOT NULL DEFAULT 'pending',
    complete BOOLEAN NOT NULL DEFAULT FALSE,
    observations JSONB NOT NULL DEFAULT '[]'::jsonb,
    upload_snapshot JSONB NOT NULL DEFAULT '[]'::jsonb,
    attempt_snapshot JSONB,
    last_attempt_at TIMESTAMPTZ,
    last_success_at TIMESTAMPTZ,
    last_error TEXT,
    requested_at TIMESTAMPTZ,
    next_check_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX twitch_vod_youtube_checks_due ON twitch_vod_youtube_checks(next_check_at);

CREATE TABLE twitch_vod_youtube_continuations (
    vod_id BIGINT PRIMARY KEY REFERENCES twitch_vod_archive_vods(id) ON DELETE CASCADE,
    auth_id INTEGER NOT NULL,
    auth_revision TEXT NOT NULL,
    channel_id TEXT NOT NULL,
    vod_snapshot JSONB NOT NULL,
    upload_snapshot JSONB NOT NULL,
    source_snapshot JSONB NOT NULL,
    refreshed JSONB NOT NULL,
    observations JSONB NOT NULL,
    started_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE twitch_vod_youtube_scans (
    twitch_user_id TEXT PRIMARY KEY,
    auth_id INTEGER NOT NULL,
    auth_revision TEXT NOT NULL,
    channel_id TEXT NOT NULL,
    playlist_id TEXT NOT NULL,
    cursor TEXT,
    generation BIGINT NOT NULL DEFAULT 1,
    complete BOOLEAN NOT NULL DEFAULT FALSE,
    next_scan_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_attempt_at TIMESTAMPTZ,
    last_success_at TIMESTAMPTZ,
    last_error TEXT
);

CREATE TABLE twitch_vod_youtube_inventory (
    twitch_user_id TEXT NOT NULL REFERENCES twitch_vod_youtube_scans(twitch_user_id) ON DELETE CASCADE,
    video_id TEXT NOT NULL,
    generation BIGINT NOT NULL,
    twitch_id TEXT NOT NULL,
    part_index INTEGER,
    part_total INTEGER,
    duration_sec BIGINT,
    state TEXT NOT NULL,
    privacy TEXT,
    observed_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY(twitch_user_id, video_id)
);
CREATE INDEX twitch_vod_youtube_inventory_source ON twitch_vod_youtube_inventory(twitch_user_id, generation, twitch_id);
