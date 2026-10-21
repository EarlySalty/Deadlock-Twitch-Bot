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

WITH overdue AS (
    SELECT id, platform, scheduled_at,
           row_number() OVER (PARTITION BY platform ORDER BY scheduled_at NULLS FIRST, id) AS slot
    FROM twitch_clips_upload_queue
    WHERE platform IN ('tiktok', 'youtube') AND produced_video_id IS NULL
      AND status = 'pending' AND (scheduled_at IS NULL OR scheduled_at <= NOW())
),
taken AS (
    SELECT DISTINCT platform, (scheduled_at AT TIME ZONE 'UTC')::date AS day
    FROM twitch_clips_upload_queue
    WHERE platform IN ('tiktok', 'youtube') AND produced_video_id IS NULL
      AND status = 'pending' AND scheduled_at > NOW()
),
free AS (
    SELECT platform, day, row_number() OVER (PARTITION BY platform ORDER BY day) AS slot
    FROM (
        SELECT p.platform, (NOW() AT TIME ZONE 'UTC')::date + offs AS day
        FROM (VALUES ('tiktok'), ('youtube')) AS p(platform)
        CROSS JOIN generate_series(1, 730) AS offs
    ) candidates
    WHERE NOT EXISTS (
        SELECT 1 FROM taken WHERE taken.platform = candidates.platform AND taken.day = candidates.day
    )
)
UPDATE twitch_clips_upload_queue q
SET scheduled_at = (free.day + COALESCE((overdue.scheduled_at AT TIME ZONE 'UTC')::time, TIME '16:00'))
        AT TIME ZONE 'UTC'
FROM overdue
JOIN free ON free.platform = overdue.platform AND free.slot = overdue.slot
WHERE q.id = overdue.id;
