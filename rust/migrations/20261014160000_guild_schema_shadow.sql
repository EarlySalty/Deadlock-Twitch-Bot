CREATE TABLE IF NOT EXISTS public.discord_guilds (
    guild_id bigint PRIMARY KEY,
    name text,
    language text NOT NULL DEFAULT 'en',
    announce_channel_id bigint,
    streamer_role_id bigint,
    ping_role_id bigint,
    alert_channel_id bigint,
    promo_invite_url text,
    auto_partner_default boolean NOT NULL DEFAULT true,
    cross_server_raids boolean NOT NULL DEFAULT true,
    ai_features boolean NOT NULL DEFAULT false,
    installed_at timestamptz NOT NULL DEFAULT now(),
    removed_at timestamptz,
    status text NOT NULL DEFAULT 'inactive',
    updated_at timestamptz NOT NULL DEFAULT now(),
    updated_by text
);

ALTER TABLE public.discord_guilds
    ADD COLUMN IF NOT EXISTS updated_at timestamptz NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS updated_by text,
    ALTER COLUMN status SET DEFAULT 'inactive',
    ALTER COLUMN ai_features SET DEFAULT false;

ALTER TABLE public.discord_guilds
    DROP CONSTRAINT IF EXISTS discord_guilds_status_check;
ALTER TABLE public.discord_guilds
    ADD CONSTRAINT discord_guilds_status_check CHECK (status IN ('inactive', 'active', 'removed'));

DO $legacy_guild_language$
BEGIN
    IF EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conrelid = 'public.discord_guilds'::regclass
          AND conname = 'discord_guilds_language_check'
          AND contype = 'c'
          AND pg_get_expr(conbin, conrelid) = '(language = ANY (ARRAY[''de''::text, ''en''::text]))'
    ) THEN
        ALTER TABLE public.discord_guilds DROP CONSTRAINT discord_guilds_language_check;
    END IF;
END
$legacy_guild_language$;

CREATE TABLE IF NOT EXISTS public.streamer_guilds (
    twitch_user_id text NOT NULL,
    guild_id bigint NOT NULL,
    active boolean NOT NULL DEFAULT false,
    deactivated_by text,
    deactivated_at timestamptz DEFAULT now(),
    joined_at timestamptz NOT NULL DEFAULT now(),
    source text NOT NULL DEFAULT 'onboarding',
    is_on_discord boolean NOT NULL DEFAULT false,
    require_discord_link boolean NOT NULL DEFAULT false,
    PRIMARY KEY (twitch_user_id, guild_id)
);

DO $membership_fields$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = 'streamer_guilds' AND column_name = 'is_on_discord'
    ) THEN
        ALTER TABLE public.streamer_guilds ADD COLUMN is_on_discord boolean NOT NULL DEFAULT false;
        UPDATE public.streamer_guilds sg
        SET is_on_discord = COALESCE(i.is_on_discord, 0) = 1
        FROM public.twitch_streamer_identities i
        WHERE sg.twitch_user_id = i.twitch_user_id AND sg.guild_id = 1289721245281292288;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = 'streamer_guilds' AND column_name = 'require_discord_link'
    ) THEN
        ALTER TABLE public.streamer_guilds ADD COLUMN require_discord_link boolean NOT NULL DEFAULT false;
        UPDATE public.streamer_guilds sg
        SET require_discord_link = p.required
        FROM (
            SELECT twitch_user_id, bool_or(COALESCE(require_discord_link, 0) = 1) AS required
            FROM public.twitch_partners GROUP BY twitch_user_id
        ) p
        WHERE sg.twitch_user_id = p.twitch_user_id AND sg.guild_id = 1289721245281292288;
    END IF;
END
$membership_fields$;

ALTER TABLE public.streamer_guilds
    ALTER COLUMN active SET DEFAULT false,
    ALTER COLUMN deactivated_at SET DEFAULT now(),
    ALTER COLUMN source SET DEFAULT 'onboarding';

ALTER TABLE public.streamer_guilds
    DROP CONSTRAINT IF EXISTS streamer_guilds_guild_id_fkey;

UPDATE public.streamer_guilds
SET deactivated_at = joined_at
WHERE NOT active AND deactivated_at IS NULL;

DO $guild_constraints$
DECLARE
    constraint_spec record;
BEGIN
    FOR constraint_spec IN
        SELECT * FROM (VALUES
            ('discord_guilds', 'discord_guilds_positive_id', 'CHECK (guild_id > 0)'),
            ('discord_guilds', 'discord_guilds_shadow_only', 'CHECK (guild_id = 1289721245281292288 OR status <> ''active'')'),
            ('discord_guilds', 'guild_ai_community_only', 'CHECK (guild_id = 1289721245281292288 OR NOT ai_features)'),
            ('discord_guilds', 'discord_guilds_language_code', 'CHECK (language ~ ''^[a-z]{2,8}(-[a-z0-9]{1,8})*$'')'),
            ('discord_guilds', 'guild_positive_channels', 'CHECK ((announce_channel_id IS NULL OR announce_channel_id > 0) AND (alert_channel_id IS NULL OR alert_channel_id > 0) AND (streamer_role_id IS NULL OR streamer_role_id > 0) AND (ping_role_id IS NULL OR ping_role_id > 0))'),
            ('discord_guilds', 'discord_guilds_invite_url', 'CHECK (promo_invite_url IS NULL OR (char_length(promo_invite_url) <= 200 AND promo_invite_url ~ ''^https://(discord\.gg|discord\.com/invite)/[A-Za-z0-9-]+$''))'),
            ('streamer_guilds', 'streamer_guilds_stable_uid', 'CHECK (twitch_user_id ~ ''^[0-9]{1,20}$'')'),
            ('streamer_guilds', 'streamer_guilds_identity_fk', 'FOREIGN KEY (twitch_user_id) REFERENCES public.twitch_streamer_identities (twitch_user_id)'),
            ('streamer_guilds', 'streamer_guilds_guild_fk', 'FOREIGN KEY (guild_id) REFERENCES public.discord_guilds (guild_id)'),
            ('streamer_guilds', 'streamer_guilds_shadow_only', 'CHECK (guild_id = 1289721245281292288 OR NOT active)'),
            ('streamer_guilds', 'streamer_guilds_inactive_timestamp', 'CHECK (active OR deactivated_at IS NOT NULL)'),
            ('streamer_guilds', 'streamer_guilds_source', 'CHECK (source IN (''onboarding'', ''migration'', ''admin''))')
        ) AS specs(table_name, constraint_name, definition)
    LOOP
        IF NOT EXISTS (
            SELECT 1 FROM pg_constraint
            WHERE conrelid = format('public.%I', constraint_spec.table_name)::regclass
              AND conname = constraint_spec.constraint_name
        ) THEN
            EXECUTE format('ALTER TABLE public.%I ADD CONSTRAINT %I %s NOT VALID',
                constraint_spec.table_name, constraint_spec.constraint_name, constraint_spec.definition);
        END IF;
        EXECUTE format('ALTER TABLE public.%I VALIDATE CONSTRAINT %I',
            constraint_spec.table_name, constraint_spec.constraint_name);
    END LOOP;
END
$guild_constraints$;

CREATE INDEX IF NOT EXISTS streamer_guilds_guild_active
    ON public.streamer_guilds (guild_id) WHERE active;

INSERT INTO public.discord_guilds (
    guild_id, name, language, announce_channel_id, alert_channel_id,
    streamer_role_id, promo_invite_url, auto_partner_default,
    cross_server_raids, ai_features, status, updated_by
) VALUES (
    1289721245281292288, 'Deutsche Deadlock Community', 'de',
    1374364800817303632, 1374364800817303632, 1313624729466441769,
    'https://discord.gg/z5TfVHuQq2', true, true, true, 'active', 'migration'
)
ON CONFLICT (guild_id) DO NOTHING;

INSERT INTO public.streamer_guilds (
    twitch_user_id, guild_id, active, source, deactivated_by, deactivated_at,
    is_on_discord, require_discord_link
)
SELECT p.twitch_user_id, 1289721245281292288, p.eligible, 'migration',
       CASE WHEN p.eligible THEN NULL ELSE 'migration' END,
       CASE WHEN p.eligible THEN NULL ELSE now() END,
       COALESCE(i.is_on_discord, 0) = 1, p.require_discord_link
FROM (
    SELECT s.twitch_user_id,
           bool_or(s.is_partner_active = 1)
           AND NOT EXISTS (
               SELECT 1 FROM public.twitch_exclusions e
               WHERE e.twitch_user_id = s.twitch_user_id AND e.reactivated_at IS NULL
           )
           AND NOT EXISTS (
               SELECT 1 FROM public.twitch_partner_signup_denylist d
               WHERE d.twitch_user_id = s.twitch_user_id OR lower(d.twitch_login) IN (
                   SELECT lower(p2.twitch_login) FROM public.twitch_partners p2
                   WHERE p2.twitch_user_id = s.twitch_user_id
               )
           )
           AND NOT EXISTS (
               SELECT 1 FROM public.twitch_partners p2
               WHERE p2.twitch_user_id = s.twitch_user_id
                 AND lower(btrim(COALESCE(p2.technical_pause_reason, ''))) IN ('blocked', 'bot_banned')
           ) AS eligible,
           bool_or(COALESCE(s.require_discord_link, 0) = 1) AS require_discord_link
    FROM public.twitch_partners_all_state s
    WHERE s.twitch_user_id ~ '^[0-9]{1,20}$'
    GROUP BY s.twitch_user_id
) p
JOIN public.twitch_streamer_identities i ON i.twitch_user_id = p.twitch_user_id
ON CONFLICT (twitch_user_id, guild_id) DO NOTHING;

ALTER TABLE public.twitch_streamer_identities
    ADD COLUMN IF NOT EXISTS broadcaster_language text,
    ADD COLUMN IF NOT EXISTS broadcaster_language_updated_at timestamptz;

ALTER TABLE public.twitch_streamer_identities
    ALTER COLUMN broadcaster_language DROP DEFAULT,
    ALTER COLUMN broadcaster_language DROP NOT NULL;

UPDATE public.twitch_streamer_identities
SET broadcaster_language = NULL,
    broadcaster_language_updated_at = NULL
WHERE broadcaster_language IS NULL
   OR lower(btrim(broadcaster_language)) IN ('', 'und', 'other');

WITH observations AS (
    SELECT u.twitch_user_id, lower(btrim(u.language)) AS language, u.recorded_at AS observed_at
    FROM public.twitch_channel_updates u
    WHERE lower(btrim(u.language)) ~ '^[a-z]{2,8}(-[a-z0-9]{1,8})*$'
      AND lower(btrim(u.language)) NOT IN ('und', 'other')
    UNION ALL
    SELECT s.twitch_user_id, lower(btrim(s.language)), s.started_at
    FROM public.twitch_stream_sessions s
    WHERE lower(btrim(s.language)) ~ '^[a-z]{2,8}(-[a-z0-9]{1,8})*$'
      AND lower(btrim(s.language)) NOT IN ('und', 'other')
), latest AS (
    SELECT DISTINCT ON (twitch_user_id) twitch_user_id, language, observed_at
    FROM observations
    ORDER BY twitch_user_id, observed_at DESC NULLS LAST, language
)
UPDATE public.twitch_streamer_identities i
SET broadcaster_language = latest.language,
    broadcaster_language_updated_at = latest.observed_at
FROM latest
WHERE i.twitch_user_id = latest.twitch_user_id AND i.broadcaster_language IS NULL;

CREATE OR REPLACE VIEW public.partner_guild_active AS
SELECT DISTINCT sg.twitch_user_id
FROM public.streamer_guilds sg
JOIN public.discord_guilds g ON g.guild_id = sg.guild_id
WHERE sg.active AND g.status = 'active' AND g.removed_at IS NULL
  AND NOT EXISTS (
      SELECT 1 FROM public.twitch_exclusions e
      WHERE e.twitch_user_id = sg.twitch_user_id AND e.reactivated_at IS NULL
  )
  AND NOT EXISTS (
      SELECT 1 FROM public.twitch_partner_signup_denylist d
      WHERE d.twitch_user_id = sg.twitch_user_id OR lower(d.twitch_login) IN (
          SELECT lower(i.twitch_login) FROM public.twitch_streamer_identities i
          WHERE i.twitch_user_id = sg.twitch_user_id
          UNION
          SELECT lower(p.twitch_login) FROM public.twitch_partners p
          WHERE p.twitch_user_id = sg.twitch_user_id
      )
  )
  AND NOT EXISTS (
      SELECT 1 FROM public.twitch_partners p
      WHERE p.twitch_user_id = sg.twitch_user_id
        AND lower(btrim(COALESCE(p.technical_pause_reason, ''))) IN ('blocked', 'bot_banned')
  );

CREATE OR REPLACE VIEW public.partner_status_shadow_mismatch AS
SELECT COALESCE(s.twitch_user_id, g.twitch_user_id) AS twitch_user_id,
       max(COALESCE(s.is_partner_active, 0)) AS old_is_partner_active,
       max(CASE WHEN g.twitch_user_id IS NOT NULL THEN 1 ELSE 0 END) AS new_is_partner_active
FROM public.twitch_partners_all_state s
FULL JOIN public.partner_guild_active g ON g.twitch_user_id = s.twitch_user_id
GROUP BY COALESCE(s.twitch_user_id, g.twitch_user_id)
HAVING max(COALESCE(s.is_partner_active, 0))
    IS DISTINCT FROM max(CASE WHEN g.twitch_user_id IS NOT NULL THEN 1 ELSE 0 END);

CREATE OR REPLACE FUNCTION public.twitch_partner_shadow_summary()
RETURNS TABLE (streamers bigint, legacy_active bigint, guild_active bigint, mismatches bigint)
LANGUAGE sql
STABLE
AS $$
    SELECT (SELECT count(*) FROM public.twitch_partners_all_state),
           (SELECT count(*) FROM public.twitch_partners_all_state WHERE is_partner_active = 1),
           (SELECT count(*) FROM public.partner_guild_active),
           (SELECT count(*) FROM public.partner_status_shadow_mismatch);
$$;
