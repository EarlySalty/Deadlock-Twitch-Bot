CREATE FUNCTION social_video_discarded(job BIGINT) RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT EXISTS(SELECT 1 FROM social_production_jobs j WHERE j.id = job AND j.source = 'test' AND j.status = 'cancelled')
$$;

CREATE FUNCTION social_clip_video_holds(clip BIGINT) RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT EXISTS(SELECT 1 FROM social_produced_videos v WHERE v.clip_id = clip AND NOT social_video_discarded(v.job_id))
$$;

CREATE OR REPLACE FUNCTION social_clip_result_history() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_TABLE_NAME = 'social_produced_videos' THEN
        IF social_video_discarded(NEW.job_id) THEN
            RETURN NEW;
        END IF;
        UPDATE social_clip_pool SET processed_at = COALESCE(processed_at, NOW()),
            published_at = CASE WHEN NEW.status = 'published' THEN COALESCE(published_at, NOW()) ELSE published_at END
        WHERE clip_db_id = NEW.clip_id AND NEW.status IN ('review', 'approved', 'scheduled', 'published', 'expired', 'rejected');
    ELSE
        UPDATE social_clip_pool SET published_at = COALESCE(published_at, NOW()) WHERE clip_db_id = NEW.clip_id AND NEW.status = 'completed';
    END IF;
    RETURN NEW;
END;
$$;
