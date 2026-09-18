-- The category collector has its own database identity. Runtime permissions
-- must remain restricted even when the broad legacy matrix is reapplied.
DO $category_collector$
DECLARE
    table_name text;
    sequence_name text;
BEGIN
    IF to_regclass('public.category_collector_config') IS NULL THEN RETURN; END IF;
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname='twitchcollector') THEN
        CREATE ROLE twitchcollector LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
    END IF;
    ALTER ROLE twitchcollector NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
    EXECUTE format('GRANT CONNECT ON DATABASE %I TO twitchcollector',current_database());
    GRANT USAGE ON SCHEMA public TO twitchcollector;
    REVOKE CREATE ON SCHEMA public FROM twitchcollector;
    EXECUTE format('ALTER ROLE twitchcollector IN DATABASE %I SET search_path=public,pg_catalog',current_database());
    FOR table_name IN SELECT tablename FROM pg_tables WHERE schemaname='public' AND left(tablename,9)='category_' LOOP
        EXECUTE format('REVOKE ALL ON TABLE public.%I FROM twitchbot,twitchdash,twitchlegacy,twitchcollector',table_name);
    END LOOP;
    FOR sequence_name IN SELECT sequencename FROM pg_sequences WHERE schemaname='public' AND left(sequencename,9)='category_' LOOP
        EXECUTE format('REVOKE ALL ON SEQUENCE public.%I FROM twitchbot,twitchdash,twitchlegacy,twitchcollector',sequence_name);
        EXECUTE format('GRANT USAGE,SELECT ON SEQUENCE public.%I TO twitchcollector',sequence_name);
    END LOOP;
    GRANT SELECT ON category_collector_config TO twitchcollector;
    GRANT UPDATE(deadlock_game_id,updated_at) ON category_collector_config TO twitchcollector;
    GRANT SELECT,INSERT,UPDATE ON category_polls,category_channels,category_media_items TO twitchcollector;
    GRANT SELECT,INSERT ON category_stream_snapshots TO twitchcollector;
    GRANT SELECT,INSERT,UPDATE,DELETE ON category_chat_messages,category_chat_dirty_hours TO twitchcollector;
    GRANT SELECT,INSERT,DELETE ON category_chat_rollup TO twitchcollector;
    GRANT SELECT,UPDATE ON category_collector_status TO twitchcollector;
    GRANT SELECT ON category_polls,category_channels,category_stream_snapshots,category_chat_rollup,
        category_collector_config,category_collector_status,category_chat_dirty_hours TO twitchdash;
    GRANT SELECT(room_user_id,source_room_id,sent_at,detected_lang) ON category_chat_messages TO twitchdash;
    -- The dashboard cannot read raw messages, tags or individual chat identities.
END
$category_collector$;
