-- Reduziertes Multi-Server-Schema für Tests mit handgeschriebenem DDL: genau
-- die Spalten, die die DB-Funktion `twitch_streamer_ai_allowed` liest
-- (Migration 20261012120000). Danach die Regel selbst einspielen:
-- `migrations/20261012140000_ai_features_rule.sql`.
CREATE TABLE discord_guilds (
    guild_id    bigint PRIMARY KEY,
    ai_features boolean NOT NULL DEFAULT false,
    removed_at  timestamptz,
    status      text NOT NULL DEFAULT 'active'
);
CREATE TABLE streamer_guilds (
    twitch_user_id text NOT NULL,
    guild_id       bigint NOT NULL REFERENCES discord_guilds (guild_id),
    active         boolean NOT NULL DEFAULT true,
    joined_at      timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (twitch_user_id, guild_id)
);
CREATE TABLE IF NOT EXISTS twitch_streamer_identities (
    twitch_user_id text PRIMARY KEY,
    twitch_login   text
);
