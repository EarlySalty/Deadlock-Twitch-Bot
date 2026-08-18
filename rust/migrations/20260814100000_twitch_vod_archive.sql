-- VOD-Archiv: Twitch-Aufzeichnungen lokal sichern und auf YouTube spiegeln.
--
-- Twitchs eigener YouTube-Export existiert nicht mehr, und VODs verfallen nach
-- kurzer Zeit. Der Verlustschutz ist deshalb zweistufig: zuerst laedt der Worker
-- die Aufzeichnung lokal (das zaehlt kein API-Kontingent und braucht keinen
-- YouTube-Login), danach schiebt er sie hoch. Faellt der Upload aus, bleibt das
-- lokale Archiv trotzdem vollstaendig.
--
-- Der Zustand liegt bewusst hier statt in einer Datei neben dem Dienst: der Bot
-- hat bereits eine Datenbank, und nur so ueberlebt ein Abbruch mitten im Upload
-- einen Neustart.
--
-- Zwei Tabellen, weil YouTube bei 12 Stunden dichtmacht: ein langes VOD wird
-- verlustfrei in mehrere Teile geschnitten, und jeder Teil ist ein eigener
-- Upload mit eigener Session und eigener Video-ID.

-- Eine Zeile je entdecktem Twitch-VOD.
CREATE TABLE IF NOT EXISTS public.twitch_vod_archive_vods (
    id             BIGSERIAL PRIMARY KEY,
    -- Twitch-Video-ID ohne fuehrendes 'v', so wie yt-dlp sie liefert.
    twitch_id      TEXT NOT NULL UNIQUE,
    channel_login  TEXT NOT NULL,
    title          TEXT NOT NULL,
    duration_sec   BIGINT NOT NULL DEFAULT 0,
    -- Aufnahmedatum aus der info.json von yt-dlp, nicht der Entdeckungszeitpunkt.
    recorded_at    DATE,
    -- 'new' | 'downloading' | 'downloaded' | 'uploaded' | 'download_failed'
    -- | 'upload_failed' | 'archived' (lokale Dateien geloescht)
    status         TEXT NOT NULL DEFAULT 'new',
    -- Ungeschnittene Quelldatei; die tatsaechlich hochgeladenen Dateien stehen
    -- in twitch_vod_archive_parts.
    local_path     TEXT,
    last_error     TEXT,
    discovered_at  TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    downloaded_at  TIMESTAMPTZ,
    uploaded_at    TIMESTAMPTZ,
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Der Worker fragt je Lauf nach offener Arbeit, aeltestes VOD zuerst.
CREATE INDEX IF NOT EXISTS idx_vod_archive_status
    ON public.twitch_vod_archive_vods (status, discovered_at);

-- Ein Upload-Ziel je Teil. Bei VODs unter der Laengengrenze gibt es genau
-- einen Teil mit part_index 0.
CREATE TABLE IF NOT EXISTS public.twitch_vod_archive_parts (
    id                 BIGSERIAL PRIMARY KEY,
    vod_id             BIGINT NOT NULL
                       REFERENCES public.twitch_vod_archive_vods (id) ON DELETE CASCADE,
    part_index         INTEGER NOT NULL,
    file_path          TEXT NOT NULL,
    size_bytes         BIGINT NOT NULL DEFAULT 0,
    -- 'pending' | 'uploading' | 'done' | 'failed'
    status             TEXT NOT NULL DEFAULT 'pending',
    -- Resumable-Session von YouTube. Solange sie gueltig ist, wird an derselben
    -- Stelle weitergeschoben statt von vorne begonnen; bei mehreren Gigabyte je
    -- Teil ist das der Unterschied zwischen Fortsetzen und Aufgeben.
    upload_session_uri TEXT,
    -- Zuletzt von YouTube bestaetigte Byte-Position innerhalb der Datei.
    upload_offset      BIGINT NOT NULL DEFAULT 0,
    youtube_video_id   TEXT,
    last_error         TEXT,
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (vod_id, part_index)
);

-- Offene Teile eines VODs in Reihenfolge abarbeiten.
CREATE INDEX IF NOT EXISTS idx_vod_archive_parts_offen
    ON public.twitch_vod_archive_parts (vod_id, part_index)
    WHERE status <> 'done';
