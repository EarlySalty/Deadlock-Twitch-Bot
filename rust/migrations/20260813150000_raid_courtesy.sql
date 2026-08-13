-- Raid-Etikette: messen, ob ein Streamer nach dem eigenen Raid im Zielchat
-- auftaucht, und daraus Score und Matching ableiten.
--
-- Bisher lief die Beobachtung ausschliesslich prozess-lokal im
-- RaidGreetingMonitor: 20 Minuten warten, im Zielchat mitlesen, sonst eine
-- Whisper senden. Das Ergebnis verfiel danach. Damit war weder ein Score noch
-- eine Drosselung der Whispers moeglich, und ein Bot-Neustart loeschte alles.
--
-- Drei Klassen je Raid (Details in `tb-raid/src/courtesy.rs`):
--   engaged  >= 3 Nachrichten, oder >= 2 ueber >= 3 Minuten
--   greeter  1 bis 2 Nachrichten
--   silent   keine Nachricht
--
-- `unknown` steht fuer nicht messbar (Chat-Beobachtung nicht verfuegbar,
-- Bot-Neustart, Zielstream vorzeitig beendet, Raid umgeleitet). Es wird
-- bewusst als eigene Klasse gespeichert statt weggelassen, damit spaeter
-- sichtbar bleibt, wie viele Raids gar nicht bewertbar waren. In den Score
-- fliesst es nie ein.

-- Eine Zeile je ausgefuehrtem Raid, geschrieben am Ende des Beobachtungsfensters.
CREATE TABLE IF NOT EXISTS public.twitch_raid_courtesy_events (
    id                     BIGSERIAL PRIMARY KEY,
    raid_history_id        BIGINT,
    from_broadcaster_id    TEXT NOT NULL,
    from_broadcaster_login TEXT NOT NULL,
    to_broadcaster_id      TEXT NOT NULL,
    to_broadcaster_login   TEXT NOT NULL,
    -- Beginn des Beobachtungsfensters (Raid-Start).
    observed_from          TIMESTAMPTZ NOT NULL,
    -- Ende des Fensters, also der Auswertungszeitpunkt.
    observed_at            TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- 'engaged' | 'greeter' | 'silent' | 'unknown'
    courtesy_class         TEXT NOT NULL,
    -- Beobachtete Nachrichten des Raiders im Zielchat.
    message_count          INTEGER NOT NULL DEFAULT 0,
    -- Sekunden zwischen erster und letzter beobachteter Nachricht.
    message_span_sec       INTEGER NOT NULL DEFAULT 0,
    -- Woher die Beobachtung stammt: 'eventsub', 'irc_probe', 'both'.
    observation_source     TEXT,
    -- Bei 'unknown': warum nicht messbar.
    unknown_reason         TEXT,
    -- Ob deswegen eine Whisper rausging (fuer die Drosselung).
    whisper_sent           BOOLEAN NOT NULL DEFAULT FALSE
);

-- Score-Aufbau liest je Streamer die Ereignisse der letzten 45 Tage.
CREATE INDEX IF NOT EXISTS idx_raid_courtesy_from_observed
    ON public.twitch_raid_courtesy_events (from_broadcaster_id, observed_at DESC);

-- Whisper-Drosselung fragt nach dem letzten Versand je Streamer.
CREATE INDEX IF NOT EXISTS idx_raid_courtesy_whisper
    ON public.twitch_raid_courtesy_events (from_broadcaster_id, observed_at DESC)
    WHERE whisper_sent;

-- Ein Raid darf nur einmal bewertet werden. Raids ohne History-Referenz
-- (etwa retargetete) bleiben ausgenommen, weil NULL in einem UNIQUE-Index
-- nicht kollidiert.
CREATE UNIQUE INDEX IF NOT EXISTS uq_raid_courtesy_history
    ON public.twitch_raid_courtesy_events (raid_history_id)
    WHERE raid_history_id IS NOT NULL;

-- Aggregat im Score-Cache: `courtesy_score` ist der Anteil eigener Raids mit
-- Nachricht, gegen 1.0 geshrinkt. Default 1.0 heisst: ohne Datenlage kein
-- Abzug. Der Wert ist ein reiner Malus fuer belegtes Schweigen, wer schreibt
-- verliert nichts.
ALTER TABLE public.twitch_partner_raid_scores
    ADD COLUMN IF NOT EXISTS courtesy_score DOUBLE PRECISION NOT NULL DEFAULT 1.0;

-- Matching-Klasse des Streamers selbst ('engaged' | 'greeter' | 'silent').
-- NULL = noch keine auswertbare Historie, dann matcht er wie bisher rein
-- ueber den Score.
ALTER TABLE public.twitch_partner_raid_scores
    ADD COLUMN IF NOT EXISTS courtesy_class TEXT;

-- Wie viele auswertbare Raids hinter dem Wert stehen (Transparenz im Dashboard).
ALTER TABLE public.twitch_partner_raid_scores
    ADD COLUMN IF NOT EXISTS courtesy_observed INTEGER NOT NULL DEFAULT 0;
