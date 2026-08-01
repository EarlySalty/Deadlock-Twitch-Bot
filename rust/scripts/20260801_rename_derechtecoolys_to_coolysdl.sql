-- Einmaliger Bestandsfall: Kanal 520300019 hieß derechtecoolys und heißt jetzt
-- coolysdl. Der laufende Bot benennt ab sofort selbst um (siehe
-- tb-monitoring/src/streamer_login.rs), aber die vor diesem Stand entstandene
-- Historie trägt noch den alten Namen.
--
-- Aufruf (nicht automatisch, bewusst von Hand):
--   psql "$TWITCH_ANALYTICS_DSN" -f 20260801_rename_derechtecoolys_to_coolysdl.sql
--
-- Zwei Eigenschaften, die dieses Skript bewusst hat:
--
--  * Es löscht nichts. Kollidiert eine Zeile mit einem bereits vorhandenen
--    Datensatz unter dem neuen Login, bleibt sie stehen und wird am Ende als
--    Konflikt gemeldet. Verworfene Zeilen wären sonst stiller Datenverlust.
--  * Komprimierte TimescaleDB-Hypertables werden übersprungen und namentlich
--    ausgegeben. TimescaleDB lehnt UPDATE auf komprimierten Chunks ab; ohne
--    diese Prüfung würde das Skript mittendrin abbrechen und alles
--    zurückrollen. Sollen diese Tabellen mit, vorher dekomprimieren:
--      SELECT decompress_chunk(c, true) FROM show_chunks('twitch_chat_messages') c;
--    und danach re-komprimieren.
BEGIN;

DO $rename$
DECLARE
    old_login CONSTANT text := 'derechtecoolys';
    new_login CONSTANT text := 'coolysdl';
    target_user_id CONSTANT text := '520300019';
    target record;
    updated bigint;
    conflicts bigint;
    total_updated bigint := 0;
    total_conflicts bigint := 0;
    komprimiert boolean;
    hat_timescale CONSTANT boolean := EXISTS (
        SELECT 1 FROM pg_extension WHERE extname = 'timescaledb'
    );
BEGIN
    RAISE NOTICE 'Twitch-Rename user_id=%: % -> %',
        target_user_id, old_login, new_login;

    -- Login-Spalten zur Laufzeit ermitteln, damit auch nach Ablage dieses
    -- Skripts hinzugekommene Tabellen erfasst werden.
    FOR target IN
        SELECT columns.table_name, columns.column_name
          FROM information_schema.columns AS columns
          JOIN information_schema.tables AS tables
            ON tables.table_schema = columns.table_schema
           AND tables.table_name = columns.table_name
         WHERE columns.table_schema = 'public'
           AND tables.table_type = 'BASE TABLE'
           AND (
               columns.data_type IN ('text', 'character varying', 'character')
               OR columns.udt_name = 'citext'
           )
           AND (
               columns.column_name = 'login'
               OR columns.column_name LIKE '%\_login' ESCAPE '\'
           )
         ORDER BY columns.table_name, columns.ordinal_position
    LOOP
        IF hat_timescale THEN
            SELECT EXISTS (
                SELECT 1 FROM timescaledb_information.chunks
                 WHERE hypertable_name = target.table_name
                   AND is_compressed
            ) INTO komprimiert;
        ELSE
            komprimiert := false;
        END IF;

        IF komprimiert THEN
            EXECUTE format(
                'SELECT COUNT(*) FROM public.%I WHERE LOWER(%I::text) = LOWER($1)',
                target.table_name, target.column_name
            ) INTO updated USING old_login;
            IF updated > 0 THEN
                RAISE NOTICE '%.%: ÜBERSPRUNGEN (komprimierte Chunks), % Zeilen bleiben auf %',
                    target.table_name, target.column_name, updated, old_login;
            END IF;
            CONTINUE;
        END IF;

        -- Erst der schnelle Weg für die ganze Tabelle. Trägt die Spalte einen
        -- Unique-Index und existiert dort bereits eine Zeile unter dem neuen
        -- Login, scheitert das — dann zeilenweise weiter, damit nur die
        -- kollidierenden Zeilen liegen bleiben statt der ganzen Tabelle.
        BEGIN
            EXECUTE format(
                'UPDATE public.%I AS ziel
                    SET %I = $1
                  WHERE LOWER(ziel.%I::text) = LOWER($2)',
                target.table_name, target.column_name, target.column_name
            ) USING new_login, old_login;
            GET DIAGNOSTICS updated = ROW_COUNT;
        EXCEPTION
            WHEN unique_violation OR exclusion_violation THEN
                updated := 0;
                DECLARE
                    alte_zeile record;
                    einzeln bigint;
                BEGIN
                    FOR alte_zeile IN EXECUTE format(
                        'SELECT ctid FROM public.%I WHERE LOWER(%I::text) = LOWER($1)',
                        target.table_name, target.column_name
                    ) USING old_login
                    LOOP
                        BEGIN
                            EXECUTE format(
                                'UPDATE public.%I SET %I = $1 WHERE ctid = $2',
                                target.table_name, target.column_name
                            ) USING new_login, alte_zeile.ctid;
                            GET DIAGNOSTICS einzeln = ROW_COUNT;
                            updated := updated + einzeln;
                        EXCEPTION
                            WHEN unique_violation OR exclusion_violation THEN
                                NULL; -- Zeile bleibt stehen, Zählung unten
                        END;
                    END LOOP;
                END;
        END;

        EXECUTE format(
            'SELECT COUNT(*) FROM public.%I WHERE LOWER(%I::text) = LOWER($1)',
            target.table_name, target.column_name
        ) INTO conflicts USING old_login;

        total_updated := total_updated + updated;
        total_conflicts := total_conflicts + conflicts;
        IF updated > 0 OR conflicts > 0 THEN
            RAISE NOTICE '%.%: % umbenannt, % Zeilen mit Konflikt belassen',
                target.table_name, target.column_name, updated, conflicts;
        END IF;
    END LOOP;

    RAISE NOTICE 'Twitch-Rename abgeschlossen: % umbenannt, % Konfliktzeilen belassen',
        total_updated, total_conflicts;
END
$rename$;

COMMIT;
