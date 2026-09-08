-- Uplink-Nutzungsabsicht getrennt vom gemeinsamen Twitch-Grant.
-- Enthält keine Tokens. Andere ausdrücklich autorisierte Bot-/Raidnutzung
-- darf beim Trennen von Uplink ihren gemeinsamen Grant behalten.
CREATE TABLE IF NOT EXISTS twitch_uplink_auth_intent (
    twitch_user_id TEXT PRIMARY KEY CHECK (twitch_user_id ~ '^[1-9][0-9]*$'),
    enabled BOOLEAN NOT NULL,
    last_disconnected_at TIMESTAMPTZ,
    CHECK (enabled OR last_disconnected_at IS NOT NULL)
);
