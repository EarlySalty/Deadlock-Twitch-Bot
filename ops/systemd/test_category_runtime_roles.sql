\set ON_ERROR_STOP on
BEGIN;
SET LOCAL statement_timeout='20s';
DO $$ BEGIN
    IF current_database() NOT LIKE 'categorytest_%' AND current_database() <> 'twitch_all_live_test' THEN
        RAISE EXCEPTION 'This test may only run in the disposable category test database';
    END IF;
END $$;
CREATE TABLE IF NOT EXISTS unrelated_private_fixture(secret text);
-- Reproduce broad grants that the existing runtime migration matrix applies.
GRANT SELECT,INSERT,UPDATE,DELETE ON ALL TABLES IN SCHEMA public TO twitchbot,twitchdash,twitchlegacy;
REVOKE ALL ON public.category_collection_runs, public.category_stream_snapshots, public.category_collector_status FROM twitchbot;
\ir ../../rust/migrations/20260930140000_category_evidence_readiness_roles.sql
DO $$
BEGIN
    IF NOT has_table_privilege('twitchbot','category_collection_runs','SELECT') OR NOT has_table_privilege('twitchbot','category_collector_status','SELECT') OR NOT has_table_privilege('twitchbot','category_stream_snapshots','SELECT') THEN RAISE EXCEPTION 'additive migration did not restore the three source reads'; END IF;
END $$;
\ir twitch-runtime-roles.sql
\ir twitch-runtime-roles.sql
DO $$
DECLARE child record;
BEGIN
    IF NOT has_table_privilege('twitchcollector','category_chat_messages','INSERT') THEN RAISE EXCEPTION 'collector cannot ingest'; END IF;
    IF has_table_privilege('twitchcollector','unrelated_private_fixture','SELECT') THEN RAISE EXCEPTION 'collector can read unrelated data'; END IF;
    IF has_table_privilege('twitchdash','category_chat_messages','SELECT') THEN RAISE EXCEPTION 'dashboard has raw chat access'; END IF;
    IF has_table_privilege('twitchbot','category_chat_messages','SELECT') THEN RAISE EXCEPTION 'bot has raw chat access'; END IF;
    IF has_table_privilege('twitchlegacy','category_chat_messages','SELECT') THEN RAISE EXCEPTION 'legacy has raw chat access'; END IF;
    IF NOT has_table_privilege('twitchdash','category_chat_rollup','SELECT') THEN RAISE EXCEPTION 'dashboard cannot report'; END IF;
    IF has_table_privilege('twitchdash','category_chat_rollup','UPDATE') THEN RAISE EXCEPTION 'dashboard can alter evidence'; END IF;
    IF NOT has_column_privilege('twitchbot','category_channels','followers_total','UPDATE') THEN RAISE EXCEPTION 'bot cannot enrich follower total'; END IF;
    IF has_column_privilege('twitchbot','category_channels','description','UPDATE') THEN RAISE EXCEPTION 'bot can alter unrelated profile fields'; END IF;
    IF NOT has_table_privilege('twitchbot','category_collector_config','SELECT') THEN RAISE EXCEPTION 'bot lost category collector configuration read access'; END IF;
    IF NOT has_table_privilege('twitchbot','category_collection_runs','SELECT') THEN RAISE EXCEPTION 'bot cannot read collection coverage'; END IF;
    IF NOT has_table_privilege('twitchbot','category_collector_status','SELECT') THEN RAISE EXCEPTION 'bot cannot read collector health'; END IF;
    IF NOT has_table_privilege('twitchbot','category_stream_snapshots','SELECT') THEN RAISE EXCEPTION 'bot cannot read stream evidence'; END IF;
    IF NOT has_table_privilege('twitchdash','category_collection_runs','SELECT') OR NOT has_table_privilege('twitchdash','category_collector_status','SELECT') OR NOT has_table_privilege('twitchdash','category_stream_snapshots','SELECT') OR NOT has_table_privilege('twitchdash','category_collector_config','SELECT') THEN RAISE EXCEPTION 'dashboard lost its category read access'; END IF;
    IF has_table_privilege('twitchbot','category_collection_runs','INSERT') OR has_table_privilege('twitchbot','category_collection_runs','UPDATE') OR has_table_privilege('twitchbot','category_collection_runs','DELETE') THEN RAISE EXCEPTION 'bot can mutate collection coverage'; END IF;
    IF has_table_privilege('twitchbot','category_collector_status','INSERT') OR has_table_privilege('twitchbot','category_collector_status','UPDATE') OR has_table_privilege('twitchbot','category_collector_status','DELETE') THEN RAISE EXCEPTION 'bot can mutate collector health'; END IF;
    IF has_table_privilege('twitchbot','category_stream_snapshots','INSERT') OR has_table_privilege('twitchbot','category_stream_snapshots','UPDATE') OR has_table_privilege('twitchbot','category_stream_snapshots','DELETE') THEN RAISE EXCEPTION 'bot can mutate stream evidence'; END IF;
    IF NOT has_function_privilege('twitchcollector','category_prepare_partitions()','EXECUTE') THEN RAISE EXCEPTION 'collector cannot maintain partitions'; END IF;
    IF has_function_privilege('twitchdash','category_prune_partitions(integer,bigint)','EXECUTE') THEN RAISE EXCEPTION 'dashboard can prune raw data'; END IF;
    FOR child IN SELECT inhrelid::regclass AS relation FROM pg_inherits WHERE inhparent='category_chat_messages'::regclass LOOP
        IF has_table_privilege('twitchdash',child.relation,'SELECT') OR has_table_privilege('twitchbot',child.relation,'SELECT') THEN
            RAISE EXCEPTION 'raw partition has leaked read permissions';
        END IF;
    END LOOP;
END $$;
SELECT 'Category role isolation assertions passed; all changes will be rolled back.' AS result;
ROLLBACK;
