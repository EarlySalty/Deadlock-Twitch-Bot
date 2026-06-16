-- Cutover-Nachzug: zwei Schemata, die Python via lazy ensure_schema zur Laufzeit
-- anlegte und die beim migrations-basierten Rust-Cutover verloren gingen. Beide
-- werden von Live-Pfaden vorausgesetzt; auf einer frischen DB schlagen sie sonst
-- fehl:
--   * twitch_self_explainer_log — tb-dashboard-api self_explainer-Handler schreibt
--     hier sein Audit-Log (insert ist best-effort `let _ =`, scheiterte daher bisher
--     still: kein Crash, aber kein Log). Port aus bot/dashboard/routes_self_explainer.py:98.
--   * twitch_engagement_settings.irc_read — tb-engagement irc_reader wählt darüber
--     seine Kanäle (`WHERE enabled = TRUE AND irc_read = TRUE`); ohne Spalte bricht
--     die Query hart und der IRC-Reader bleibt komplett tot. Port aus
--     bot/engagement/irc_reader.py:47 (dort lazy `ADD COLUMN IF NOT EXISTS`).

CREATE TABLE IF NOT EXISTS public.twitch_self_explainer_log (
    id BIGSERIAL PRIMARY KEY,
    question TEXT NOT NULL,
    answer TEXT NOT NULL,
    grounded BOOLEAN NOT NULL DEFAULT FALSE,
    flagged_injection BOOLEAN NOT NULL DEFAULT FALSE,
    peer TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ALTER TABLE public.twitch_engagement_settings
    ADD COLUMN IF NOT EXISTS irc_read BOOLEAN NOT NULL DEFAULT FALSE;
