-- Multi-Server, minimales Guild-Schema (Plan 2026-10-12, Abschnitt 4.2) für
-- Paket R (Raids nach Sprache, Netzwerk-Schalter, Werbung pro Server).
--
-- Bestand bleibt unverändert: Der deutsche Community-Server wird als einzige
-- Guild angelegt, alle bestehenden Partner bekommen dort eine aktive Zeile
-- (Plan E1). Streamer ganz ohne Zeile gelten im Code ebenfalls als Mitglied des
-- deutschen Servers. Kanal- und Rollenwerte des deutschen Servers füllt Paket G
-- aus der Konfiguration; Paket R nutzt nur Sprache, Netzwerk-Schalter und
-- Werbe-Einladung.

CREATE TABLE IF NOT EXISTS discord_guilds (
    guild_id             bigint      PRIMARY KEY,
    name                 text,
    -- Sprache des Servers (Sprachcode wie 'de' oder 'en').
    language             text        NOT NULL DEFAULT 'en'
        CHECK (language ~ '^[a-z]{2,8}(-[a-z0-9]{1,8})*$'),
    announce_channel_id  bigint,
    streamer_role_id     bigint,
    ping_role_id         bigint,
    alert_channel_id     bigint,
    -- Discord-Einladung, mit der der Server im Twitch-Chat seiner Streamer wirbt.
    promo_invite_url     text
        CHECK (promo_invite_url IS NULL
               OR promo_invite_url ~ '^https://(discord\.gg|discord\.com/invite)/[A-Za-z0-9-]+$'),
    -- Neue Streamer werden automatisch Partner des Servers (Plan E10).
    auto_partner_default boolean     NOT NULL DEFAULT true,
    -- Netzwerk-Schalter: Raids über Servergrenzen innerhalb derselben Sprache (Plan E11).
    cross_server_raids   boolean     NOT NULL DEFAULT true,
    -- KI-Funktionen mit Streamerbezug (Plan E14); nur der deutsche Server hat sie an.
    ai_features          boolean     NOT NULL DEFAULT false,
    installed_at         timestamptz NOT NULL DEFAULT now(),
    removed_at           timestamptz,
    status               text        NOT NULL DEFAULT 'active'
        CHECK (status IN ('active', 'removed'))
);

CREATE TABLE IF NOT EXISTS streamer_guilds (
    twitch_user_id text        NOT NULL,
    guild_id       bigint      NOT NULL REFERENCES discord_guilds (guild_id),
    active         boolean     NOT NULL DEFAULT true,
    deactivated_by text,
    deactivated_at timestamptz,
    joined_at      timestamptz NOT NULL DEFAULT now(),
    source         text        NOT NULL
        CHECK (source IN ('onboarding', 'migration', 'admin')),
    PRIMARY KEY (twitch_user_id, guild_id),
    CHECK (active OR deactivated_at IS NOT NULL)
);

CREATE INDEX IF NOT EXISTS streamer_guilds_guild_active
    ON streamer_guilds (guild_id)
    WHERE active;

-- Deutscher Community-Server: Sprache de, Netzwerk an, KI an. Die Werbe-
-- Einladung ist die bisherige feste Community-Einladung.
INSERT INTO discord_guilds (
    guild_id, name, language, promo_invite_url,
    auto_partner_default, cross_server_raids, ai_features, status
) VALUES (
    1289721245281292288, 'Deutsche Deadlock Community', 'de',
    'https://discord.gg/z5TfVHuQq2', true, true, true, 'active'
)
ON CONFLICT (guild_id) DO NOTHING;

-- Alle bestehenden Partner gehören zum deutschen Server.
INSERT INTO streamer_guilds (twitch_user_id, guild_id, active, source)
SELECT DISTINCT BTRIM(p.twitch_user_id), 1289721245281292288::bigint, true, 'migration'
  FROM twitch_partners p
 WHERE p.status = 'active'
   AND COALESCE(BTRIM(p.twitch_user_id), '') <> ''
ON CONFLICT (twitch_user_id, guild_id) DO NOTHING;

-- Kanalsprache je Streamer (Helix broadcaster_language bzw. Stream-Sprache).
-- Der Poller und EventSub channel.update halten sie aktuell.
ALTER TABLE twitch_streamer_identities
    ADD COLUMN IF NOT EXISTS broadcaster_language text,
    ADD COLUMN IF NOT EXISTS broadcaster_language_updated_at timestamptz;

-- Startwert aus der Historie: jüngste nicht leere Sprache aus channel.update
-- (echte Kanaleinstellung) oder aus Stream-Sessions (Stream-Sprache).
WITH quellen AS (
    SELECT u.twitch_user_id,
           LOWER(BTRIM(u.language)) AS sprache,
           u.recorded_at AS gesehen
      FROM twitch_channel_updates u
     WHERE COALESCE(BTRIM(u.language), '') <> ''
    UNION ALL
    SELECT s.twitch_user_id,
           LOWER(BTRIM(s.language)),
           s.started_at
      FROM twitch_stream_sessions s
     WHERE COALESCE(BTRIM(s.language), '') <> ''
       AND COALESCE(BTRIM(s.twitch_user_id), '') <> ''
),
juengste AS (
    SELECT DISTINCT ON (twitch_user_id) twitch_user_id, sprache, gesehen
      FROM quellen
     ORDER BY twitch_user_id, gesehen DESC
)
UPDATE twitch_streamer_identities i
   SET broadcaster_language = j.sprache,
       broadcaster_language_updated_at = j.gesehen
  FROM juengste j
 WHERE i.twitch_user_id = j.twitch_user_id
   AND i.broadcaster_language IS NULL;
