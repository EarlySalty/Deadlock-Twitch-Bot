-- Persistenter Merker für den täglichen Voll-Sweep der globalen Bannliste.
--
-- Vorher lebte `last_full_sweep_day` nur im Prozessspeicher. Ein Neustart
-- setzte ihn auf NULL zurück, und weil der Interval-Tick sofort feuert, lief
-- nach jedem Restart ab 6 Uhr sogleich ein kompletter Voll-Sweep. Beobachtet am
-- 2026-07-10: Sweeps um 06:00 (planmäßig), 14:12 und 14:50 (beides Neustarts).
--
-- Single-Row-Tabelle: `id` ist auf TRUE fixiert, damit es genau eine Zeile
-- geben kann und das Upsert ohne Race gegen einen zweiten Prozess läuft.
CREATE TABLE IF NOT EXISTS public.twitch_global_ban_full_sweep_state (
    id           boolean PRIMARY KEY DEFAULT TRUE CHECK (id),
    last_run_day date NOT NULL,
    updated_at   timestamptz NOT NULL DEFAULT NOW()
);
