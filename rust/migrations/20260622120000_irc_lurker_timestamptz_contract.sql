-- Produktionsvertrag fuer den experimentellen IRC-Lurker-Schreibpfad (P2.6/P2.7):
--   * twitch_session_chatters.first_message_at / .last_seen_at sind TIMESTAMPTZ
--   * twitch_live_state.active_session_id und twitch_session_chatters.session_id sind INTEGER
--
-- Idempotent: jede Spalte wird nur konvertiert, wenn sie existiert und noch nicht
-- dem Zieltyp entspricht. Andere Telemetrie-Tabellen bleiben unberuehrt.

DO $$
DECLARE
    target RECORD;
BEGIN
    FOR target IN
        SELECT * FROM (VALUES
            ('twitch_session_chatters', 'first_message_at', 'NOW()'),
            ('twitch_session_chatters', 'last_seen_at',      'NULL')
        ) AS t(tbl, col, empty_value)
    LOOP
        IF EXISTS (
            SELECT 1
            FROM pg_attribute
            WHERE attrelid = to_regclass(target.tbl)
              AND attname = target.col
              AND NOT attisdropped
              AND atttypid <> 'timestamptz'::regtype
        ) THEN
            EXECUTE format(
                'ALTER TABLE %I '
                || 'ALTER COLUMN %I DROP DEFAULT, '
                || 'ALTER COLUMN %I TYPE TIMESTAMPTZ USING CASE '
                || '  WHEN %I IS NULL OR BTRIM(%I::text) = '''' THEN %s '
                || '  ELSE %I::text::timestamptz END',
                target.tbl,
                target.col,
                target.col,
                target.col, target.col,
                target.empty_value,
                target.col
            );
        END IF;
    END LOOP;

    FOR target IN
        SELECT * FROM (VALUES
            ('twitch_live_state',       'active_session_id'),
            ('twitch_session_chatters', 'session_id')
        ) AS t(tbl, col)
    LOOP
        IF EXISTS (
            SELECT 1
            FROM pg_attribute
            WHERE attrelid = to_regclass(target.tbl)
              AND attname = target.col
              AND NOT attisdropped
              AND atttypid <> 'int4'::regtype
        ) THEN
            EXECUTE format(
                'ALTER TABLE %I '
                || 'ALTER COLUMN %I DROP DEFAULT, '
                || 'ALTER COLUMN %I TYPE INTEGER USING CASE '
                || '  WHEN %I IS NULL OR BTRIM(%I::text) = '''' THEN NULL '
                || '  ELSE %I::text::integer END',
                target.tbl,
                target.col,
                target.col,
                target.col, target.col,
                target.col
            );
        END IF;
    END LOOP;
END $$;
