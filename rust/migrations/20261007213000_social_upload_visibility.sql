ALTER TABLE twitch_clips_upload_queue
    ADD COLUMN IF NOT EXISTS youtube_visibility TEXT
    CHECK (youtube_visibility IN ('public', 'private', 'unlisted'));
