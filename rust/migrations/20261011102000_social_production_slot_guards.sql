CREATE TABLE social_autopilot_schedule_state (
    twitch_user_id TEXT PRIMARY KEY REFERENCES twitch_streamers(twitch_user_id) ON DELETE CASCADE,
    planned_at TIMESTAMPTZ NOT NULL,
    production_hold TEXT CHECK (production_hold IN ('disabled', 'paused', 'review_backlog', 'empty_pool', 'no_schedule', 'outside_lead'))
);

CREATE UNIQUE INDEX social_production_shared_slot
ON social_production_jobs (twitch_user_id, scheduled_at)
WHERE scheduled_at IS NOT NULL AND status IN ('queued', 'running', 'completed');

CREATE FUNCTION social_production_guard_shared_slot() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    owner_id TEXT;
    slot_time TIMESTAMPTZ;
    clip_subject BIGINT;
    job_subject BIGINT;
    video_subject BIGINT;
BEGIN
    slot_time := NEW.scheduled_at;
    IF slot_time IS NULL THEN
        RETURN NEW;
    END IF;
    IF TG_TABLE_NAME = 'social_production_jobs' THEN
        IF NEW.status NOT IN ('queued', 'running', 'completed') THEN
            RETURN NEW;
        END IF;
        owner_id := NEW.twitch_user_id;
        clip_subject := NEW.clip_id;
        job_subject := NEW.id;
    ELSIF TG_TABLE_NAME = 'social_produced_videos' THEN
        owner_id := NEW.twitch_user_id;
        clip_subject := NEW.clip_id;
        job_subject := NEW.job_id;
        video_subject := NEW.id;
    ELSE
        IF COALESCE(NEW.status, '') IN ('failed', 'cancelled') THEN
            RETURN NEW;
        END IF;
        SELECT c.twitch_user_id INTO owner_id FROM twitch_clips_social_media c WHERE c.id = NEW.clip_id;
        clip_subject := NEW.clip_id;
        video_subject := NEW.produced_video_id;
        SELECT v.job_id INTO job_subject FROM social_produced_videos v WHERE v.id = video_subject;
    END IF;
    PERFORM pg_advisory_xact_lock(hashtextextended('social_autopilot:' || owner_id, 0));
    IF EXISTS (
        SELECT 1 FROM social_production_jobs j
        WHERE j.twitch_user_id = owner_id AND j.scheduled_at = slot_time
        AND j.status IN ('queued', 'running', 'completed')
        AND (j.id IS DISTINCT FROM job_subject OR j.clip_id IS DISTINCT FROM clip_subject)
    ) THEN
        RAISE EXCEPTION 'shared production slot already reserved' USING ERRCODE = '23505';
    END IF;
    IF TG_TABLE_NAME = 'social_production_jobs' AND (
        EXISTS (SELECT 1 FROM social_produced_videos v
            WHERE v.twitch_user_id = owner_id AND v.scheduled_at = slot_time
            AND (v.job_id IS DISTINCT FROM job_subject OR v.clip_id IS DISTINCT FROM clip_subject))
        OR EXISTS (SELECT 1 FROM twitch_clips_upload_queue q
            JOIN twitch_clips_social_media c ON c.id = q.clip_id
            LEFT JOIN social_produced_videos v ON v.id = q.produced_video_id
            WHERE c.twitch_user_id = owner_id AND q.scheduled_at = slot_time
            AND COALESCE(q.status, '') NOT IN ('failed', 'cancelled')
            AND (v.job_id IS DISTINCT FROM job_subject OR v.clip_id IS DISTINCT FROM clip_subject))
    ) THEN
        RAISE EXCEPTION 'shared publication slot already reserved' USING ERRCODE = '23505';
    END IF;
    IF TG_TABLE_NAME = 'social_produced_videos' AND EXISTS (
        SELECT 1 FROM twitch_clips_upload_queue q
        JOIN twitch_clips_social_media c ON c.id = q.clip_id
        WHERE c.twitch_user_id = owner_id AND q.scheduled_at = slot_time
        AND COALESCE(q.status, '') NOT IN ('failed', 'cancelled')
        AND q.produced_video_id IS DISTINCT FROM video_subject
    ) THEN
        RAISE EXCEPTION 'shared upload slot already reserved' USING ERRCODE = '23505';
    END IF;
    IF TG_TABLE_NAME = 'twitch_clips_upload_queue' AND EXISTS (
        SELECT 1 FROM social_produced_videos v
        WHERE v.twitch_user_id = owner_id AND v.scheduled_at = slot_time
        AND v.id IS DISTINCT FROM video_subject
    ) THEN
        RAISE EXCEPTION 'shared produced slot already reserved' USING ERRCODE = '23505';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER social_production_shared_slot_guard
BEFORE INSERT OR UPDATE OF scheduled_at, status ON social_production_jobs
FOR EACH ROW EXECUTE FUNCTION social_production_guard_shared_slot();

CREATE TRIGGER social_produced_production_slot_guard
BEFORE INSERT OR UPDATE OF scheduled_at ON social_produced_videos
FOR EACH ROW EXECUTE FUNCTION social_production_guard_shared_slot();

CREATE TRIGGER social_upload_production_slot_guard
BEFORE INSERT OR UPDATE OF scheduled_at, status ON twitch_clips_upload_queue
FOR EACH ROW EXECUTE FUNCTION social_production_guard_shared_slot();

DO $$
DECLARE
    runtime_role TEXT;
BEGIN
    FOR runtime_role IN SELECT rolname FROM pg_roles WHERE rolname IN ('twitchbot', 'twitchdash', 'twitchlegacy') LOOP
        EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON social_autopilot_schedule_state TO %I', runtime_role);
    END LOOP;
END;
$$;
