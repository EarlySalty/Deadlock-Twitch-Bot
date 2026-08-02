-- Schritt 1 des Umbaus "Identität ist die twitch_user_id, nicht der Login":
-- Betriebstabellen, die einen Kanal bisher ausschließlich über seinen Namen
-- kennen, bekommen die stabile ID dazu. Die Login-Spalte bleibt vorerst — der
-- Code liest noch über sie, und ohne Übergang wäre jede Zeile sofort blind.
--
-- Die Spalte ist bewusst nullable: für Zeilen, deren Login weder in der
-- Identitätstabelle noch in der Alias-Historie auflösbar ist, ist NULL die
-- ehrliche Antwort. Erraten wäre schlimmer als offen unbekannt.

ALTER TABLE twitch_engagement_settings ADD COLUMN IF NOT EXISTS channel_user_id TEXT;
ALTER TABLE twitch_engagement_channel_profile ADD COLUMN IF NOT EXISTS channel_user_id TEXT;
ALTER TABLE twitch_engagement_log ADD COLUMN IF NOT EXISTS channel_user_id TEXT;
ALTER TABLE twitch_engagement_stream_transcripts ADD COLUMN IF NOT EXISTS channel_user_id TEXT;
ALTER TABLE twitch_outreach_shadow_events ADD COLUMN IF NOT EXISTS channel_user_id TEXT;
ALTER TABLE twitch_scam_guard_settings ADD COLUMN IF NOT EXISTS channel_user_id TEXT;
ALTER TABLE twitch_smalltalk_messages ADD COLUMN IF NOT EXISTS channel_user_id TEXT;
ALTER TABLE twitch_channel_match_state ADD COLUMN IF NOT EXISTS channel_user_id TEXT;
ALTER TABLE twitch_chat_word_groups ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE twitch_live_announcement_configs ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE twitch_scout_pitch_blacklist ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE twitch_scout_pitch_ledger ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE twitch_promo_cooldowns ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;

CREATE INDEX IF NOT EXISTS idx_engagement_settings_user_id ON twitch_engagement_settings (channel_user_id);
CREATE INDEX IF NOT EXISTS idx_engagement_channel_profile_user_id ON twitch_engagement_channel_profile (channel_user_id);
CREATE INDEX IF NOT EXISTS idx_engagement_log_user_id ON twitch_engagement_log (channel_user_id);
CREATE INDEX IF NOT EXISTS idx_engagement_transcripts_user_id ON twitch_engagement_stream_transcripts (channel_user_id);
CREATE INDEX IF NOT EXISTS idx_outreach_shadow_events_user_id ON twitch_outreach_shadow_events (channel_user_id);
CREATE INDEX IF NOT EXISTS idx_scam_guard_settings_user_id ON twitch_scam_guard_settings (channel_user_id);
CREATE INDEX IF NOT EXISTS idx_smalltalk_messages_user_id ON twitch_smalltalk_messages (channel_user_id);
CREATE INDEX IF NOT EXISTS idx_channel_match_state_user_id ON twitch_channel_match_state (channel_user_id);
CREATE INDEX IF NOT EXISTS idx_chat_word_groups_user_id ON twitch_chat_word_groups (twitch_user_id);
CREATE INDEX IF NOT EXISTS idx_live_announcement_configs_user_id ON twitch_live_announcement_configs (twitch_user_id);
CREATE INDEX IF NOT EXISTS idx_scout_pitch_blacklist_user_id ON twitch_scout_pitch_blacklist (twitch_user_id);
CREATE INDEX IF NOT EXISTS idx_scout_pitch_ledger_user_id ON twitch_scout_pitch_ledger (twitch_user_id);
CREATE INDEX IF NOT EXISTS idx_promo_cooldowns_user_id ON twitch_promo_cooldowns (twitch_user_id);

-- Backfill in drei Stufen, absteigend nach Verlässlichkeit:
--   1. twitch_streamer_identities — die kanonische Zuordnung
--   2. twitch_streamers — das Monitoring-Roster
--   3. twitch_login_aliases — frühere Namen, deckt Zeilen aus der Zeit vor
--      einer Umbenennung ab
DO $backfill$
DECLARE
    ziel record;
    quelle record;
    gefuellt bigint;
    offen bigint;
BEGIN
    FOR ziel IN
        SELECT * FROM (VALUES
            ('twitch_engagement_settings', 'channel_login', 'channel_user_id'),
            ('twitch_engagement_channel_profile', 'channel_login', 'channel_user_id'),
            ('twitch_engagement_log', 'channel_login', 'channel_user_id'),
            ('twitch_engagement_stream_transcripts', 'channel_login', 'channel_user_id'),
            ('twitch_outreach_shadow_events', 'channel_login', 'channel_user_id'),
            ('twitch_scam_guard_settings', 'channel_login', 'channel_user_id'),
            ('twitch_smalltalk_messages', 'channel_login', 'channel_user_id'),
            ('twitch_channel_match_state', 'channel_login', 'channel_user_id'),
            ('twitch_chat_word_groups', 'streamer_login', 'twitch_user_id'),
            ('twitch_live_announcement_configs', 'streamer_login', 'twitch_user_id'),
            ('twitch_scout_pitch_blacklist', 'streamer_login', 'twitch_user_id'),
            ('twitch_scout_pitch_ledger', 'streamer_login', 'twitch_user_id'),
            ('twitch_promo_cooldowns', 'login', 'twitch_user_id')
        ) AS t(tabelle, login_spalte, id_spalte)
    LOOP
        FOR quelle IN
            SELECT * FROM (VALUES
                ('twitch_streamer_identities', 'twitch_login', 'twitch_user_id'),
                ('twitch_streamers', 'twitch_login', 'twitch_user_id'),
                ('twitch_login_aliases', 'login', 'twitch_user_id')
            ) AS q(tabelle, login_spalte, id_spalte)
        LOOP
            -- Nur eindeutige Zuordnungen. Twitch gibt aufgegebene Namen wieder
            -- frei; steht ein Login in der Alias-Historie unter mehreren IDs,
            -- wäre jede Wahl geraten. Solche Zeilen bleiben offen.
            EXECUTE format(
                'UPDATE public.%I AS ziel
                    SET %I = eindeutig.id
                   FROM (
                          SELECT LOWER(%I) AS login, MIN(%I) AS id
                            FROM public.%I
                           WHERE COALESCE(TRIM(%I), '''') <> ''''
                           GROUP BY LOWER(%I)
                          HAVING COUNT(DISTINCT %I) = 1
                        ) AS eindeutig
                  WHERE ziel.%I IS NULL
                    AND LOWER(ziel.%I) = eindeutig.login',
                ziel.tabelle, ziel.id_spalte,
                quelle.login_spalte, quelle.id_spalte,
                quelle.tabelle,
                quelle.id_spalte,
                quelle.login_spalte,
                quelle.id_spalte,
                ziel.id_spalte,
                ziel.login_spalte
            );
        END LOOP;

        EXECUTE format(
            'SELECT COUNT(*) FILTER (WHERE %I IS NOT NULL), COUNT(*) FILTER (WHERE %I IS NULL)
               FROM public.%I',
            ziel.id_spalte, ziel.id_spalte, ziel.tabelle
        ) INTO gefuellt, offen;
        IF gefuellt > 0 OR offen > 0 THEN
            RAISE NOTICE '%: % Zeilen mit ID, % ohne', ziel.tabelle, gefuellt, offen;
        END IF;
    END LOOP;
END
$backfill$;
