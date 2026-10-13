-- Paket A (Multi-Server, PLAN.md 4.5/E9/E11): Server-Admin-Dashboard.
--
-- Paket G (Datenmodell Guilds) ist noch nicht gebaut. Diese Migration legt nur
-- das minimale Fundament an, das das Server-Admin-Dashboard braucht. Die Spalten
-- folgen PLAN.md 4.2, damit G darauf aufbauen kann (Migration bestehender
-- Partner, /setup, Onboarding, neuer Partnerbegriff bleiben Aufgabe von G).
-- Es werden keine bestehenden Tabellen geändert und keine Daten migriert.

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
    -- KI-Schalter (E14): nur globale Ebene, im Server-Admin-Dashboard gesperrt.
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

-- Nachvollziehbarkeit aller Änderungen aus dem Server-Admin-Dashboard.
CREATE TABLE IF NOT EXISTS public.server_admin_audit_log (
    id BIGSERIAL PRIMARY KEY,
    guild_id BIGINT NOT NULL CHECK (guild_id > 0),
    actor TEXT NOT NULL,
    action TEXT NOT NULL,
    detail JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(detail) = 'object'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS server_admin_audit_log_guild
    ON public.server_admin_audit_log (guild_id, created_at DESC);
