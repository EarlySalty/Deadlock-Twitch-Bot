BEGIN;

DO $rename$
DECLARE
    old_login CONSTANT text := 'derechtecoolys';
    new_login CONSTANT text := 'coolysdl';
    target_user_id CONSTANT text := '520300019';
    target record;
    old_row record;
    changed bigint;
    updated bigint;
    discarded bigint;
    total_updated bigint := 0;
    total_discarded bigint := 0;
    remaining bigint;
BEGIN
    RAISE NOTICE 'Twitch-Rename user_id=%: % -> %',
        target_user_id, old_login, new_login;

    -- Alle textuellen Login-Spalten im aktuellen public-Schema werden zur
    -- Laufzeit ermittelt. Das deckt Betriebsdaten und Historie ab, auch wenn
    -- nach Ablage dieses Skripts weitere Tabellen hinzugekommen sind.
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
        updated := 0;
        discarded := 0;

        -- Zeilenweise, damit ein bereits vorhandener Datensatz unter dem
        -- neuen Login bei jedem beliebigen Unique-/PK-Schlüssel gewinnt.
        FOR old_row IN EXECUTE format(
            'SELECT ctid FROM public.%I WHERE LOWER(%I::text) = LOWER($1)',
            target.table_name,
            target.column_name
        ) USING old_login
        LOOP
            BEGIN
                EXECUTE format(
                    'UPDATE public.%I
                        SET %I = $1
                      WHERE ctid = $2
                        AND LOWER(%I::text) = LOWER($3)',
                    target.table_name,
                    target.column_name,
                    target.column_name
                ) USING new_login, old_row.ctid, old_login;
                GET DIAGNOSTICS changed = ROW_COUNT;
                updated := updated + changed;
            EXCEPTION
                WHEN unique_violation OR exclusion_violation THEN
                    EXECUTE format(
                        'DELETE FROM public.%I
                          WHERE ctid = $1
                            AND LOWER(%I::text) = LOWER($2)',
                        target.table_name,
                        target.column_name
                    ) USING old_row.ctid, old_login;
                    GET DIAGNOSTICS changed = ROW_COUNT;
                    discarded := discarded + changed;
            END;
        END LOOP;

        EXECUTE format(
            'SELECT COUNT(*) FROM public.%I WHERE LOWER(%I::text) = LOWER($1)',
            target.table_name,
            target.column_name
        ) INTO remaining USING old_login;
        IF remaining <> 0 THEN
            RAISE EXCEPTION '%.% enthält noch % Zeilen für %',
                target.table_name, target.column_name, remaining, old_login;
        END IF;

        total_updated := total_updated + updated;
        total_discarded := total_discarded + discarded;
        RAISE NOTICE '%.%: % aktualisiert, % Konfliktzeilen verworfen',
            target.table_name, target.column_name, updated, discarded;
    END LOOP;

    RAISE NOTICE 'Twitch-Rename abgeschlossen: % aktualisiert, % Konfliktzeilen verworfen',
        total_updated, total_discarded;
END
$rename$;

COMMIT;
