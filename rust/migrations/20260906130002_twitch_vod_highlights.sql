CREATE TABLE IF NOT EXISTS public.twitch_vod_highlights (
    id bigserial PRIMARY KEY,
    streamer_twitch_id text NOT NULL,
    vod_id text NOT NULL,
    start_s double precision NOT NULL,
    end_s double precision NOT NULL,
    score double precision NOT NULL,
    signale jsonb NOT NULL DEFAULT '{}'::jsonb,
    status text NOT NULL DEFAULT 'neu',
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_vod_highlights_fenster
    ON public.twitch_vod_highlights USING btree (vod_id, start_s, end_s);

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.twitch_vod_highlights TO twitchbot;
        GRANT USAGE, SELECT ON SEQUENCE public.twitch_vod_highlights_id_seq TO twitchbot;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.twitch_vod_highlights TO twitchdash;
        GRANT USAGE, SELECT ON SEQUENCE public.twitch_vod_highlights_id_seq TO twitchdash;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'deadlock') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.twitch_vod_highlights TO deadlock;
        GRANT USAGE, SELECT ON SEQUENCE public.twitch_vod_highlights_id_seq TO deadlock;
    END IF;
END $$;
