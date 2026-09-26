\set ON_ERROR_STOP on
-- The category service has no access to bot auth, web sessions or unrelated data.
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
DECLARE membership record; relation record; table_name text;
BEGIN
    FOR membership IN SELECT granted.rolname FROM pg_auth_members m
        JOIN pg_roles granted ON granted.oid=m.roleid
        JOIN pg_roles member ON member.oid=m.member WHERE member.rolname='twitchcollector'
    LOOP EXECUTE format('REVOKE %I FROM twitchcollector',membership.rolname); END LOOP;
    -- Override the legacy matrix's broad grants on every category table,
    -- including existing daily partitions. Future partitions have no public ACL.
    FOR relation IN SELECT c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
        WHERE n.nspname='public' AND c.relkind IN ('r','p') AND c.relname LIKE 'category\_%' ESCAPE '\'
    LOOP
        EXECUTE format('REVOKE ALL ON TABLE public.%I FROM twitchbot,twitchdash,twitchlegacy',relation.relname);
    END LOOP;
    FOREACH table_name IN ARRAY ARRAY['category_channels','category_collection_runs','category_stream_snapshots',
        'category_chat_messages','category_chat_dirty','category_chat_rollup','category_collector_status','category_media','category_media_jobs']
    LOOP
        IF to_regclass(format('public.%I',table_name)) IS NOT NULL THEN
            EXECUTE format('GRANT SELECT,INSERT,UPDATE,DELETE ON TABLE public.%I TO twitchcollector',table_name);
        END IF;
    END LOOP;
    FOREACH table_name IN ARRAY ARRAY['category_channels','category_collection_runs','category_stream_snapshots',
        'category_chat_rollup','category_collector_status','category_media']
    LOOP
        IF to_regclass(format('public.%I',table_name)) IS NOT NULL THEN
            EXECUTE format('GRANT SELECT ON TABLE public.%I TO twitchdash',table_name);
        END IF;
    END LOOP;
    IF to_regclass('public.category_channels') IS NOT NULL THEN
        GRANT SELECT ON public.category_channels TO twitchbot;
        GRANT UPDATE(followers_total,followers_checked_at,followers_http_status) ON public.category_channels TO twitchbot;
    END IF;
    -- Archive tables are append-only for the service. Targeted Twitch removal
    -- events use the separately constrained function, never generic DELETE.
    IF to_regclass('public.category_chat_messages') IS NOT NULL THEN
        REVOKE UPDATE,DELETE,TRUNCATE ON public.category_chat_messages FROM twitchcollector;
    END IF;
    IF to_regclass('public.category_chat_redactions') IS NOT NULL THEN
        GRANT SELECT ON public.category_chat_redactions TO twitchcollector;
    END IF;
    IF to_regclass('public.category_collector_config') IS NOT NULL THEN
        REVOKE ALL ON public.category_collector_config FROM twitchcollector;
        GRANT SELECT ON public.category_collector_config TO twitchcollector,twitchdash;
    END IF;
    IF to_regprocedure('public.category_redact_chat_event(text,text,text)') IS NOT NULL THEN
        REVOKE ALL ON FUNCTION public.category_redact_chat_event(text,text,text) FROM PUBLIC,twitchbot,twitchdash,twitchlegacy;
        GRANT EXECUTE ON FUNCTION public.category_redact_chat_event(text,text,text) TO twitchcollector;
    END IF;
    IF to_regprocedure('public.category_prepare_partitions()') IS NOT NULL THEN
        REVOKE ALL ON FUNCTION public.category_prepare_partitions() FROM PUBLIC,twitchbot,twitchdash,twitchlegacy;
        GRANT EXECUTE ON FUNCTION public.category_prepare_partitions() TO twitchcollector;
    END IF;
    IF to_regprocedure('public.category_prune_partitions(integer,bigint)') IS NOT NULL THEN
        REVOKE ALL ON FUNCTION public.category_prune_partitions(integer,bigint) FROM PUBLIC,twitchbot,twitchdash,twitchlegacy;
        GRANT EXECUTE ON FUNCTION public.category_prune_partitions(integer,bigint) TO twitchcollector;
    END IF;
END $$;
