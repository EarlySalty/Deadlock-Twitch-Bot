\set ON_ERROR_STOP on
-- Run only against an isolated database containing the full migrations.
BEGIN;
SET SESSION AUTHORIZATION twitchdash;
SELECT count(*) FROM public.partner_effort_events;
SELECT count(*) FROM public.twitch_clip_contest_months;
DO $reader$
BEGIN
    BEGIN
        INSERT INTO public.twitch_clip_contest_months(contest_month) VALUES ('2000-01-01');
        RAISE EXCEPTION 'reader could insert a contest month';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
    BEGIN
        SET LOCAL ROLE twitchcontest;
        RAISE EXCEPTION 'reader could enter contest writer role';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
    IF has_table_privilege(current_user, 'public.partner_effort_events', 'INSERT') THEN
        RAISE EXCEPTION 'reader can insert effort events';
    END IF;
END
$reader$;
RESET SESSION AUTHORIZATION;
SET SESSION AUTHORIZATION twitchcontest;
INSERT INTO public.twitch_clip_contest_months(contest_month) VALUES ('2000-01-01');
UPDATE public.twitch_clip_contest_months SET finalized_at=clock_timestamp() WHERE contest_month='2000-01-01';
SELECT * FROM public.twitch_clip_contest_active_partner('999999999999');
DO $writer$
BEGIN
    IF has_table_privilege(current_user, 'public.affiliate_accounts', 'SELECT')
       OR has_table_privilege(current_user, 'public.partner_effort_events', 'INSERT')
       OR has_table_privilege(current_user, 'public.dashboard_sessions', 'DELETE')
       OR has_table_privilege(current_user, 'public.twitch_partners', 'UPDATE') THEN
        RAISE EXCEPTION 'contest role exceeds its write boundary';
    END IF;
END
$writer$;
RESET SESSION AUTHORIZATION;
SET SESSION AUTHORIZATION twitchbot;
SELECT count(*) FROM public.category_collector_config;
SELECT * FROM public.twitch_referral_claim_candidates('999999999999');
DO $bot$
BEGIN
    IF has_table_privilege(current_user, 'public.affiliate_accounts', 'SELECT') THEN
        RAISE EXCEPTION 'bot can read private affiliate rows';
    END IF;
END
$bot$;
ROLLBACK;
