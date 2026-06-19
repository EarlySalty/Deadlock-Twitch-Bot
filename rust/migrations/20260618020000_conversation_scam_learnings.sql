-- Self-Learning des Conversation-Scam-Guards: netzwerkweit destillierte
-- Erkenntnisse aus bestätigten Scams + aufgehobenen Fehlalarmen. Singleton-Zeile
-- (genau eine, id = TRUE), die der Judge-Prompt als Zusatzhinweis lädt.
CREATE TABLE IF NOT EXISTS public.twitch_scam_guard_learnings (
    id           BOOLEAN PRIMARY KEY DEFAULT TRUE,
    guidance     TEXT NOT NULL,
    source_count INTEGER NOT NULL DEFAULT 0,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT twitch_scam_guard_learnings_singleton CHECK (id)
);
