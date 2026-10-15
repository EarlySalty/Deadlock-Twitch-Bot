CREATE TABLE public.twitch_streamer_ai_policy (
    twitch_user_id text PRIMARY KEY REFERENCES public.twitch_streamer_identities(twitch_user_id) ON DELETE CASCADE,
    guild_id bigint NOT NULL CHECK (guild_id > 0),
    ai_features boolean NOT NULL DEFAULT false
);

INSERT INTO public.twitch_streamer_ai_policy (twitch_user_id, guild_id, ai_features)
SELECT twitch_user_id, 1289721245281292288, true
FROM public.twitch_streamer_identities
WHERE twitch_user_id ~ '^[0-9]{1,20}$';

CREATE FUNCTION public.twitch_streamer_ai_allowed(
    p_twitch_user_id text,
    p_channel_login text DEFAULT NULL
) RETURNS boolean
LANGUAGE sql
STABLE
AS $$
    WITH identity AS (
        SELECT i.twitch_user_id
        FROM public.twitch_streamer_identities i
        WHERE i.twitch_user_id ~ '^[0-9]{1,20}$'
          AND NULLIF(BTRIM(i.twitch_login), '') IS NOT NULL
          AND (
              (NULLIF(BTRIM(p_twitch_user_id), '') IS NOT NULL
               AND i.twitch_user_id = BTRIM(p_twitch_user_id)
               AND (NULLIF(BTRIM(p_channel_login), '') IS NULL
                    OR LOWER(i.twitch_login) = LOWER(BTRIM(p_channel_login))))
              OR (NULLIF(BTRIM(p_twitch_user_id), '') IS NULL
                  AND LOWER(i.twitch_login) = LOWER(BTRIM(p_channel_login)))
          )
    )
    SELECT (SELECT COUNT(*) FROM identity) = 1
       AND EXISTS (
           SELECT 1 FROM identity i
           JOIN public.twitch_streamer_ai_policy p USING (twitch_user_id)
           WHERE p.ai_features
       );
$$;

DO $$
DECLARE role_name text;
BEGIN
    FOREACH role_name IN ARRAY ARRAY['twitchbot','twitchdash','twitchanalysis','twitchlegacy','twitchaudit'] LOOP
        IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = role_name) THEN
            EXECUTE format('GRANT SELECT ON public.twitch_streamer_identities, public.twitch_streamer_ai_policy TO %I', role_name);
        END IF;
    END LOOP;
END $$;
