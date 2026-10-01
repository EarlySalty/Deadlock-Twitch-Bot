-- Multiple verified Steam accounts per Twitch viewer.
-- twitch_player_steam_links.steam_id64 remains the selected primary account for
-- backwards-compatible reads by the chat runtime.
CREATE TABLE IF NOT EXISTS twitch_player_steam_accounts (
    twitch_user_id TEXT NOT NULL
        REFERENCES twitch_player_steam_links(twitch_user_id) ON DELETE CASCADE,
    steam_id64 BIGINT NOT NULL
        CHECK (steam_id64 > 76561197960265728 AND steam_id64 <= 76561202255233023),
    is_primary BOOLEAN NOT NULL DEFAULT FALSE,
    linked_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (twitch_user_id, steam_id64)
);

CREATE UNIQUE INDEX IF NOT EXISTS twitch_player_steam_accounts_one_primary
    ON twitch_player_steam_accounts (twitch_user_id)
    WHERE is_primary;

INSERT INTO twitch_player_steam_accounts (twitch_user_id, steam_id64, is_primary)
SELECT twitch_user_id, steam_id64, TRUE
FROM twitch_player_steam_links
WHERE steam_id64 IS NOT NULL
ON CONFLICT (twitch_user_id, steam_id64) DO UPDATE
SET is_primary = TRUE, updated_at = NOW();
