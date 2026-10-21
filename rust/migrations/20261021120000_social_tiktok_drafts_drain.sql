UPDATE twitch_clips_upload_queue
SET status = 'pending', last_error = NULL, last_attempt_at = NULL, next_retry_at = NULL
WHERE platform = 'tiktok' AND produced_video_id IS NULL AND tiktok_publish_id IS NULL
  AND status = 'waiting_tiktok_approval';

UPDATE twitch_clips_upload_queue
SET status = 'pending', attempts = 0, last_error = NULL, last_attempt_at = NULL,
    next_retry_at = NULL, outcome_reason = NULL, completed_at = NULL
WHERE platform = 'tiktok' AND produced_video_id IS NULL AND tiktok_publish_id IS NULL
  AND status = 'failed' AND external_id IS NULL
  AND (last_error LIKE '%unaudited_client%'
       OR last_error LIKE 'validation failed: Bitte öffne die TikTok-Freigabe%');

UPDATE twitch_clips_upload_queue
SET status = 'pending', attempts = 0, last_error = NULL, last_attempt_at = NULL,
    next_retry_at = NULL, outcome_reason = NULL, completed_at = NULL
WHERE platform = 'youtube' AND produced_video_id IS NULL AND status = 'failed'
  AND external_id IS NULL AND last_error LIKE '%(HTTP 410)%';

WITH legacy AS (
    SELECT q.id, q.platform, q.scheduled_at, q.status, c.twitch_user_id AS owner
    FROM twitch_clips_upload_queue q
    JOIN twitch_clips_social_media c ON c.id = q.clip_id
    WHERE q.platform IN ('tiktok', 'youtube') AND q.produced_video_id IS NULL
      AND q.status = 'pending'
),
overdue AS (
    SELECT id, platform, owner, scheduled_at,
           row_number() OVER (PARTITION BY owner, platform ORDER BY scheduled_at NULLS FIRST, id) AS slot
    FROM legacy
    WHERE scheduled_at IS NULL OR scheduled_at <= NOW()
),
taken AS (
    SELECT DISTINCT owner, platform, (scheduled_at AT TIME ZONE 'UTC')::date AS day
    FROM legacy
    WHERE scheduled_at > NOW()
),
free AS (
    SELECT owner, platform, day,
           row_number() OVER (PARTITION BY owner, platform ORDER BY day) AS slot
    FROM (
        SELECT o.owner, o.platform, (NOW() AT TIME ZONE 'UTC')::date + offs AS day
        FROM (SELECT DISTINCT owner, platform FROM overdue) AS o
        CROSS JOIN generate_series(1, 730) AS offs
    ) candidates
    WHERE NOT EXISTS (
        SELECT 1 FROM taken
        WHERE taken.owner = candidates.owner AND taken.platform = candidates.platform
          AND taken.day = candidates.day
    )
)
UPDATE twitch_clips_upload_queue q
SET scheduled_at = (free.day + COALESCE((overdue.scheduled_at AT TIME ZONE 'UTC')::time, TIME '16:00'))
        AT TIME ZONE 'UTC'
FROM overdue
JOIN free ON free.owner = overdue.owner AND free.platform = overdue.platform
         AND free.slot = overdue.slot
WHERE q.id = overdue.id;
