-- Stufe 1 des Social-Media-Dashboards: geplantes Posten statt Sofort-Upload,
-- Freigabe-Modi und Auto-Posting pro Streamer, Kategorien als Grundgeruest.
--
-- Ablösung globaler Key/Value-Settings nach dem Muster aus
-- 20260814150000_vod_archive_pro_streamer.sql: die globalen `auto_approve_*`-Keys
-- konnten von jedem freigegebenen Partner umgeschaltet werden und galten fuer die
-- ganze Instanz. Sie werden hier in Pro-Streamer-Tabellen ueberfuehrt und geloescht.

-- 1) Kategorie-Katalog -------------------------------------------------------
-- Erstmal ist nur Deadlock aktiv; das Datenmodell kann Kategorien aber von Anfang
-- an. `enrichment_enabled` haelt die LLM-Anreicherung strikt bei Deadlock, alle
-- anderen Kategorien bekommen das nackte Auto-Posting.
CREATE TABLE IF NOT EXISTS public.social_media_category (
    category_key       TEXT PRIMARY KEY,
    display_name       TEXT NOT NULL,
    twitch_game_id     TEXT,
    match_game_names   TEXT[] NOT NULL DEFAULT '{}',
    enrichment_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    sort_order         INTEGER NOT NULL DEFAULT 100,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- `twitch_game_id` bleibt leer, bis der Bot die Helix-Kategorie-ID einmal
-- aufgeloest hat (HelixClient::search_category_id). Bis dahin greift der
-- Namensabgleich ueber `match_game_names` (immer klein geschrieben).
INSERT INTO public.social_media_category
    (category_key, display_name, twitch_game_id, match_game_names, enrichment_enabled, sort_order)
VALUES
    ('deadlock', 'Deadlock',      NULL, ARRAY['deadlock'],  TRUE,  10),
    ('other',    'Andere Spiele', NULL, ARRAY[]::TEXT[],    FALSE, 900)
ON CONFLICT (category_key) DO NOTHING;

-- 2) Kategorie am Clip -------------------------------------------------------
ALTER TABLE public.twitch_clips_social_media
    ADD COLUMN IF NOT EXISTS game_id TEXT,
    ADD COLUMN IF NOT EXISTS category_key TEXT NOT NULL DEFAULT 'other';

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conname = 'twitch_clips_social_media_category_fkey'
    ) THEN
        ALTER TABLE public.twitch_clips_social_media
            ADD CONSTRAINT twitch_clips_social_media_category_fkey
            FOREIGN KEY (category_key)
            REFERENCES public.social_media_category (category_key)
            ON UPDATE CASCADE;
    END IF;
END $$;

-- Bestand einordnen: bisher stand die Kategorie nur als Freitext in `game_name`.
UPDATE public.twitch_clips_social_media c
   SET category_key = k.category_key
  FROM public.social_media_category k
 WHERE c.category_key = 'other'
   AND c.game_name IS NOT NULL
   AND LOWER(BTRIM(c.game_name)) = ANY (k.match_game_names);

CREATE INDEX IF NOT EXISTS idx_twitch_clips_social_media_category
    ON public.twitch_clips_social_media (category_key, discarded_at);

-- 3) Freigabe-Modus pro Streamer ---------------------------------------------
--   manual      : jeder Clip braucht eine ausdrueckliche Freigabe (Default)
--   veto_window : Clip wird eingeplant und geht raus, wenn bis zum Termin
--                 niemand widerspricht
--   full_auto   : Clip wird ohne Sichtung eingeplant
CREATE TABLE IF NOT EXISTS public.social_media_streamer_settings (
    streamer_login TEXT PRIMARY KEY
        REFERENCES public.twitch_streamers (twitch_login) ON UPDATE CASCADE ON DELETE CASCADE,
    approval_mode  TEXT NOT NULL DEFAULT 'manual',
    timezone       TEXT NOT NULL DEFAULT 'Europe/Berlin',
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_by     TEXT,
    CONSTRAINT social_media_streamer_settings_mode_chk
        CHECK (approval_mode IN ('manual', 'veto_window', 'full_auto'))
);

-- 4) Kadenz und Auto-Posting pro Streamer und Plattform ----------------------
-- Defaults aus der Kadenz-Recherche: hoechstens ein Post pro Tag und Plattform,
-- rund vier pro Woche.
CREATE TABLE IF NOT EXISTS public.social_media_platform_schedule (
    streamer_login    TEXT NOT NULL
        REFERENCES public.twitch_streamers (twitch_login) ON UPDATE CASCADE ON DELETE CASCADE,
    platform          TEXT NOT NULL,
    auto_post         BOOLEAN NOT NULL DEFAULT FALSE,
    posts_per_week    INTEGER NOT NULL DEFAULT 4,
    max_posts_per_day INTEGER NOT NULL DEFAULT 1,
    post_times        JSONB   NOT NULL DEFAULT '["18:00"]'::jsonb,
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_by        TEXT,
    PRIMARY KEY (streamer_login, platform),
    CONSTRAINT social_media_platform_schedule_platform_chk
        CHECK (platform IN ('youtube', 'tiktok', 'instagram')),
    CONSTRAINT social_media_platform_schedule_week_chk
        CHECK (posts_per_week BETWEEN 0 AND 70),
    CONSTRAINT social_media_platform_schedule_day_chk
        CHECK (max_posts_per_day BETWEEN 0 AND 10),
    CONSTRAINT social_media_platform_schedule_times_chk
        CHECK (jsonb_typeof(post_times) = 'array')
);

-- 5) Auto-Posting pro Kategorie ----------------------------------------------
CREATE TABLE IF NOT EXISTS public.social_media_category_settings (
    streamer_login TEXT NOT NULL
        REFERENCES public.twitch_streamers (twitch_login) ON UPDATE CASCADE ON DELETE CASCADE,
    category_key   TEXT NOT NULL
        REFERENCES public.social_media_category (category_key) ON UPDATE CASCADE ON DELETE CASCADE,
    auto_post      BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_by     TEXT,
    PRIMARY KEY (streamer_login, category_key)
);

-- 6) Faellige Queue-Eintraege schnell finden ---------------------------------
-- `twitch_clips_upload_queue.scheduled_at` gibt es laenger, gefuellt hat es bisher
-- nur der manuelle Dashboard-Weg. Ab jetzt plant der Approval-Pfad selbst ein.
CREATE INDEX IF NOT EXISTS idx_twitch_clips_upload_queue_scheduled
    ON public.twitch_clips_upload_queue (scheduled_at)
    WHERE status = 'pending';

-- 7) Globale Settings in die Pro-Streamer-Welt ueberfuehren -------------------
-- Bestandswahrung: waren die globalen Flags aus (Prod-Zustand), bleibt es bei
-- `manual` und `auto_post = false`, das Verhalten aendert sich also nicht.
INSERT INTO public.social_media_streamer_settings (streamer_login, approval_mode, timezone, updated_by)
SELECT p.streamer_login,
       CASE
           WHEN EXISTS (
               SELECT 1 FROM public.social_media_settings s
                WHERE s.key IN ('auto_approve_youtube', 'auto_approve_tiktok', 'auto_approve_instagram')
                  AND s.value = 'true'::jsonb
           ) THEN 'full_auto'
           ELSE 'manual'
       END,
       COALESCE(
           (SELECT s.value ->> 'timezone'
              FROM public.social_media_settings s
             WHERE s.key = 'posting_schedule'
               AND jsonb_typeof(s.value -> 'timezone') = 'string'),
           'Europe/Berlin'
       ),
       'migration_20260815120000'
  FROM public.social_media_partner_access p
 WHERE p.granted
ON CONFLICT (streamer_login) DO NOTHING;

INSERT INTO public.social_media_platform_schedule
    (streamer_login, platform, auto_post, post_times, updated_by)
SELECT p.streamer_login,
       v.platform,
       COALESCE(
           (SELECT s.value = 'true'::jsonb
              FROM public.social_media_settings s
             WHERE s.key = 'auto_approve_' || v.platform),
           FALSE
       ),
       COALESCE(
           (SELECT s.value -> 'times'
              FROM public.social_media_settings s
             WHERE s.key = 'posting_schedule'
               AND jsonb_typeof(s.value -> 'times') = 'array'
               AND jsonb_array_length(s.value -> 'times') > 0),
           '["18:00"]'::jsonb
       ),
       'migration_20260815120000'
  FROM public.social_media_partner_access p
 CROSS JOIN (VALUES ('youtube'), ('tiktok'), ('instagram')) AS v (platform)
 WHERE p.granted
ON CONFLICT (streamer_login, platform) DO NOTHING;

-- Deadlock ist die einzige aktive Kategorie und darf deshalb von vornherein
-- automatisch posten; der scharfe Schalter bleibt der Plattform-Schalter oben.
INSERT INTO public.social_media_category_settings
    (streamer_login, category_key, auto_post, updated_by)
SELECT p.streamer_login,
       k.category_key,
       (k.category_key = 'deadlock'),
       'migration_20260815120000'
  FROM public.social_media_partner_access p
 CROSS JOIN public.social_media_category k
 WHERE p.granted
ON CONFLICT (streamer_login, category_key) DO NOTHING;

-- Die globalen Auto-Approve-Keys haben ab hier keinen Leser mehr.
DELETE FROM public.social_media_settings
 WHERE key IN ('auto_approve_youtube', 'auto_approve_tiktok', 'auto_approve_instagram');
