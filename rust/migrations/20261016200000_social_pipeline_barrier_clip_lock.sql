CREATE OR REPLACE FUNCTION social_pipeline_queue_barrier() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    owner_id TEXT;
BEGIN
    IF NOT social_publication_open(NEW.status) THEN
        RETURN NEW;
    END IF;
    IF TG_OP = 'UPDATE' AND social_publication_open(OLD.status)
        AND OLD.clip_id IS NOT DISTINCT FROM NEW.clip_id
        AND OLD.platform IS NOT DISTINCT FROM NEW.platform
        AND (OLD.produced_video_id IS NULL) = (NEW.produced_video_id IS NULL) THEN
        RETURN NEW;
    END IF;
    owner_id := social_clip_pipeline_owner(NEW.clip_id);
    IF owner_id IS NOT NULL THEN
        PERFORM pg_advisory_xact_lock(hashtextextended('social_autopilot:' || owner_id, 0));
    ELSE
        RAISE WARNING 'social pipeline barrier: no owner for clip %, platform %', NEW.clip_id, NEW.platform;
    END IF;
    PERFORM pg_advisory_xact_lock(hashtextextended('social_clip_publication:' || COALESCE(NEW.clip_id::TEXT, ''), 0));
    IF NEW.produced_video_id IS NULL AND social_pipeline_legacy_closed(owner_id) THEN
        RAISE EXCEPTION 'Dieser Kanal veröffentlicht nur noch über den Autopiloten'
            USING ERRCODE = '23514';
    END IF;
    IF EXISTS (
        SELECT 1 FROM twitch_clips_upload_queue q
        WHERE q.clip_id = NEW.clip_id AND q.platform = NEW.platform
        AND q.id IS DISTINCT FROM NEW.id
        AND (q.produced_video_id IS NULL) <> (NEW.produced_video_id IS NULL)
        AND social_publication_open(q.status)
    ) THEN
        RAISE EXCEPTION 'Für diesen Clip läuft auf dieser Plattform schon eine Veröffentlichung'
            USING ERRCODE = '23505';
    END IF;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION social_pipeline_approval_barrier() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    owner_id TEXT;
BEGIN
    IF NEW.produced_video_id IS NOT NULL OR NEW.state IS DISTINCT FROM 'approved' THEN
        RETURN NEW;
    END IF;
    IF TG_OP = 'UPDATE' AND OLD.state = 'approved'
        AND OLD.approved_platforms IS NOT DISTINCT FROM NEW.approved_platforms THEN
        RETURN NEW;
    END IF;
    owner_id := social_clip_pipeline_owner(NEW.clip_db_id);
    IF owner_id IS NOT NULL THEN
        PERFORM pg_advisory_xact_lock(hashtextextended('social_autopilot:' || owner_id, 0));
    ELSE
        RAISE WARNING 'social pipeline barrier: no owner for approved clip %', NEW.clip_db_id;
    END IF;
    IF social_pipeline_legacy_closed(owner_id) THEN
        RAISE EXCEPTION 'Dieser Kanal veröffentlicht nur noch über den Autopiloten'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;
