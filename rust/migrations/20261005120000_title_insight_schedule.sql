-- Fälligkeit bleibt bei Neustarts und abgebrochenen KI-Aufrufen erhalten.
CREATE TABLE title_generator_insight_schedule (
    streamer_id TEXT PRIMARY KEY,
    attempted_at TIMESTAMPTZ NOT NULL,
    next_attempt_at TIMESTAMPTZ NOT NULL
);
