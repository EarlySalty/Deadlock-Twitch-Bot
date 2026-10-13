-- Multi-Server Paket K (Plan 2026-10-12, Abschnitt 4.9, E14): Ledger-Beleg
-- für den KI-Schalter pro Server. Jeder Versuch mit Streamerbezug trägt die
-- Twitch-User-ID des Streamers; so lässt sich belegen, dass für Streamer eines
-- Servers mit `ai_features = false` kein KI-Aufruf stattfand. Versuche ohne
-- Streamerbezug (globale Jobs) bleiben NULL.
ALTER TABLE public.llm_usage
    ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;

CREATE INDEX IF NOT EXISTS idx_llm_usage_streamer
    ON public.llm_usage (twitch_user_id, ts)
    WHERE twitch_user_id IS NOT NULL;
