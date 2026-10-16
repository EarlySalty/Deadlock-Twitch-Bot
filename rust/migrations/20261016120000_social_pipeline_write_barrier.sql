CREATE FUNCTION social_clip_pipeline_owner(clip BIGINT) RETURNS TEXT
LANGUAGE sql STABLE SECURITY DEFINER SET search_path FROM CURRENT AS $$
    SELECT COALESCE(c.twitch_user_id, (
        SELECT s.twitch_user_id FROM twitch_streamers s
        WHERE lower(s.twitch_login) = lower(c.streamer_login)
        ORDER BY s.twitch_user_id LIMIT 1))
    FROM twitch_clips_social_media c WHERE c.id = clip
$$;

CREATE FUNCTION social_pipeline_legacy_closed(owner_id TEXT) RETURNS BOOLEAN
LANGUAGE sql STABLE SECURITY DEFINER SET search_path FROM CURRENT AS $$
    SELECT EXISTS (
        SELECT 1 FROM social_pipeline_cutover
        WHERE twitch_user_id = owner_id AND state IN ('unified', 'paused'))
$$;

CREATE FUNCTION social_pipeline_legacy_paused(owner_id TEXT) RETURNS BOOLEAN
LANGUAGE sql STABLE SECURITY DEFINER SET search_path FROM CURRENT AS $$
    SELECT EXISTS (
        SELECT 1 FROM social_pipeline_cutover
        WHERE twitch_user_id = owner_id AND state = 'paused')
$$;

CREATE FUNCTION social_publication_open(status TEXT) RETURNS BOOLEAN
LANGUAGE sql IMMUTABLE AS $$
    SELECT COALESCE(status, '') NOT IN
        ('completed', 'failed', 'cancelled', 'autopilot_discarded', 'autopilot_failed')
$$;

CREATE FUNCTION social_pipeline_queue_barrier() RETURNS trigger
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
    END IF;
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

CREATE TRIGGER social_pipeline_queue_barrier
BEFORE INSERT OR UPDATE OF status, clip_id, platform, produced_video_id ON twitch_clips_upload_queue
FOR EACH ROW EXECUTE FUNCTION social_pipeline_queue_barrier();

CREATE FUNCTION social_pipeline_approval_barrier() RETURNS trigger
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
    END IF;
    IF social_pipeline_legacy_closed(owner_id) THEN
        RAISE EXCEPTION 'Dieser Kanal veröffentlicht nur noch über den Autopiloten'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER social_pipeline_approval_barrier
BEFORE INSERT OR UPDATE OF state, approved_platforms ON social_media_clip_approval
FOR EACH ROW EXECUTE FUNCTION social_pipeline_approval_barrier();

ALTER TABLE social_pipeline_cutover ADD COLUMN streamer_paused BOOLEAN;

CREATE FUNCTION social_pipeline_hold_streamer_pause() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path FROM CURRENT AS $$
BEGIN
    IF NOT social_pipeline_legacy_paused(NEW.twitch_user_id) THEN
        RETURN NEW;
    END IF;
    IF (TG_OP = 'UPDATE' AND NEW.paused IS DISTINCT FROM OLD.paused)
        OR (TG_OP = 'INSERT' AND NOT EXISTS (
            SELECT 1 FROM social_media_autopilot_settings s
            WHERE s.twitch_user_id = NEW.twitch_user_id)) THEN
        UPDATE social_pipeline_cutover SET streamer_paused = NEW.paused, updated_at = NOW()
        WHERE twitch_user_id = NEW.twitch_user_id;
    END IF;
    NEW.paused := TRUE;
    RETURN NEW;
END;
$$;

CREATE TRIGGER social_pipeline_hold_streamer_pause
BEFORE INSERT OR UPDATE OF paused ON social_media_autopilot_settings
FOR EACH ROW EXECUTE FUNCTION social_pipeline_hold_streamer_pause();
