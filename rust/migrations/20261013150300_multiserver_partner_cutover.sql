LOCK TABLE public.twitch_partners, public.twitch_streamer_identities,
    public.twitch_exclusions, public.twitch_partner_signup_denylist,
    public.discord_guilds, public.streamer_guilds IN SHARE MODE;

DO $partner_shadow_gate$
DECLARE mismatch_count bigint;
BEGIN
    SELECT COUNT(*) INTO mismatch_count
    FROM (
        (SELECT * FROM public.twitch_partners_all_state
         EXCEPT ALL SELECT * FROM public.twitch_partners_legacy_state)
        UNION ALL
        (SELECT * FROM public.twitch_partners_legacy_state
         EXCEPT ALL SELECT * FROM public.twitch_partners_all_state)
    ) differences;
    IF mismatch_count <> 0 THEN
        RAISE EXCEPTION 'Partner-Bestandsvergleich verhindert die Umschaltung: % Abweichungen', mismatch_count;
    END IF;
    SELECT mismatches INTO mismatch_count FROM public.twitch_partner_shadow_summary();
    IF mismatch_count IS DISTINCT FROM 0 THEN
        RAISE EXCEPTION 'Partner-Schattenvergleich verhindert die Umschaltung: % Abweichungen', mismatch_count;
    END IF;
END
$partner_shadow_gate$;

CREATE OR REPLACE VIEW public.twitch_partners_all_state AS
SELECT * FROM public.twitch_partners_guild_state;

CREATE OR REPLACE VIEW public.twitch_streamers_partner_state AS
SELECT twitch_login, twitch_user_id, require_discord_link, next_link_check_at,
       discord_user_id, discord_display_name, is_on_discord, manual_partner_opt_out,
       created_at, archived_at, raid_bot_enabled, silent_ban, silent_raid,
       is_monitored_only, is_verified, is_partner, is_partner_active,
       live_ping_role_id, live_ping_enabled, technical_pause_reason, operational_state
FROM public.twitch_partners_all_state
WHERE status = 'active';
