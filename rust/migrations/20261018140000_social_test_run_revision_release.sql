CREATE FUNCTION social_video_job(job BIGINT, meta JSONB) RETURNS BIGINT LANGUAGE sql IMMUTABLE AS $$
    SELECT COALESCE(job, CASE WHEN meta->>'production_ai_job_id' ~ '^[0-9]{1,18}$' THEN (meta->>'production_ai_job_id')::bigint END)
$$;

CREATE FUNCTION social_video_discarded(job BIGINT, meta JSONB) RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT social_video_discarded(social_video_job(job, meta))
$$;

CREATE OR REPLACE FUNCTION social_clip_video_holds(clip BIGINT) RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT EXISTS(SELECT 1 FROM social_produced_videos v WHERE v.clip_id = clip AND NOT social_video_discarded(v.job_id, v.metadata))
$$;

CREATE OR REPLACE FUNCTION social_clip_result_history() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_TABLE_NAME = 'social_produced_videos' THEN
        IF social_video_discarded(NEW.job_id, NEW.metadata) THEN
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

CREATE OR REPLACE FUNCTION social_clip_capture_history(c twitch_clips_social_media) RETURNS VOID LANGUAGE plpgsql AS $$
BEGIN
    IF c.source_kind = 'manual_upload' OR c.game_id IS DISTINCT FROM '2132205352' OR c.pool_managed_at IS NULL THEN
        RETURN;
    END IF;
    IF NOT c.pool_legacy_reserved AND c.converted_file_path IS NULL
        AND NOT COALESCE(c.uploaded_youtube, FALSE) AND NOT COALESCE(c.uploaded_tiktok, FALSE) AND NOT COALESCE(c.uploaded_instagram, FALSE)
        AND NOT EXISTS (SELECT 1 FROM social_clip_pool WHERE twitch_user_id = c.twitch_user_id AND clip_id = c.clip_id)
        AND NOT EXISTS (SELECT 1 FROM social_produced_videos v WHERE v.clip_id = c.id
            AND v.status IN ('review', 'approved', 'scheduled', 'published', 'expired', 'rejected')
            AND NOT social_video_discarded(v.job_id, v.metadata))
        AND NOT EXISTS (SELECT 1 FROM twitch_clips_upload_queue q WHERE q.clip_id = c.id AND q.status = 'completed') THEN
        RETURN;
    END IF;
    INSERT INTO social_clip_pool (twitch_user_id, clip_id, clip_db_id, source_created_at, game_id, source_metadata,
        processed_at, published_at, legacy_reserved, first_selected_at)
    VALUES (c.twitch_user_id, c.clip_id, c.id, c.created_at, c.game_id,
        jsonb_build_object('clip_url', c.clip_url, 'clip_title', c.clip_title, 'thumbnail_url', c.clip_thumbnail_url,
            'duration_seconds', c.duration_seconds, 'streamer_login', c.streamer_login),
        CASE WHEN c.converted_file_path IS NOT NULL OR EXISTS (SELECT 1 FROM social_produced_videos v WHERE v.clip_id = c.id
            AND v.status IN ('review', 'approved', 'scheduled', 'published', 'expired', 'rejected')
            AND NOT social_video_discarded(v.job_id, v.metadata)) THEN NOW() END,
        CASE WHEN COALESCE(c.uploaded_youtube, FALSE) OR COALESCE(c.uploaded_tiktok, FALSE) OR COALESCE(c.uploaded_instagram, FALSE)
            OR EXISTS (SELECT 1 FROM twitch_clips_upload_queue q WHERE q.clip_id = c.id AND q.status = 'completed')
            OR EXISTS (SELECT 1 FROM social_produced_videos v WHERE v.clip_id = c.id AND v.status = 'published'
                AND NOT social_video_discarded(v.job_id, v.metadata)) THEN NOW() END,
        c.pool_legacy_reserved,
        (SELECT MIN(q.created_at) FROM twitch_clips_upload_queue q WHERE q.clip_id = c.id))
    ON CONFLICT (twitch_user_id, clip_id) DO UPDATE SET
        clip_db_id = EXCLUDED.clip_db_id,
        processed_at = COALESCE(social_clip_pool.processed_at, EXCLUDED.processed_at),
        published_at = COALESCE(social_clip_pool.published_at, EXCLUDED.published_at),
        legacy_reserved = social_clip_pool.legacy_reserved OR EXCLUDED.legacy_reserved;
END;
$$;

CREATE FUNCTION social_discarded_video_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.status IN ('review', 'approved', 'scheduled') AND social_video_discarded(NEW.job_id, NEW.metadata) THEN
        NEW.status := 'rejected';
        NEW.needs_review := TRUE;
        NEW.metadata := NEW.metadata - 'revision_pending';
        NEW.expires_at := LEAST(COALESCE(NEW.expires_at, clock_timestamp()), clock_timestamp());
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER social_discarded_video_guard BEFORE INSERT OR UPDATE ON social_produced_videos
    FOR EACH ROW EXECUTE FUNCTION social_discarded_video_guard();
