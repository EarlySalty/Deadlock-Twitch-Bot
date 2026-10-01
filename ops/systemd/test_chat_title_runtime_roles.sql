\set ON_ERROR_STOP on
BEGIN;
SET LOCAL statement_timeout='20s';
DO $$ BEGIN
    IF current_database() <> 'twitch_all_live_test' THEN
        RAISE EXCEPTION 'Only the disposable Twitch test database is allowed';
    END IF;
END $$;
\ir twitch-runtime-roles.sql
\ir twitch-runtime-roles.sql
DO $$
BEGIN
    IF NOT (has_table_privilege('twitchbot','tb_chat_brain_answers','SELECT') AND has_table_privilege('twitchbot','tb_chat_brain_answers','INSERT') AND has_table_privilege('twitchbot','tb_chat_brain_answers','UPDATE')) THEN RAISE EXCEPTION 'bot cannot persist Brain answers'; END IF;
    IF has_table_privilege('twitchbot','tb_chat_brain_answers','DELETE') THEN RAISE EXCEPTION 'bot can delete Brain audit'; END IF;
    IF NOT has_sequence_privilege('twitchbot','tb_chat_brain_answers_id_seq','USAGE') THEN RAISE EXCEPTION 'bot cannot allocate Brain audit id'; END IF;
    IF NOT has_table_privilege('twitchdash','tb_chat_brain_answers','SELECT') THEN RAISE EXCEPTION 'dashboard cannot read Brain answers'; END IF;
    IF has_table_privilege('twitchdash','tb_chat_brain_answers','INSERT,UPDATE,DELETE,TRUNCATE') THEN RAISE EXCEPTION 'dashboard can alter Brain answers'; END IF;
    IF has_sequence_privilege('twitchdash','tb_chat_brain_answers_id_seq','USAGE,UPDATE') THEN RAISE EXCEPTION 'dashboard can allocate Brain audit ids'; END IF;
    IF has_table_privilege('twitchlegacy','tb_chat_brain_answers','SELECT,INSERT,UPDATE,DELETE') THEN RAISE EXCEPTION 'legacy can access Brain answers'; END IF;
    IF NOT (has_table_privilege('twitchdash','title_generator_preferences','SELECT') AND has_table_privilege('twitchdash','title_generator_preferences','INSERT') AND has_table_privilege('twitchdash','title_generator_preferences','UPDATE')) THEN RAISE EXCEPTION 'dashboard title preferences path broken'; END IF;
END $$;
SET LOCAL ROLE twitchbot;
INSERT INTO tb_chat_brain_answers(broadcaster_user_id,broadcaster_login,chatter_user_id,message_id,question,status)
VALUES ('100','synthetic','101','chat-title-role-probe','synthetic','Pending');
UPDATE tb_chat_brain_answers SET status='Answered', answer='synthetic' WHERE message_id='chat-title-role-probe';
RESET ROLE;
SET LOCAL ROLE twitchdash;
SELECT count(*) AS readable_brain_answer FROM tb_chat_brain_answers WHERE message_id='chat-title-role-probe';
INSERT INTO title_generator_preferences(twitch_user_id,never_words) VALUES('chat-title-role-probe','verboten');
UPDATE title_generator_preferences SET never_words='weiterhin verboten' WHERE twitch_user_id='chat-title-role-probe';
SELECT never_words FROM title_generator_preferences WHERE twitch_user_id='chat-title-role-probe';
RESET ROLE;
ROLLBACK;
