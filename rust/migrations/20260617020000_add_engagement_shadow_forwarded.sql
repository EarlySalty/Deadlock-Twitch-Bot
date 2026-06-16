-- B19-shadow-discord-out: Idempotenz-Marker für die Shadow→Discord-Review-
-- Auslieferung. Gestagte Shadow-Antworten (decision='shadowed') werden nach
-- Discord zum Review weitergeleitet; dieser Zeitstempel verhindert Doppel-
-- Versand.
--
--   NULL      = noch nicht weitergeleitet (Default für alle bestehenden Zeilen)
--   not NULL  = Zeitpunkt der erfolgreichen Discord-Weiterleitung
--
-- Additiv und ohne Default-Wert: bestehende Zeilen bleiben NULL und werden vom
-- Review-Worker einmalig nachgereicht. Kein Effekt im Normalbetrieb — nur
-- 'shadowed'-Zeilen (die nur bei opt-in output_mode='shadow' entstehen) werden
-- vom Worker überhaupt betrachtet.
ALTER TABLE public.twitch_engagement_log
    ADD COLUMN IF NOT EXISTS shadow_forwarded_at timestamp with time zone;

-- Partial-Index auf die noch offene Review-Queue: der Worker fragt
-- ausschließlich nach decision='shadowed' AND shadow_forwarded_at IS NULL.
-- Hält die wiederholte Abfrage günstig, ohne den Haupt-Insert-Pfad zu belasten.
CREATE INDEX IF NOT EXISTS twitch_engagement_log_shadow_pending_idx
    ON public.twitch_engagement_log (ts)
    WHERE decision = 'shadowed' AND shadow_forwarded_at IS NULL;
