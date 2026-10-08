CREATE TABLE category_native_processes (
    process_id bigint PRIMARY KEY,
    heartbeat_at timestamptz NOT NULL,
    details jsonb NOT NULL
);

CREATE TABLE category_native_runtime (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    heartbeat_at timestamptz NOT NULL,
    details jsonb NOT NULL
);

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
            EXECUTE format('GRANT SELECT,INSERT ON TABLE public.category_collection_runs,public.category_stream_snapshots,public.category_chat_messages TO %I',role_name);
            EXECUTE format('GRANT SELECT,INSERT,DELETE ON TABLE public.category_chat_dirty TO %I',role_name);
            EXECUTE format('GRANT UPDATE(hour_at) ON TABLE public.category_chat_dirty TO %I',role_name);
            EXECUTE format('GRANT SELECT ON TABLE public.category_collector_config,public.category_chat_redactions,public.category_chat_user_redactions TO %I',role_name);
            EXECUTE format('GRANT EXECUTE ON FUNCTION public.category_prepare_partitions(),public.category_lock_chat_rooms(text[]),public.category_redact_chat_event(text,text,text,timestamptz) TO %I',role_name);
        END IF;
    END LOOP;
    IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname='twitchbot') THEN
        GRANT SELECT,INSERT,UPDATE ON TABLE public.category_native_runtime,public.category_native_processes TO twitchbot;
    END IF;
    IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname='twitchdash') THEN
        GRANT SELECT ON TABLE public.category_channels,public.category_collection_runs,
            public.category_stream_snapshots,public.category_chat_rollup,
            public.category_collector_status,public.category_media,public.category_collector_config TO twitchdash;
    END IF;
END $$;
