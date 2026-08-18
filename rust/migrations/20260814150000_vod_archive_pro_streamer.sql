-- VOD-Archiv wird eine Einstellung je Streamer.
--
-- Gebaut war es als globaler Schalter in `social_media_settings`
-- (`vod_archive_enabled`, `vod_archive_privacy`) plus ein fest verdrahteter
-- Kanal aus der Umgebung. Im Social-Media-Dashboard haengt aber alles andere
-- am Streamer (Layout, Clips, und die YouTube-Tokens in
-- `social_media_platform_auth` tragen laengst eine `streamer_login`-Spalte).
-- Ein freigeschalteter Partner haette mit dem globalen Schalter die
-- Archivierung eines fremden Kanals umgelegt.
--
-- Deshalb: eine Zeile je Streamer, im selben Muster wie
-- `social_media_streamer_layout` (Login als Primaerschluessel, Fremdschluessel
-- auf `twitch_streamers`, `updated_at`/`updated_by` fuer die Nachvollziehbarkeit).

CREATE TABLE IF NOT EXISTS public.social_media_vod_archive (
    streamer_login TEXT PRIMARY KEY
                   REFERENCES public.twitch_streamers (twitch_login)
                   ON UPDATE CASCADE ON DELETE CASCADE,
    -- Aus, bis der Streamer es im Dashboard einschaltet.
    enabled        BOOLEAN NOT NULL DEFAULT FALSE,
    -- Sichtbarkeit auf YouTube. Solange das Google-Projekt nicht auditiert ist,
    -- dreht YouTube ohnehin jeden Upload auf 'private' zurueck; die Wahl steht
    -- trotzdem hier, damit sie nach dem Audit ohne Codeaenderung greift.
    privacy        TEXT NOT NULL DEFAULT 'private',
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_by     TEXT,
    CONSTRAINT social_media_vod_archive_privacy_chk
        CHECK (privacy IN ('private', 'unlisted', 'public'))
);

-- Der Worker fragt je Lauf nach den eingeschalteten Kanaelen.
CREATE INDEX IF NOT EXISTS idx_social_media_vod_archive_enabled
    ON public.social_media_vod_archive (streamer_login)
    WHERE enabled;

-- Bestehende globale Einstellung uebernehmen, damit ein bereits laufendes
-- Archiv nach dem Update nicht stillsteht. Der globale Schalter kannte nur den
-- eigenen Kanal, deshalb geht er genau dorthin.
INSERT INTO public.social_media_vod_archive (streamer_login, enabled, privacy, updated_by)
SELECT
    s.twitch_login,
    COALESCE((SELECT value::text = 'true'
                FROM public.social_media_settings
               WHERE key = 'vod_archive_enabled'), FALSE),
    COALESCE((SELECT trim(both '"' from value::text)
                FROM public.social_media_settings
               WHERE key = 'vod_archive_privacy'
                 AND trim(both '"' from value::text) IN ('private', 'unlisted', 'public')),
             'private'),
    'migration'
FROM public.twitch_streamers s
WHERE s.twitch_login = 'earlysalty'
ON CONFLICT (streamer_login) DO NOTHING;

-- Die globalen Schluessel verschwinden, sonst bleiben zwei Wahrheiten stehen.
DELETE FROM public.social_media_settings
 WHERE key IN ('vod_archive_enabled', 'vod_archive_privacy');

-- Zustand je Streamer: die VOD-Tabelle trug den Kanal schon, hiess aber anders
-- als ueberall sonst im Dashboard.
DO $do$ BEGIN
    ALTER TABLE public.twitch_vod_archive_vods
        RENAME COLUMN channel_login TO streamer_login;
EXCEPTION WHEN undefined_column OR duplicate_column THEN NULL;
END $do$;

-- Kein Fremdschluessel auf `twitch_streamers`: ein bereits archiviertes VOD
-- soll nicht verschwinden, nur weil ein Kanal spaeter aus der Streamer-Liste
-- faellt. Der Index traegt die Warteschlangen-Abfrage des Workers, die jetzt
-- je Streamer laeuft.
DROP INDEX IF EXISTS public.idx_vod_archive_status;
CREATE INDEX IF NOT EXISTS idx_vod_archive_streamer_status
    ON public.twitch_vod_archive_vods (streamer_login, status, discovered_at);

-- Auch die Teile bekommen den Streamer direkt. Hochgeladen wird je Teil, und
-- das Tageskontingent wird je Kanal nachgehalten; ohne die Spalte braeuchte
-- jede dieser Abfragen einen Join auf die VOD-Tabelle.
ALTER TABLE public.twitch_vod_archive_parts
    ADD COLUMN IF NOT EXISTS streamer_login TEXT;

UPDATE public.twitch_vod_archive_parts p
   SET streamer_login = v.streamer_login
  FROM public.twitch_vod_archive_vods v
 WHERE v.id = p.vod_id
   AND p.streamer_login IS DISTINCT FROM v.streamer_login;

CREATE INDEX IF NOT EXISTS idx_vod_archive_parts_streamer
    ON public.twitch_vod_archive_parts (streamer_login, status);
