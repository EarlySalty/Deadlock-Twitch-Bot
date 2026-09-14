CREATE TABLE IF NOT EXISTS public.twitch_clip_merkmale (
    id bigserial PRIMARY KEY,
    clip_id text NOT NULL,
    streamer_twitch_id text,
    views integer,
    dauer_s double precision,
    ersteller text,
    signal text NOT NULL,
    offset_vom_ende_s double precision,
    wert double precision,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_twitch_clip_merkmale_clip_id
    ON public.twitch_clip_merkmale USING btree (clip_id);

CREATE INDEX IF NOT EXISTS idx_twitch_clip_merkmale_signal
    ON public.twitch_clip_merkmale USING btree (signal);

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.twitch_clip_merkmale TO twitchbot;
        GRANT USAGE, SELECT ON SEQUENCE public.twitch_clip_merkmale_id_seq TO twitchbot;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.twitch_clip_merkmale TO twitchdash;
        GRANT USAGE, SELECT ON SEQUENCE public.twitch_clip_merkmale_id_seq TO twitchdash;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'deadlock') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.twitch_clip_merkmale TO deadlock;
        GRANT USAGE, SELECT ON SEQUENCE public.twitch_clip_merkmale_id_seq TO deadlock;
    END IF;
END $$;
