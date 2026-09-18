-- Eigene Verbotsliste je Streamer fuer den Titelgenerator ("Das will ich nie im
-- Titel"). Newline-getrennte Eintraege, hoechstens 40 Zeilen zu je 60 Zeichen;
-- die Laengengrenze deckelt nur den groben Missbrauch, die genaue Zeilenpruefung
-- macht der Settings-Handler. Wirkung: eigener Prompt-Block plus harter
-- Nachfilter, zusaetzlich zur eingebauten Slop-Liste.
ALTER TABLE title_generator_preferences
    ADD COLUMN IF NOT EXISTS never_words TEXT NOT NULL DEFAULT '';

ALTER TABLE title_generator_preferences
    ADD CONSTRAINT title_generator_preferences_never_words_len
        CHECK (char_length(never_words) <= 2600);
