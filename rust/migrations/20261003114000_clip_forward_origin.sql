-- Der ursprüngliche DB-Claimzeitpunkt bleibt bei erneuter Weitergabe erhalten.
-- Bestehende created_at-Werte können bereits Retryzeiten enthalten.
-- Unbelegte Altbestände bleiben deshalb ohne Herkunftszeitpunkt.
ALTER TABLE public.twitch_clip_contest_forwards
    ADD COLUMN submitted_at TIMESTAMPTZ;
