CREATE TABLE twitch_private_vod_settings (
    owner_id TEXT PRIMARY KEY CHECK (owner_id ~ '^[1-9][0-9]*$'),
    enabled BOOLEAN NOT NULL DEFAULT false
);

CREATE TABLE twitch_private_vod_jobs (
    id TEXT PRIMARY KEY CHECK (id ~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'),
    owner_id TEXT NOT NULL CHECK (owner_id ~ '^[1-9][0-9]*$'),
    video_id TEXT NOT NULL CHECK (video_id ~ '^[1-9][0-9]*$'),
    start_seconds INTEGER NOT NULL CHECK (start_seconds >= 0),
    end_seconds INTEGER NOT NULL CHECK (end_seconds > start_seconds AND end_seconds - start_seconds BETWEEN 60 AND 600 AND end_seconds <= 172800),
    status TEXT NOT NULL DEFAULT 'queued' CHECK (status IN ('queued', 'running', 'ready', 'failed', 'expired')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL DEFAULT now() + INTERVAL '24 hours',
    started_at TIMESTAMPTZ,
    file_bytes BIGINT CHECK (file_bytes BETWEEN 1 AND 67108864),
    error_code TEXT
);
CREATE INDEX twitch_private_vod_jobs_owner_created ON twitch_private_vod_jobs (owner_id, created_at DESC);
CREATE INDEX twitch_private_vod_jobs_expiry ON twitch_private_vod_jobs (expires_at);
CREATE UNIQUE INDEX twitch_private_vod_jobs_owner_active ON twitch_private_vod_jobs (owner_id) WHERE status IN ('queued', 'running');

CREATE FUNCTION public.twitch_apply_private_vod_roles()
RETURNS void
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
DECLARE role_name TEXT;
BEGIN
    REVOKE ALL ON public.twitch_private_vod_settings, public.twitch_private_vod_jobs FROM PUBLIC;
    FOREACH role_name IN ARRAY ARRAY['twitchbot','twitchdash','twitchlegacy','twitchcollector','twitchcontest','twitchanalysis','twitchaudit'] LOOP
        IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = role_name) THEN
            EXECUTE format('REVOKE ALL ON public.twitch_private_vod_settings, public.twitch_private_vod_jobs FROM %I', role_name);
        END IF;
    END LOOP;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.twitch_private_vod_settings, public.twitch_private_vod_jobs TO twitchdash;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT ON public.twitch_private_vod_settings TO twitchbot;
        GRANT SELECT, INSERT ON public.twitch_private_vod_jobs TO twitchbot;
    END IF;
END $$;
REVOKE ALL ON FUNCTION public.twitch_apply_private_vod_roles() FROM PUBLIC;
SELECT public.twitch_apply_private_vod_roles();
