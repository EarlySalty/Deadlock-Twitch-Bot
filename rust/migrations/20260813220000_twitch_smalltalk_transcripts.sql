-- Stream-Ton einer Smalltalk-Testsitzung, lokal transkribiert.
--
-- Bewusst getrennt von `twitch_engagement_stream_transcripts`: die ist ein
-- Ringpuffer fuer den Prompt und wird nach 60 Minuten getrimmt. Eine Sitzung
-- dauert selbst 60 Minuten, ihre Auswertung geht erst danach raus. Ohne eigene
-- Ablage waere der Anfang der Sitzung beim Auswerten schon geloescht.
--
-- Aufbewahrung haengt an der Sitzung (CASCADE), damit die Loeschung des
-- Reports auch den Ton mitnimmt.
CREATE TABLE twitch_smalltalk_transcripts (
    id BIGSERIAL PRIMARY KEY,
    session_id UUID NOT NULL
        REFERENCES twitch_smalltalk_sessions(id) ON DELETE CASCADE,
    channel_login TEXT NOT NULL,
    started_at TIMESTAMPTZ NOT NULL,
    ended_at TIMESTAMPTZ NOT NULL,
    text TEXT NOT NULL,
    engine TEXT NOT NULL,
    model TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (ended_at >= started_at),
    CHECK (BTRIM(text) <> '')
);

CREATE INDEX twitch_smalltalk_transcripts_session_time_idx
    ON twitch_smalltalk_transcripts (session_id, ended_at, id);
