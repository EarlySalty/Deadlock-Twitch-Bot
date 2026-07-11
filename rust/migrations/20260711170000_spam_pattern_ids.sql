-- Korrektur-Buttons in Discord referenzieren gelernte Spam-Muster über eine
-- numerische ID (custom_id darf max. 100 Zeichen, das Muster selbst bis 200).
-- `pattern` bleibt Primary Key; `id` ist nur stabiler Kurz-Handle für die UI.
-- Die Safe-Tabelle bekommt bewusst keine ID: Safe-Lernen ist abgeschafft
-- (Safe-List-Poisoning, siehe CHANGELOG), die Tabelle bleibt nur als Archiv.

ALTER TABLE public.twitch_auto_learned_spam_patterns
    ADD COLUMN IF NOT EXISTS id BIGINT GENERATED ALWAYS AS IDENTITY;

CREATE UNIQUE INDEX IF NOT EXISTS idx_talsp_id
    ON public.twitch_auto_learned_spam_patterns (id);
