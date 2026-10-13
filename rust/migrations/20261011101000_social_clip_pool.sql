CREATE TABLE social_clip_pool_owners (
    twitch_user_id TEXT PRIMARY KEY REFERENCES twitch_streamers(twitch_user_id) ON DELETE CASCADE,
    managed_since TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);

ALTER TABLE twitch_clips_social_media ADD COLUMN pool_managed_at TIMESTAMPTZ;
ALTER TABLE twitch_clips_social_media ADD COLUMN pool_legacy_reserved BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE twitch_clips_social_media ADD COLUMN pool_legacy_prerequisite BOOLEAN NOT NULL DEFAULT FALSE;

CREATE TABLE social_clip_pool (
    twitch_user_id TEXT NOT NULL REFERENCES social_clip_pool_owners(twitch_user_id) ON DELETE CASCADE,
    clip_id TEXT NOT NULL,
    clip_db_id BIGINT REFERENCES twitch_clips_social_media(id) ON DELETE SET NULL,
    source_created_at TIMESTAMPTZ NOT NULL,
    game_id TEXT NOT NULL CHECK (game_id = '2132205352'),
    source_metadata JSONB NOT NULL,
    selected_at TIMESTAMPTZ,
    first_selected_at TIMESTAMPTZ,
    processed_at TIMESTAMPTZ,
    published_at TIMESTAMPTZ,
    legacy_reserved BOOLEAN NOT NULL DEFAULT FALSE,
    imported_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (twitch_user_id, clip_id)
);
CREATE INDEX social_clip_pool_chronology ON social_clip_pool (twitch_user_id, source_created_at, clip_id);

CREATE TABLE social_clip_import_jobs (
    id BIGSERIAL PRIMARY KEY,
    twitch_user_id TEXT NOT NULL REFERENCES social_clip_pool_owners(twitch_user_id) ON DELETE CASCADE,
    streamer_login TEXT NOT NULL,
    clip_date_filter JSONB NOT NULL,
    timezone TEXT NOT NULL,
    started_at TIMESTAMPTZ NOT NULL,
    ended_at TIMESTAMPTZ NOT NULL,
    pending_windows JSONB NOT NULL,
    active_window JSONB,
    incomplete_windows JSONB NOT NULL DEFAULT '[]',
    cursor TEXT,
    raw_fetched BIGINT NOT NULL DEFAULT 0,
    eligible_fetched BIGINT NOT NULL DEFAULT 0,
    persisted_count BIGINT NOT NULL DEFAULT 0,
    completed_windows BIGINT NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'queued' CHECK (status IN ('queued', 'running', 'retry', 'error', 'incomplete', 'completed')),
    lease_generation BIGINT NOT NULL DEFAULT 0,
    lease_until TIMESTAMPTZ,
    attempts INTEGER NOT NULL DEFAULT 0,
    total_attempts BIGINT NOT NULL DEFAULT 0,
    next_retry_at TIMESTAMPTZ,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    CHECK (started_at <= ended_at)
);
CREATE UNIQUE INDEX social_clip_import_active_owner ON social_clip_import_jobs (twitch_user_id) WHERE status IN ('queued', 'running', 'retry');
CREATE INDEX social_clip_import_pending ON social_clip_import_jobs (next_retry_at, id) WHERE status IN ('queued', 'running', 'retry');

CREATE FUNCTION social_clip_capture_history(c twitch_clips_social_media) RETURNS VOID LANGUAGE plpgsql AS $$
BEGIN
    IF c.source_kind = 'manual_upload' OR c.game_id IS DISTINCT FROM '2132205352' OR c.pool_managed_at IS NULL THEN
        RETURN;
    END IF;
    IF NOT c.pool_legacy_reserved AND c.converted_file_path IS NULL
        AND NOT COALESCE(c.uploaded_youtube, FALSE) AND NOT COALESCE(c.uploaded_tiktok, FALSE) AND NOT COALESCE(c.uploaded_instagram, FALSE)
        AND NOT EXISTS (SELECT 1 FROM social_clip_pool WHERE twitch_user_id = c.twitch_user_id AND clip_id = c.clip_id)
        AND NOT EXISTS (SELECT 1 FROM social_produced_videos v WHERE v.clip_id = c.id AND v.status IN ('review', 'approved', 'scheduled', 'published', 'expired', 'rejected'))
        AND NOT EXISTS (SELECT 1 FROM twitch_clips_upload_queue q WHERE q.clip_id = c.id AND q.status = 'completed') THEN
        RETURN;
    END IF;
    INSERT INTO social_clip_pool (twitch_user_id, clip_id, clip_db_id, source_created_at, game_id, source_metadata,
        processed_at, published_at, legacy_reserved, first_selected_at)
    VALUES (c.twitch_user_id, c.clip_id, c.id, c.created_at, c.game_id,
        jsonb_build_object('clip_url', c.clip_url, 'clip_title', c.clip_title, 'thumbnail_url', c.clip_thumbnail_url,
            'duration_seconds', c.duration_seconds, 'streamer_login', c.streamer_login),
        CASE WHEN c.converted_file_path IS NOT NULL OR EXISTS (SELECT 1 FROM social_produced_videos v WHERE v.clip_id = c.id
            AND v.status IN ('review', 'approved', 'scheduled', 'published', 'expired', 'rejected')) THEN NOW() END,
        CASE WHEN COALESCE(c.uploaded_youtube, FALSE) OR COALESCE(c.uploaded_tiktok, FALSE) OR COALESCE(c.uploaded_instagram, FALSE)
            OR EXISTS (SELECT 1 FROM twitch_clips_upload_queue q WHERE q.clip_id = c.id AND q.status = 'completed')
            OR EXISTS (SELECT 1 FROM social_produced_videos v WHERE v.clip_id = c.id AND v.status = 'published') THEN NOW() END,
        c.pool_legacy_reserved,
        (SELECT MIN(q.created_at) FROM twitch_clips_upload_queue q WHERE q.clip_id = c.id))
    ON CONFLICT (twitch_user_id, clip_id) DO UPDATE SET
        clip_db_id = EXCLUDED.clip_db_id,
        processed_at = COALESCE(social_clip_pool.processed_at, EXCLUDED.processed_at),
        published_at = COALESCE(social_clip_pool.published_at, EXCLUDED.published_at),
        legacy_reserved = social_clip_pool.legacy_reserved OR EXCLUDED.legacy_reserved;
END;
$$;

CREATE FUNCTION social_clip_manage_owner(owner_id TEXT) RETURNS VOID LANGUAGE plpgsql AS $$
DECLARE
    managed_at TIMESTAMPTZ;
BEGIN
    PERFORM pg_advisory_xact_lock(hashtextextended('social_autopilot:' || owner_id, 0));
    INSERT INTO social_clip_pool_owners (twitch_user_id) VALUES (owner_id) ON CONFLICT DO NOTHING;
    SELECT managed_since INTO managed_at FROM social_clip_pool_owners WHERE twitch_user_id = owner_id;
    UPDATE twitch_clips_social_media c SET pool_managed_at = managed_at,
        pool_legacy_reserved = EXISTS (SELECT 1 FROM twitch_clips_upload_queue q WHERE q.clip_id = c.id AND q.produced_video_id IS NULL),
        pool_legacy_prerequisite = EXISTS (SELECT 1 FROM twitch_clips_upload_queue q WHERE q.clip_id = c.id AND q.produced_video_id IS NULL
            AND q.status IN ('pending', 'processing', 'waiting_connection', 'waiting_tiktok_approval', 'waiting_schedule'))
    WHERE c.twitch_user_id = owner_id AND c.source_kind <> 'manual_upload' AND c.pool_managed_at IS NULL;
END;
$$;

CREATE FUNCTION social_clip_source_boundary() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    managed_at TIMESTAMPTZ;
BEGIN
    IF TG_OP = 'DELETE' THEN
        PERFORM social_clip_capture_history(OLD);
        RETURN OLD;
    END IF;
    IF TG_OP = 'INSERT' THEN
        PERFORM pg_advisory_xact_lock(hashtextextended('social_autopilot:' || NEW.twitch_user_id, 0));
        SELECT managed_since INTO managed_at FROM social_clip_pool_owners WHERE twitch_user_id = NEW.twitch_user_id;
        NEW.pool_managed_at := CASE WHEN NEW.source_kind = 'manual_upload' THEN NULL ELSE managed_at END;
        NEW.pool_legacy_reserved := FALSE;
        NEW.pool_legacy_prerequisite := FALSE;
    ELSE
        NEW.pool_managed_at := COALESCE(OLD.pool_managed_at, NEW.pool_managed_at);
        NEW.pool_legacy_reserved := OLD.pool_legacy_reserved OR NEW.pool_legacy_reserved;
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER social_clip_source_boundary BEFORE INSERT OR UPDATE OR DELETE ON twitch_clips_social_media
    FOR EACH ROW EXECUTE FUNCTION social_clip_source_boundary();

CREATE FUNCTION social_clip_source_history() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    PERFORM social_clip_capture_history(NEW);
    RETURN NEW;
END;
$$;
CREATE TRIGGER social_clip_source_history AFTER INSERT OR UPDATE ON twitch_clips_social_media
    FOR EACH ROW EXECUTE FUNCTION social_clip_source_history();

CREATE FUNCTION social_clip_settings_boundary() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.enabled THEN
        PERFORM social_clip_manage_owner(NEW.twitch_user_id);
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER social_clip_settings_boundary AFTER INSERT OR UPDATE ON social_media_autopilot_settings
    FOR EACH ROW EXECUTE FUNCTION social_clip_settings_boundary();
SELECT social_clip_manage_owner(twitch_user_id) FROM social_media_autopilot_settings WHERE enabled;

CREATE FUNCTION social_clip_legacy_write_boundary() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    source_id BIGINT;
    source_row twitch_clips_social_media;
BEGIN
    IF NEW.produced_video_id IS NOT NULL THEN
        RETURN NEW;
    END IF;
    IF TG_TABLE_NAME = 'twitch_clips_upload_queue' THEN
        source_id := NEW.clip_id;
    ELSE
        source_id := NEW.clip_db_id;
    END IF;
    SELECT * INTO source_row FROM twitch_clips_social_media WHERE id = source_id;
    PERFORM pg_advisory_xact_lock(hashtextextended('social_autopilot:' || source_row.twitch_user_id, 0));
    SELECT * INTO source_row FROM twitch_clips_social_media WHERE id = source_id;
    IF source_row.pool_managed_at IS NOT NULL
        AND (TG_TABLE_NAME = 'twitch_clips_upload_queue' OR NOT source_row.pool_legacy_reserved) THEN
        RAISE EXCEPTION 'Dieser Clip wird im Clip-Pool verwaltet' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER social_clip_legacy_queue_boundary BEFORE INSERT ON twitch_clips_upload_queue
    FOR EACH ROW EXECUTE FUNCTION social_clip_legacy_write_boundary();
CREATE TRIGGER social_clip_legacy_approval_boundary BEFORE INSERT OR UPDATE ON social_media_clip_approval
    FOR EACH ROW EXECUTE FUNCTION social_clip_legacy_write_boundary();

CREATE FUNCTION social_clip_result_history() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_TABLE_NAME = 'social_produced_videos' THEN
        UPDATE social_clip_pool SET processed_at = COALESCE(processed_at, NOW()),
            published_at = CASE WHEN NEW.status = 'published' THEN COALESCE(published_at, NOW()) ELSE published_at END
        WHERE clip_db_id = NEW.clip_id AND NEW.status IN ('review', 'approved', 'scheduled', 'published', 'expired', 'rejected');
    ELSE
        UPDATE social_clip_pool SET published_at = COALESCE(published_at, NOW()) WHERE clip_db_id = NEW.clip_id AND NEW.status = 'completed';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER social_clip_produced_history AFTER INSERT OR UPDATE ON social_produced_videos
    FOR EACH ROW EXECUTE FUNCTION social_clip_result_history();
CREATE TRIGGER social_clip_upload_history AFTER INSERT OR UPDATE ON twitch_clips_upload_queue
    FOR EACH ROW EXECUTE FUNCTION social_clip_result_history();

CREATE FUNCTION social_clip_queue_prerequisite() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    source_id BIGINT;
BEGIN
    source_id := CASE WHEN TG_OP = 'DELETE' THEN OLD.clip_id ELSE NEW.clip_id END;
    UPDATE twitch_clips_social_media c SET pool_legacy_prerequisite = EXISTS (
        SELECT 1 FROM twitch_clips_upload_queue q WHERE q.clip_id = c.id AND q.produced_video_id IS NULL
            AND q.status IN ('pending', 'processing', 'waiting_connection', 'waiting_tiktok_approval', 'waiting_schedule'))
    WHERE c.id = source_id AND c.pool_managed_at IS NOT NULL AND c.pool_legacy_reserved;
    RETURN NULL;
END;
$$;
CREATE TRIGGER social_clip_queue_prerequisite AFTER INSERT OR UPDATE OR DELETE ON twitch_clips_upload_queue
    FOR EACH ROW EXECUTE FUNCTION social_clip_queue_prerequisite();

DO $$
DECLARE
    runtime_role TEXT;
BEGIN
    FOR runtime_role IN SELECT rolname FROM pg_roles WHERE rolname IN ('twitchbot', 'twitchdash', 'twitchlegacy') LOOP
        EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON social_clip_pool_owners, social_clip_pool, social_clip_import_jobs TO %I', runtime_role);
        EXECUTE format('GRANT USAGE, SELECT ON SEQUENCE social_clip_import_jobs_id_seq TO %I', runtime_role);
    END LOOP;
END;
$$;
