ALTER TABLE title_generator_preferences
    ADD COLUMN IF NOT EXISTS never_words TEXT NOT NULL DEFAULT '';

ALTER TABLE title_generator_preferences
    ADD CONSTRAINT title_generator_preferences_never_words_len
        CHECK (char_length(never_words) <= 2600);
