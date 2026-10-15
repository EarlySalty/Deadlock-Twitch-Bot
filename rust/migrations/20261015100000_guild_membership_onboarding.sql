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
            ('discord_guilds', 'guild_ai_community_only', 'CHECK (guild_id = 1289721245281292288 OR NOT ai_features)'),
            ('discord_guilds', 'discord_guilds_language_code', 'CHECK (language ~ ''^[a-z]{2,8}(-[a-z0-9]{1,8})*$'')'),
            ('discord_guilds', 'guild_positive_channels', 'CHECK ((announce_channel_id IS NULL OR announce_channel_id > 0) AND (alert_channel_id IS NULL OR alert_channel_id > 0) AND (streamer_role_id IS NULL OR streamer_role_id > 0) AND (ping_role_id IS NULL OR ping_role_id > 0))'),
            ('discord_guilds', 'discord_guilds_invite_url', 'CHECK (promo_invite_url IS NULL OR (char_length(promo_invite_url) <= 200 AND promo_invite_url ~ ''^https://(discord\.gg|discord\.com/invite)/[A-Za-z0-9-]+$''))'),
            ('streamer_guilds', 'streamer_guilds_stable_uid', 'CHECK (twitch_user_id ~ ''^[0-9]{1,20}$'')'),
            ('streamer_guilds', 'streamer_guilds_identity_fk', 'FOREIGN KEY (twitch_user_id) REFERENCES public.twitch_streamer_identities (twitch_user_id)'),
            ('streamer_guilds', 'streamer_guilds_guild_fk', 'FOREIGN KEY (guild_id) REFERENCES public.discord_guilds (guild_id)'),
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
  AND (NOT sg.require_discord_link OR (sg.is_on_discord AND EXISTS (
      SELECT 1 FROM public.twitch_streamer_identities i
      WHERE i.twitch_user_id = sg.twitch_user_id AND i.discord_user_id IS NOT NULL
  )))
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
       max(CASE WHEN s.status = 'active' AND COALESCE(s.manual_partner_opt_out, 0) = 0
           AND COALESCE(s.technical_pause_reason, '') = '' AND s.archived_at IS NULL THEN 1 ELSE 0 END) AS old_is_partner_active,
       max(CASE WHEN g.twitch_user_id IS NOT NULL THEN 1 ELSE 0 END) AS new_is_partner_active
FROM public.twitch_partners_all_state s
FULL JOIN public.partner_guild_active g ON g.twitch_user_id = s.twitch_user_id
GROUP BY COALESCE(s.twitch_user_id, g.twitch_user_id)
HAVING max(CASE WHEN s.status = 'active' AND COALESCE(s.manual_partner_opt_out, 0) = 0
           AND COALESCE(s.technical_pause_reason, '') = '' AND s.archived_at IS NULL THEN 1 ELSE 0 END)
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

ALTER TABLE public.discord_guilds DROP CONSTRAINT IF EXISTS discord_guilds_shadow_only;
ALTER TABLE public.streamer_guilds DROP CONSTRAINT IF EXISTS streamer_guilds_shadow_only;

CREATE TABLE public.guild_runtime (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    enabled boolean NOT NULL DEFAULT false,
    enabled_at timestamptz
);
INSERT INTO public.guild_runtime (singleton) VALUES (true);

CREATE FUNCTION public.enable_guild_runtime() RETURNS void LANGUAGE plpgsql AS $$
BEGIN
    LOCK TABLE public.twitch_partners, public.streamer_guilds, public.discord_guilds IN SHARE ROW EXCLUSIVE MODE;
    IF EXISTS (SELECT 1 FROM public.partner_status_shadow_mismatch) THEN
        RAISE EXCEPTION 'Guild partner parity mismatch';
    END IF;
    UPDATE public.guild_runtime SET enabled = true, enabled_at = now() WHERE singleton;
END;
$$;

CREATE TABLE public.guild_onboarding_sessions (
    session_hash text PRIMARY KEY CHECK (session_hash ~ '^[0-9a-f]{64}$'),
    twitch_user_id text NOT NULL CHECK (twitch_user_id ~ '^[0-9]{1,20}$'),
    twitch_login text NOT NULL,
    discord_user_id text,
    discord_name text,
    guild_ids bigint[] NOT NULL DEFAULT '{}',
    oauth_hash text UNIQUE CHECK (oauth_hash ~ '^[0-9a-f]{64}$'),
    oauth_redirect_uri text,
    oauth_expires_at timestamptz,
    discord_verified_at timestamptz,
    expires_at timestamptz NOT NULL DEFAULT now() + interval '20 minutes'
);
CREATE INDEX guild_onboarding_sessions_expiry ON public.guild_onboarding_sessions (expires_at);

CREATE FUNCTION public.guild_partner_allowed(uid text) RETURNS boolean LANGUAGE sql STABLE AS $$
    SELECT EXISTS (
        SELECT 1 FROM public.partner_guild_active a
        JOIN public.streamer_guilds sg ON sg.twitch_user_id = a.twitch_user_id
        JOIN public.discord_guilds g ON g.guild_id = sg.guild_id
        WHERE a.twitch_user_id = uid AND sg.active AND g.status = 'active' AND g.removed_at IS NULL
          AND (NOT sg.require_discord_link OR (sg.is_on_discord AND EXISTS (
              SELECT 1 FROM public.twitch_streamer_identities i
              WHERE i.twitch_user_id = uid AND i.discord_user_id IS NOT NULL
          )))
          AND ((SELECT enabled FROM public.guild_runtime WHERE singleton) OR sg.guild_id = 1289721245281292288)
    );
$$;

ALTER TABLE public.twitch_partners ADD COLUMN legacy_home_admitted boolean NOT NULL DEFAULT false;
UPDATE public.twitch_partners SET legacy_home_admitted = status = 'active';

CREATE FUNCTION public.record_legacy_home_admission() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF current_setting('tb.foreign_guild_admission', true) IS DISTINCT FROM 'true' THEN
        NEW.legacy_home_admitted := NEW.status = 'active';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER record_legacy_home_admission BEFORE INSERT OR UPDATE OF status
ON public.twitch_partners FOR EACH ROW EXECUTE FUNCTION public.record_legacy_home_admission();

CREATE FUNCTION public.sync_legacy_home_membership() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE eligible boolean;
BEGIN
    IF current_setting('tb.foreign_guild_admission', true) = 'true' THEN
        RETURN NEW;
    END IF;
    SELECT COALESCE(bool_or(p.legacy_home_admitted AND p.status = 'active' AND COALESCE(p.manual_partner_opt_out, 0) = 0
        AND COALESCE(p.technical_pause_reason, '') = '' AND p.admin_archived_at IS NULL), false)
    INTO eligible FROM public.twitch_partners p WHERE p.twitch_user_id = NEW.twitch_user_id;
    INSERT INTO public.streamer_guilds (twitch_user_id, guild_id, active, source, deactivated_at, require_discord_link, is_on_discord)
    SELECT NEW.twitch_user_id, 1289721245281292288, eligible, 'migration', CASE WHEN eligible THEN NULL ELSE now() END,
        EXISTS (SELECT 1 FROM public.twitch_partners WHERE twitch_user_id = NEW.twitch_user_id AND COALESCE(require_discord_link, 0) = 1),
        EXISTS (SELECT 1 FROM public.twitch_streamer_identities WHERE twitch_user_id = NEW.twitch_user_id AND is_on_discord = 1)
    WHERE EXISTS (SELECT 1 FROM public.twitch_streamer_identities WHERE twitch_user_id = NEW.twitch_user_id)
      AND (eligible OR EXISTS (SELECT 1 FROM public.streamer_guilds WHERE twitch_user_id = NEW.twitch_user_id AND guild_id = 1289721245281292288))
    ON CONFLICT (twitch_user_id, guild_id) DO UPDATE
       SET active = EXCLUDED.active AND (streamer_guilds.active OR (TG_ARGV[0] = 'admission' AND NEW.legacy_home_admitted AND NEW.status = 'active')),
           deactivated_at = CASE WHEN NOT EXCLUDED.active THEN EXCLUDED.deactivated_at
               WHEN streamer_guilds.active OR (TG_ARGV[0] = 'admission' AND NEW.legacy_home_admitted AND NEW.status = 'active') THEN NULL
               ELSE streamer_guilds.deactivated_at END,
           require_discord_link = EXCLUDED.require_discord_link
       WHERE coalesce(streamer_guilds.deactivated_by, '') <> 'admin';
    RETURN NEW;
END;
$$;
CREATE TRIGGER sync_legacy_home_membership AFTER INSERT OR UPDATE OF status
ON public.twitch_partners FOR EACH ROW EXECUTE FUNCTION public.sync_legacy_home_membership('admission');
CREATE TRIGGER sync_legacy_home_membership_metadata AFTER UPDATE OF manual_partner_opt_out, technical_pause_reason, admin_archived_at, require_discord_link
ON public.twitch_partners FOR EACH ROW EXECUTE FUNCTION public.sync_legacy_home_membership('metadata');

CREATE FUNCTION public.revoke_membership_onboarding_proof() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF (OLD.active AND NOT NEW.active) OR (OLD.is_on_discord AND NOT NEW.is_on_discord) THEN
        UPDATE public.guild_onboarding_sessions
        SET guild_ids = array_remove(guild_ids, NEW.guild_id)
        WHERE twitch_user_id = NEW.twitch_user_id AND NEW.guild_id = ANY(guild_ids);
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER revoke_membership_onboarding_proof AFTER UPDATE OF active, is_on_discord
ON public.streamer_guilds FOR EACH ROW EXECUTE FUNCTION public.revoke_membership_onboarding_proof();

CREATE FUNCTION public.revoke_guild_onboarding_proof() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.status <> 'active' OR NEW.removed_at IS NOT NULL THEN
        UPDATE public.guild_onboarding_sessions SET guild_ids = array_remove(guild_ids, NEW.guild_id)
        WHERE NEW.guild_id = ANY(guild_ids);
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER revoke_guild_onboarding_proof AFTER UPDATE OF status, removed_at
ON public.discord_guilds FOR EACH ROW EXECUTE FUNCTION public.revoke_guild_onboarding_proof();

CREATE FUNCTION public.sync_home_discord_membership() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF (OLD.discord_user_id IS NOT NULL AND NEW.discord_user_id IS DISTINCT FROM OLD.discord_user_id)
        OR (COALESCE(OLD.is_on_discord, 0) = 1 AND COALESCE(NEW.is_on_discord, 0) <> 1) THEN
        UPDATE public.guild_onboarding_sessions SET guild_ids = '{}', discord_verified_at = NULL
        WHERE twitch_user_id = NEW.twitch_user_id;
    END IF;
    IF OLD.discord_user_id IS NOT NULL AND NEW.discord_user_id IS DISTINCT FROM OLD.discord_user_id THEN
        NEW.is_on_discord := 0;
        UPDATE public.streamer_guilds
        SET is_on_discord = false,
            active = CASE WHEN guild_id = 1289721245281292288 THEN active ELSE false END,
            deactivated_by = CASE WHEN guild_id = 1289721245281292288 OR deactivated_by = 'admin' THEN deactivated_by ELSE 'identity_change' END,
            deactivated_at = CASE WHEN guild_id = 1289721245281292288 THEN deactivated_at ELSE COALESCE(deactivated_at, now()) END
        WHERE twitch_user_id = NEW.twitch_user_id;
    ELSIF NEW.is_on_discord IS DISTINCT FROM OLD.is_on_discord THEN
        UPDATE public.streamer_guilds SET is_on_discord = COALESCE(NEW.is_on_discord, 0) = 1
        WHERE twitch_user_id = NEW.twitch_user_id AND guild_id = 1289721245281292288;
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER sync_home_discord_membership BEFORE UPDATE OF is_on_discord, discord_user_id
ON public.twitch_streamer_identities FOR EACH ROW EXECUTE FUNCTION public.sync_home_discord_membership();

CREATE FUNCTION public.record_broadcaster_language() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE observed_language text;
BEGIN
    observed_language := lower(btrim(NEW.language));
    IF observed_language ~ '^[a-z]{2,8}(-[a-z0-9]{1,8})*$' AND observed_language NOT IN ('und', 'other') THEN
        UPDATE public.twitch_streamer_identities
        SET broadcaster_language = observed_language, broadcaster_language_updated_at = NEW.recorded_at
        WHERE twitch_user_id = NEW.twitch_user_id
          AND (broadcaster_language_updated_at IS NULL OR broadcaster_language_updated_at <= NEW.recorded_at);
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER record_broadcaster_language AFTER INSERT ON public.twitch_channel_updates
FOR EACH ROW EXECUTE FUNCTION public.record_broadcaster_language();

CREATE OR REPLACE VIEW twitch_partners_all_state AS
SELECT p.id,
    p.twitch_login,
    p.twitch_user_id,
    p.require_discord_link,
    p.next_link_check_at,
    i.discord_user_id,
    i.discord_display_name,
    COALESCE(i.is_on_discord, 0) AS is_on_discord,
    p.manual_partner_opt_out,
    p.partnered_at AS created_at,
    COALESCE(p.admin_archived_at,
        CASE
            WHEN p.status = 'archived'::text THEN p.departnered_at
            ELSE NULL::text
        END) AS archived_at,
    p.raid_bot_enabled,
    p.silent_ban,
    p.silent_raid,
    0 AS is_monitored_only,
    CASE
        WHEN p.verified THEN 1
        ELSE 0
    END AS is_verified,
    CASE WHEN public.guild_partner_allowed(p.twitch_user_id) THEN 1 ELSE 0 END AS is_partner,
    CASE
        WHEN p.status = 'active'::text AND COALESCE(p.manual_partner_opt_out, 0) = 0 AND COALESCE(p.technical_pause_reason, ''::text) = ''::text AND p.admin_archived_at IS NULL AND public.guild_partner_allowed(p.twitch_user_id) THEN 1
        ELSE 0
    END AS is_partner_active,
    p.live_ping_role_id,
    COALESCE(p.live_ping_enabled, 1) AS live_ping_enabled,
    p.status,
    p.departnered_at,
    p.technical_pause_reason,
    CASE
        WHEN p.status <> 'active'::text THEN 'inactive'::text
        WHEN p.admin_archived_at IS NOT NULL THEN 'inactive'::text
        WHEN COALESCE(p.technical_pause_reason, ''::text) = 'blocked'::text THEN 'blocked'::text
        WHEN COALESCE(p.manual_partner_opt_out, 0) = 1 THEN 'admin_non_partner'::text
        WHEN COALESCE(p.technical_pause_reason, ''::text) <> ''::text THEN p.technical_pause_reason
        WHEN p.inactivity_flagged_at IS NOT NULL THEN 'inactive'::text
        ELSE 'active'::text
    END AS operational_state
   FROM twitch_partners p
     LEFT JOIN twitch_streamer_identities i ON i.twitch_user_id = p.twitch_user_id;
