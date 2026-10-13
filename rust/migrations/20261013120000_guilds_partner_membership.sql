CREATE TABLE IF NOT EXISTS public.discord_guilds (
    guild_id BIGINT PRIMARY KEY CHECK (guild_id > 0),
    name TEXT NOT NULL DEFAULT '' CHECK (char_length(name) <= 200),
    language TEXT NOT NULL DEFAULT 'en' CHECK (language IN ('de', 'en')),
    announce_channel_id BIGINT CHECK (announce_channel_id > 0),
    alert_channel_id BIGINT CHECK (alert_channel_id > 0),
    streamer_role_id BIGINT CHECK (streamer_role_id > 0),
    ping_role_id BIGINT CHECK (ping_role_id > 0),
    promo_invite_url TEXT CHECK (char_length(promo_invite_url) <= 200),
    auto_partner_default BOOLEAN NOT NULL DEFAULT TRUE,
    cross_server_raids BOOLEAN NOT NULL DEFAULT TRUE,
    ai_features BOOLEAN NOT NULL DEFAULT FALSE,
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'removed')),
    installed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    removed_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_by TEXT
);

CREATE TABLE IF NOT EXISTS public.streamer_guilds (
    twitch_user_id TEXT NOT NULL CHECK (twitch_user_id ~ '^[0-9]{1,20}$'),
    guild_id BIGINT NOT NULL REFERENCES public.discord_guilds(guild_id) ON DELETE CASCADE,
    active BOOLEAN NOT NULL DEFAULT TRUE,
    deactivated_by TEXT,
    deactivated_at TIMESTAMPTZ,
    joined_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    source TEXT NOT NULL DEFAULT 'onboarding' CHECK (source IN ('onboarding', 'migration', 'admin')),
    PRIMARY KEY (twitch_user_id, guild_id)
);

CREATE INDEX IF NOT EXISTS streamer_guilds_guild_active
    ON public.streamer_guilds (guild_id, active);

ALTER TABLE public.twitch_streamer_identities
    ADD COLUMN IF NOT EXISTS broadcaster_language TEXT NOT NULL DEFAULT 'und';

INSERT INTO public.discord_guilds (
    guild_id,
    name,
    language,
    announce_channel_id,
    alert_channel_id,
    streamer_role_id,
    promo_invite_url,
    auto_partner_default,
    cross_server_raids,
    ai_features,
    status,
    updated_by
) VALUES (
    1289721245281292288,
    'Deutsche Deadlock Community',
    'de',
    1374364800817303632,
    1374364800817303632,
    1313624729466441769,
    'https://discord.gg/z5TfVHuQq2',
    TRUE,
    TRUE,
    TRUE,
    'active',
    'migration'
)
ON CONFLICT (guild_id) DO UPDATE SET
    name = EXCLUDED.name,
    language = EXCLUDED.language,
    announce_channel_id = COALESCE(public.discord_guilds.announce_channel_id, EXCLUDED.announce_channel_id),
    alert_channel_id = COALESCE(public.discord_guilds.alert_channel_id, EXCLUDED.alert_channel_id),
    streamer_role_id = COALESCE(public.discord_guilds.streamer_role_id, EXCLUDED.streamer_role_id),
    promo_invite_url = COALESCE(public.discord_guilds.promo_invite_url, EXCLUDED.promo_invite_url),
    auto_partner_default = public.discord_guilds.auto_partner_default,
    cross_server_raids = public.discord_guilds.cross_server_raids,
    ai_features = TRUE,
    status = 'active',
    removed_at = NULL,
    updated_at = NOW(),
    updated_by = COALESCE(public.discord_guilds.updated_by, EXCLUDED.updated_by);

INSERT INTO public.streamer_guilds (twitch_user_id, guild_id, active, source)
SELECT DISTINCT ON (p.twitch_user_id)
    p.twitch_user_id,
    1289721245281292288,
    (
        p.status = 'active'
        AND COALESCE(p.manual_partner_opt_out, 0) = 0
        AND COALESCE(p.technical_pause_reason, '') = ''
        AND p.admin_archived_at IS NULL
    ),
    'migration'
FROM public.twitch_partners p
WHERE p.twitch_user_id ~ '^[0-9]{1,20}$'
ORDER BY p.twitch_user_id,
         CASE WHEN p.status = 'active' THEN 0 ELSE 1 END,
         p.id DESC
ON CONFLICT (twitch_user_id, guild_id) DO NOTHING;

CREATE OR REPLACE VIEW public.partner_guild_active AS
SELECT DISTINCT sg.twitch_user_id
FROM public.streamer_guilds sg
JOIN public.discord_guilds dg ON dg.guild_id = sg.guild_id
WHERE sg.active
  AND dg.status = 'active';

CREATE OR REPLACE VIEW public.partner_status_shadow_mismatch AS
SELECT
    COALESCE(old_state.twitch_user_id, guild_state.twitch_user_id) AS twitch_user_id,
    COALESCE(old_state.is_partner_active, 0) AS old_is_partner_active,
    CASE WHEN guild_state.twitch_user_id IS NULL THEN 0 ELSE 1 END AS new_is_partner_active
FROM (
    SELECT twitch_user_id, MAX(COALESCE(is_partner_active, 0)) AS is_partner_active
    FROM public.twitch_partners_all_state
    WHERE twitch_user_id ~ '^[0-9]{1,20}$'
    GROUP BY twitch_user_id
) old_state
FULL OUTER JOIN public.partner_guild_active guild_state
    ON guild_state.twitch_user_id = old_state.twitch_user_id
WHERE COALESCE(old_state.is_partner_active, 0)
      IS DISTINCT FROM CASE WHEN guild_state.twitch_user_id IS NULL THEN 0 ELSE 1 END;

CREATE TABLE IF NOT EXISTS public.onboarding_discord_states (
    state_token TEXT PRIMARY KEY CHECK (char_length(state_token) BETWEEN 20 AND 200),
    twitch_user_id TEXT NOT NULL CHECK (twitch_user_id ~ '^[0-9]{1,20}$'),
    twitch_login TEXT NOT NULL DEFAULT '',
    redirect_uri TEXT NOT NULL CHECK (char_length(redirect_uri) <= 500),
    locale TEXT NOT NULL DEFAULT 'de' CHECK (locale IN ('de', 'en')),
    discord_user_id TEXT,
    guild_ids BIGINT[] NOT NULL DEFAULT '{}',
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ
);
