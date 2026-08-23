-- Clip-Kontingent: eigene Verbrauchsspalte statt `created_at`.
--
-- `created_at` ist bei gefetchten Clips der Zeitstempel, den Twitch dem Clip
-- gegeben hat, nicht der Zeitpunkt der Aufnahme in unsere DB. Damit zaehlte das
-- Monatskontingent in beide falschen Richtungen: aeltere Twitch-Clips fielen aus
-- dem Monat heraus (Grenze griff nie), und Clips, die Zuschauer im laufenden
-- Monat auf Twitch erstellt hatten, verbrauchten das Kontingent, ohne dass der
-- Streamer etwas getan hatte.
--
-- `kontingent_verbraucht_at` wird deshalb genau dann gesetzt, wenn der Streamer
-- den Clip selbst in unsere DB holt: eigener Upload ueber das Dashboard oder
-- der von ihm ausgeloeste Clip-Fetch. Der Hintergrund-Fetcher laesst die Spalte
-- NULL, seine Clips zaehlen nicht.
ALTER TABLE public.twitch_clips_social_media
    ADD COLUMN IF NOT EXISTS kontingent_verbraucht_at timestamp with time zone;

COMMENT ON COLUMN public.twitch_clips_social_media.kontingent_verbraucht_at IS
    'Zeitpunkt der Aufnahme in unsere DB, wenn der Streamer sie selbst ausgeloest hat (Upload oder Dashboard-Fetch). NULL = zaehlt nicht gegen das Monatskontingent.';

-- Backfill fuer manuelle Uploads: dort ist `created_at` bereits die Insert-Zeit
-- (`clip_manager::register_manual_upload` setzt `Utc::now()`), also ist die
-- Umbuchung verlustfrei. Gefetchte Clips bleiben bewusst NULL: fuer sie ist
-- nicht rekonstruierbar, ob und wann jemand sie selbst geholt hat, und ein
-- geratener Verbrauch waere schlimmer als ein zu niedriger Zaehlerstand.
UPDATE public.twitch_clips_social_media
   SET kontingent_verbraucht_at = created_at
 WHERE source_kind = 'manual_upload'
   AND kontingent_verbraucht_at IS NULL;

-- Der Zaehler fragt immer nach Streamer plus Monatsfenster und laesst
-- verworfene Clips aussen vor.
CREATE INDEX IF NOT EXISTS idx_twitch_clips_kontingent_verbrauch
    ON public.twitch_clips_social_media (streamer_login, kontingent_verbraucht_at)
    WHERE discarded_at IS NULL AND kontingent_verbraucht_at IS NOT NULL;
