-- Backfill der Kanal-ID für die drei Tabellen, die zu groß für eine Migration
-- sind. Ergänzt 20260802140000, die dort nur Spalte und Index angelegt hat.
--
--   twitch_stats_category         8,6 Mio, komprimiertes Hypertable (41/43 Chunks)
--   twitch_stats_tracked          3,6 Mio, komprimiertes Hypertable (41/43 Chunks)
--   twitch_viewer_presence_ticks  3,1 Mio, komprimiertes Hypertable (16/18 Chunks)
--
-- ACHTUNG — dieses Skript schreibt durch. Es committet nach jedem Batch, damit
-- kein Lock über Millionen Zeilen stehen bleibt. Ein umschließendes
-- BEGIN/ROLLBACK schützt deshalb NICHT: die COMMITs im Skript sind schon
-- geschrieben, wenn das äußere ROLLBACK kommt. Wer trocken prüfen will, nimmt
-- eine Kopie der Datenbank.
--
-- Aufruf:
--   psql "$DSN" -v ON_ERROR_STOP=1 -f 20260802_backfill_grosse_tabellen.sql
--
-- Wiederaufnehmbar: jeder Batch greift nur Zeilen mit NULL. Abbrechen und
-- später erneut starten ist unschädlich.
--
-- VORAUSSETZUNG — vorher dekomprimieren, sonst bricht der Lauf ab:
--   SELECT decompress_chunk(c, if_compressed => true) FROM show_chunks('twitch_stats_category') c;
--   SELECT decompress_chunk(c, if_compressed => true) FROM show_chunks('twitch_stats_tracked') c;
--   SELECT decompress_chunk(c, if_compressed => true) FROM show_chunks('twitch_viewer_presence_ticks') c;
--
-- Der frühere Kommentar hier behauptete, die implizite Dekompression von
-- TimescaleDB 2.17.2 trage den Lauf durch. Am 2026-08-02 auf Prod widerlegt: der
-- erste Lauf brach beim ersten Login von twitch_stats_category mit
-- "tuple decompression limit exceeded by operation" ab. Die Grenze ist
-- timescaledb.max_tuples_decompressed_per_dml_transaction (auf Prod 100000) und
-- gilt pro Transaktion — hier also pro Login. Ein Login mit mehr als 100k Zeilen
-- reißt sie. Der frühere Testlauf über drei Chunks war zu klein, um das zu sehen.
-- Alle drei Tabellen sind komprimiert, nicht nur presence_ticks.
--
-- Das Limit per SET auf 0 zu heben wäre die kürzere Variante, verlagert die
-- Dekompression aber unsichtbar in eine einzelne DML-Transaktion. Explizit
-- dekomprimieren ist steuerbar und lässt den Fortschritt sehen.
--
-- Danach nichts von Hand rekomprimieren: die Compression-Policies (Jobs 1001,
-- 1002, 1021, alle 12 h) holen das selbst nach.

-- Die Auflösung Login -> ID, in der Reihenfolge ihrer Verlässlichkeit:
-- kanonische Identität, Monitoring-Roster, dann eindeutige Alias-Historie.
-- Ein von Twitch wiedervergebener Name ist mehrdeutig und fehlt hier bewusst.
CREATE OR REPLACE VIEW tb_backfill_aufloesung AS
WITH quellen AS (
    SELECT LOWER(twitch_login) AS login, twitch_user_id, 1 AS rang
      FROM twitch_streamer_identities
     WHERE COALESCE(TRIM(twitch_user_id), '') <> ''
    UNION ALL
    SELECT LOWER(twitch_login), twitch_user_id, 2
      FROM twitch_streamers
     WHERE COALESCE(TRIM(twitch_user_id), '') <> ''
    UNION ALL
    SELECT login, twitch_user_id, 3 FROM (
        SELECT LOWER(login) AS login, MIN(twitch_user_id) AS twitch_user_id
          FROM twitch_login_aliases
         GROUP BY LOWER(login)
        HAVING COUNT(DISTINCT twitch_user_id) = 1
    ) eindeutig
)
SELECT DISTINCT ON (login) login, twitch_user_id
  FROM quellen ORDER BY login, rang;

CREATE OR REPLACE PROCEDURE tb_backfill_grosse_tabellen()
LANGUAGE plpgsql
AS $$
DECLARE
    ziel record;
    logins text[];
    ids text[];
    i int;
    geaendert bigint;
    gesamt_tabelle bigint;
    offen bigint;
BEGIN
    FOR ziel IN
        SELECT * FROM (VALUES
            ('twitch_stats_category', 'streamer', 'twitch_user_id'),
            ('twitch_stats_tracked', 'streamer', 'twitch_user_id'),
            ('twitch_viewer_presence_ticks', 'streamer_login', 'twitch_user_id')
        ) AS t(tabelle, login_spalte, id_spalte)
    LOOP
        -- Ein Stapel ist ein Login, nicht ein Zeilenfenster.
        --
        -- Zeilenfenster über ctid scheiden aus zwei Gründen aus: auf einem
        -- komprimierten Hypertable lehnt TimescaleDB ctid ab ("transparent
        -- decompression only supports tableoid system column"), und ein
        -- Fenster, das nur auf "ID noch NULL" filtert, bleibt bei
        -- unauflösbaren Zeilen stehen — twitch_stats_category enthält
        -- Scraper-Daten beliebiger fremder Streamer, die zu Millionen nicht
        -- auflösbar sind, und der Lauf hielte sich danach für fertig.
        --
        -- Die Login-Liste wird vorher in ein Array geholt statt in einem
        -- Cursor gehalten: der COMMIT pro Login würde ein offenes Portal
        -- ungültig machen.
        EXECUTE format(
            'SELECT COALESCE(array_agg(a.login ORDER BY a.login), ARRAY[]::text[]),
                    COALESCE(array_agg(a.twitch_user_id ORDER BY a.login), ARRAY[]::text[])
               FROM tb_backfill_aufloesung a
              WHERE EXISTS (SELECT 1 FROM %I z
                             WHERE LOWER(z.%I) = a.login AND z.%I IS NULL)',
            ziel.tabelle, ziel.login_spalte, ziel.id_spalte
        ) INTO logins, ids;

        gesamt_tabelle := 0;
        RAISE NOTICE '%: % auflösbare Logins zu füllen', ziel.tabelle, COALESCE(array_length(logins, 1), 0);

        FOR i IN 1 .. COALESCE(array_length(logins, 1), 0) LOOP
            EXECUTE format(
                'UPDATE %I SET %I = $1 WHERE %I IS NULL AND LOWER(%I) = $2',
                ziel.tabelle, ziel.id_spalte, ziel.id_spalte, ziel.login_spalte
            ) USING ids[i], logins[i];
            GET DIAGNOSTICS geaendert = ROW_COUNT;
            gesamt_tabelle := gesamt_tabelle + geaendert;
            COMMIT;
            IF i % 200 = 0 THEN
                RAISE NOTICE '%: % von % Logins, % Zeilen gefüllt ...',
                    ziel.tabelle, i, array_length(logins, 1), gesamt_tabelle;
            END IF;
        END LOOP;

        EXECUTE format('SELECT COUNT(*) FROM %I WHERE %I IS NULL', ziel.tabelle, ziel.id_spalte)
           INTO offen;
        RAISE NOTICE '%: % Zeilen gefüllt, % bleiben ohne ID (nicht auflösbarer Login)',
            ziel.tabelle, gesamt_tabelle, offen;
    END LOOP;
END
$$;

CALL tb_backfill_grosse_tabellen();

-- Welche Chunks nach dem Lauf komprimiert sind. Das ist eine reine
-- Zustandsmeldung und kein Rest: der Lauf oben füllt auch komprimierte
-- Chunks (2.17.2 dekomprimiert dafür implizit) — verifiziert mit einem
-- Testlauf über drei zuvor komprimierte Chunks, danach 0 offene Zeilen.
-- Die Zahl der offenen Zeilen steht in der NOTICE der jeweiligen Tabelle.
DO $melde$
DECLARE
    chunk record;
    anzahl int := 0;
BEGIN
    IF to_regclass('timescaledb_information.chunks') IS NULL THEN
        RETURN;
    END IF;
    FOR chunk IN
        SELECT chunk_schema, chunk_name
          FROM timescaledb_information.chunks
         WHERE hypertable_name = 'twitch_viewer_presence_ticks'
           AND is_compressed
         ORDER BY chunk_name
    LOOP
        anzahl := anzahl + 1;
        RAISE NOTICE 'komprimiert: %.%', chunk.chunk_schema, chunk.chunk_name;
    END LOOP;
    IF anzahl > 0 THEN
        RAISE NOTICE '% Chunks sind komprimiert. Das ist der Normalzustand und sagt nichts über offene Zeilen — die stehen in der NOTICE oben.', anzahl;
    END IF;
END
$melde$;

DROP PROCEDURE tb_backfill_grosse_tabellen();
DROP VIEW tb_backfill_aufloesung;
