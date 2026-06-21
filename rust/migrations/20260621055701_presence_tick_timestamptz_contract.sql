-- Produktionsvertrag für den Rust-Presence-Tick-Schreibpfad (P1.23/P1.25):
-- `twitch_viewer_presence_ticks.tick_at` ist TIMESTAMPTZ.
--
-- `record_presence_ticks` (tb-monitoring) bindet `tick_at` als `DateTime<Utc>`
-- und der ON-CONFLICT-Schlüssel (session_id, viewer_login, tick_at) verlangt
-- einen vergleichbaren Typ. Legacy-Prod könnte die Spalte als TEXT-Timestamp
-- führen (wie andere Alt-Tabellen). Diese Migration konvertiert NUR dann nach
-- TIMESTAMPTZ, wenn sie noch nicht so typisiert ist (idempotent: erneutes
-- Anwenden ist ein No-op). Existiert die Tabelle/Spalte nicht, bleibt die
-- Migration ebenfalls folgenlos.

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM pg_attribute
        WHERE attrelid = to_regclass('twitch_viewer_presence_ticks')
          AND attname = 'tick_at'
          AND NOT attisdropped
          AND atttypid <> 'timestamptz'::regtype
    ) THEN
        ALTER TABLE twitch_viewer_presence_ticks
            ALTER COLUMN tick_at DROP DEFAULT,
            ALTER COLUMN tick_at TYPE TIMESTAMPTZ
                USING CASE
                    WHEN tick_at IS NULL OR BTRIM(tick_at::text) = '' THEN NOW()
                    ELSE tick_at::text::timestamptz
                END;
    END IF;
END $$;
