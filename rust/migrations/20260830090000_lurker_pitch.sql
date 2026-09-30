-- Lurker-Discord-Pitch: einmalige direkte Ansprache haeufiger stiller Lurker
-- im eigenen Kanal (Contract .tasks/2026-08-30-lurker-pitch, INV-2: default aus,
-- ohne Betreiber-Flag geht keine neue Zeile live).
--
-- lurker_pitch_enabled folgt dem Muster von streamer_plans.lurker_tax_enabled
-- (integer, Default 0, Betreiber-Schalter, Dashboard-Endpoint lurker-tax-settings).
ALTER TABLE streamer_plans
    ADD COLUMN IF NOT EXISTS lurker_pitch_enabled integer DEFAULT 0 NOT NULL;

-- Nie-wieder-Log: je (Kanal, Chatter-Identitaet) genau ein Pitch fuer immer.
-- chatter_identity_key bildet die Kandidaten-Query ab: 'id:<chatter_id>',
-- sonst 'login:<lower(chatter_login)>' (promos.rs get_lurker_tax_candidates).
CREATE TABLE IF NOT EXISTS twitch_lurker_pitch_log (
    twitch_user_id TEXT NOT NULL,
    chatter_identity_key TEXT NOT NULL,
    chatter_login TEXT NOT NULL,
    mentioned_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (twitch_user_id, chatter_identity_key)
);
