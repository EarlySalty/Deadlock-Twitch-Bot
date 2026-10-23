CREATE TABLE IF NOT EXISTS public.social_media_access_requests (
    twitch_user_id TEXT PRIMARY KEY CHECK (twitch_user_id ~ '^[0-9]+$'),
    twitch_login TEXT NOT NULL,
    requested_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.social_media_access_requests TO twitchdash;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.social_media_access_requests TO twitchbot;
    END IF;
END
$$;
