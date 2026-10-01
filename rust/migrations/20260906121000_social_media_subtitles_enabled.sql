ALTER TABLE public.social_media_streamer_settings
    ADD COLUMN IF NOT EXISTS subtitles_enabled BOOLEAN NOT NULL DEFAULT TRUE;
