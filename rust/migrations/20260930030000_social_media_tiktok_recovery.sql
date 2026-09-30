ALTER TABLE twitch_clips_upload_queue
    ADD COLUMN tiktok_publish_id TEXT;

UPDATE twitch_clips_upload_queue q
SET tiktok_publish_id = c.tiktok_video_id
FROM twitch_clips_social_media c
WHERE c.id = q.clip_id
  AND q.platform = 'tiktok'
  AND q.status IN ('inbox', 'inbox_pending');

UPDATE twitch_clips_upload_queue
SET status = 'inbox_pending',
    last_error = 'Die vorherige TikTok-Übertragung wurde unterbrochen. Bitte prüfe dein TikTok-Postfach; ein zweiter Upload bleibt gesperrt.'
WHERE platform = 'tiktok' AND status = 'processing';
