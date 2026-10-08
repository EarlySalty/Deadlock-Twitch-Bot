\set ON_ERROR_STOP on
BEGIN;
SET LOCAL statement_timeout='20s';
DO $$ BEGIN
    IF current_database() NOT LIKE 'categorytest_%' AND current_database() <> 'twitch_all_live_test' THEN
        RAISE EXCEPTION 'This test may only run in the disposable category test database';
    END IF;
END $$;
CREATE TABLE IF NOT EXISTS unrelated_private_fixture(secret text);
GRANT SELECT,INSERT,UPDATE,DELETE ON ALL TABLES IN SCHEMA public TO twitchbot,twitchdash,twitchlegacy;
REVOKE ALL ON public.category_collection_runs, public.category_stream_snapshots, public.category_collector_status FROM twitchbot;
\ir ../../rust/migrations/20260930140000_category_evidence_readiness_roles.sql
DO $$
BEGIN
    IF NOT has_table_privilege('twitchbot','category_collection_runs','SELECT') OR NOT has_table_privilege('twitchbot','category_collector_status','SELECT') OR NOT has_table_privilege('twitchbot','category_stream_snapshots','SELECT') THEN RAISE EXCEPTION 'additive migration did not restore the three source reads'; END IF;
END $$;
\ir twitch-runtime-roles.sql
\ir twitch-runtime-roles.sql
ALTER DEFAULT PRIVILEGES FOR ROLE postgres IN SCHEMA public
    GRANT SELECT,INSERT,UPDATE,DELETE ON TABLES TO twitchbot,twitchdash,twitchlegacy;
DO $$
BEGIN
    EXECUTE format('DROP TABLE public.%I','category_chat_messages_p' || to_char((now() AT TIME ZONE 'UTC')::date + 1,'YYYYMMDD'));
END $$;
SELECT category_prepare_partitions();
DO $$
DECLARE child record; relation_name text; role_name text;
BEGIN
    IF NOT has_table_privilege('twitchcollector','category_chat_messages','INSERT') THEN RAISE EXCEPTION 'collector cannot ingest'; END IF;
    IF has_table_privilege('twitchcollector','unrelated_private_fixture','SELECT') THEN RAISE EXCEPTION 'collector can read unrelated data'; END IF;
    IF has_table_privilege('twitchdash','category_chat_messages','SELECT') THEN RAISE EXCEPTION 'dashboard has raw chat access'; END IF;
    IF NOT has_table_privilege('twitchbot','category_chat_messages','SELECT') OR NOT has_table_privilege('twitchbot','category_chat_messages','INSERT') THEN RAISE EXCEPTION 'native bot cannot ingest raw chat'; END IF;
    IF has_table_privilege('twitchbot','category_chat_messages','UPDATE') OR has_table_privilege('twitchbot','category_chat_messages','DELETE') OR has_table_privilege('twitchbot','category_chat_messages','TRUNCATE') THEN RAISE EXCEPTION 'native bot can mutate raw chat'; END IF;
    IF has_table_privilege('twitchlegacy','category_chat_messages','SELECT') THEN RAISE EXCEPTION 'legacy has raw chat access'; END IF;
    IF NOT has_table_privilege('twitchdash','category_chat_rollup','SELECT') THEN RAISE EXCEPTION 'dashboard cannot report'; END IF;
    IF has_table_privilege('twitchdash','category_chat_rollup','UPDATE') THEN RAISE EXCEPTION 'dashboard can alter evidence'; END IF;
    IF NOT has_column_privilege('twitchbot','category_channels','followers_total','UPDATE') THEN RAISE EXCEPTION 'bot cannot enrich follower total'; END IF;
    IF NOT has_column_privilege('twitchbot','category_channels','description','UPDATE') THEN RAISE EXCEPTION 'native bot cannot enrich public profile fields'; END IF;
    IF NOT has_table_privilege('twitchbot','category_collector_config','SELECT') THEN RAISE EXCEPTION 'bot lost category collector configuration read access'; END IF;
    IF NOT has_table_privilege('twitchbot','category_collection_runs','SELECT') THEN RAISE EXCEPTION 'bot cannot read collection coverage'; END IF;
    IF NOT has_table_privilege('twitchbot','category_collector_status','SELECT') THEN RAISE EXCEPTION 'bot cannot read collector health'; END IF;
    IF NOT has_table_privilege('twitchbot','category_stream_snapshots','SELECT') THEN RAISE EXCEPTION 'bot cannot read stream evidence'; END IF;
    IF NOT has_table_privilege('twitchdash','category_collection_runs','SELECT') OR NOT has_table_privilege('twitchdash','category_collector_status','SELECT') OR NOT has_table_privilege('twitchdash','category_stream_snapshots','SELECT') OR NOT has_table_privilege('twitchdash','category_collector_config','SELECT') THEN RAISE EXCEPTION 'dashboard lost its category read access'; END IF;
    IF NOT has_table_privilege('twitchbot','category_collection_runs','INSERT') OR has_table_privilege('twitchbot','category_collection_runs','UPDATE') OR has_table_privilege('twitchbot','category_collection_runs','DELETE') THEN RAISE EXCEPTION 'native collection coverage is not append-only'; END IF;
    IF NOT has_table_privilege('twitchbot','category_collector_status','INSERT') OR NOT has_table_privilege('twitchbot','category_collector_status','UPDATE') OR has_table_privilege('twitchbot','category_collector_status','DELETE') THEN RAISE EXCEPTION 'native bot heartbeat grants invalid'; END IF;
    IF NOT has_table_privilege('twitchbot','category_stream_snapshots','INSERT') OR has_table_privilege('twitchbot','category_stream_snapshots','UPDATE') OR has_table_privilege('twitchbot','category_stream_snapshots','DELETE') THEN RAISE EXCEPTION 'native snapshots are not append-only'; END IF;
    IF NOT has_function_privilege('twitchbot','category_prepare_partitions()','EXECUTE') OR NOT has_function_privilege('twitchbot','category_redact_chat_event(text,text,text,timestamptz)','EXECUTE') OR NOT has_function_privilege('twitchbot','category_lock_chat_rooms(text[])','EXECUTE') THEN RAISE EXCEPTION 'native bot lacks constrained maintenance functions'; END IF;
    IF has_function_privilege('twitchbot','category_redact_chat_event_locked(text,text,text,timestamptz)','EXECUTE') OR has_function_privilege('twitchbot','category_prune_partitions(integer,bigint)','EXECUTE') THEN RAISE EXCEPTION 'native bot can bypass moderation constraints or prune'; END IF;
    IF NOT has_function_privilege('twitchcollector','category_prepare_partitions()','EXECUTE') THEN RAISE EXCEPTION 'collector cannot maintain partitions'; END IF;
    IF has_function_privilege('twitchdash','category_prune_partitions(integer,bigint)','EXECUTE') THEN RAISE EXCEPTION 'dashboard can prune raw data'; END IF;
    IF NOT has_function_privilege('twitchbot','category_redact_chat_event(text,text,text,timestamptz)','EXECUTE') THEN RAISE EXCEPTION 'bot cannot honor targeted moderation'; END IF;
    IF has_function_privilege('twitchbot','category_redact_chat_event_locked(text,text,text,timestamptz)','EXECUTE') THEN RAISE EXCEPTION 'bot can bypass redaction locks'; END IF;
    IF has_function_privilege('twitchbot','category_prune_partitions(integer,bigint)','EXECUTE') THEN RAISE EXCEPTION 'bot can prune raw data'; END IF;
    IF has_table_privilege('twitchbot','category_chat_messages','UPDATE') OR has_table_privilege('twitchbot','category_chat_messages','DELETE') OR has_table_privilege('twitchbot','category_chat_messages','TRUNCATE') THEN RAISE EXCEPTION 'bot can change raw archive'; END IF;
    IF has_table_privilege('twitchbot','category_stream_snapshots','UPDATE') OR has_table_privilege('twitchbot','category_stream_snapshots','DELETE') THEN RAISE EXCEPTION 'bot can change stream archive'; END IF;
    FOREACH relation_name IN ARRAY ARRAY['category_native_runtime','twitch_watchdog_incidents',
        'category_watchdog_suspensions','category_watchdog_storage_incidents','category_watchdog_storage_notifications'] LOOP
        IF NOT has_table_privilege('twitchbot',relation_name,'SELECT') OR
            NOT has_table_privilege('twitchbot',relation_name,'INSERT') OR
            NOT has_table_privilege('twitchbot',relation_name,'UPDATE') OR
            has_table_privilege('twitchbot',relation_name,'DELETE,TRUNCATE,REFERENCES,TRIGGER') THEN
            RAISE EXCEPTION 'native watchdog grants invalid for %',relation_name;
        END IF;
        FOREACH role_name IN ARRAY ARRAY['twitchdash','twitchlegacy','twitchcollector'] LOOP
            IF has_table_privilege(role_name,relation_name,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER') THEN
                RAISE EXCEPTION 'foreign watchdog access for % on %',role_name,relation_name;
            END IF;
        END LOOP;
    END LOOP;
    FOREACH relation_name IN ARRAY ARRAY['twitch_watchdog_incidents_id_seq','category_watchdog_suspensions_id_seq',
        'category_watchdog_storage_incidents_id_seq'] LOOP
        IF NOT has_sequence_privilege('twitchbot',relation_name,'USAGE') OR
            has_sequence_privilege('twitchbot',relation_name,'UPDATE') THEN
            RAISE EXCEPTION 'watchdog sequence grants invalid for %',relation_name;
        END IF;
        FOREACH role_name IN ARRAY ARRAY['twitchdash','twitchlegacy','twitchcollector'] LOOP
            IF has_sequence_privilege(role_name,relation_name,'USAGE,SELECT,UPDATE') THEN
                RAISE EXCEPTION 'foreign watchdog sequence access for % on %',role_name,relation_name;
            END IF;
        END LOOP;
    END LOOP;
    FOR child IN SELECT inhrelid::regclass AS relation FROM pg_inherits WHERE inhparent='category_chat_messages'::regclass LOOP
        IF has_table_privilege('twitchdash',child.relation,'SELECT') OR has_table_privilege('twitchlegacy',child.relation,'SELECT') OR has_table_privilege('twitchbot',child.relation,'UPDATE') OR has_table_privilege('twitchbot',child.relation,'DELETE') OR has_table_privilege('twitchbot',child.relation,'TRUNCATE') THEN
            RAISE EXCEPTION 'raw partition has leaked read permissions';
        END IF;
    END LOOP;
END $$;
SELECT 'Category role isolation assertions passed; all changes will be rolled back.' AS result;
ROLLBACK;
