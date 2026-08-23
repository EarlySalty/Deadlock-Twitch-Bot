-- Eigenes Konto fuer Vertagungen wegen vollem Tageskontingent.
--
-- `attempts` zaehlt echte Fehlversuche und ist nach fuenf erschoepft. Ein volles
-- Tageskontingent sagt nichts ueber den Clip und darf dieses Konto deshalb nicht
-- aufzehren. Ohne eigenes Konto blieb ein dauerhaft abgelehnter Job (falsche
-- Projekt-Quota, gesperrte App) allerdings endlos in der Warteschlange und
-- vertagte sich alle 24 Stunden neu, ohne je auf `failed` zu gehen.
ALTER TABLE twitch_clips_upload_queue
    ADD COLUMN IF NOT EXISTS quota_deferrals INTEGER NOT NULL DEFAULT 0;
