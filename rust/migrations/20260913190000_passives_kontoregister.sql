ALTER TABLE twitch_zuschauer_register
    ADD COLUMN unauffaellig_seit TIMESTAMPTZ,
    ADD COLUMN vertrauen_widerrufen_am TIMESTAMPTZ,
    ADD COLUMN historie_geprueft_am TIMESTAMPTZ,
    ADD COLUMN radar_meldung_am TIMESTAMPTZ,
    ADD COLUMN radar_vorherige_meldung_am TIMESTAMPTZ,
    ADD COLUMN radar_wiederholungen BIGINT NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS twitch_session_chatters_id_history_idx
    ON twitch_session_chatters (chatter_id, session_id) WHERE messages > 0;
CREATE INDEX IF NOT EXISTS twitch_crew_radar_log_id_created_idx
    ON twitch_crew_radar_log (chatter_id, created_at DESC);

CREATE FUNCTION twitch_widerrufe_kontovertrauen() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    bad BOOLEAN;
    affected_ids TEXT[];
    subject_id TEXT;
BEGIN
    IF TG_OP = 'INSERT' THEN
        affected_ids := ARRAY[NEW.chatter_id];
    ELSIF TG_OP = 'DELETE' THEN
        affected_ids := ARRAY[OLD.chatter_id];
    ELSE
        affected_ids := ARRAY[OLD.chatter_id, NEW.chatter_id];
    END IF;
    FOR subject_id IN
        SELECT DISTINCT id FROM unnest(affected_ids) AS ids(id)
        WHERE NULLIF(BTRIM(id), '') IS NOT NULL ORDER BY id
    LOOP
        INSERT INTO twitch_zuschauer_register
            (twitch_user_id, community_probability, computed_at)
        VALUES (subject_id, 0.2, 'epoch')
        ON CONFLICT (twitch_user_id) DO NOTHING;
        PERFORM 1 FROM twitch_zuschauer_register
            WHERE twitch_user_id = subject_id FOR UPDATE;
        SELECT EXISTS (
            SELECT 1 FROM twitch_spam_review_decisions
                WHERE chatter_id = subject_id AND verdict = 'spam'
        ) OR EXISTS (
            SELECT 1 FROM twitch_scam_guard_verdicts
                WHERE chatter_id = subject_id AND verdict = 'scam' AND action_taken <> 'overturned'
        ) OR EXISTS (
            SELECT 1 FROM twitch_chatter_global_ban WHERE chatter_id = subject_id
        ) OR EXISTS (
            SELECT 1 FROM tb_chat_autoban_log WHERE chatter_id = subject_id
                AND action IN ('ban', 'timeout') AND source_path IN ('spam', 'scam', 'global_ban')
        ) INTO bad;
        IF bad THEN
            UPDATE twitch_zuschauer_register SET
                unauffaellig_seit = NULL, vertrauen_widerrufen_am = NOW()
            WHERE twitch_user_id = subject_id;
        ELSE
            UPDATE twitch_zuschauer_register SET
                unauffaellig_seit = NULL, vertrauen_widerrufen_am = NULL,
                historie_geprueft_am = NULL
            WHERE twitch_user_id = subject_id AND vertrauen_widerrufen_am IS NOT NULL;
        END IF;
    END LOOP;
    RETURN NULL;
END $$;

CREATE TRIGGER twitch_spam_vertrauen_widerrufen
    AFTER INSERT OR UPDATE OR DELETE ON twitch_spam_review_decisions
    FOR EACH ROW EXECUTE FUNCTION twitch_widerrufe_kontovertrauen();
CREATE TRIGGER twitch_scam_vertrauen_widerrufen
    AFTER INSERT OR UPDATE OR DELETE ON twitch_scam_guard_verdicts
    FOR EACH ROW EXECUTE FUNCTION twitch_widerrufe_kontovertrauen();
CREATE TRIGGER twitch_globalban_vertrauen_widerrufen
    AFTER INSERT OR UPDATE OR DELETE ON twitch_chatter_global_ban
    FOR EACH ROW EXECUTE FUNCTION twitch_widerrufe_kontovertrauen();
CREATE TRIGGER twitch_autoban_vertrauen_widerrufen
    AFTER INSERT OR UPDATE OR DELETE ON tb_chat_autoban_log
    FOR EACH ROW EXECUTE FUNCTION twitch_widerrufe_kontovertrauen();

INSERT INTO twitch_zuschauer_register
    (twitch_user_id, community_probability, computed_at, vertrauen_widerrufen_am)
SELECT DISTINCT chatter_id, 0.2, 'epoch'::timestamptz, NOW() FROM (
    SELECT chatter_id FROM twitch_spam_review_decisions WHERE verdict = 'spam'
    UNION SELECT chatter_id FROM twitch_scam_guard_verdicts WHERE verdict = 'scam' AND action_taken <> 'overturned'
    UNION SELECT chatter_id FROM twitch_chatter_global_ban
    UNION SELECT chatter_id FROM tb_chat_autoban_log WHERE action IN ('ban', 'timeout') AND source_path IN ('spam', 'scam', 'global_ban')
) bad WHERE NULLIF(BTRIM(chatter_id), '') IS NOT NULL
ON CONFLICT (twitch_user_id) DO UPDATE SET
    unauffaellig_seit = NULL, vertrauen_widerrufen_am = NOW();
