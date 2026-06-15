-- Block-19-Grillme: Output-Modus der Engagement-KI als drei klare Zustände
-- `off` | `shadow` | `live`. Additiv zur bestehenden `enabled`-Logik.
--
--   off    = no-op: kein KI-Output (Default, damit die KI ohne expliziten
--            Dashboard-Toggle stumm bleibt)
--   shadow = Antwort wird erzeugt und gestaged (Decision-Log), aber NICHT in
--            den Twitch-Chat gesendet (für späteres Discord-Review)
--   live   = Antwort wird normal in den Chat gesendet
--
-- Default-AUS-Garantie: NOT NULL DEFAULT 'off' — bestehende Zeilen und neue
-- Channels starten ohne Output. Dashboard-Toggle und Shadow→Discord-Out kommen
-- in separaten Tickets.
ALTER TABLE public.twitch_engagement_settings
    ADD COLUMN IF NOT EXISTS output_mode text NOT NULL DEFAULT 'off';

-- Nur die drei gültigen Zustände erlauben (fail-fast bei falscher Eingabe).
DO $do$ BEGIN
    ALTER TABLE public.twitch_engagement_settings
        ADD CONSTRAINT twitch_engagement_settings_output_mode_chk
        CHECK (output_mode IN ('off', 'shadow', 'live'));
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;
