\set ON_ERROR_STOP on
BEGIN;
SET LOCAL statement_timeout='20s';
DO $$ BEGIN
    IF current_database() NOT LIKE 'categorytest_%' THEN
        RAISE EXCEPTION 'This test may only run in the disposable category test database';
    END IF;
END $$;
CREATE TABLE IF NOT EXISTS unrelated_private_fixture(secret text);
-- Reproduce broad grants that the existing runtime migration matrix applies.
GRANT SELECT,INSERT,UPDATE,DELETE ON ALL TABLES IN SCHEMA public TO twitchbot,twitchdash,twitchlegacy;
\ir category-runtime-roles.sql
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
