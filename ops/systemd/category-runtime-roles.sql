\set ON_ERROR_STOP on
DO $$ BEGIN
    IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='twitchcollector') THEN
        CREATE ROLE twitchcollector LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
    END IF;
END $$;
ALTER ROLE twitchcollector LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
ALTER ROLE twitchcollector PASSWORD NULL;
ALTER ROLE twitchcollector RESET ALL;
ALTER ROLE twitchcollector IN DATABASE twitch_analytics SET search_path=pg_catalog,public;
GRANT CONNECT ON DATABASE twitch_analytics TO twitchcollector;
GRANT USAGE ON SCHEMA public TO twitchcollector;
REVOKE CREATE ON SCHEMA public FROM twitchcollector;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM twitchcollector;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public FROM twitchcollector;
DO $$
DECLARE membership record;
BEGIN
    FOR membership IN SELECT granted.rolname FROM pg_auth_members m
        JOIN pg_roles granted ON granted.oid=m.roleid
        JOIN pg_roles member ON member.oid=m.member WHERE member.rolname='twitchcollector'
    LOOP EXECUTE format('REVOKE %I FROM twitchcollector',membership.rolname); END LOOP;
END $$;
CREATE OR REPLACE FUNCTION category_prepare_partitions() RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public AS $$
DECLARE d date; n text; role_name text;
BEGIN
    FOR d IN SELECT ((now() AT TIME ZONE 'UTC')::date + x) FROM generate_series(-1,1) AS x LOOP
        n := 'category_chat_messages_p' || to_char(d,'YYYYMMDD');
        EXECUTE format('CREATE TABLE IF NOT EXISTS public.%I PARTITION OF public.category_chat_messages FOR VALUES FROM (%L) TO (%L)',
            n, d::text || ' 00:00:00+00', (d+1)::text || ' 00:00:00+00');
        EXECUTE format('REVOKE ALL ON TABLE public.%I FROM PUBLIC', n);
        FOREACH role_name IN ARRAY ARRAY['twitchbot','twitchdash','twitchlegacy','twitchcollector'] LOOP
            IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname=role_name) THEN
                EXECUTE format('REVOKE ALL ON TABLE public.%I FROM %I', n, role_name);
            END IF;
        END LOOP;
    END LOOP;
END $$;
REVOKE ALL ON FUNCTION category_prepare_partitions() FROM PUBLIC;

DO $$
DECLARE role_name text; relation record; signature text;
BEGIN
    FOR relation IN SELECT c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
        WHERE n.nspname='public' AND c.relkind IN ('r','p') AND c.relname LIKE 'category\_%' ESCAPE '\'
    LOOP
        EXECUTE format('REVOKE ALL ON TABLE public.%I FROM PUBLIC',relation.relname);
        FOREACH role_name IN ARRAY ARRAY['twitchbot','twitchdash','twitchlegacy','twitchcollector'] LOOP
            IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname=role_name) THEN
                EXECUTE format('REVOKE ALL ON TABLE public.%I FROM %I',relation.relname,role_name);
            END IF;
        END LOOP;
    END LOOP;
    FOREACH signature IN ARRAY ARRAY['category_prepare_partitions()',
        'category_prune_partitions(integer,bigint)','category_lock_chat_rooms(text[])',
        'category_redact_chat_event(text,text,text,timestamptz)',
        'category_redact_chat_event_locked(text,text,text,timestamptz)']
    LOOP
        EXECUTE format('REVOKE ALL ON FUNCTION public.%s FROM PUBLIC',signature);
        FOREACH role_name IN ARRAY ARRAY['twitchbot','twitchdash','twitchlegacy','twitchcollector'] LOOP
            IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname=role_name) THEN
                EXECUTE format('REVOKE ALL ON FUNCTION public.%s FROM %I',signature,role_name);
            END IF;
        END LOOP;
    END LOOP;
    FOREACH role_name IN ARRAY ARRAY['twitchbot','twitchcollector'] LOOP
        IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname=role_name) THEN
            EXECUTE format('GRANT SELECT,INSERT,UPDATE ON TABLE public.category_channels,public.category_collector_status,public.category_chat_rollup,public.category_media,public.category_media_jobs TO %I',role_name);
            EXECUTE format('GRANT SELECT,INSERT ON TABLE public.category_collection_runs,public.category_chat_messages TO %I',role_name);
            IF to_regclass('public.category_stream_snapshots') IS NOT NULL THEN
                EXECUTE format('GRANT SELECT,INSERT ON TABLE public.category_stream_snapshots TO %I',role_name);
            END IF;
            IF to_regclass('public.category_snapshot_samples') IS NOT NULL THEN
                EXECUTE format('GRANT SELECT ON TABLE public.category_snapshot_versions,public.category_snapshot_samples,public.category_snapshots_normalized,public.category_snapshots_read,public.category_snapshot_metrics,public.category_snapshot_metric_source,public.category_chat_metrics,public.category_chat_metric_source,public.category_storage_state,public.category_archive_manifest,public.category_archive_members TO %I',role_name);
                EXECUTE format('GRANT EXECUTE ON FUNCTION public.category_snapshot_put(timestamptz,text,text,text,bigint,text,text,timestamptz,text[],text,boolean,double precision),public.category_snapshot_wire(public.category_snapshots_normalized),public.category_chat_wire(public.category_chat_messages) TO %I',role_name);
                EXECUTE format('GRANT UPDATE(writer_snapshot_at) ON public.category_storage_state TO %I',role_name);
                IF role_name='twitchbot' THEN
                    GRANT INSERT,UPDATE ON public.category_archive_manifest TO twitchbot;
                    GRANT INSERT,DELETE ON public.category_archive_members TO twitchbot;
                    GRANT UPDATE(last_alert_at,failures) ON public.category_storage_state TO twitchbot;
                    GRANT SELECT,INSERT,UPDATE ON public.category_archive_notifications TO twitchbot;
                END IF;
            END IF;
            EXECUTE format('GRANT SELECT,INSERT,DELETE ON TABLE public.category_chat_dirty TO %I',role_name);
            EXECUTE format('GRANT UPDATE(hour_at) ON TABLE public.category_chat_dirty TO %I',role_name);
            EXECUTE format('GRANT SELECT ON TABLE public.category_collector_config,public.category_chat_redactions,public.category_chat_user_redactions TO %I',role_name);
            EXECUTE format('GRANT EXECUTE ON FUNCTION public.category_prepare_partitions(),public.category_lock_chat_rooms(text[]),public.category_redact_chat_event(text,text,text,timestamptz) TO %I',role_name);
        END IF;
    END LOOP;
    FOREACH signature IN ARRAY ARRAY['category_native_runtime','category_native_processes',
        'category_watchdog_suspensions','category_watchdog_storage_incidents',
        'category_watchdog_storage_notifications'] LOOP
        IF to_regclass('public.'||signature) IS NOT NULL
            AND EXISTS(SELECT 1 FROM pg_roles WHERE rolname='twitchbot') THEN
            EXECUTE format('GRANT SELECT,INSERT,UPDATE ON TABLE public.%I TO twitchbot',signature);
        END IF;
    END LOOP;
    FOREACH signature IN ARRAY ARRAY['category_watchdog_suspensions_id_seq',
        'category_watchdog_storage_incidents_id_seq'] LOOP
        IF to_regclass('public.'||signature) IS NOT NULL THEN
            EXECUTE format('REVOKE ALL ON SEQUENCE public.%I FROM PUBLIC',signature);
            FOREACH role_name IN ARRAY ARRAY['twitchbot','twitchdash','twitchlegacy','twitchcollector'] LOOP
                IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname=role_name) THEN
                    EXECUTE format('REVOKE ALL ON SEQUENCE public.%I FROM %I',signature,role_name);
                END IF;
            END LOOP;
            IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname='twitchbot') THEN
                EXECUTE format('GRANT USAGE,SELECT ON SEQUENCE public.%I TO twitchbot',signature);
            END IF;
        END IF;
    END LOOP;
    IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname='twitchdash') THEN
        GRANT SELECT ON TABLE public.category_channels,public.category_collection_runs,
            public.category_chat_rollup,
            public.category_collector_status,public.category_media,public.category_collector_config TO twitchdash;
        IF to_regclass('public.category_stream_snapshots') IS NOT NULL THEN
            GRANT SELECT ON public.category_stream_snapshots TO twitchdash;
        END IF;
        IF to_regclass('public.category_snapshot_metric_source') IS NOT NULL THEN
            GRANT SELECT ON public.category_snapshot_metric_source TO twitchdash;
        END IF;
    END IF;
END $$;
