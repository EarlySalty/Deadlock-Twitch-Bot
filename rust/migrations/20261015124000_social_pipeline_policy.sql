ALTER TABLE social_media_autopilot_settings
    ADD COLUMN pipeline_mode TEXT CHECK (pipeline_mode IN ('upload', 'upload_text', 'ai_edit')),
    ADD COLUMN review_policy TEXT NOT NULL DEFAULT 'required' CHECK (review_policy IN ('off', 'required', 'veto_until_deadline')),
    ADD COLUMN metadata_scope TEXT NOT NULL DEFAULT 'shared' CHECK (metadata_scope IN ('shared', 'per_platform')),
    ADD COLUMN text_defaults JSONB NOT NULL DEFAULT '{"shared":{"description":"","hashtags":[]},"per_platform":{}}' CHECK (jsonb_typeof(text_defaults) = 'object'),
    ADD COLUMN schedule_scope TEXT NOT NULL DEFAULT 'shared' CHECK (schedule_scope IN ('shared', 'per_platform')),
    ADD COLUMN platform_schedule_slots JSONB NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(platform_schedule_slots) = 'object'),
    ADD COLUMN distribution_policy TEXT NOT NULL DEFAULT 'same_clip' CHECK (distribution_policy IN ('same_clip', 'independent_clips')),
    ADD COLUMN policy_revision BIGINT NOT NULL DEFAULT 0 CHECK (policy_revision >= 0);

ALTER TABLE social_production_jobs ADD COLUMN policy_snapshot JSONB CHECK (jsonb_typeof(policy_snapshot) = 'object');
ALTER TABLE social_produced_videos ADD COLUMN policy_snapshot JSONB CHECK (jsonb_typeof(policy_snapshot) = 'object');

CREATE TABLE social_pipeline_cutover (
    twitch_user_id TEXT PRIMARY KEY REFERENCES twitch_streamers(twitch_user_id) ON DELETE CASCADE,
    state TEXT NOT NULL DEFAULT 'legacy' CHECK (state IN ('legacy', 'shadow', 'paused', 'unified')),
    migration_version INTEGER NOT NULL CHECK (migration_version > 0),
    reconciliation JSONB NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(reconciliation) = 'object'),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE social_pipeline_migration_map (
    twitch_user_id TEXT NOT NULL REFERENCES twitch_streamers(twitch_user_id) ON DELETE CASCADE,
    legacy_table TEXT NOT NULL CHECK (legacy_table <> ''),
    legacy_key TEXT NOT NULL CHECK (legacy_key <> ''),
    migration_version INTEGER NOT NULL CHECK (migration_version > 0),
    input_digest TEXT NOT NULL CHECK (input_digest <> ''),
    canonical_subject JSONB CHECK (jsonb_typeof(canonical_subject) = 'object'),
    disposition TEXT NOT NULL CHECK (disposition IN ('mapped', 'history', 'unresolved')),
    unresolved_reason TEXT,
    original_snapshot JSONB NOT NULL CHECK (jsonb_typeof(original_snapshot) = 'object'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (twitch_user_id, legacy_table, legacy_key, migration_version),
    CHECK ((disposition = 'unresolved') = (unresolved_reason IS NOT NULL)),
    CHECK (disposition <> 'mapped' OR canonical_subject IS NOT NULL)
);

DO $$
DECLARE
    runtime_role TEXT;
BEGIN
    FOR runtime_role IN SELECT rolname FROM pg_roles WHERE rolname IN ('twitchbot', 'twitchdash', 'twitchlegacy') LOOP
        EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON social_pipeline_cutover, social_pipeline_migration_map TO %I', runtime_role);
    END LOOP;
END;
$$;
