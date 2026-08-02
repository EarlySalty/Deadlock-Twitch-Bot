-- Die letzten beiden Betriebstabellen, die einen Kanal nur über seinen Namen
-- kannten, bekommen die stabile ID.
--
-- Beide sind über `streamer_login` primärgeschlüsselt. Bisher blieb ihre Zeile
-- bei einer Umbenennung stehen, sobald der neue Login dort schon belegt war —
-- eine Einladung und der Ingest-Zustand des Kanals verloren damit den Anschluss.
-- Mit der ID greift für sie dieselbe Behandlung wie für die übrigen
-- Betriebstabellen: eigene Zeile folgt dem Kanal, eine veraltete Fremdzeile
-- gibt den Login über einen Platzhalter frei und behält ihren Inhalt.
--
-- Wie in 20260801190000 ist die Spalte nullable: für Zeilen, deren Login weder
-- in der Identitätstabelle noch in der Alias-Historie auflösbar ist, ist NULL
-- die ehrliche Antwort.

ALTER TABLE twitch_streamer_invites ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
ALTER TABLE twitch_raw_chat_ingest_health ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;

CREATE INDEX IF NOT EXISTS idx_twitch_streamer_invites_user_id
    ON twitch_streamer_invites (twitch_user_id);
CREATE INDEX IF NOT EXISTS idx_twitch_raw_chat_ingest_health_user_id
    ON twitch_raw_chat_ingest_health (twitch_user_id);

-- Backfill in drei Stufen, absteigend nach Verlässlichkeit — identisch zu
-- 20260801190000: kanonische Identität, dann Monitoring-Roster, dann frühere
-- Namen. Ein mehrdeutiger Alias (Twitch gibt aufgegebene Namen wieder frei)
-- bleibt NULL statt geraten zu werden.
DO $backfill$
DECLARE
    ziel record;
    quelle record;
    gefuellt bigint;
    offen bigint;
BEGIN
    FOR ziel IN
        SELECT * FROM (VALUES
            ('twitch_streamer_invites', 'streamer_login', 'twitch_user_id'),
            ('twitch_raw_chat_ingest_health', 'streamer_login', 'twitch_user_id')
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
                    SET %I = quelle.%I
                   FROM %I quelle
                  WHERE ziel.%I IS NULL
                    AND LOWER(ziel.%I) = LOWER(quelle.%I)
                    AND COALESCE(TRIM(quelle.%I), '''') <> ''''',
                ziel.tabelle, ziel.id_spalte, quelle.id_spalte,
                quelle.tabelle,
                ziel.id_spalte,
                ziel.login_spalte, quelle.login_spalte,
                quelle.id_spalte
            );
            GET DIAGNOSTICS gefuellt = ROW_COUNT;
            RAISE NOTICE 'Backfill %: % Zeilen aus %', ziel.tabelle, gefuellt, quelle.tabelle;
        END LOOP;

        -- Frühere Namen zuletzt und nur, wenn sie eindeutig sind.
        IF to_regclass('twitch_login_aliases') IS NOT NULL THEN
            EXECUTE format(
                'UPDATE %I ziel
                    SET %I = alias.twitch_user_id
                   FROM (
                         SELECT LOWER(login) AS login, MIN(twitch_user_id) AS twitch_user_id
                           FROM twitch_login_aliases
                          GROUP BY LOWER(login)
                         HAVING COUNT(DISTINCT twitch_user_id) = 1
                        ) alias
                  WHERE ziel.%I IS NULL
                    AND LOWER(ziel.%I) = alias.login',
                ziel.tabelle, ziel.id_spalte, ziel.id_spalte, ziel.login_spalte
            );
            GET DIAGNOSTICS gefuellt = ROW_COUNT;
            RAISE NOTICE 'Backfill %: % Zeilen aus der Alias-Historie', ziel.tabelle, gefuellt;
        END IF;

        -- Was offen bleibt, wird benannt statt verschwiegen: diese Zeilen
        -- gehören zu Logins, die das System nicht mehr kennt.
        EXECUTE format(
            'SELECT COUNT(*) FROM %I WHERE %I IS NULL', ziel.tabelle, ziel.id_spalte
        ) INTO offen;
        RAISE NOTICE 'Backfill %: % Zeilen bleiben ohne ID', ziel.tabelle, offen;
    END LOOP;
END
$backfill$;

-- Übergangsbrücke wie in 20260801200000: bis die Schreibpfade die ID selbst
-- mitgeben, trägt der Trigger sie beim Schreiben nach.
DO $trigger$
DECLARE
    ziel record;
BEGIN
    FOR ziel IN
        SELECT * FROM (VALUES
            ('twitch_streamer_invites', 'streamer_login', 'twitch_user_id'),
            ('twitch_raw_chat_ingest_health', 'streamer_login', 'twitch_user_id')
        ) AS t(tabelle, login_spalte, id_spalte)
    LOOP
        EXECUTE format(
            'DROP TRIGGER IF EXISTS trg_%s_fill_user_id ON %I',
            ziel.tabelle, ziel.tabelle
        );
        EXECUTE format(
            'CREATE TRIGGER trg_%s_fill_user_id
             BEFORE INSERT OR UPDATE OF %I ON %I
             FOR EACH ROW EXECUTE FUNCTION tb_fill_twitch_user_id_from_login(%L, %L)',
            ziel.tabelle, ziel.login_spalte, ziel.tabelle,
            ziel.login_spalte, ziel.id_spalte
        );
    END LOOP;
END
$trigger$;
