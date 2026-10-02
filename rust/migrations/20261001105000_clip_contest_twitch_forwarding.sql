-- Community-Streamer-Brücke, Paket E: Clip-Einreichung aus Twitch für den
-- wöchentlichen Clip-Contest im Discord (Paket D, Deadlock-Bots).
-- Rein additiv. Eine Zeile je Twitch-Clip; der Clip selbst und das Voting
-- liegen in der zentralen DB. Diese Tabelle trägt nur das, was der Twitch-Bot
-- für Tageslimit, Doppelsend-Schutz und die Anzeige im Dashboard braucht.
--
-- status:
--   pending    Einreichung läuft gerade (Claim vor dem Broker-Aufruf)
--   accepted   Broker hat angenommen
--   duplicate  Clip wurde bereits eingereicht (auch aus dem Discord)
--   rejected   Broker hat abgelehnt (reason vom Broker)
--   failed     Broker nicht erreichbar; ein neuer Versuch ist erlaubt
CREATE TABLE IF NOT EXISTS twitch_clip_contest_forwards (
    clip_id TEXT PRIMARY KEY,
    clip_url TEXT NOT NULL,
    broadcaster_twitch_id TEXT NOT NULL,
    broadcaster_login TEXT NOT NULL,
    submitted_by_twitch_id TEXT,
    via TEXT NOT NULL CHECK (via IN ('chat', 'dashboard')),
    status TEXT NOT NULL CHECK (status IN ('pending', 'accepted', 'duplicate', 'rejected', 'failed')),
    broker_submission_id BIGINT,
    reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);

CREATE INDEX IF NOT EXISTS idx_twitch_clip_contest_forwards_channel_day
    ON twitch_clip_contest_forwards (broadcaster_twitch_id, created_at);

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT, INSERT, UPDATE ON twitch_clip_contest_forwards TO twitchbot;
    END IF;
    -- Das Dashboard liest; Einreichungen schreibt die authentifizierte Bot-API.
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT ON twitch_clip_contest_forwards TO twitchdash;
    END IF;
END
$$;
