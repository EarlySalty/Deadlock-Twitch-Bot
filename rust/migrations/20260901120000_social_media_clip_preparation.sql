-- Plattformfreie, universelle 60-Sekunden-9:16-Variante je Clip. Externe
-- Veröffentlichungen bleiben nach der Migration standardmäßig gesperrt.

ALTER TABLE public.social_media_streamer_settings
    ADD COLUMN IF NOT EXISTS release_mode TEXT NOT NULL DEFAULT 'prepare_only';

-- OAuth-Verbindung und Erlaubnis für echte Provider-Aufrufe sind getrennte
-- Zustände. Bestehende wie neue Verbindungen bleiben bis zum bewussten
-- Plattform-Audit und Testkonto standardmäßig gesperrt.
ALTER TABLE public.social_media_platform_auth
    ADD COLUMN IF NOT EXISTS provider_calls_enabled BOOLEAN NOT NULL DEFAULT FALSE;

-- Der alte allgemeine Auto-Post-Schalter bildet die von TikTok verlangte
-- per-Clip-Zustimmung nicht ab. Bestehende Legacy-Schalter werden deshalb
-- sichtbar ausgeschaltet und dürfen erst mit einer eigenen Consent-UX ersetzt
-- werden.
UPDATE public.social_media_platform_schedule
   SET auto_post = FALSE,
       updated_at = CURRENT_TIMESTAMP
 WHERE platform = 'tiktok'
   AND auto_post;

ALTER TABLE public.social_media_clip_approval
    ADD COLUMN IF NOT EXISTS approved_render_fingerprint TEXT;

ALTER TABLE public.twitch_clips_upload_queue
    ADD COLUMN IF NOT EXISTS provider_started_at TIMESTAMPTZ;
ALTER TABLE public.twitch_clips_upload_queue
    ADD COLUMN IF NOT EXISTS provider_lease_token TEXT;
ALTER TABLE public.twitch_clips_upload_queue
    ADD COLUMN IF NOT EXISTS provider_external_id TEXT;
ALTER TABLE public.twitch_clips_upload_queue
    ADD COLUMN IF NOT EXISTS provider_accepted_at TIMESTAMPTZ;

-- Beim Cutover ist für alte `processing`-Zeilen nicht mehr beweisbar, ob der
-- Legacy-Worker den Provider bereits erreicht hatte. Solche Versuche dürfen
-- der neue Reclaimer deshalb niemals automatisch erneut einreihen. Der
-- stabile Marker macht sie explizit manuell abgleichpflichtig; die Anweisung
-- ist bei einer erneuten Ausführung idempotent.
UPDATE public.twitch_clips_upload_queue
   SET status = 'reconciliation_required',
       provider_started_at = COALESCE(last_attempt_at, created_at, CURRENT_TIMESTAMP),
       provider_lease_token = NULL,
       completed_at = NULL,
       last_error = 'legacy_processing_requires_reconciliation'
 WHERE status = 'processing'
   AND provider_started_at IS NULL;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
          FROM public.twitch_clips_upload_queue
         WHERE status IN ('pending', 'processing', 'reconciliation_required')
         GROUP BY clip_id, platform
        HAVING COUNT(*) > 1
    ) THEN
        RAISE EXCEPTION USING
            MESSAGE = 'Social-Media-Migration abgebrochen: mehrere offene Upload-Versuche für denselben Clip und dieselbe Plattform',
            HINT = 'Offene Queue-Duplikate vor der Migration manuell abgleichen; gestartete Provider-Versuche niemals automatisch löschen oder erneut einreihen.';
    END IF;
    IF EXISTS (
        SELECT 1
          FROM public.twitch_clips_upload_queue
         WHERE provider_started_at IS NOT NULL
           AND status IN ('processing', 'reconciliation_required')
         GROUP BY clip_id, platform
        HAVING COUNT(*) > 1
    ) THEN
        RAISE EXCEPTION USING
            MESSAGE = 'Social-Media-Migration abgebrochen: mehrere unklare Provider-Versuche für denselben Clip und dieselbe Plattform',
            HINT = 'Provider-Ergebnisse anhand externer IDs manuell abgleichen, bevor die Migration erneut läuft.';
    END IF;
END $$;

CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_clips_upload_queue_provider_inflight
    ON public.twitch_clips_upload_queue (clip_id, platform)
    WHERE provider_started_at IS NOT NULL
      AND status IN ('processing', 'reconciliation_required');

CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_clips_upload_queue_one_open_attempt
    ON public.twitch_clips_upload_queue (clip_id, platform)
    WHERE status IN ('pending', 'processing', 'reconciliation_required');

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conname = 'social_media_streamer_settings_release_mode_chk'
           AND conrelid = 'public.social_media_streamer_settings'::regclass
           AND contype = 'c'
    ) THEN
        ALTER TABLE public.social_media_streamer_settings
            ADD CONSTRAINT social_media_streamer_settings_release_mode_chk
            CHECK (release_mode IN ('prepare_only', 'live'));
    END IF;
END $$;

CREATE TABLE IF NOT EXISTS public.social_media_clip_preparation (
    clip_db_id        BIGINT PRIMARY KEY
        REFERENCES public.twitch_clips_social_media (id) ON DELETE CASCADE,
    state             TEXT NOT NULL DEFAULT 'pending',
    lease_token       TEXT,
    source_fingerprint TEXT,
    render_fingerprint TEXT,
    render_path       TEXT,
    error_code        TEXT,
    error_message     TEXT,
    requested_at      TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    started_at        TIMESTAMPTZ,
    completed_at      TIMESTAMPTZ,
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT social_media_clip_preparation_state_chk CHECK (
        state IN ('pending', 'materializing', 'source_ready', 'rendering', 'preview_ready', 'failed')
    )
);

CREATE INDEX IF NOT EXISTS idx_social_media_clip_preparation_pending
    ON public.social_media_clip_preparation (requested_at, clip_db_id)
    WHERE state = 'pending';

CREATE INDEX IF NOT EXISTS idx_social_media_clip_preparation_active_lease
    ON public.social_media_clip_preparation (updated_at, clip_db_id)
    WHERE state IN ('materializing', 'rendering');

CREATE OR REPLACE FUNCTION public.ensure_social_media_clip_preparation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    INSERT INTO public.social_media_clip_preparation (clip_db_id)
    VALUES (NEW.id)
    ON CONFLICT (clip_db_id) DO NOTHING;
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS trg_social_media_clip_preparation ON public.twitch_clips_social_media;
CREATE TRIGGER trg_social_media_clip_preparation
AFTER INSERT ON public.twitch_clips_social_media
FOR EACH ROW EXECUTE FUNCTION public.ensure_social_media_clip_preparation();

-- Frühere Rust-/Python-Pfade kopierten den Kanalstandard als scheinbares
-- Clip-Override. Nur unveröffentlichte, exakt mit dem damaligen Standard
-- übereinstimmende Snapshots werden wieder auf dynamische Vererbung gestellt;
-- abweichende Individualanpassungen bleiben unangetastet. Das Backup bewahrt
-- den exakten Altwert samt Erkennungsgrund für einen App-/Datenrollback.
CREATE TABLE IF NOT EXISTS public.social_media_layout_override_migration_backup_20260901 (
    clip_db_id          BIGINT PRIMARY KEY,
    layout_override_json JSONB NOT NULL,
    reason              TEXT NOT NULL,
    backed_up_at        TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO public.social_media_layout_override_migration_backup_20260901
    (clip_db_id, layout_override_json, reason)
SELECT c.id, c.layout_override_json, 'matched_streamer_default'
  FROM public.twitch_clips_social_media AS c
  JOIN public.social_media_streamer_layout AS l
    ON LOWER(l.streamer_login) = LOWER(c.streamer_login)
 WHERE c.layout_override_json = (
       l.layout_json || jsonb_build_object(
           'cam_enabled', l.cam_enabled,
           'mode', l.mode
       )
   )
   AND c.discarded_at IS NULL
   AND NOT COALESCE(c.uploaded_tiktok, FALSE)
   AND NOT COALESCE(c.uploaded_youtube, FALSE)
   AND NOT COALESCE(c.uploaded_instagram, FALSE)
ON CONFLICT (clip_db_id) DO NOTHING;

INSERT INTO public.social_media_layout_override_migration_backup_20260901
    (clip_db_id, layout_override_json, reason)
SELECT c.id, c.layout_override_json, 'matched_global_default'
  FROM public.twitch_clips_social_media AS c
 WHERE c.layout_override_json = '{
       "version": 1,
       "source": {"width": 1920, "height": 1080},
       "game_crop": {"x": 0, "y": 0, "w": 1080, "h": 1080},
       "cam_crop": {"x": 1500, "y": 50, "w": 380, "h": 380},
       "cam_position": {"x": 712, "y": 48, "w": 320, "h": 320},
       "cam_enabled": true,
       "mode": "pip"
   }'::jsonb
   AND c.discarded_at IS NULL
   AND NOT COALESCE(c.uploaded_tiktok, FALSE)
   AND NOT COALESCE(c.uploaded_youtube, FALSE)
   AND NOT COALESCE(c.uploaded_instagram, FALSE)
ON CONFLICT (clip_db_id) DO NOTHING;

UPDATE public.twitch_clips_social_media AS c
   SET layout_override_json = NULL
  FROM public.social_media_layout_override_migration_backup_20260901 AS backup
 WHERE backup.clip_db_id = c.id
   AND c.layout_override_json = backup.layout_override_json;

-- Trigger zuerst, Backfill zuletzt: so kann zwischen Snapshot und Triggeranlage
-- kein parallel eingefügter Clip ohne Preparation-Zeile durchs Raster fallen.
INSERT INTO public.social_media_clip_preparation (clip_db_id)
SELECT id FROM public.twitch_clips_social_media
ON CONFLICT (clip_db_id) DO NOTHING;
