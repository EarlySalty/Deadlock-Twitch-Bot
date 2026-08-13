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

-- Die eigenen Nachrichten = die Response-Seite. `mapped_at IS NULL` heißt:
-- wartet noch auf den Mapper.
CREATE TABLE IF NOT EXISTS public.twitch_engagement_learn_messages (
    id            BIGSERIAL PRIMARY KEY,
    channel_login TEXT NOT NULL,
    twitch_login  TEXT NOT NULL,
    content       TEXT NOT NULL,
    message_id    TEXT,
    ts            TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    mapped_at     TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_engagement_learn_messages_pending
    ON public.twitch_engagement_learn_messages (ts)
    WHERE mapped_at IS NULL;

-- Chat-Ringpuffer der lern-heißen Kanäle = die Umgebungs-Seite. Nötig, weil
-- `twitch_engagement_conversation` nur für engagement-aktive Partner-Kanäle
-- gefüllt wird.
CREATE TABLE IF NOT EXISTS public.twitch_engagement_learn_chat (
    id            BIGSERIAL PRIMARY KEY,
    channel_login TEXT NOT NULL,
    twitch_login  TEXT NOT NULL,
    content       TEXT NOT NULL,
    ts            TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_engagement_learn_chat_channel_ts
    ON public.twitch_engagement_learn_chat (channel_login, ts DESC);

-- Whisper-Segmente der lern-heißen Kanäle = die Stimulus-Seite.
CREATE TABLE IF NOT EXISTS public.twitch_engagement_learn_transcripts (
    id            BIGSERIAL PRIMARY KEY,
    channel_login TEXT NOT NULL,
    started_at    TIMESTAMPTZ NOT NULL,
    ended_at      TIMESTAMPTZ NOT NULL,
    text          TEXT NOT NULL,
    engine        TEXT NOT NULL,
    model         TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_engagement_learn_transcripts_channel_ended
    ON public.twitch_engagement_learn_transcripts (channel_login, ended_at DESC);

-- Das fertige Stimulus/Response-Paar. Überlebt das Trimmen der Rohdaten.
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
