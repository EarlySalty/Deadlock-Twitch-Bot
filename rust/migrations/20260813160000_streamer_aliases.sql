-- Zweit-Accounts einem Menschen zuordnen.
--
-- Manche Streamer betreiben mehrere Kanaele, etwa einen fuer die Woche und
-- einen fuers Wochenende. Fuer den Bot sind das bisher zwei fremde Personen.
-- Folgen: raidet der eine Account und der Mensch begruesst vom anderen aus im
-- Zielchat, sieht der Bot Schweigen und schickt eine Erinnerung, die keinen
-- Fehler benennt. Genauso zaehlt er Raid-Fairness und Etikette doppelt statt
-- pro Mensch.
--
-- `person_key` ist ein frei gewaehlter Bezeichner (praktisch der Login des
-- Hauptaccounts). Alle Zeilen mit demselben `person_key` gehoeren zusammen.
-- Genau eine davon sollte `is_primary` sein: dorthin gehen Whispers, weil ein
-- selten genutzter Zweitaccount sie womoeglich nie sieht.
--
-- Kein Fremdschluessel auf `twitch_streamers`: ein Zweitaccount ist nicht
-- zwingend selbst im Netzwerk registriert.
CREATE TABLE IF NOT EXISTS public.twitch_streamer_aliases (
    twitch_user_id TEXT PRIMARY KEY,
    twitch_login   TEXT NOT NULL,
    -- Klammer ueber alle Accounts derselben Person.
    person_key     TEXT NOT NULL,
    -- Der Account, an den Whispers gehen.
    is_primary     BOOLEAN NOT NULL DEFAULT FALSE,
    -- Freitext, etwa "Wochenend-Account".
    note           TEXT,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Aufloesung laeuft in beide Richtungen: Account -> Person und Person -> alle
-- Accounts.
CREATE INDEX IF NOT EXISTS idx_streamer_aliases_person
    ON public.twitch_streamer_aliases (person_key);

CREATE UNIQUE INDEX IF NOT EXISTS uq_streamer_aliases_login
    ON public.twitch_streamer_aliases (LOWER(twitch_login));

-- Hoechstens ein Hauptaccount je Person.
CREATE UNIQUE INDEX IF NOT EXISTS uq_streamer_aliases_primary
    ON public.twitch_streamer_aliases (person_key)
    WHERE is_primary;
