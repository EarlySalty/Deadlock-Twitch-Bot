ALTER TABLE public.twitch_partners
    ADD COLUMN IF NOT EXISTS inactivity_flagged_at text;

CREATE OR REPLACE VIEW public.twitch_partners_all_state AS
 SELECT p.id,
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
    COALESCE(p.admin_archived_at,
        CASE
            WHEN (p.status = 'archived'::text) THEN p.departnered_at
            ELSE NULL::text
        END) AS archived_at,
    p.raid_bot_enabled,
    p.silent_ban,
    p.silent_raid,
    0 AS is_monitored_only,
        CASE
            WHEN ((COALESCE(p.manual_verified_permanent, 0) = 1) OR ((p.manual_verified_until IS NOT NULL) AND ((p.manual_verified_until)::timestamp with time zone >= now())) OR (p.manual_verified_at IS NOT NULL)) THEN 1
            ELSE 0
        END AS is_verified,
    1 AS is_partner,
        CASE
            WHEN ((p.status = 'active'::text) AND (COALESCE(p.manual_partner_opt_out, 0) = 0) AND (COALESCE(p.technical_pause_reason, ''::text) = ''::text) AND (p.admin_archived_at IS NULL)) THEN 1
            ELSE 0
        END AS is_partner_active,
    p.live_ping_role_id,
    COALESCE(p.live_ping_enabled, 1) AS live_ping_enabled,
    p.status,
    p.departnered_at,
    p.technical_pause_reason,
        CASE
            WHEN (p.status <> 'active'::text) THEN 'inactive'::text
            WHEN (p.admin_archived_at IS NOT NULL) THEN 'inactive'::text
            WHEN (COALESCE(p.technical_pause_reason, ''::text) = 'blocked'::text) THEN 'blocked'::text
            WHEN (COALESCE(p.manual_partner_opt_out, 0) = 1) THEN 'admin_non_partner'::text
            WHEN (COALESCE(p.technical_pause_reason, ''::text) <> ''::text) THEN p.technical_pause_reason
            WHEN (p.inactivity_flagged_at IS NOT NULL) THEN 'inactive'::text
            ELSE 'active'::text
        END AS operational_state
   FROM (public.twitch_partners p
     LEFT JOIN public.twitch_streamer_identities i ON ((i.twitch_user_id = p.twitch_user_id)));
