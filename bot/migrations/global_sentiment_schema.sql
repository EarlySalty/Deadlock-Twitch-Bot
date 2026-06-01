-- Globaler Community-Sentiment (alle Channels gepoolt, entkoppelt vom Pipeline-Pfad).
-- Ein Background-Job destilliert periodisch aus den letzten Chat-Nachrichten ALLER
-- Channels ein kompaktes Stimmungsbild ("wie fuehlt sich Deadlock gerade an" — Meta,
-- Patches, was nervt/gefeiert wird). Die Engagement-Pipeline liest nur die neueste
-- Zeile. Append-only; der Job trimmt alte Zeilen.
-- Idempotent: CREATE ... IF NOT EXISTS.
CREATE TABLE IF NOT EXISTS twitch_engagement_global_sentiment (
    id              BIGSERIAL PRIMARY KEY,
    sentiment_text  TEXT NOT NULL,
    msg_count       INT NOT NULL DEFAULT 0,
    model           TEXT,
    built_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_eng_global_sentiment_built
    ON twitch_engagement_global_sentiment (built_at DESC);
