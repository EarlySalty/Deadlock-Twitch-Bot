-- Zustandsgedächtnis der aktiven Moderator-Prüfung.
--
-- Der Sweep läuft stündlich und meldete jede abgelehnte Moderator-Einsetzung
-- erneut, für denselben Kanal also 24 Meldungen pro Tag. Gemeldet werden soll
-- der Zustand, und zwar wenn er wechselt. Diese Tabelle hält je Kanal fest, was
-- zuletzt gemeldet wurde; solange sich daran nichts ändert, bleibt es still.
CREATE TABLE IF NOT EXISTS twitch_ban_probe_zustand (
    twitch_user_id TEXT PRIMARY KEY,
    twitch_login   TEXT NOT NULL DEFAULT '',
    -- 'mod_rechte_weg' (Chat läuft weiter, nur keine Mod-Rechte) oder 'gebannt'
    zustand        TEXT NOT NULL,
    seit           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    letzte_probe   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    proben         BIGINT NOT NULL DEFAULT 1
);
