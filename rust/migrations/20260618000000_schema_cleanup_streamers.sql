CREATE TABLE IF NOT EXISTS public.twitch_exclusions (
    twitch_user_id   TEXT PRIMARY KEY,
    kind             TEXT NOT NULL CHECK (kind IN ('opt_out', 'banned')),
    reason           TEXT,
    excluded_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    reactivated_at   TIMESTAMPTZ
);

INSERT INTO public.twitch_exclusions (twitch_user_id, kind, excluded_at)
SELECT twitch_user_id, 'opt_out', now()
FROM public.twitch_streamers
WHERE twitch_login IN ('fr4gm1nt', 'snaqeu')
ON CONFLICT DO NOTHING;

INSERT INTO public.twitch_exclusions (twitch_user_id, kind, excluded_at)
SELECT twitch_user_id, 'banned', now()
FROM public.twitch_streamers
WHERE twitch_login = 'skifahrertv'
ON CONFLICT DO NOTHING;

DROP TRIGGER IF EXISTS trg_twitch_streamers_sync_identity ON public.twitch_streamers;

CREATE OR REPLACE FUNCTION public.sync_twitch_streamer_identity_from_streamers() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
        BEGIN
            IF COALESCE(NEW.twitch_user_id, '') <> '' THEN
                INSERT INTO twitch_streamer_identities (
                    twitch_user_id,
                    twitch_login,
                    created_at,
                    updated_at
                ) VALUES (
                    NEW.twitch_user_id,
                    LOWER(NEW.twitch_login),
                    CURRENT_TIMESTAMP::text,
                    CURRENT_TIMESTAMP::text
                )
                ON CONFLICT (twitch_user_id) DO UPDATE SET
                    twitch_login = EXCLUDED.twitch_login,
                    updated_at = CURRENT_TIMESTAMP::text;
            END IF;
            RETURN NEW;
        END;
        $$;

ALTER TABLE public.twitch_streamers DROP COLUMN IF EXISTS is_monitored_only;
ALTER TABLE public.twitch_streamers DROP COLUMN IF EXISTS discord_user_id;
ALTER TABLE public.twitch_streamers DROP COLUMN IF EXISTS discord_display_name;
ALTER TABLE public.twitch_streamers DROP COLUMN IF EXISTS is_on_discord;
ALTER TABLE public.twitch_streamers DROP COLUMN IF EXISTS archived_at;

CREATE TRIGGER trg_twitch_streamers_sync_identity
AFTER INSERT OR UPDATE OF twitch_login, twitch_user_id ON public.twitch_streamers
FOR EACH ROW EXECUTE FUNCTION public.sync_twitch_streamer_identity_from_streamers();
