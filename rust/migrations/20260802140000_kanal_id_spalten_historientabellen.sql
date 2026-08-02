-- Die Historien- und Nebentabellen bekommen die stabile Kanal-ID.
--
-- Reihenfolge ist der Punkt: `twitch_stream_sessions` wird zuerst gefüllt,
-- weil mehrere Tabellen ihren Kanal nur über `session_id` sauber auflösen
-- können. Die Quelle muss vor ihren Abnehmern stehen.
--
-- BEWUSST NICHT ENTHALTEN — Akteur-Rollen:
--   twitch_chatter_global_ban_applied.chatter_login
--   twitch_viewer_presence_ticks.viewer_login
--   affiliate_*.affiliate_twitch_login
-- Diese Spalten benennen nicht den Kanal der Zeile, sondern eine handelnde
-- Person. Ihre ID gehört aus dem ursprünglichen Event-Payload nachgezogen; eine
-- session_id belegt nur den Broadcaster, und eine Namensauflösung über die
-- aktuelle Identität würde bei einem von Twitch freigegebenen Namen die
-- falsche Person treffen. Das ist eine eigene, sorgfältigere Runde.
--
-- BACKFILL BEWUSST AUSGELASSEN — zu groß für eine Migration:
--   twitch_stats_category      (8,6 Mio)
--   twitch_stats_tracked       (3,6 Mio)
--   twitch_viewer_presence_ticks (3,1 Mio, komprimiertes Hypertable —
--                                 ein UPDATE scheitert auf komprimierten Chunks)
-- Diese drei bekommen hier nur Spalte und Index. Der Backfill läuft in Batches
-- über scripts/20260802_backfill_grosse_tabellen.sql, damit der Deploy nicht
-- minutenlang unter einem Lock hängt.

-- 1. Spalten. ADD COLUMN mit NULL-Default ist in PG16 eine reine
--    Katalogänderung und schreibt keine Zeilen um.
ALTER TABLE twitch_stream_sessions ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE affiliate_commissions ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE affiliate_pii ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE affiliate_streamer_claims ADD COLUMN IF NOT EXISTS claimed_streamer_user_id TEXT;
ALTER TABLE ai_analyses ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE clip_fetch_history ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE clip_last_hashtags ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE clip_templates_streamer ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE exp_game_transitions ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE exp_sessions ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE oauth_state_tokens ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE social_media_platform_auth ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE social_media_reauth_notifications ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE social_media_reports ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE social_media_streamer_layout ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE twitch_crew_review_events ADD COLUMN IF NOT EXISTS channel_user_id TEXT;
ALTER TABLE twitch_link_clicks ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE twitch_outreach_shadow_sessions ADD COLUMN IF NOT EXISTS channel_user_id TEXT;
ALTER TABLE twitch_partner_outreach ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE twitch_partner_outreach_audit ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE twitch_partner_outreach_conversations ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE twitch_raid_retention ADD COLUMN IF NOT EXISTS from_broadcaster_id TEXT;
ALTER TABLE twitch_raid_retention ADD COLUMN IF NOT EXISTS to_broadcaster_id TEXT;
ALTER TABLE twitch_raw_chat_backfill_runs ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE twitch_smalltalk_sessions ADD COLUMN IF NOT EXISTS channel_user_id TEXT;
ALTER TABLE twitch_stream_ai_reports ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE twitch_stream_report_ab_votes ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE twitch_stream_report_ratings ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE twitch_stats_category ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE twitch_stats_tracked ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE twitch_viewer_presence_ticks ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;

CREATE INDEX IF NOT EXISTS idx_twitch_stream_sessions_user_id ON twitch_stream_sessions (twitch_user_id);
CREATE INDEX IF NOT EXISTS idx_ai_analyses_user_id ON ai_analyses (twitch_user_id);
CREATE INDEX IF NOT EXISTS idx_clip_fetch_history_user_id ON clip_fetch_history (twitch_user_id);
CREATE INDEX IF NOT EXISTS idx_exp_sessions_user_id ON exp_sessions (twitch_user_id);
CREATE INDEX IF NOT EXISTS idx_exp_game_transitions_user_id ON exp_game_transitions (twitch_user_id);
CREATE INDEX IF NOT EXISTS idx_crew_review_events_user_id ON twitch_crew_review_events (channel_user_id);
CREATE INDEX IF NOT EXISTS idx_outreach_shadow_sessions_user_id ON twitch_outreach_shadow_sessions (channel_user_id);
CREATE INDEX IF NOT EXISTS idx_partner_outreach_user_id ON twitch_partner_outreach (twitch_user_id);
CREATE INDEX IF NOT EXISTS idx_smalltalk_sessions_user_id ON twitch_smalltalk_sessions (channel_user_id);
CREATE INDEX IF NOT EXISTS idx_stream_ai_reports_user_id ON twitch_stream_ai_reports (twitch_user_id);
CREATE INDEX IF NOT EXISTS idx_stats_category_user_id ON twitch_stats_category (twitch_user_id);
CREATE INDEX IF NOT EXISTS idx_stats_tracked_user_id ON twitch_stats_tracked (twitch_user_id);
CREATE INDEX IF NOT EXISTS idx_viewer_presence_ticks_user_id ON twitch_viewer_presence_ticks (twitch_user_id);

-- 2. Backfill. Erst die Quelle (`twitch_stream_sessions`), dann alles, was an
--    ihr hängt, dann der Rest über die Namensauflösung.
DO $backfill$
DECLARE
    ziel record;
    quelle record;
    gefuellt bigint;
    offen bigint;
    gesamt bigint;
BEGIN
    -- 2a. Die Quelle selbst: nur über Namensauflösung möglich.
    FOR quelle IN
        SELECT * FROM (VALUES
            ('twitch_streamer_identities', 'twitch_login', 'twitch_user_id'),
            ('twitch_streamers', 'twitch_login', 'twitch_user_id')
        ) AS q(tabelle, login_spalte, id_spalte)
    LOOP
        IF to_regclass(quote_ident(quelle.tabelle)) IS NULL THEN
            CONTINUE;
        END IF;
        EXECUTE format(
            'UPDATE twitch_stream_sessions ziel
                SET twitch_user_id = q.%I
               FROM %I q
              WHERE ziel.twitch_user_id IS NULL
                AND LOWER(ziel.streamer_login) = LOWER(q.%I)
                AND COALESCE(TRIM(q.%I), '''') <> ''''',
            quelle.id_spalte, quelle.tabelle, quelle.login_spalte, quelle.id_spalte
        );
        GET DIAGNOSTICS gefuellt = ROW_COUNT;
        RAISE NOTICE 'twitch_stream_sessions: % Zeilen aus %', gefuellt, quelle.tabelle;
    END LOOP;

    IF to_regclass('twitch_login_aliases') IS NOT NULL THEN
        -- Alias bewusst `sess` statt `ziel`: in statischem SQL innerhalb
        -- plpgsql würde `ziel` gegen die gleichnamige Record-Variable
        -- aufgelöst und die Anweisung scheitern.
        UPDATE twitch_stream_sessions sess
           SET twitch_user_id = alias.twitch_user_id
          FROM (SELECT LOWER(login) AS login, MIN(twitch_user_id) AS twitch_user_id
                  FROM twitch_login_aliases
                 GROUP BY LOWER(login)
                HAVING COUNT(DISTINCT twitch_user_id) = 1) alias
         WHERE sess.twitch_user_id IS NULL
           AND LOWER(sess.streamer_login) = alias.login;
        GET DIAGNOSTICS gefuellt = ROW_COUNT;
        RAISE NOTICE 'twitch_stream_sessions: % Zeilen aus der Alias-Historie', gefuellt;
    END IF;

    SELECT COUNT(*) FROM twitch_stream_sessions WHERE twitch_user_id IS NULL INTO offen;
    SELECT COUNT(*) FROM twitch_stream_sessions INTO gesamt;
    RAISE NOTICE 'twitch_stream_sessions: % von % Zeilen bleiben ohne ID', offen, gesamt;

    -- 2b. Abnehmer der Quelle: die session_id ist der verlässlichste Bezug,
    --     weil sie den Broadcaster unabhängig vom damaligen Namen festhält.
    --     twitch_viewer_presence_ticks fehlt hier bewusst (siehe Kopf).
    FOR ziel IN
        SELECT * FROM (VALUES
            ('twitch_stream_ai_reports', 'twitch_user_id'),
            ('twitch_stream_report_ab_votes', 'twitch_user_id'),
            ('twitch_stream_report_ratings', 'twitch_user_id')
        ) AS t(tabelle, id_spalte)
    LOOP
        EXECUTE format(
            'UPDATE %I ziel
                SET %I = s.twitch_user_id
               FROM twitch_stream_sessions s
              WHERE ziel.%I IS NULL
                AND ziel.session_id = s.id
                AND s.twitch_user_id IS NOT NULL',
            ziel.tabelle, ziel.id_spalte, ziel.id_spalte
        );
        GET DIAGNOSTICS gefuellt = ROW_COUNT;
        RAISE NOTICE '%: % Zeilen über session_id', ziel.tabelle, gefuellt;
    END LOOP;

    -- 2b'. twitch_raid_retention hängt über (raid_id, executed_at) an
    --      twitch_raid_history, und die trägt beide Broadcaster-IDs bereits.
    --      Sie von dort zu übernehmen ist genauer als jede Namensauflösung.
    --
    --      Der Join ist zugleich notwendig: retention enthält Zeilen ohne
    --      passende History-Zeile, die den Fremdschlüssel
    --      twitch_raid_retention_raid_history_ref_fkey schon heute verletzen.
    --      Solange niemand sie anfasst, fällt das nicht auf — ein UPDATE
    --      löst die Prüfung aus und scheitert. Über den Join bleiben genau
    --      diese Zeilen unberührt.
    UPDATE twitch_raid_retention ret
       SET from_broadcaster_id = COALESCE(ret.from_broadcaster_id, hist.from_broadcaster_id),
           to_broadcaster_id   = COALESCE(ret.to_broadcaster_id, hist.to_broadcaster_id)
      FROM twitch_raid_history hist
     WHERE ret.raid_id = hist.id
       AND ret.executed_at = hist.executed_at
       AND (ret.from_broadcaster_id IS NULL OR ret.to_broadcaster_id IS NULL)
       AND (hist.from_broadcaster_id IS NOT NULL OR hist.to_broadcaster_id IS NOT NULL);
    GET DIAGNOSTICS gefuellt = ROW_COUNT;
    SELECT COUNT(*) FROM twitch_raid_retention
     WHERE from_broadcaster_id IS NULL OR to_broadcaster_id IS NULL INTO offen;
    RAISE NOTICE 'twitch_raid_retention: % Zeilen aus der Raid-Historie, % bleiben offen (keine passende History-Zeile)',
        gefuellt, offen;

    -- 2c. Der Rest über Namensauflösung. Die drei Millionen-Tabellen fehlen
    --     hier bewusst und werden per Batch-Skript nachgezogen.
    FOR ziel IN
        SELECT * FROM (VALUES
            ('affiliate_commissions', 'streamer_login', 'twitch_user_id'),
            ('affiliate_pii', 'twitch_login', 'twitch_user_id'),
            ('affiliate_streamer_claims', 'claimed_streamer_login', 'claimed_streamer_user_id'),
            ('ai_analyses', 'streamer', 'twitch_user_id'),
            ('clip_fetch_history', 'streamer_login', 'twitch_user_id'),
            ('clip_last_hashtags', 'streamer_login', 'twitch_user_id'),
            ('clip_templates_streamer', 'streamer_login', 'twitch_user_id'),
            ('exp_game_transitions', 'streamer', 'twitch_user_id'),
            ('exp_sessions', 'streamer', 'twitch_user_id'),
            ('oauth_state_tokens', 'streamer_login', 'twitch_user_id'),
            ('social_media_platform_auth', 'streamer_login', 'twitch_user_id'),
            ('social_media_reauth_notifications', 'streamer_login', 'twitch_user_id'),
            ('social_media_reports', 'streamer_login', 'twitch_user_id'),
            ('social_media_streamer_layout', 'streamer_login', 'twitch_user_id'),
            ('twitch_crew_review_events', 'channel_login', 'channel_user_id'),
            ('twitch_link_clicks', 'streamer_login', 'twitch_user_id'),
            ('twitch_outreach_shadow_sessions', 'channel_login', 'channel_user_id'),
            ('twitch_partner_outreach', 'streamer_login', 'twitch_user_id'),
            ('twitch_partner_outreach_audit', 'streamer_login', 'twitch_user_id'),
            ('twitch_partner_outreach_conversations', 'streamer_login', 'twitch_user_id'),
            ('twitch_raw_chat_backfill_runs', 'streamer_login', 'twitch_user_id'),
            ('twitch_smalltalk_sessions', 'channel_login', 'channel_user_id'),
            ('twitch_stream_ai_reports', 'streamer_login', 'twitch_user_id'),
            ('twitch_stream_report_ab_votes', 'streamer_login', 'twitch_user_id'),
            ('twitch_stream_report_ratings', 'streamer_login', 'twitch_user_id')
        ) AS t(tabelle, login_spalte, id_spalte)
    LOOP
        FOR quelle IN
            SELECT * FROM (VALUES
                ('twitch_streamer_identities', 'twitch_login', 'twitch_user_id'),
                ('twitch_streamers', 'twitch_login', 'twitch_user_id')
            ) AS q(tabelle, login_spalte, id_spalte)
        LOOP
            IF to_regclass(quote_ident(quelle.tabelle)) IS NULL THEN
                CONTINUE;
            END IF;
            EXECUTE format(
                'UPDATE %I ziel
                    SET %I = q.%I
                   FROM %I q
                  WHERE ziel.%I IS NULL
                    AND LOWER(ziel.%I) = LOWER(q.%I)
                    AND COALESCE(TRIM(q.%I), '''') <> ''''',
                ziel.tabelle, ziel.id_spalte, quelle.id_spalte,
                quelle.tabelle,
                ziel.id_spalte,
                ziel.login_spalte, quelle.login_spalte,
                quelle.id_spalte
            );
        END LOOP;

        IF to_regclass('twitch_login_aliases') IS NOT NULL THEN
            EXECUTE format(
                'UPDATE %I ziel
                    SET %I = alias.twitch_user_id
                   FROM (SELECT LOWER(login) AS login, MIN(twitch_user_id) AS twitch_user_id
                           FROM twitch_login_aliases
                          GROUP BY LOWER(login)
                         HAVING COUNT(DISTINCT twitch_user_id) = 1) alias
                  WHERE ziel.%I IS NULL
                    AND LOWER(ziel.%I) = alias.login',
                ziel.tabelle, ziel.id_spalte, ziel.id_spalte, ziel.login_spalte
            );
        END IF;

        EXECUTE format('SELECT COUNT(*) FROM %I WHERE %I IS NULL', ziel.tabelle, ziel.id_spalte)
           INTO offen;
        EXECUTE format('SELECT COUNT(*) FROM %I', ziel.tabelle) INTO gesamt;
        RAISE NOTICE '%.%: % von % Zeilen bleiben ohne ID',
            ziel.tabelle, ziel.id_spalte, offen, gesamt;
    END LOOP;

    RAISE NOTICE 'Offen und bewusst nicht hier: twitch_stats_category, twitch_stats_tracked, twitch_viewer_presence_ticks (Batch-Skript) sowie alle Akteur-Rollen.';
END
$backfill$;
