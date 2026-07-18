-- Repariert saubere gewachsene Datenbanken, ohne bestehende Duplikate zu löschen.
-- Der Schreibpfad bleibt auch ohne diesen optionalen Beschleunigungsindex korrekt.
DO $$
BEGIN
    CREATE UNIQUE INDEX IF NOT EXISTS idx_exp_snapshots_session_ts
        ON public.exp_snapshots (exp_session_id, ts_utc);
EXCEPTION
    WHEN unique_violation THEN
        RAISE NOTICE 'idx_exp_snapshots_session_ts ausgelassen: bestehende Duplikate';
END
$$;
