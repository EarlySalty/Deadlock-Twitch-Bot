-- Eigener Cursor für jede individuelle Community-Vorschlagszuordnung.
-- Der Kandidat bleibt nach Erstautor-Erasure ohne Ersatzautor erhalten.
ALTER TABLE public.twitch_scout_community_suggestions
    ADD COLUMN outcome_updated_at TIMESTAMPTZ;
CREATE UNIQUE INDEX twitch_scout_suggestion_outcome_cursor
    ON public.twitch_scout_community_suggestions(outcome_updated_at)
    WHERE outcome_updated_at IS NOT NULL;
