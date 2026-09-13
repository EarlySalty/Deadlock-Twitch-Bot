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
BEGIN
    IF TG_TABLE_NAME = 'twitch_spam_review_decisions' THEN
        bad := NEW.verdict = 'spam';
    ELSIF TG_TABLE_NAME = 'twitch_scam_guard_verdicts' THEN
        bad := NEW.verdict = 'scam' AND NEW.action_taken <> 'overturned';
    ELSIF TG_TABLE_NAME = 'tb_chat_autoban_log' THEN
        bad := NEW.action IN ('ban', 'timeout') AND NEW.source_path IN ('spam', 'scam', 'global_ban');
    ELSE
        bad := TRUE;
    END IF;
    IF bad AND NULLIF(BTRIM(NEW.chatter_id), '') IS NOT NULL THEN
        INSERT INTO twitch_zuschauer_register
            (twitch_user_id, community_probability, computed_at, vertrauen_widerrufen_am)
        VALUES (NEW.chatter_id, 0.2, 'epoch', NOW())
        ON CONFLICT (twitch_user_id) DO UPDATE SET
            unauffaellig_seit = NULL, vertrauen_widerrufen_am = NOW();
    END IF;
    RETURN NEW;
END $$;

CREATE TRIGGER twitch_spam_vertrauen_widerrufen
    AFTER INSERT OR UPDATE ON twitch_spam_review_decisions
    FOR EACH ROW EXECUTE FUNCTION twitch_widerrufe_kontovertrauen();
CREATE TRIGGER twitch_scam_vertrauen_widerrufen
    AFTER INSERT OR UPDATE ON twitch_scam_guard_verdicts
    FOR EACH ROW EXECUTE FUNCTION twitch_widerrufe_kontovertrauen();
CREATE TRIGGER twitch_globalban_vertrauen_widerrufen
    AFTER INSERT OR UPDATE ON twitch_chatter_global_ban
    FOR EACH ROW EXECUTE FUNCTION twitch_widerrufe_kontovertrauen();
CREATE TRIGGER twitch_autoban_vertrauen_widerrufen
    AFTER INSERT OR UPDATE ON tb_chat_autoban_log
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
