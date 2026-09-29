CREATE TABLE IF NOT EXISTS public.twitch_clip_command_events (
    clip_id text PRIMARY KEY,
    streamer_login text NOT NULL,
    twitch_user_id text NOT NULL,
    requested_at timestamp with time zone NOT NULL,
    vod_id text,
    vod_offset_s integer,
    resolution_status text NOT NULL,
    CHECK ((vod_id IS NULL) = (vod_offset_s IS NULL))
);

CREATE TABLE IF NOT EXISTS public.twitch_clip_context_runs (
    clip_id text PRIMARY KEY,
    clip_url text NOT NULL,
    streamer_login text NOT NULL,
    vod_id text NOT NULL,
    moment_offset_s integer NOT NULL CHECK (moment_offset_s >= 0),
    window_start_s integer NOT NULL CHECK (window_start_s >= 0),
    window_end_s integer NOT NULL CHECK (window_end_s > window_start_s),
    clip_duration_s double precision NOT NULL,
    peak_s integer,
    recommended_start_s integer,
    recommended_end_s integer,
    stt_status text NOT NULL,
    visual_status text NOT NULL,
    analyzed_at timestamp with time zone NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS public.twitch_clip_context_seconds (
    clip_id text NOT NULL REFERENCES public.twitch_clip_context_runs (clip_id) ON DELETE CASCADE,
    vod_second integer NOT NULL CHECK (vod_second >= 0),
    lufs double precision,
    peak_dbfs double precision,
    speech text,
    laughter boolean,
    exclamation boolean,
    kill_feed text,
    souls integer,
    soul_jump integer,
    objective boolean,
    death_screen boolean,
    scene_change boolean,
    chat_messages integer NOT NULL DEFAULT 0,
    ocr_sampled boolean NOT NULL DEFAULT false,
    PRIMARY KEY (clip_id, vod_second)
);

CREATE TABLE IF NOT EXISTS public.twitch_clip_cut_templates (
    name text PRIMARY KEY,
    weights jsonb NOT NULL,
    lead_seconds integer NOT NULL CHECK (lead_seconds > 0),
    trail_seconds integer NOT NULL CHECK (trail_seconds > 0),
    sample_count integer NOT NULL CHECK (sample_count >= 0),
    updated_at timestamp with time zone NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS twitch_clip_context_runs_vod_idx
    ON public.twitch_clip_context_runs (vod_id, moment_offset_s);

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON
            public.twitch_clip_command_events,
            public.twitch_clip_context_runs,
            public.twitch_clip_context_seconds,
            public.twitch_clip_cut_templates
        TO twitchbot;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT ON
            public.twitch_clip_command_events,
            public.twitch_clip_context_runs,
            public.twitch_clip_context_seconds,
            public.twitch_clip_cut_templates
        TO twitchdash;
    END IF;
END $$;
