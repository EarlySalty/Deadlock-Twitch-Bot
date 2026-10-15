CREATE TABLE social_publication_reservations (
    id BIGSERIAL PRIMARY KEY,
    twitch_user_id TEXT NOT NULL REFERENCES twitch_streamers(twitch_user_id) ON DELETE CASCADE,
    platform TEXT NOT NULL CHECK (platform IN ('youtube', 'tiktok', 'instagram')),
    scheduled_at TIMESTAMPTZ NOT NULL,
    job_id BIGINT NOT NULL REFERENCES social_production_jobs(id) ON DELETE CASCADE,
    produced_video_id BIGINT REFERENCES social_produced_videos(id) ON DELETE SET NULL,
    state TEXT NOT NULL DEFAULT 'reserved' CHECK (state IN ('reserved', 'released')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    released_at TIMESTAMPTZ,
    CHECK ((state = 'released') = (released_at IS NOT NULL))
);

CREATE UNIQUE INDEX social_reservation_destination_slot
ON social_publication_reservations (twitch_user_id, platform, scheduled_at)
WHERE state = 'reserved';

CREATE UNIQUE INDEX social_reservation_job_platform
ON social_publication_reservations (job_id, platform)
WHERE state = 'reserved';

CREATE INDEX social_reservation_job ON social_publication_reservations (job_id);

DROP TRIGGER IF EXISTS social_production_shared_slot_guard ON social_production_jobs;
DROP TRIGGER IF EXISTS social_produced_production_slot_guard ON social_produced_videos;
DROP TRIGGER IF EXISTS social_upload_production_slot_guard ON twitch_clips_upload_queue;
DROP FUNCTION IF EXISTS social_production_guard_shared_slot();
DROP INDEX IF EXISTS social_production_shared_slot;
DROP INDEX IF EXISTS social_produced_shared_slot;

CREATE INDEX social_production_owner_slot ON social_production_jobs (twitch_user_id, scheduled_at)
WHERE scheduled_at IS NOT NULL;
CREATE INDEX social_produced_owner_slot ON social_produced_videos (twitch_user_id, scheduled_at)
WHERE scheduled_at IS NOT NULL;

CREATE FUNCTION social_release_job_reservations() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.scheduled_at IS NULL OR NEW.status NOT IN ('queued', 'running', 'completed')
        OR NEW.scheduled_at IS DISTINCT FROM OLD.scheduled_at THEN
        UPDATE social_publication_reservations SET state = 'released', released_at = clock_timestamp()
        WHERE job_id = NEW.id AND state = 'reserved';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER social_production_release_reservations
AFTER UPDATE OF scheduled_at, status ON social_production_jobs
FOR EACH ROW EXECUTE FUNCTION social_release_job_reservations();

CREATE FUNCTION social_channel_slot_committed(owner_id TEXT, at TIMESTAMPTZ, subject BIGINT, subject_video BIGINT)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT EXISTS (
        SELECT 1 FROM social_production_jobs j
        WHERE j.twitch_user_id = owner_id AND j.scheduled_at = at
        AND j.status IN ('queued', 'running', 'completed') AND j.id IS DISTINCT FROM subject
        AND NOT EXISTS (SELECT 1 FROM social_publication_reservations r WHERE r.job_id = j.id AND r.state = 'reserved')
    ) OR EXISTS (
        SELECT 1 FROM social_produced_videos v
        WHERE v.twitch_user_id = owner_id AND v.scheduled_at = at
        AND v.id IS DISTINCT FROM subject_video
        AND (v.job_id IS NULL OR v.job_id IS DISTINCT FROM subject)
        AND NOT EXISTS (SELECT 1 FROM social_publication_reservations r WHERE r.job_id = v.job_id
            AND r.state = 'reserved' AND r.scheduled_at = v.scheduled_at)
    )
$$;

CREATE FUNCTION social_destination_guard_slot() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    owner_id TEXT;
    job_subject BIGINT;
BEGIN
    IF NEW.scheduled_at IS NULL THEN
        RETURN NEW;
    END IF;
    IF TG_TABLE_NAME = 'social_publication_reservations' THEN
        IF NEW.state <> 'reserved' THEN
            RETURN NEW;
        END IF;
        PERFORM pg_advisory_xact_lock(hashtextextended('social_autopilot:' || NEW.twitch_user_id, 0));
        IF EXISTS (
            SELECT 1 FROM twitch_clips_upload_queue q
            JOIN twitch_clips_social_media c ON c.id = q.clip_id
            LEFT JOIN social_produced_videos v ON v.id = q.produced_video_id
            WHERE c.twitch_user_id = NEW.twitch_user_id AND q.platform = NEW.platform
            AND q.scheduled_at = NEW.scheduled_at
            AND COALESCE(q.status, '') NOT IN ('failed', 'cancelled')
            AND v.job_id IS DISTINCT FROM NEW.job_id
        ) OR social_channel_slot_committed(NEW.twitch_user_id, NEW.scheduled_at, NEW.job_id, NEW.produced_video_id) THEN
            RAISE EXCEPTION 'destination slot already used' USING ERRCODE = '23505';
        END IF;
        RETURN NEW;
    END IF;
    IF COALESCE(NEW.status, '') IN ('failed', 'cancelled') THEN
        RETURN NEW;
    END IF;
    IF TG_OP = 'UPDATE' AND NEW.produced_video_id IS NULL
        AND OLD.scheduled_at IS NOT DISTINCT FROM NEW.scheduled_at
        AND OLD.platform IS NOT DISTINCT FROM NEW.platform
        AND COALESCE(OLD.status, '') NOT IN ('failed', 'cancelled') THEN
        RETURN NEW;
    END IF;
    SELECT c.twitch_user_id INTO owner_id FROM twitch_clips_social_media c WHERE c.id = NEW.clip_id;
    IF owner_id IS NULL THEN
        RETURN NEW;
    END IF;
    PERFORM pg_advisory_xact_lock(hashtextextended('social_autopilot:' || owner_id, 0));
    SELECT v.job_id INTO job_subject FROM social_produced_videos v WHERE v.id = NEW.produced_video_id;
    IF EXISTS (
        SELECT 1 FROM social_publication_reservations r
        WHERE r.twitch_user_id = owner_id AND r.platform = NEW.platform
        AND r.scheduled_at = NEW.scheduled_at AND r.state = 'reserved'
        AND r.job_id IS DISTINCT FROM job_subject
    ) OR EXISTS (
        SELECT 1 FROM twitch_clips_upload_queue q
        JOIN twitch_clips_social_media c ON c.id = q.clip_id
        WHERE c.twitch_user_id = owner_id AND q.platform = NEW.platform
        AND q.scheduled_at = NEW.scheduled_at AND q.id IS DISTINCT FROM NEW.id
        AND COALESCE(q.status, '') NOT IN ('failed', 'cancelled')
        AND (q.produced_video_id IS DISTINCT FROM NEW.produced_video_id OR q.clip_id IS DISTINCT FROM NEW.clip_id)
    ) OR social_channel_slot_committed(owner_id, NEW.scheduled_at, job_subject, NEW.produced_video_id) THEN
        RAISE EXCEPTION 'destination slot already used' USING ERRCODE = '23505';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER social_reservation_destination_guard
BEFORE INSERT OR UPDATE OF scheduled_at, state, platform ON social_publication_reservations
FOR EACH ROW EXECUTE FUNCTION social_destination_guard_slot();

CREATE TRIGGER social_upload_destination_guard
BEFORE INSERT OR UPDATE OF scheduled_at, status, platform ON twitch_clips_upload_queue
FOR EACH ROW EXECUTE FUNCTION social_destination_guard_slot();

DO $$
DECLARE
    runtime_role TEXT;
BEGIN
    FOR runtime_role IN SELECT rolname FROM pg_roles WHERE rolname IN ('twitchbot', 'twitchdash', 'twitchlegacy') LOOP
        EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON social_publication_reservations TO %I', runtime_role);
        EXECUTE format('GRANT USAGE, SELECT ON SEQUENCE social_publication_reservations_id_seq TO %I', runtime_role);
    END LOOP;
END;
$$;
