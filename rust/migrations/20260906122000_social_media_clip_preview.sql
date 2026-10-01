ALTER TABLE public.twitch_clips_social_media
    ADD COLUMN IF NOT EXISTS preview_path       TEXT,
    ADD COLUMN IF NOT EXISTS preview_status     TEXT,
    ADD COLUMN IF NOT EXISTS preview_error      TEXT,
    ADD COLUMN IF NOT EXISTS preview_updated_at TIMESTAMPTZ;
