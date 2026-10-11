CREATE TABLE social_media_autopilot_settings (
    twitch_user_id TEXT PRIMARY KEY REFERENCES twitch_streamers(twitch_user_id) ON DELETE CASCADE,
    enabled BOOLEAN NOT NULL DEFAULT FALSE,
    paused BOOLEAN NOT NULL DEFAULT FALSE,
    ai_editing_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    publish_mode TEXT NOT NULL DEFAULT 'review' CHECK (publish_mode IN ('review', 'full_auto')),
    lead_time_hours INTEGER NOT NULL DEFAULT 24 CHECK (lead_time_hours >= 0),
    timezone TEXT NOT NULL DEFAULT 'Europe/Berlin',
    schedule_slots JSONB NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(schedule_slots) = 'array'),
    target_platforms JSONB NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(target_platforms) = 'array'),
    clip_date_filter JSONB NOT NULL DEFAULT '{"kind":"all"}' CHECK (jsonb_typeof(clip_date_filter) = 'object'),
    clip_sort TEXT NOT NULL DEFAULT 'oldest_first' CHECK (clip_sort IN ('oldest_first', 'newest_first')),
    multi_output_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    experimental_ai_layout BOOLEAN NOT NULL DEFAULT FALSE,
    publish_language TEXT NOT NULL DEFAULT 'de' CHECK (publish_language IN ('de', 'en', 'auto')),
    notifications JSONB NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(notifications) = 'object'),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE social_production_jobs (
    id BIGSERIAL PRIMARY KEY,
    twitch_user_id TEXT NOT NULL REFERENCES twitch_streamers(twitch_user_id) ON DELETE CASCADE,
    clip_id BIGINT NOT NULL REFERENCES twitch_clips_social_media(id),
    source TEXT NOT NULL DEFAULT 'autopilot' CHECK (source IN ('autopilot', 'manual')),
    requested_mode TEXT CHECK (requested_mode IN ('static', 'ai')),
    mode TEXT CHECK (mode IN ('static', 'ai')),
    queue_order BIGINT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued' CHECK (status IN ('queued', 'running', 'review', 'completed', 'failed', 'cancelled')),
    stage TEXT NOT NULL DEFAULT 'queued',
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    max_attempts INTEGER NOT NULL DEFAULT 3 CHECK (max_attempts > 0),
    review_reasons JSONB NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(review_reasons) = 'array'),
    metadata JSONB NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(metadata) = 'object'),
    last_error TEXT,
    scheduled_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ,
    CHECK (status <> 'running' OR (mode IS NOT NULL AND started_at IS NOT NULL)),
    CHECK (source <> 'manual' OR requested_mode IS NOT NULL)
);
CREATE UNIQUE INDEX social_production_one_running ON social_production_jobs ((1)) WHERE status = 'running';
CREATE UNIQUE INDEX social_production_clip_active ON social_production_jobs (clip_id) WHERE status IN ('queued', 'running');
CREATE INDEX social_production_order ON social_production_jobs (queue_order, id) WHERE status = 'queued';

CREATE TABLE social_produced_videos (
    id BIGSERIAL PRIMARY KEY,
    twitch_user_id TEXT NOT NULL REFERENCES twitch_streamers(twitch_user_id) ON DELETE CASCADE,
    clip_id BIGINT NOT NULL REFERENCES twitch_clips_social_media(id),
    job_id BIGINT REFERENCES social_production_jobs(id),
    mode TEXT NOT NULL CHECK (mode IN ('static', 'ai')),
    version_no INTEGER NOT NULL DEFAULT 1 CHECK (version_no > 0),
    parent_version_id BIGINT REFERENCES social_produced_videos(id),
    variant_index INTEGER NOT NULL DEFAULT 0 CHECK (variant_index >= 0),
    file_path TEXT,
    duration_ms BIGINT CHECK (duration_ms > 0),
    status TEXT NOT NULL DEFAULT 'rendering' CHECK (status IN ('rendering', 'review', 'approved', 'scheduled', 'published', 'expired', 'rejected', 'failed')),
    needs_review BOOLEAN NOT NULL DEFAULT TRUE,
    review_reasons JSONB NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(review_reasons) = 'array'),
    metadata JSONB NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(metadata) = 'object'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ,
    scheduled_at TIMESTAMPTZ,
    UNIQUE (clip_id, version_no, variant_index),
    UNIQUE (id, clip_id),
    CHECK (status NOT IN ('review', 'approved', 'scheduled') OR (file_path IS NOT NULL AND duration_ms IS NOT NULL)),
    CHECK (status <> 'scheduled' OR scheduled_at IS NOT NULL)
);
CREATE UNIQUE INDEX social_produced_shared_slot ON social_produced_videos (twitch_user_id, scheduled_at) WHERE scheduled_at IS NOT NULL;
CREATE INDEX social_produced_user_review ON social_produced_videos (twitch_user_id, status, created_at);

ALTER TABLE social_media_clip_approval ADD COLUMN approval_id BIGSERIAL;
ALTER TABLE social_media_clip_approval ADD COLUMN produced_video_id BIGINT REFERENCES social_produced_videos(id);
ALTER TABLE social_media_clip_approval ADD COLUMN platform TEXT CHECK (platform IN ('youtube', 'tiktok', 'instagram'));
ALTER TABLE social_media_clip_approval DROP CONSTRAINT social_media_clip_approval_pkey;
ALTER TABLE social_media_clip_approval ALTER COLUMN clip_db_id DROP NOT NULL;
ALTER TABLE social_media_clip_approval ADD PRIMARY KEY (approval_id);
ALTER TABLE social_media_clip_approval ADD UNIQUE (clip_db_id);
ALTER TABLE social_media_clip_approval ADD UNIQUE (produced_video_id, platform);
ALTER TABLE social_media_clip_approval ADD CONSTRAINT social_approval_subject CHECK (
    (clip_db_id IS NOT NULL AND produced_video_id IS NULL AND platform IS NULL)
    OR (clip_db_id IS NULL AND produced_video_id IS NOT NULL AND platform IS NOT NULL AND approved_platforms = '[]'::jsonb)
);

ALTER TABLE twitch_clips_upload_queue ADD COLUMN produced_video_id BIGINT;
ALTER TABLE twitch_clips_upload_queue ADD CONSTRAINT social_upload_produced_clip FOREIGN KEY (produced_video_id, clip_id) REFERENCES social_produced_videos(id, clip_id);
CREATE UNIQUE INDEX social_upload_produced_platform ON twitch_clips_upload_queue (produced_video_id, platform) WHERE produced_video_id IS NOT NULL;
