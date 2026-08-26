-- Verbundene Chat-Plattformen je Streamer fuer den Uplink-Multi-Chat.
--
-- Der Streamer verbindet im Dashboard seinen Twitch-Chat (spaeter weitere
-- Plattformen). Die Tokens liegen hier verschluesselt (FieldCipher, AES-GCM,
-- AAD "platform_connections:<streamer_id>:<platform>"), das Relay (rs-relay)
-- holt sich ueber die interne Route nur den kurzlebigen Access-Token und sieht
-- den Refresh-Token nie. `twitch_raid_auth` bleibt davon unberuehrt: das ist
-- ein anderer Scope-Satz fuer einen anderen Zweck.
--
-- streamer_id ist die numerische Twitch-User-ID (dieselbe Kennung, mit der
-- das Relay seine Sessions fuehrt).
--
-- Bewusst ohne Schema-Praefix, damit derselbe Text in einem Testschema
-- (`search_path`) laufen kann.
CREATE TABLE IF NOT EXISTS platform_connections (
    streamer_id BIGINT NOT NULL,
    platform TEXT NOT NULL,
    platform_user_id TEXT NOT NULL,
    platform_login TEXT NOT NULL,
    access_token_enc BYTEA NOT NULL,
    refresh_token_enc BYTEA NOT NULL,
    enc_kid TEXT NOT NULL DEFAULT 'v1',
    scopes TEXT[] NOT NULL DEFAULT '{}',
    expires_at TIMESTAMPTZ NOT NULL,
    needs_reauth BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (streamer_id, platform)
);

-- Der Refresh-Job sucht nach bald ablaufenden Tokens.
CREATE INDEX IF NOT EXISTS platform_connections_expires_at_idx
    ON platform_connections (expires_at)
    WHERE needs_reauth = FALSE;

COMMENT ON TABLE platform_connections IS
    'Chat-Verbindungen je Streamer und Plattform fuer den Uplink-Multi-Chat. Tokens verschluesselt (FieldCipher), Leser der Access-Tokens ist rs-relay ueber /twitch/api/v2/internal/platform-token.';
COMMENT ON COLUMN platform_connections.needs_reauth IS
    'TRUE, sobald ein Refresh mit 400/401 scheitert. Der Streamer muss die Plattform im Dashboard neu verbinden.';
