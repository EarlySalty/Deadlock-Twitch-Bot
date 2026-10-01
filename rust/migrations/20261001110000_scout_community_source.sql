-- Community-Streamer-Brücke, Paket F (Twitch-Seite): Streamer-Vorschläge aus
-- der Discord-Community landen als Scout-Kandidaten. Rein additiv.
--
-- twitch_scout_candidates bekommt die Quelle (auto = Scout-Erkennung,
-- community = Vorschlag aus dem Discord), den ersten Vorschlagenden samt
-- Grund und Zeit, die Zahl verschiedener Vorschlagender und den Stand für den
-- Lese-Endpunkt der Partner-Ergebnisse (partner_active_since,
-- community_updated_at als streng monotoner Cursor).
-- Die bestehende Admin-Freigabe bleibt der einzige Weg in die Outreach-Kette.
ALTER TABLE twitch_scout_candidates
    ADD COLUMN IF NOT EXISTS source TEXT NOT NULL DEFAULT 'auto',
    ADD COLUMN IF NOT EXISTS suggested_by_discord_id TEXT,
    ADD COLUMN IF NOT EXISTS suggestion_reason TEXT,
    ADD COLUMN IF NOT EXISTS suggested_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS suggestion_count INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS partner_active_since TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS community_updated_at TIMESTAMPTZ;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'twitch_scout_candidates_source_chk'
    ) THEN
        ALTER TABLE twitch_scout_candidates
            ADD CONSTRAINT twitch_scout_candidates_source_chk
            CHECK (source IN ('auto', 'community'));
    END IF;
END
$$;

CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_scout_candidates_community_cursor
    ON twitch_scout_candidates (community_updated_at, streamer_login)
    WHERE source = 'community';

-- Jeder eingegangene Vorschlag (auch abgewiesene) mit Idempotenz-Schlüssel.
-- result_status ist die Antwort an Deadlock-Bots und wird bei einer
-- Wiederholung desselben Schlüssels unverändert zurückgegeben.
CREATE TABLE IF NOT EXISTS twitch_scout_community_suggestions (
    id BIGSERIAL PRIMARY KEY,
    idempotency_key TEXT NOT NULL UNIQUE,
    twitch_user_id TEXT NOT NULL,
    twitch_login TEXT NOT NULL,
    suggested_by_discord_id TEXT NOT NULL,
    reason TEXT,
    result_status TEXT NOT NULL CHECK (
        result_status IN ('created', 'already_known', 'already_partner', 'blocked')
    ),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);

CREATE INDEX IF NOT EXISTS idx_twitch_scout_community_suggestions_channel
    ON twitch_scout_community_suggestions (twitch_user_id, created_at);

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT, INSERT ON twitch_scout_community_suggestions TO twitchbot;
        GRANT USAGE, SELECT ON SEQUENCE twitch_scout_community_suggestions_id_seq TO twitchbot;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT ON twitch_scout_community_suggestions TO twitchdash;
    END IF;
END
$$;
