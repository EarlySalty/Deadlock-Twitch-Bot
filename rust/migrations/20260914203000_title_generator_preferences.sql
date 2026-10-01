-- Persistente Titelgenerator-Praeferenzen pro Twitch-Konto.
--
-- `style_preference` ist bewusst Freitext: Streamer koennen ihren eigenen Ton
-- beschreiben, ohne dass wir starre Stil-Presets erzwingen. `experimental_auto_set`
-- ist opt-in und darf nur wirken, wenn der gespeicherte Twitch-Grant
-- channel:manage:broadcast enthaelt.
CREATE TABLE IF NOT EXISTS title_generator_preferences (
    twitch_user_id TEXT PRIMARY KEY,
    style_preference TEXT NOT NULL DEFAULT '',
    experimental_auto_set BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT title_generator_preferences_style_len
        CHECK (char_length(style_preference) <= 1200)
);

-- Human-Feedback-Lernschleife fuer Titelvorschlaege. Das ist bewusst kein
-- undurchsichtiges Online-Finetuning: wir speichern nachvollziehbar, welche
-- Vorschlaege der Streamer mochte, ablehnte, auswaehlte oder selbst verbesserte
-- und geben diese Beispiele bei der naechsten Generierung als hoch priorisierte
-- Stilreferenzen an das Modell.
CREATE TABLE IF NOT EXISTS title_generator_feedback (
    generation_id TEXT PRIMARY KEY,
    twitch_user_id TEXT NOT NULL,
    keywords TEXT NOT NULL DEFAULT '',
    primary_title TEXT NOT NULL,
    alternatives JSONB NOT NULL DEFAULT '[]'::jsonb,
    feedback TEXT,
    selected_title TEXT,
    edited_title TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    feedback_at TIMESTAMPTZ,
    CONSTRAINT title_generator_feedback_kind
        CHECK (feedback IS NULL OR feedback IN ('liked','disliked','selected','edited')),
    CONSTRAINT title_generator_feedback_primary_len
        CHECK (char_length(primary_title) BETWEEN 1 AND 140),
    CONSTRAINT title_generator_feedback_selected_len
        CHECK (selected_title IS NULL OR char_length(selected_title) BETWEEN 1 AND 140),
    CONSTRAINT title_generator_feedback_edited_len
        CHECK (edited_title IS NULL OR char_length(edited_title) BETWEEN 1 AND 140)
);
CREATE INDEX IF NOT EXISTS idx_title_generator_feedback_streamer
    ON title_generator_feedback (twitch_user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_title_generator_feedback_learning
    ON title_generator_feedback (twitch_user_id, feedback, feedback_at DESC)
    WHERE feedback IS NOT NULL;
