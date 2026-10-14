CREATE TABLE IF NOT EXISTS discord_guilds (
    guild_id             bigint      PRIMARY KEY,
    name                 text,
    language             text        NOT NULL DEFAULT 'en'
        CHECK (language ~ '^[a-z]{2,8}(-[a-z0-9]{1,8})*$'),
    announce_channel_id  bigint,
    streamer_role_id     bigint,
    ping_role_id         bigint,
    alert_channel_id     bigint,
    promo_invite_url     text
        CHECK (promo_invite_url IS NULL
               OR promo_invite_url ~ '^https://(discord\.gg|discord\.com/invite)/[A-Za-z0-9-]+$'),
    auto_partner_default boolean     NOT NULL DEFAULT true,
    cross_server_raids   boolean     NOT NULL DEFAULT true,
    ai_features          boolean     NOT NULL DEFAULT false,
    updated_at           timestamptz NOT NULL DEFAULT now(),
    updated_by           text,
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

INSERT INTO discord_guilds (
    guild_id, name, language, promo_invite_url,
    auto_partner_default, cross_server_raids, ai_features, status
) VALUES (
    1289721245281292288, 'Deutsche Deadlock Community', 'de',
    'https://discord.gg/z5TfVHuQq2', true, true, true, 'active'
)
ON CONFLICT (guild_id) DO NOTHING;

INSERT INTO public.streamer_guilds (
    twitch_user_id, guild_id, active, source, deactivated_by, deactivated_at
)
SELECT s.twitch_user_id, 1289721245281292288::bigint, s.eligible, 'migration',
       CASE WHEN s.eligible THEN NULL ELSE 'migration' END,
       CASE WHEN s.eligible THEN NULL ELSE NOW() END
FROM (
    SELECT p.twitch_user_id,
           BOOL_OR(p.status = 'active' AND COALESCE(p.manual_partner_opt_out, 0) = 0 AND COALESCE(p.technical_pause_reason, '') = '' AND p.admin_archived_at IS NULL)
           AND NOT EXISTS (
               SELECT 1 FROM public.twitch_exclusions e
               WHERE e.twitch_user_id = p.twitch_user_id AND e.reactivated_at IS NULL
           )
           AND NOT EXISTS (
               SELECT 1 FROM public.twitch_partner_signup_denylist d
               WHERE d.twitch_user_id = p.twitch_user_id
                  OR LOWER(d.twitch_login) IN (
                      SELECT LOWER(p2.twitch_login) FROM public.twitch_partners p2
                      WHERE p2.twitch_user_id = p.twitch_user_id
                  )
           ) AS eligible
    FROM public.twitch_partners p
    WHERE p.twitch_user_id ~ '^[0-9]{1,20}$'
    GROUP BY p.twitch_user_id
) s
ON CONFLICT (twitch_user_id, guild_id) DO NOTHING;

ALTER TABLE twitch_streamer_identities
    ADD COLUMN IF NOT EXISTS broadcaster_language text,
    ADD COLUMN IF NOT EXISTS broadcaster_language_updated_at timestamptz;

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
