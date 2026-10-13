CREATE OR REPLACE VIEW public.twitch_partners_legacy_state AS
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

CREATE OR REPLACE VIEW public.partner_guild_active AS
SELECT DISTINCT sg.twitch_user_id
FROM public.streamer_guilds sg
JOIN public.discord_guilds g ON g.guild_id = sg.guild_id
WHERE sg.active AND g.status = 'active' AND g.removed_at IS NULL;

CREATE OR REPLACE VIEW public.twitch_partners_all_state AS
SELECT s.id,
       s.twitch_login,
       s.twitch_user_id,
       s.require_discord_link,
       s.next_link_check_at,
       s.discord_user_id,
       s.discord_display_name,
       s.is_on_discord,
       s.manual_partner_opt_out,
       s.created_at,
       s.archived_at,
       s.raid_bot_enabled,
       s.silent_ban,
       s.silent_raid,
       s.is_monitored_only,
       s.is_verified,
       CASE WHEN allowed.guild_active AND allowed.global_allowed THEN 1 ELSE 0 END AS is_partner,
       CASE WHEN s.is_partner_active = 1 AND allowed.guild_active AND allowed.global_allowed
            THEN 1 ELSE 0 END AS is_partner_active,
       s.live_ping_role_id,
       s.live_ping_enabled,
       s.status,
       s.departnered_at,
       s.technical_pause_reason,
       CASE WHEN NOT allowed.global_allowed THEN 'blocked'
            WHEN s.operational_state <> 'active' THEN s.operational_state
            WHEN NOT allowed.guild_active THEN 'inactive'
            ELSE s.operational_state END AS operational_state
FROM public.twitch_partners_legacy_state s
CROSS JOIN LATERAL (
    SELECT EXISTS (
               SELECT 1 FROM public.partner_guild_active g
               WHERE g.twitch_user_id = s.twitch_user_id
           ) AS guild_active,
           NOT EXISTS (
               SELECT 1 FROM public.twitch_exclusions e
               WHERE e.twitch_user_id = s.twitch_user_id AND e.reactivated_at IS NULL
           ) AND NOT EXISTS (
               SELECT 1 FROM public.twitch_partner_signup_denylist d
               WHERE d.twitch_user_id = s.twitch_user_id
                  OR LOWER(d.twitch_login) = LOWER(s.twitch_login)
           ) AND NOT EXISTS (
               SELECT 1 FROM public.twitch_partners p
               WHERE p.twitch_user_id = s.twitch_user_id
                 AND LOWER(TRIM(COALESCE(p.technical_pause_reason, ''))) IN ('blocked', 'bot_banned')
           ) AS global_allowed
) allowed;

CREATE OR REPLACE VIEW public.twitch_streamers_partner_state AS
SELECT twitch_login, twitch_user_id, require_discord_link, next_link_check_at,
       discord_user_id, discord_display_name, is_on_discord, manual_partner_opt_out,
       created_at, archived_at, raid_bot_enabled, silent_ban, silent_raid,
       is_monitored_only, is_verified, is_partner, is_partner_active,
       live_ping_role_id, live_ping_enabled, technical_pause_reason, operational_state
FROM public.twitch_partners_all_state
WHERE status = 'active';

CREATE OR REPLACE VIEW public.partner_status_shadow_mismatch AS
SELECT COALESCE(old_state.twitch_user_id, new_state.twitch_user_id) AS twitch_user_id,
       COALESCE(old_state.is_partner_active, 0) AS old_is_partner_active,
       COALESCE(new_state.is_partner_active, 0) AS new_is_partner_active
FROM (
    SELECT twitch_user_id, MAX(COALESCE(is_partner_active, 0)) AS is_partner_active
    FROM public.twitch_partners_legacy_state
    WHERE twitch_user_id ~ '^[0-9]{1,20}$'
    GROUP BY twitch_user_id
) old_state
FULL OUTER JOIN (
    SELECT twitch_user_id, MAX(COALESCE(is_partner_active, 0)) AS is_partner_active
    FROM public.twitch_streamers_partner_state
    WHERE twitch_user_id ~ '^[0-9]{1,20}$'
    GROUP BY twitch_user_id
) new_state ON new_state.twitch_user_id = old_state.twitch_user_id
WHERE COALESCE(old_state.is_partner_active, 0) IS DISTINCT FROM COALESCE(new_state.is_partner_active, 0);
