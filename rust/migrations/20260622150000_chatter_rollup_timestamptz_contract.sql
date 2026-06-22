-- Produktionsvertrag fuer twitch_chatter_rollup (#11 Chatters/Presence-Poller):
--   * twitch_chatter_rollup.first_seen_at / .last_seen_at sind TIMESTAMPTZ
--
-- Hintergrund: Die Tabelle stammt aus dem Python-Schema (timestamptz). Der Rust-
-- baseline (20260601000000_baseline_schema.sql) legt beide Spalten als TEXT an, und
-- kein bisheriger Contract konvertiert sie -- anders als twitch_session_chatters
-- (irc_lurker_timestamptz_contract) und twitch_viewer_presence_ticks. Ein frischer
-- Migrator-Lauf erzeugte daher TEXT, waehrend Prod TIMESTAMPTZ fuehrt. Der neue
-- Chatters-Poller (und der bestehende message-getriebene ChatterTracker) binden
-- first_seen_at/last_seen_at als DateTime<Utc> -> dieser Vertrag richtet das frische
-- Schema (Tests + Neudeploys) an der Prod-Realitaet aus.
--
-- Idempotent: jede Spalte wird nur konvertiert, wenn sie existiert und noch nicht
-- TIMESTAMPTZ ist. Auf Prod (bereits timestamptz) ist die Migration ein No-Op.

DO $$
DECLARE
    target RECORD;
BEGIN
    FOR target IN
        SELECT * FROM (VALUES
            ('twitch_chatter_rollup', 'first_seen_at'),
            ('twitch_chatter_rollup', 'last_seen_at')
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
                || '  WHEN %I IS NULL OR BTRIM(%I::text) = '''' THEN NOW() '
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
