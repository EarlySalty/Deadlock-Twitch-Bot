-- Entprellen der Ban-Klassifikation: Meldung nur beim Zustandswechsel,
-- einmal pro Vorfall. Die aktive Prüfung schreibt hierhin, was zuletzt
-- gemeldet wurde. Wiederholungen zählen still mit.
CREATE TABLE IF NOT EXISTS public.twitch_bot_ban_klassifikation (
    twitch_user_id TEXT PRIMARY KEY,
    twitch_login TEXT NOT NULL,
    klassifikation TEXT NOT NULL,
    seit TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    letzte_meldung TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    meldungen INTEGER NOT NULL DEFAULT 1
);

COMMENT ON TABLE public.twitch_bot_ban_klassifikation IS
    'Zuletzt gemeldeter Ban-Klassifikationszustand je Kanal. Nur Wechsel werden gemeldet.';
