\set ON_ERROR_STOP on
BEGIN;
SET LOCAL statement_timeout = '20s';
DO $$ BEGIN
    IF current_database() <> 'twitch_all_live_test' THEN
        RAISE EXCEPTION 'Nur die eigene Wegwerf-Datenbank ist erlaubt';
    END IF;
    IF to_regclass('public.twitch_watchdog_incidents') IS NULL
        OR pg_get_serial_sequence('public.twitch_watchdog_incidents', 'id') <> 'public.twitch_watchdog_incidents_id_seq'
        OR NOT EXISTS (SELECT 1 FROM pg_attribute WHERE attrelid='public.twitch_watchdog_incidents'::regclass AND attname='id' AND attidentity='a')
        OR NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='twitchcollector') THEN
        RAISE EXCEPTION 'Autoritative Watchdogtabelle, Identitysequenz oder Collectorrolle fehlt';
    END IF;
END $$;
\ir twitch-runtime-roles.sql
\ir twitch-runtime-roles.sql
DO $$
DECLARE
    community_table text;
BEGIN
    FOREACH community_table IN ARRAY ARRAY[
        'twitch_community_points_viewer_daily', 'twitch_community_points_streamer_daily',
        'twitch_community_points_discoveries',
        'twitch_clip_contest_forwards', 'twitch_scout_community_suggestions'
    ] LOOP
        IF NOT has_table_privilege('twitchdash', community_table, 'SELECT') THEN
            RAISE EXCEPTION 'Dashboard kann % nicht lesen', community_table;
        END IF;
        IF has_table_privilege('twitchdash', community_table, 'INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER') THEN
            RAISE EXCEPTION 'Dashboard kann % verändern', community_table;
        END IF;
        IF NOT (has_table_privilege('twitchbot',community_table,'INSERT') AND has_table_privilege('twitchbot',community_table,'UPDATE')) THEN
            RAISE EXCEPTION 'Producer kann % nicht schreiben', community_table;
        END IF;
    END LOOP;
    IF has_sequence_privilege('twitchdash','twitch_scout_community_suggestions_id_seq','USAGE,UPDATE') THEN
        RAISE EXCEPTION 'Dashboard kann Vorschlagssequenz verändern';
    END IF;
    IF NOT has_sequence_privilege('twitchbot','twitch_scout_community_suggestions_id_seq','USAGE') THEN
        RAISE EXCEPTION 'Producer kann Vorschlagssequenz nicht verwenden';
    END IF;
    IF EXISTS (SELECT 1 FROM pg_auth_members WHERE member = (SELECT oid FROM pg_roles WHERE rolname = 'twitchdash')) THEN
        RAISE EXCEPTION 'Dashboard kann fremde Schreibrollen übernehmen';
    END IF;
    IF NOT (has_table_privilege('twitchdash','twitch_clips_social_media','SELECT') AND has_table_privilege('twitchdash','twitch_clips_social_media','UPDATE')) THEN
        RAISE EXCEPTION 'Bestehende Clipverwaltung verlor legitime Rechte';
    END IF;
    FOREACH community_table IN ARRAY ARRAY['twitch_community_points_dirty_days','twitch_scout_community_privacy','twitch_scout_erased_replay_keys'] LOOP
        IF to_regclass(format('public.%I',community_table)) IS NULL
            OR has_table_privilege('twitchdash',community_table,'SELECT,INSERT,UPDATE,DELETE') THEN
            RAISE EXCEPTION 'Technische Community-/Privacytabelle % fehlt oder ist fürs Dashboard freigegeben',community_table;
        END IF;
    END LOOP;
    IF NOT has_table_privilege('twitchbot','twitch_community_points_dirty_days','DELETE')
        OR NOT has_table_privilege('twitchbot','twitch_scout_community_privacy','UPDATE')
        OR NOT has_table_privilege('twitchbot','twitch_scout_erased_replay_keys','INSERT') THEN
        RAISE EXCEPTION 'Producer fehlen technische Community-/Privacyrechte';
    END IF;
END $$;
SET LOCAL ROLE twitchbot;
INSERT INTO twitch_clip_contest_forwards(clip_id,clip_url,broadcaster_twitch_id,broadcaster_login,via,status)
VALUES ('RoleProbe','https://clips.twitch.tv/RoleProbe','456','synthetic','dashboard','accepted');
RESET ROLE;
SET LOCAL ROLE twitchdash;
SELECT status FROM twitch_clip_contest_forwards WHERE clip_id='RoleProbe';
DO $$
DECLARE
    community_table text;
    stamp_column text;
BEGIN
    FOREACH community_table IN ARRAY ARRAY[
        'twitch_community_points_viewer_daily', 'twitch_community_points_streamer_daily',
        'twitch_community_points_discoveries',
        'twitch_clip_contest_forwards', 'twitch_scout_community_suggestions'
    ] LOOP
        EXECUTE format('SELECT COUNT(*) FROM public.%I',community_table);
        stamp_column := CASE WHEN community_table='twitch_scout_community_suggestions' THEN 'created_at'
            WHEN community_table='twitch_community_points_discoveries' THEN 'first_seen_at' ELSE 'updated_at' END;
        BEGIN
            EXECUTE format('INSERT INTO public.%I DEFAULT VALUES',community_table);
            RAISE EXCEPTION 'Dashboard-Insert in % unerwartet erlaubt',community_table;
        EXCEPTION WHEN insufficient_privilege THEN NULL;
        END;
        BEGIN
            EXECUTE format('UPDATE public.%I SET %I=DEFAULT',community_table,stamp_column);
            RAISE EXCEPTION 'Dashboard-Update in % unerwartet erlaubt',community_table;
        EXCEPTION WHEN insufficient_privilege THEN NULL;
        END;
        BEGIN
            EXECUTE format('DELETE FROM public.%I',community_table);
            RAISE EXCEPTION 'Dashboard-Delete in % unerwartet erlaubt',community_table;
        EXCEPTION WHEN insufficient_privilege THEN NULL;
        END;
    END LOOP;
    BEGIN
        PERFORM nextval('twitch_scout_community_suggestions_id_seq');
        RAISE EXCEPTION 'Dashboard-Sequenzzugriff unerwartet erlaubt';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
END $$;
RESET ROLE;
SET LOCAL ROLE twitchcollector;
INSERT INTO public.twitch_watchdog_incidents(service,started_at)
VALUES ('deadlock-twitch-bot-rust.service','2026-10-03 00:00Z');
UPDATE public.twitch_watchdog_incidents SET notified_at='2026-10-03 00:01Z'
WHERE service='deadlock-twitch-bot-rust.service';
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM public.twitch_watchdog_incidents WHERE notified_at='2026-10-03 00:01Z') THEN
        RAISE EXCEPTION 'Collector kann Watchdogincident nicht schreiben oder lesen';
    END IF;
    IF NOT (has_sequence_privilege(current_user,'public.twitch_watchdog_incidents_id_seq','USAGE')
        AND has_sequence_privilege(current_user,'public.twitch_watchdog_incidents_id_seq','SELECT')) THEN
        RAISE EXCEPTION 'Collector fehlen Identitysequenzrechte';
    END IF;
    BEGIN
        DELETE FROM public.twitch_watchdog_incidents;
        RAISE EXCEPTION 'Collector-Delete unerwartet erlaubt';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
END $$;
RESET ROLE;
ROLLBACK;
