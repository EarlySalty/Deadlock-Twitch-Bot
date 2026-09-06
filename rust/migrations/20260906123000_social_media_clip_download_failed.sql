ALTER TABLE public.twitch_clips_social_media
    ADD COLUMN IF NOT EXISTS download_failed_at TIMESTAMPTZ;
