-- Schema-Hygiene: Legacy-Spalten manual_verified_permanent/_until/_at aus twitch_partners entfernen.
-- Neue Quellspalte `verified` (boolean) ersetzt sie als is_verified-Quelle (geseedet aus bisheriger Ableitung).
-- Invarianten (Prod 2026-06-23): aktive=53, is_partner_active=49, is_verified=55 (vorher==nachher, verifiziert).
-- Gate erfüllt: Python-Dienste gestoppt+disabled; 0 Rust-Roh-Reads der Spalten; 0 aktive Partner haengen
-- allein an einem zukuenftigen manual_verified_until (Freeze gefahrlos).

ALTER TABLE twitch_partners
    ADD COLUMN IF NOT EXISTS verified boolean NOT NULL DEFAULT false;

UPDATE twitch_partners p
SET verified = (
    COALESCE(p.manual_verified_permanent, 0) = 1
    OR (p.manual_verified_until IS NOT NULL AND p.manual_verified_until::timestamptz >= now())
    OR p.manual_verified_at IS NOT NULL
);

DROP VIEW IF EXISTS twitch_streamers_partner_state;
DROP VIEW IF EXISTS twitch_partners_all_state;

CREATE VIEW twitch_partners_all_state AS
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
    1 AS is_partner,
    CASE
        WHEN p.status = 'active'::text AND COALESCE(p.manual_partner_opt_out, 0) = 0 AND COALESCE(p.technical_pause_reason, ''::text) = ''::text AND p.admin_archived_at IS NULL THEN 1
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

CREATE VIEW twitch_streamers_partner_state AS
SELECT twitch_login,
    twitch_user_id,
    require_discord_link,
    next_link_check_at,
    discord_user_id,
    discord_display_name,
    is_on_discord,
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
    live_ping_enabled,
    technical_pause_reason,
    operational_state
   FROM twitch_partners_all_state
  WHERE status = 'active'::text;

ALTER TABLE twitch_partners DROP COLUMN IF EXISTS manual_verified_permanent;
ALTER TABLE twitch_partners DROP COLUMN IF EXISTS manual_verified_until;
ALTER TABLE twitch_partners DROP COLUMN IF EXISTS manual_verified_at;
