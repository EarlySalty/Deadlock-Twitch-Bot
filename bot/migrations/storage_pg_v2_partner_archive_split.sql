-- storage_pg runtime schema v2
-- Separates admin archive visibility from operational partner state.
-- Apply this before startup in environments that keep
-- TWITCH_ALLOW_RUNTIME_SCHEMA_BOOTSTRAP disabled.

ALTER TABLE IF EXISTS twitch_partners
    ADD COLUMN IF NOT EXISTS admin_archived_at TEXT;

WITH legacy_archived AS (
    SELECT
        p.id,
        p.twitch_user_id,
        p.twitch_login,
        legacy_streamer.archived_at AS streamer_archived_at
    FROM twitch_partners p
    LEFT JOIN LATERAL (
        SELECT
            MAX(s.archived_at) AS archived_at
        FROM twitch_streamers s
        WHERE (
            NULLIF(BTRIM(p.twitch_user_id), '') IS NOT NULL
            AND s.twitch_user_id = p.twitch_user_id
        )
           OR LOWER(s.twitch_login) = LOWER(p.twitch_login)
    ) AS legacy_streamer ON TRUE
    WHERE p.status = 'archived'
)
UPDATE twitch_partners AS p
SET admin_archived_at = COALESCE(
        p.admin_archived_at,
        legacy_archived.streamer_archived_at,
        p.departnered_at,
        CURRENT_TIMESTAMP::text
    )
FROM legacy_archived
WHERE p.id = legacy_archived.id
  AND legacy_archived.streamer_archived_at IS NOT NULL;

DROP VIEW IF EXISTS twitch_streamers_partner_state;
DROP VIEW IF EXISTS twitch_partners_all_state;

CREATE VIEW twitch_partners_all_state AS
SELECT
    p.id,
    p.twitch_login,
    p.twitch_user_id,
    p.require_discord_link,
    p.next_link_check_at,
    i.discord_user_id,
    i.discord_display_name,
    COALESCE(i.is_on_discord, 0) AS is_on_discord,
    p.manual_verified_permanent,
    p.manual_verified_until,
    p.manual_verified_at,
    p.manual_partner_opt_out,
    p.partnered_at AS created_at,
    COALESCE(
        p.admin_archived_at,
        CASE WHEN p.status = 'archived' THEN p.departnered_at ELSE NULL END
    ) AS archived_at,
    p.raid_bot_enabled,
    p.silent_ban,
    p.silent_raid,
    0 AS is_monitored_only,
    CASE
        WHEN (
            COALESCE(p.manual_verified_permanent, 0) = 1
            OR (
                p.manual_verified_until IS NOT NULL
                AND p.manual_verified_until::timestamptz >= NOW()
            )
            OR p.manual_verified_at IS NOT NULL
        )
        THEN 1 ELSE 0
    END AS is_verified,
    1 AS is_partner,
    CASE
        WHEN p.status = 'active'
             AND COALESCE(p.manual_partner_opt_out, 0) = 0
        THEN 1 ELSE 0
    END AS is_partner_active,
    p.live_ping_role_id,
    COALESCE(p.live_ping_enabled, 1) AS live_ping_enabled,
    p.status,
    p.departnered_at
FROM twitch_partners p
LEFT JOIN twitch_streamer_identities i
  ON i.twitch_user_id = p.twitch_user_id;

CREATE VIEW twitch_streamers_partner_state AS
SELECT
    twitch_login,
    twitch_user_id,
    require_discord_link,
    next_link_check_at,
    discord_user_id,
    discord_display_name,
    is_on_discord,
    manual_verified_permanent,
    manual_verified_until,
    manual_verified_at,
    manual_partner_opt_out,
    created_at,
    archived_at,
    raid_bot_enabled,
    silent_ban,
    silent_raid,
    is_monitored_only,
    is_verified,
    is_partner,
    is_partner_active,
    live_ping_role_id,
    live_ping_enabled
FROM twitch_partners_all_state
WHERE status = 'active';

CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_partners_active_user_id
    ON twitch_partners(twitch_user_id)
    WHERE status = 'active';

CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_partners_active_login_lower
    ON twitch_partners(LOWER(twitch_login))
    WHERE status = 'active';

INSERT INTO schema_version (component, version, updated_at)
VALUES ('storage_pg', 2, now())
ON CONFLICT (component) DO UPDATE SET
    version = EXCLUDED.version,
    updated_at = now();
