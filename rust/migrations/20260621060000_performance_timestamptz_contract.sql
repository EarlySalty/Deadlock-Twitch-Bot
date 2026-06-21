-- Produktionsvertrag für die Performance-/Heatmap-/Tag-Analytics-Lesepfade (P1.35).
--
-- Die Rust-Handler (tb-dashboard-api: performance.rs, tag_analysis.rs) binden den
-- Zeitfilter `since` als `DateTime<Utc>` (sqlx sendet einen expliziten
-- `timestamptz`-Typ-OID) gegen die Spalten:
--   * twitch_stream_sessions.started_at / .ended_at
--   * twitch_stats_tracked.ts_utc
--   * twitch_stats_category.ts_utc
-- Führt Legacy-Prod diese Spalten als TEXT-Timestamp, fehlt der implizite
-- `text >= timestamptz`-Operator → Query-Fehler 500. Python band ISO-Strings
-- (text >= text), Rust kann das nicht; daher der saubere Weg: Spalten nach
-- TIMESTAMPTZ konvertieren.
--
-- Idempotent: konvertiert eine Spalte NUR, wenn sie existiert und noch nicht
-- TIMESTAMPTZ ist; erneutes Anwenden ist ein No-op. Fehlt Tabelle/Spalte,
-- bleibt die Migration folgenlos. Leere/NULL-Werte werden zu NULL gemappt
-- (started_at/ended_at sind nullbar), bei ts_utc analog defensiv.

DO $$
DECLARE
    target RECORD;
BEGIN
    FOR target IN
        SELECT * FROM (VALUES
            ('twitch_stream_sessions', 'started_at'),
            ('twitch_stream_sessions', 'ended_at'),
            ('twitch_stats_tracked',   'ts_utc'),
            ('twitch_stats_category',  'ts_utc')
        ) AS t(tbl, col)
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
                || '  WHEN %I IS NULL OR BTRIM(%I::text) = '''' THEN NULL '
                || '  ELSE %I::text::timestamptz END',
                target.tbl,
                target.col,
                target.col,
                target.col, target.col,
                target.col
            );
        END IF;
    END LOOP;
END $$;
