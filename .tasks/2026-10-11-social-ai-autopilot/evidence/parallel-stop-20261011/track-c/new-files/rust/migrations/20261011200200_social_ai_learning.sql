CREATE TABLE social_ai_learning_observations (
    sequence BIGSERIAL PRIMARY KEY,
    twitch_user_id BIGINT NOT NULL CHECK (twitch_user_id > 0),
    event_key TEXT NOT NULL CHECK (length(event_key) BETWEEN 1 AND 128),
    video_version_id BIGINT NOT NULL REFERENCES social_produced_videos(id) ON DELETE CASCADE,
    root_job_id BIGINT NOT NULL REFERENCES social_ai_jobs(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('feedback', 'accepted', 'rejected')),
    payload JSONB NOT NULL CHECK (jsonb_typeof(payload) = 'object'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    UNIQUE (twitch_user_id, event_key)
);
CREATE INDEX social_ai_learning_owner_sequence ON social_ai_learning_observations(twitch_user_id, sequence);
CREATE INDEX social_ai_learning_window ON social_ai_learning_observations(created_at, twitch_user_id);

CREATE TABLE social_ai_skill_versions (
    skill_id TEXT NOT NULL CHECK (skill_id IN ('deadlock_understanding', 'highlight_analysis', 'editing_strategy', 'video_layout', 'subtitles', 'copywriting', 'quality_validation', 'feedback_learning')),
    version BIGINT NOT NULL CHECK (version > 0),
    sha256 TEXT NOT NULL CHECK (sha256 ~ '^[a-f0-9]{64}$'),
    snapshot JSONB NOT NULL CHECK (jsonb_typeof(snapshot) = 'object'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (skill_id, version)
);
CREATE TABLE social_ai_active_skills (
    skill_id TEXT PRIMARY KEY,
    version BIGINT NOT NULL,
    FOREIGN KEY (skill_id, version) REFERENCES social_ai_skill_versions(skill_id, version)
);
CREATE TABLE social_ai_skill_changes (
    id BIGSERIAL PRIMARY KEY,
    skill_id TEXT NOT NULL,
    previous_version BIGINT NOT NULL,
    new_version BIGINT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('automatic', 'rollback')),
    summary TEXT NOT NULL CHECK (length(summary) BETWEEN 1 AND 512),
    reason TEXT NOT NULL CHECK (length(reason) BETWEEN 1 AND 1024),
    evidence JSONB NOT NULL CHECK (jsonb_typeof(evidence) = 'array'),
    restored_from_version BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    FOREIGN KEY (skill_id, previous_version) REFERENCES social_ai_skill_versions(skill_id, version),
    FOREIGN KEY (skill_id, new_version) REFERENCES social_ai_skill_versions(skill_id, version),
    FOREIGN KEY (skill_id, restored_from_version) REFERENCES social_ai_skill_versions(skill_id, version),
    UNIQUE (skill_id, new_version)
);
CREATE TABLE social_ai_critical_proposals (
    id BIGSERIAL PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('code', 'architecture', 'tool')),
    pattern TEXT NOT NULL CHECK (pattern IN ('preserve_context', 'shorter_opening', 'smaller_subtitles', 'larger_subtitles', 'preserve_reactions')),
    summary TEXT NOT NULL CHECK (length(summary) BETWEEN 1 AND 512),
    reason TEXT NOT NULL CHECK (length(reason) BETWEEN 1 AND 1024),
    proposed_change TEXT NOT NULL CHECK (length(proposed_change) BETWEEN 1 AND 2048),
    evidence JSONB NOT NULL CHECK (jsonb_typeof(evidence) = 'array'),
    status TEXT NOT NULL DEFAULT 'proposed' CHECK (status IN ('proposed', 'reviewed', 'dismissed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    UNIQUE (kind, pattern)
);
