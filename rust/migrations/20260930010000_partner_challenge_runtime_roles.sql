-- Additive final permissions for the integrated challenges and clip contest.
-- The deploy role matrix invokes this same function after its legacy grants.
CREATE FUNCTION public.twitch_apply_partner_challenge_roles() RETURNS void
LANGUAGE plpgsql SET search_path = pg_catalog, public AS $roles$
DECLARE feature_table text; role_name text; sequence_name text;
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchcontest') THEN
        CREATE ROLE twitchcontest LOGIN NOINHERIT NOSUPERUSER NOCREATEDB
            NOCREATEROLE NOREPLICATION NOBYPASSRLS;
    END IF;
    ALTER ROLE twitchcontest LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
    ALTER ROLE twitchcontest PASSWORD NULL;
    GRANT USAGE ON SCHEMA public TO twitchcontest;
    REVOKE ALL ON ALL TABLES IN SCHEMA public FROM twitchcontest;
    REVOKE ALL ON ALL SEQUENCES IN SCHEMA public FROM twitchcontest;

    FOREACH feature_table IN ARRAY ARRAY[
        'partner_effort_program','partner_effort_events','partner_effort_weekly_quests',
        'partner_effort_stream_weeks','partner_effort_weeks','partner_effort_streaks',
        'partner_effort_achievements','partner_effort_shared_chat_observations',
        'partner_effort_party_observations','partner_effort_source_receipts',
        'partner_effort_source_cursors','partner_effort_source_state',
        'twitch_partner_effort_season_closures','twitch_partner_effort_season_results',
        'twitch_partner_raid_boost_grants','twitch_partner_raid_boost_streams',
        'twitch_streamer_referral_credits',
        'twitch_clip_contest_months','twitch_clip_contest_submissions',
        'twitch_clip_contest_votes','twitch_clip_contest_hall_of_fame',
        'twitch_clip_contest_moderation','twitch_clip_contest_effort_outbox'
    ] LOOP
        FOREACH role_name IN ARRAY ARRAY['twitchbot','twitchdash','twitchlegacy'] LOOP
            IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = role_name) THEN
                EXECUTE format('REVOKE ALL ON TABLE public.%I FROM %I', feature_table, role_name);
                EXECUTE format('GRANT SELECT ON TABLE public.%I TO %I', feature_table, role_name);
                sequence_name := NULL;
                IF EXISTS (SELECT 1 FROM information_schema.columns c WHERE c.table_schema='public' AND c.table_name=feature_table AND c.column_name='id') THEN
                    sequence_name := pg_get_serial_sequence(format('public.%I', feature_table), 'id');
                END IF;
                IF sequence_name IS NOT NULL THEN
                    EXECUTE format('REVOKE ALL ON SEQUENCE %s FROM %I', sequence_name, role_name);
                END IF;
            END IF;
        END LOOP;
        IF feature_table LIKE 'partner_effort_%' AND EXISTS (SELECT 1 FROM pg_roles WHERE rolname='twitchbot') THEN
            EXECUTE format('GRANT INSERT, UPDATE, DELETE ON TABLE public.%I TO twitchbot', feature_table);
        END IF;
        IF feature_table LIKE 'twitch_clip_contest_%' THEN
            EXECUTE format('GRANT SELECT, INSERT ON TABLE public.%I TO twitchcontest', feature_table);
            sequence_name := NULL;
                IF EXISTS (SELECT 1 FROM information_schema.columns c WHERE c.table_schema='public' AND c.table_name=feature_table AND c.column_name='id') THEN
                    sequence_name := pg_get_serial_sequence(format('public.%I', feature_table), 'id');
                END IF;
            IF sequence_name IS NOT NULL THEN
                EXECUTE format('GRANT USAGE, SELECT ON SEQUENCE %s TO twitchcontest', sequence_name);
            END IF;
        END IF;
    END LOOP;

    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname='twitchbot') THEN
        REVOKE UPDATE, DELETE, TRUNCATE ON public.partner_effort_events,
            public.partner_effort_weekly_quests, public.partner_effort_achievements FROM twitchbot;
        GRANT USAGE, SELECT ON SEQUENCE public.partner_effort_events_id_seq,
            public.twitch_partner_raid_boost_grants_id_seq,
            public.twitch_partner_raid_boost_streams_id_seq TO twitchbot;
        GRANT SELECT ON public.category_collector_config TO twitchbot;
        GRANT INSERT ON public.twitch_partner_effort_season_closures,
            public.twitch_partner_effort_season_results, public.twitch_streamer_referral_credits TO twitchbot;
        GRANT INSERT, UPDATE ON public.twitch_partner_raid_boost_grants,
            public.twitch_partner_raid_boost_streams TO twitchbot;
        GRANT EXECUTE ON FUNCTION public.twitch_referral_claim_candidates(text) TO twitchbot;
    END IF;
    -- Separate peer identity: dashboard reader sessions cannot SET ROLE to writer.
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname='twitchdash') THEN
        REVOKE twitchcontest FROM twitchdash;
    END IF;
    GRANT UPDATE (finalized_at) ON public.twitch_clip_contest_months TO twitchcontest;
    GRANT UPDATE (hidden_at, hidden_by) ON public.twitch_clip_contest_submissions TO twitchcontest;
    GRANT SELECT ON public.twitch_partners, public.twitch_streamers,
        public.twitch_clips_social_media, public.social_media_streamer_layout,
        public.twitch_streamer_identities, public.social_media_category TO twitchcontest;
    GRANT EXECUTE ON FUNCTION public.twitch_clip_contest_active_partner(text) TO twitchcontest;
    GRANT INSERT (twitch_login, twitch_user_id) ON public.twitch_streamers TO twitchcontest;
    GRANT INSERT (clip_id, clip_url, clip_title, clip_thumbnail_url, streamer_login,
        twitch_user_id, created_at, duration_seconds, view_count, game_name, game_id,
        category_key, status, vod_id, vod_offset_s)
        ON public.twitch_clips_social_media TO twitchcontest;
    GRANT UPDATE (layout_override_json) ON public.twitch_clips_social_media TO twitchcontest;
    FOREACH feature_table IN ARRAY ARRAY['twitch_streamers','twitch_clips_social_media'] LOOP
        sequence_name := NULL;
                IF EXISTS (SELECT 1 FROM information_schema.columns c WHERE c.table_schema='public' AND c.table_name=feature_table AND c.column_name='id') THEN
                    sequence_name := pg_get_serial_sequence(format('public.%I', feature_table), 'id');
                END IF;
        IF sequence_name IS NOT NULL THEN
            EXECUTE format('GRANT USAGE, SELECT ON SEQUENCE %s TO twitchcontest', sequence_name);
        END IF;
    END LOOP;
END
$roles$;
REVOKE ALL ON FUNCTION public.twitch_apply_partner_challenge_roles() FROM PUBLIC;

-- Bot receives only eligible referral facts, never affiliate contact/payment data.
-- Stable target identity is mandatory: unresolved historic claims cannot qualify.
CREATE FUNCTION public.twitch_referral_claim_candidates(target_twitch_user_id text)
RETURNS TABLE(streamer_user_id text, streamer_login text, claimed_at text)
LANGUAGE sql SECURITY DEFINER SET search_path = pg_catalog, public AS $claims$
    SELECT a.twitch_user_id, p.twitch_login, c.claimed_at
    FROM public.affiliate_streamer_claims c
    JOIN public.affiliate_accounts a ON a.twitch_login = c.affiliate_twitch_login
    JOIN public.twitch_partners p ON p.twitch_user_id = a.twitch_user_id
    WHERE c.claimed_streamer_user_id = target_twitch_user_id
        AND a.is_active <> 0 AND a.twitch_user_id <> target_twitch_user_id
        AND p.status = 'active' AND p.admin_archived_at IS NULL
        AND p.departnered_at IS NULL AND COALESCE(p.manual_partner_opt_out, 0) = 0
        AND EXISTS (SELECT 1 FROM public.twitch_partners target
            WHERE target.twitch_user_id = target_twitch_user_id AND target.status = 'active'
                AND target.admin_archived_at IS NULL AND target.departnered_at IS NULL
                AND COALESCE(target.manual_partner_opt_out, 0) = 0)
    FOR SHARE OF c, a, p
$claims$;
REVOKE ALL ON FUNCTION public.twitch_referral_claim_candidates(text) FROM PUBLIC;

-- Locking a partner row requires UPDATE rights on PostgreSQL. Keep those rights
-- out of the contest role; this function only locks and returns an active login.
CREATE FUNCTION public.twitch_clip_contest_active_partner(target_twitch_user_id text)
RETURNS TABLE(twitch_login text) LANGUAGE sql SECURITY DEFINER SET search_path = pg_catalog, public AS $active_partner$
    SELECT p.twitch_login FROM public.twitch_partners p
    WHERE p.twitch_user_id = target_twitch_user_id
      AND status = 'active' AND admin_archived_at IS NULL AND departnered_at IS NULL
      AND COALESCE(manual_partner_opt_out, 0) = 0
    FOR SHARE
$active_partner$;
REVOKE ALL ON FUNCTION public.twitch_clip_contest_active_partner(text) FROM PUBLIC;
SELECT public.twitch_apply_partner_challenge_roles();
