ALTER TABLE twitch_clips_upload_queue ADD COLUMN external_id TEXT;
ALTER TABLE twitch_clips_upload_queue ADD COLUMN next_retry_at TIMESTAMPTZ;
ALTER TABLE twitch_clips_upload_queue ADD COLUMN outcome_reason TEXT;
ALTER TABLE twitch_clips_upload_queue ADD COLUMN account_id TEXT;
ALTER TABLE twitch_clips_upload_queue ADD COLUMN effect_started_at TIMESTAMPTZ;
ALTER TABLE twitch_clips_upload_queue ADD COLUMN claim_generation BIGINT NOT NULL DEFAULT 0;
CREATE INDEX social_upload_produced_due ON twitch_clips_upload_queue (status, next_retry_at)
    WHERE produced_video_id IS NOT NULL;

ALTER TABLE social_produced_videos ADD COLUMN publication_executed_at TIMESTAMPTZ;
ALTER TABLE social_produced_videos ADD COLUMN publication_completed_at TIMESTAMPTZ;
ALTER TABLE social_produced_videos ADD COLUMN artifact_deleted_at TIMESTAMPTZ;
ALTER TABLE social_produced_videos ADD COLUMN cleanup_error TEXT;
ALTER TABLE social_produced_videos ADD COLUMN autopilot_deferred_stage TEXT
    CHECK (autopilot_deferred_stage IN ('promote', 'schedule', 'execute'));
ALTER TABLE social_produced_videos ADD COLUMN autopilot_deferred_until TIMESTAMPTZ;
CREATE INDEX social_produced_due ON social_produced_videos (scheduled_at)
    WHERE status IN ('review', 'approved', 'scheduled') AND publication_executed_at IS NULL;

CREATE TABLE social_autopilot_notifications (
    id BIGSERIAL PRIMARY KEY,
    twitch_user_id TEXT NOT NULL REFERENCES twitch_streamers(twitch_user_id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('ready_for_review', 'publication_deadline', 'processing_failed', 'publication_failed', 'empty_pool', 'review_expiring')),
    content TEXT NOT NULL,
    link TEXT NOT NULL,
    dedupe_key TEXT NOT NULL UNIQUE,
    show_in_dashboard BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    read_at TIMESTAMPTZ,
    discord_state TEXT NOT NULL DEFAULT 'disabled' CHECK (discord_state IN ('disabled', 'pending', 'sending', 'sent', 'failed', 'unavailable')),
    discord_sent_at TIMESTAMPTZ
);
CREATE INDEX social_autopilot_notifications_owner ON social_autopilot_notifications (twitch_user_id, created_at DESC);
CREATE INDEX social_autopilot_notifications_discord ON social_autopilot_notifications (created_at) WHERE discord_state = 'pending';

DO $$
DECLARE
    runtime_role TEXT;
BEGIN
    FOR runtime_role IN SELECT rolname FROM pg_roles WHERE rolname IN ('twitchbot', 'twitchdash', 'twitchlegacy') LOOP
        EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON social_autopilot_notifications TO %I', runtime_role);
        EXECUTE format('GRANT USAGE ON SEQUENCE social_autopilot_notifications_id_seq TO %I', runtime_role);
    END LOOP;
END;
$$;
