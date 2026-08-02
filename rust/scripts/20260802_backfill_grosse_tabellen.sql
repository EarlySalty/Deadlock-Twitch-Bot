-- Backfill der Kanal-ID für die drei Tabellen, die zu groß für eine Migration
-- sind. Ergänzt 20260802140000, die dort nur Spalte und Index angelegt hat.
--
--   twitch_stats_category         8,6 Mio
--   twitch_stats_tracked          3,6 Mio
--   twitch_viewer_presence_ticks  3,1 Mio, komprimiertes Hypertable
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
-- Komprimierte Chunks von twitch_viewer_presence_ticks lassen kein UPDATE zu.
-- Sie werden NICHT still übersprungen, sondern am Ende namentlich gemeldet.
-- Erst danach entscheidet ein Mensch, ob dekomprimiert wird:
--   SELECT decompress_chunk(c) FROM show_chunks('twitch_viewer_presence_ticks') c;

CREATE OR REPLACE PROCEDURE tb_backfill_grosse_tabellen(batch_groesse INT DEFAULT 50000)
LANGUAGE plpgsql
AS $$
DECLARE
    ziel record;
    geaendert bigint;
    gesamt_tabelle bigint;
    offen bigint;
    runden int;
BEGIN
    FOR ziel IN
        SELECT * FROM (VALUES
            ('twitch_stats_category', 'streamer', 'twitch_user_id'),
            ('twitch_stats_tracked', 'streamer', 'twitch_user_id'),
            ('twitch_viewer_presence_ticks', 'streamer_login', 'twitch_user_id')
        ) AS t(tabelle, login_spalte, id_spalte)
    LOOP
        gesamt_tabelle := 0;
        runden := 0;
        LOOP
            -- Auflösung in der Reihenfolge ihrer Verlässlichkeit: kanonische
            -- Identität, Monitoring-Roster, dann eindeutige Alias-Historie.
            -- Ein wiedervergebener Name ist mehrdeutig und bleibt NULL.
            EXECUTE format(
                'WITH aufloesung AS (
                     SELECT LOWER(twitch_login) AS login, twitch_user_id, 1 AS rang
                       FROM twitch_streamer_identities
                      WHERE COALESCE(TRIM(twitch_user_id), '''') <> ''''
                     UNION ALL
                     SELECT LOWER(twitch_login), twitch_user_id, 2
                       FROM twitch_streamers
                      WHERE COALESCE(TRIM(twitch_user_id), '''') <> ''''
                     UNION ALL
                     SELECT login, twitch_user_id, 3 FROM (
                         SELECT LOWER(login) AS login,
                                MIN(twitch_user_id) AS twitch_user_id
                           FROM twitch_login_aliases
                          GROUP BY LOWER(login)
                         HAVING COUNT(DISTINCT twitch_user_id) = 1
                     ) eindeutig
                 ),
                 beste AS (
                     SELECT DISTINCT ON (login) login, twitch_user_id
                       FROM aufloesung ORDER BY login, rang
                 ),
                 stapel AS (
                     SELECT ctid FROM %I
                      WHERE %I IS NULL
                        AND COALESCE(TRIM(%I), '''') <> ''''
                      LIMIT %s
                 )
                 UPDATE %I ziel
                    SET %I = beste.twitch_user_id
                   FROM stapel, beste
                  WHERE ziel.ctid = stapel.ctid
                    AND LOWER(ziel.%I) = beste.login',
                ziel.tabelle, ziel.id_spalte, ziel.login_spalte, batch_groesse,
                ziel.tabelle, ziel.id_spalte, ziel.login_spalte
            );
            GET DIAGNOSTICS geaendert = ROW_COUNT;
            gesamt_tabelle := gesamt_tabelle + geaendert;
            runden := runden + 1;
            COMMIT;
            -- Abbruch, wenn ein Stapel nichts mehr ändert: die restlichen
            -- Zeilen sind nicht auflösbar, nicht "noch nicht dran".
            EXIT WHEN geaendert = 0;
            IF runden % 20 = 0 THEN
                RAISE NOTICE '%: % Zeilen gefüllt ...', ziel.tabelle, gesamt_tabelle;
            END IF;
        END LOOP;

        EXECUTE format('SELECT COUNT(*) FROM %I WHERE %I IS NULL', ziel.tabelle, ziel.id_spalte)
           INTO offen;
        RAISE NOTICE '%: % Zeilen gefüllt, % bleiben ohne ID',
            ziel.tabelle, gesamt_tabelle, offen;
    END LOOP;
END
$$;

CALL tb_backfill_grosse_tabellen();

-- Komprimierte Chunks melden. Sie haben das UPDATE oben nicht angenommen und
-- tragen deshalb weiter NULL — das ist kein stiller Rest, sondern eine
-- Arbeitsliste.
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
        RAISE NOTICE 'komprimiert, nicht gefüllt: %.%', chunk.chunk_schema, chunk.chunk_name;
    END LOOP;
    IF anzahl > 0 THEN
        RAISE NOTICE '% komprimierte Chunks übrig. Zum Nachziehen erst dekomprimieren, dann dieses Skript erneut starten.', anzahl;
    END IF;
END
$melde$;

DROP PROCEDURE tb_backfill_grosse_tabellen(INT);
