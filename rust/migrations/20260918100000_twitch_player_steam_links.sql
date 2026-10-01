-- Voluntary, directly authenticated Twitch -> Steam links; independent of Discord.
-- A disconnected row retains only the Twitch ID and opt-out, never the Steam ID.
CREATE TABLE IF NOT EXISTS twitch_player_steam_links (
    twitch_user_id TEXT PRIMARY KEY CHECK (twitch_user_id <> ''),
    steam_id64 BIGINT CHECK (steam_id64 > 76561197960265728 AND steam_id64 <= 76561202255233023),
    lookup_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    revision BIGINT NOT NULL DEFAULT 0 CHECK (revision >= 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (lookup_enabled OR steam_id64 IS NULL)
);

-- OpenID assertion nonces are global to this relying party, not just one browser.
CREATE TABLE IF NOT EXISTS twitch_steam_openid_nonces (
    nonce_hash TEXT PRIMARY KEY,
    expires_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS twitch_steam_openid_nonces_expiry
    ON twitch_steam_openid_nonces (expires_at);

-- Bot runtime can clear an association without permission to assign Steam IDs.
CREATE OR REPLACE FUNCTION tb_clear_disconnected_player_steam_id()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT NEW.lookup_enabled THEN
        NEW.steam_id64 := NULL;
    END IF;
    RETURN NEW;
END;
$$;
DROP TRIGGER IF EXISTS clear_disconnected_player_steam_id ON twitch_player_steam_links;
CREATE TRIGGER clear_disconnected_player_steam_id
    BEFORE INSERT OR UPDATE ON twitch_player_steam_links
    FOR EACH ROW EXECUTE FUNCTION tb_clear_disconnected_player_steam_id();
