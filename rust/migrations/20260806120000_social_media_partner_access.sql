-- Tabelle für die Partner-Freigabe im Social-Media-Dashboard.
-- Jeder freigegebene Streamer darf Social-Media-Posts erstellen/bearbeiten.
-- Guard: nur Streamer mit granted=true passieren die zentrale Prüfung.

CREATE TABLE IF NOT EXISTS social_media_partner_access (
    streamer_login TEXT PRIMARY KEY REFERENCES twitch_streamers(twitch_login) ON DELETE CASCADE,
    granted BOOLEAN NOT NULL DEFAULT FALSE,
    granted_by TEXT,
    granted_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Initiale Freigabe für den Owner (EarlySalty).
INSERT INTO social_media_partner_access (streamer_login, granted, granted_by)
VALUES ('earlysalty', TRUE, 'system')
ON CONFLICT (streamer_login) DO NOTHING;
