-- Zweit-Accounts einem Menschen zuordnen.
--
-- Manche Streamer betreiben mehrere Kanaele, etwa einen fuer die Woche und
-- einen fuers Wochenende. Fuer den Bot sind das bisher zwei fremde Personen.
-- Folgen: raidet der eine Account und der Mensch begruesst vom anderen aus im
-- Zielchat, sieht der Bot Schweigen und schickt eine Erinnerung, die keinen
-- Fehler benennt. Genauso zaehlt er Raid-Fairness und Etikette doppelt statt
-- pro Mensch.
--
-- Zugeordnet wird ausschliesslich ueber die **Twitch-User-ID**. Logins sind
-- nicht dauerhaft: gibt jemand seinen Namen auf, kann ihn ein Fremder
-- uebernehmen, und ein Namens-Match wuerde diesen Fremden als Zweit-Account
-- durchgehen lassen. `twitch_login` steht nur zur Lesbarkeit in Logs und
-- Dashboard mit drin.
--
-- `person_key` ist die Twitch-User-ID des Hauptaccounts. Alle Zeilen mit
-- demselben `person_key` gehoeren zusammen. Genau eine davon sollte
-- `is_primary` sein: dorthin gehen Whispers, weil ein selten genutzter
-- Zweitaccount sie womoeglich nie sieht.
--
-- Kein Fremdschluessel auf `twitch_streamers`: ein Zweitaccount ist nicht
-- zwingend selbst im Netzwerk registriert.
CREATE TABLE IF NOT EXISTS public.twitch_streamer_aliases (
    twitch_user_id TEXT PRIMARY KEY,
    -- Nur zur Lesbarkeit, nie fuer die Zuordnung. Darf veralten.
    twitch_login   TEXT NOT NULL,
    -- Klammer ueber alle Accounts derselben Person: die User-ID des Hauptaccounts.
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

-- Hoechstens ein Hauptaccount je Person.
CREATE UNIQUE INDEX IF NOT EXISTS uq_streamer_aliases_primary
    ON public.twitch_streamer_aliases (person_key)
    WHERE is_primary;
