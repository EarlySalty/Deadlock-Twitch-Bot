ALTER TABLE public.twitch_clips_social_media
    ADD COLUMN IF NOT EXISTS vod_id text,
    ADD COLUMN IF NOT EXISTS vod_offset_s integer;

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.twitch_clips_social_media TO twitchbot;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.twitch_clips_social_media TO twitchdash;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'deadlock') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.twitch_clips_social_media TO deadlock;
    END IF;
END $$;
