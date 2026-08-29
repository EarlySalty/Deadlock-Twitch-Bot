-- Scout-Kandidaten: kleine, erstmalig gesehene Deadlock-Kanäle, die der
-- Nutzer im Admin-Dashboard vor der Ansprache freigeben muss
-- (tb-scout, Contract INV-06: Zustand nur in der zentralen PG).
--
-- Statuswerte: vorgeschlagen | approved | uebersprungen | pausiert |
-- persoenlich ("Owner übernimmt den Kanal persönlich") |
-- bekannter_kontakt (manueller Override, nie Vorschlag, nie KI). Freigaben
-- und Überspringungen werden nie automatisch überschrieben; der Upsert-Pfad
-- im Code aktualisiert deshalb nur Zeilen mit status 'vorgeschlagen'. Status
-- bleibt wie im Bestand üblich TEXT.

CREATE TABLE IF NOT EXISTS twitch_scout_candidates (
    streamer_login TEXT PRIMARY KEY,
    twitch_user_id TEXT,
    sessions_count INTEGER NOT NULL DEFAULT 0,
    avg_viewers REAL NOT NULL DEFAULT 0,
    first_seen TIMESTAMPTZ,
    last_seen TIMESTAMPTZ,
    language TEXT,
    deadlock_share REAL NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'vorgeschlagen',
    entscheid_grund TEXT,
    approver TEXT,
    decided_at TIMESTAMPTZ,
    dispatched_at TIMESTAMPTZ,
    visited_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_twitch_scout_candidates_status
    ON twitch_scout_candidates (status, decided_at);
