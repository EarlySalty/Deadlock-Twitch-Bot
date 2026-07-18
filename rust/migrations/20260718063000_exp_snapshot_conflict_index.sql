-- Repariert gewachsene Datenbanken, denen der Konfliktindex trotz Baseline fehlt.
CREATE UNIQUE INDEX IF NOT EXISTS idx_exp_snapshots_session_ts
    ON public.exp_snapshots (exp_session_id, ts_utc);
