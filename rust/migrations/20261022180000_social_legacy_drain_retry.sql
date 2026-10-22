CREATE OR REPLACE FUNCTION social_legacy_retry_allowed(clip BIGINT, platform TEXT) RETURNS BOOLEAN
LANGUAGE sql STABLE AS $$
    SELECT NOT EXISTS (
        SELECT 1 FROM social_production_jobs j
        WHERE j.clip_id = clip AND NOT social_job_releases_clip(j.id)
    )
    AND NOT EXISTS (
        SELECT 1 FROM twitch_clips_social_media c
        WHERE c.id = clip
        AND CASE platform
            WHEN 'tiktok' THEN COALESCE(c.uploaded_tiktok, FALSE)
            WHEN 'youtube' THEN COALESCE(c.uploaded_youtube, FALSE)
            WHEN 'instagram' THEN COALESCE(c.uploaded_instagram, FALSE)
            ELSE TRUE
        END
    )
$$;

CREATE OR REPLACE FUNCTION social_pipeline_queue_barrier() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    owner_id TEXT;
    legacy_retry BOOLEAN;
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
    legacy_retry := TG_OP = 'UPDATE' AND OLD.status = 'failed'
        AND OLD.produced_video_id IS NULL AND NEW.produced_video_id IS NULL
        AND OLD.clip_id IS NOT DISTINCT FROM NEW.clip_id
        AND OLD.platform IS NOT DISTINCT FROM NEW.platform
        AND social_legacy_retry_allowed(NEW.clip_id, NEW.platform);
    IF NEW.produced_video_id IS NULL AND social_pipeline_legacy_closed(owner_id) AND NOT legacy_retry THEN
        RAISE EXCEPTION 'Dieser Kanal veröffentlicht nur noch über den Autopiloten'
            USING ERRCODE = '23514';
    END IF;
    IF EXISTS (
        SELECT 1 FROM twitch_clips_upload_queue q
        WHERE q.clip_id = NEW.clip_id AND q.platform = NEW.platform
        AND q.id IS DISTINCT FROM NEW.id
        AND social_publication_open(q.status)
        AND ((q.produced_video_id IS NULL) <> (NEW.produced_video_id IS NULL) OR legacy_retry)
    ) THEN
        RAISE EXCEPTION 'Für diesen Clip läuft auf dieser Plattform schon eine Veröffentlichung'
            USING ERRCODE = '23505';
    END IF;
    RETURN NEW;
END;
$$;

UPDATE twitch_clips_upload_queue
SET status = 'pending', attempts = 0, last_error = NULL, last_attempt_at = NULL,
    next_retry_at = NULL, outcome_reason = NULL, completed_at = NULL, scheduled_at = NULL
WHERE platform = 'tiktok' AND produced_video_id IS NULL AND tiktok_publish_id IS NULL
  AND status = 'failed' AND external_id IS NULL
  AND (last_error LIKE '%unaudited_client%'
       OR last_error LIKE 'validation failed: Bitte öffne die TikTok-Freigabe%')
  AND social_pipeline_legacy_closed(social_clip_pipeline_owner(clip_id))
  AND social_legacy_retry_allowed(clip_id, platform);

UPDATE twitch_clips_upload_queue
SET status = 'pending', attempts = 0, last_error = NULL, last_attempt_at = NULL,
    next_retry_at = NULL, outcome_reason = NULL, completed_at = NULL, scheduled_at = NULL
WHERE platform = 'youtube' AND produced_video_id IS NULL AND status = 'failed'
  AND external_id IS NULL AND last_error LIKE '%(HTTP 410)%'
  AND social_pipeline_legacy_closed(social_clip_pipeline_owner(clip_id))
  AND social_legacy_retry_allowed(clip_id, platform);

WITH legacy AS (
    SELECT q.id, q.platform, q.scheduled_at, q.status, COALESCE(c.twitch_user_id, '') AS owner
    FROM twitch_clips_upload_queue q
    JOIN twitch_clips_social_media c ON c.id = q.clip_id
    WHERE q.platform IN ('tiktok', 'youtube') AND q.produced_video_id IS NULL
      AND q.status IN ('pending', 'waiting_connection')
),
reopened AS (
    SELECT id, platform, owner,
           row_number() OVER (PARTITION BY owner, platform ORDER BY id) AS slot
    FROM legacy
    WHERE status = 'pending' AND scheduled_at IS NULL
),
taken AS (
    SELECT DISTINCT owner, platform, (scheduled_at AT TIME ZONE 'UTC')::date AS day
    FROM legacy
    WHERE scheduled_at IS NOT NULL
),
free AS (
    SELECT owner, platform, day,
           row_number() OVER (PARTITION BY owner, platform ORDER BY day) AS slot
    FROM (
        SELECT r.owner, r.platform, (NOW() AT TIME ZONE 'UTC')::date + offs AS day
        FROM (SELECT DISTINCT owner, platform FROM reopened) AS r
        CROSS JOIN generate_series(1, 730) AS offs
    ) candidates
    WHERE NOT EXISTS (
        SELECT 1 FROM taken
        WHERE taken.owner = candidates.owner AND taken.platform = candidates.platform
          AND taken.day = candidates.day
    )
)
UPDATE twitch_clips_upload_queue q
SET scheduled_at = (free.day + TIME '16:00') AT TIME ZONE 'UTC'
FROM reopened
JOIN free ON free.owner = reopened.owner AND free.platform = reopened.platform
         AND free.slot = reopened.slot
WHERE q.id = reopened.id;
