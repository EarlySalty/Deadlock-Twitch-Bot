-- Reaktions-Lernmodus für den Engagement-Layer.
--
-- Zweck: aufzeichnen, WORAUF der Owner im fremden Twitch-Chat reagiert und WIE.
-- Dafür wird jede eigene Chat-Nachricht mit dem Stream-Audio-Transkript der
-- Sekunden davor und den Chat-Zeilen davor zu einem Stimulus/Response-Paar
-- verknüpft. Das Ergebnis speist später den Few-Shot-Stil und ein destilliertes
-- Reaktionsprofil.
--
-- Abgrenzung zu `twitch_engagement_stream_transcripts`: die operative Tabelle
-- ist bewusst flüchtig (Trim nach 60 min / 40 Segmente je Kanal) und läuft nur
-- für Kanäle mit `twitch_engagement_settings.enabled = TRUE`. Der Lernmodus
-- braucht das Gegenteil: beliebige Kanäle, längere Haltbarkeit. Darum eigene
-- Tabellen statt Erweiterung der bestehenden.
--
-- Kein Trigger `tb_fill_twitch_user_id_from_login`: Lern-Kanäle stehen nicht
-- zwingend in `twitch_streamers`, die ID kommt direkt aus dem Chat-Event.

-- Kanäle, in denen der Owner zuletzt geschrieben hat ("lern-heiß").
CREATE TABLE IF NOT EXISTS public.twitch_engagement_learn_channels (
    channel_login   TEXT PRIMARY KEY,
    channel_user_id TEXT,
    first_seen_at   TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_seen_at    TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    message_count   BIGINT NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_engagement_learn_channels_last_seen
    ON public.twitch_engagement_learn_channels (last_seen_at DESC);

-- Der gebündelte Zeitstrahl eines lern-heißen Kanals: was gesagt wurde und was
-- gleichzeitig im Chat stand, in EINER Tabelle statt in drei nebeneinander.
--
-- Getrennte Tabellen für Audio und Chat hätten bedeutet, dass die beiden Seiten
-- erst im fertigen Sample zusammenfinden — also nur in den Sekunden rund um
-- eine eigene Nachricht. Der Rest der Sitzung wäre in zwei Hälften zerfallen,
-- die niemand mehr am Stück lesen kann. Hier liegt beides auf einer Zeitachse.
--
-- `kind`:
--   'stream' — Whisper-Segment, `started_at` gesetzt, `author` leer
--   'chat'   — fremde Chat-Zeile
--   'own'    — Chat-Zeile des Owners; nur diese Sorte wird gemappt
--
-- `ts` ist immer der maßgebliche Zeitpunkt: bei Chat die Sendezeit, bei Audio
-- das ENDE des Segments (dann war der Satz zu Ende gesprochen). So sortiert
-- ein einziges ORDER BY die ganze Sitzung richtig.
CREATE TABLE IF NOT EXISTS public.twitch_engagement_learn_timeline (
    id            BIGSERIAL PRIMARY KEY,
    channel_login TEXT NOT NULL,
    kind          TEXT NOT NULL
                  CHECK (kind IN ('stream', 'chat', 'own')),
    ts            TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    started_at    TIMESTAMPTZ,
    author        TEXT,
    content       TEXT NOT NULL,
    engine        TEXT,
    model         TEXT,
    message_id    TEXT,
    mapped_at     TIMESTAMPTZ,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_engagement_learn_timeline_channel_ts
    ON public.twitch_engagement_learn_timeline (channel_login, ts DESC);

-- Der Arbeitsvorrat des Mappers: eigene Nachrichten, die noch kein Sample sind.
CREATE INDEX IF NOT EXISTS idx_engagement_learn_timeline_pending
    ON public.twitch_engagement_learn_timeline (ts)
    WHERE kind = 'own' AND mapped_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_engagement_learn_timeline_created
    ON public.twitch_engagement_learn_timeline (created_at);

-- Das fertige Stimulus/Response-Paar. Überlebt das Trimmen des Zeitstrahls.
-- `verdict` ist die manuelle Sichtung: 'good' = so soll die KI klingen,
-- 'bad' = kein Vorbild, NULL = ungesichtet.
CREATE TABLE IF NOT EXISTS public.twitch_engagement_reaction_samples (
    id                 BIGSERIAL PRIMARY KEY,
    channel_login      TEXT NOT NULL,
    message_ts         TIMESTAMPTZ NOT NULL,
    my_message         TEXT NOT NULL,
    stream_context     TEXT NOT NULL DEFAULT '',
    chat_context       TEXT NOT NULL DEFAULT '',
    has_stream_context BOOLEAN NOT NULL DEFAULT FALSE,
    verdict            TEXT,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT twitch_engagement_reaction_samples_unique
        UNIQUE (channel_login, message_ts, my_message)
);

CREATE INDEX IF NOT EXISTS idx_engagement_reaction_samples_channel_ts
    ON public.twitch_engagement_reaction_samples (channel_login, message_ts DESC);

CREATE INDEX IF NOT EXISTS idx_engagement_reaction_samples_verdict
    ON public.twitch_engagement_reaction_samples (verdict, message_ts DESC);
