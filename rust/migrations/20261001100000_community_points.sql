-- Community-Streamer-Brücke, Paket B: Community-Punkte je Tag (Europe/Berlin).
-- Rein additiv. Der tb-bot rechnet die Tageswerte alle 5 Minuten aus den
-- vorhandenen Rohdaten neu (tb_analytics::community_points) und schreibt nur
-- geänderte Zeilen. updated_at ist je Tabelle streng monoton und eindeutig,
-- damit der Lese-Cursor der internen Schnittstelle stabil bleibt.

-- Zuschauer je Partnerkanal und Tag.
CREATE TABLE IF NOT EXISTS twitch_community_points_viewer_daily (
    twitch_user_id         TEXT        NOT NULL CHECK (twitch_user_id ~ '^[0-9]+$'),
    channel_twitch_user_id TEXT        NOT NULL CHECK (channel_twitch_user_id ~ '^[0-9]+$'),
    day                    DATE        NOT NULL,
    twitch_login           TEXT        NOT NULL DEFAULT '',
    watch_minutes          INTEGER     NOT NULL DEFAULT 0 CHECK (watch_minutes >= 0),
    chat_messages          INTEGER     NOT NULL DEFAULT 0 CHECK (chat_messages >= 0),
    points_watch           INTEGER     NOT NULL DEFAULT 0 CHECK (points_watch BETWEEN 0 AND 72),
    points_chat            INTEGER     NOT NULL DEFAULT 0 CHECK (points_chat BETWEEN 0 AND 30),
    points_discovery       INTEGER     NOT NULL DEFAULT 0 CHECK (points_discovery IN (0, 10)),
    updated_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (twitch_user_id, channel_twitch_user_id, day),
    CHECK (twitch_user_id <> channel_twitch_user_id)
);
CREATE UNIQUE INDEX IF NOT EXISTS twitch_community_points_viewer_daily_cursor_idx
    ON twitch_community_points_viewer_daily (updated_at, twitch_user_id, channel_twitch_user_id, day);
CREATE INDEX IF NOT EXISTS twitch_community_points_viewer_daily_day_idx
    ON twitch_community_points_viewer_daily (day);

-- Partner je Tag.
CREATE TABLE IF NOT EXISTS twitch_community_points_streamer_daily (
    streamer_twitch_user_id TEXT        NOT NULL CHECK (streamer_twitch_user_id ~ '^[0-9]+$'),
    day                     DATE        NOT NULL,
    streamer_login          TEXT        NOT NULL DEFAULT '',
    discord_user_id         TEXT,
    viewer_minutes          INTEGER     NOT NULL DEFAULT 0 CHECK (viewer_minutes >= 0),
    unique_viewers          INTEGER     NOT NULL DEFAULT 0 CHECK (unique_viewers >= 0),
    raids_to_partners       INTEGER     NOT NULL DEFAULT 0 CHECK (raids_to_partners >= 0),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (streamer_twitch_user_id, day)
);
CREATE UNIQUE INDEX IF NOT EXISTS twitch_community_points_streamer_daily_cursor_idx
    ON twitch_community_points_streamer_daily (updated_at, streamer_twitch_user_id, day);

-- Entdecker-Bonus: erstes Auftauchen je (Zuschauer, Partnerkanal), endgültig.
-- bonus_awarded = FALSE heißt: Paar bekannt, aber kein Bonus (Spuren vor dem
-- Tag oder Tagesgrenze von 3 Boni erreicht).
CREATE TABLE IF NOT EXISTS twitch_community_points_discoveries (
    twitch_user_id         TEXT        NOT NULL CHECK (twitch_user_id ~ '^[0-9]+$'),
    channel_twitch_user_id TEXT        NOT NULL CHECK (channel_twitch_user_id ~ '^[0-9]+$'),
    day                    DATE        NOT NULL,
    first_seen_at          TIMESTAMPTZ NOT NULL,
    bonus_awarded          BOOLEAN     NOT NULL,
    created_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (twitch_user_id, channel_twitch_user_id),
    CHECK (twitch_user_id <> channel_twitch_user_id)
);
CREATE INDEX IF NOT EXISTS twitch_community_points_discoveries_bonus_day_idx
    ON twitch_community_points_discoveries (twitch_user_id, day) WHERE bonus_awarded;

COMMENT ON TABLE twitch_community_points_viewer_daily IS
    'Community-Punkte je Zuschauer, Partnerkanal und Berliner Tag. Wird aus Rohdaten neu berechnet.';
COMMENT ON TABLE twitch_community_points_streamer_daily IS
    'Community-Tageswerte je Partner (Zuschauerminuten, Zuschauer, Raids an Partner).';
COMMENT ON TABLE twitch_community_points_discoveries IS
    'Erstes Auftauchen je Zuschauer und Partnerkanal; Grundlage des Entdecker-Bonus.';

DO $$
DECLARE
    role_name TEXT;
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT, INSERT, UPDATE ON twitch_community_points_viewer_daily TO twitchbot;
        GRANT SELECT, INSERT, UPDATE ON twitch_community_points_streamer_daily TO twitchbot;
        GRANT SELECT, INSERT ON twitch_community_points_discoveries TO twitchbot;
    END IF;
    FOREACH role_name IN ARRAY ARRAY['twitchdash'] LOOP
        IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = role_name) THEN
            EXECUTE format(
                'GRANT SELECT ON twitch_community_points_viewer_daily, '
                'twitch_community_points_streamer_daily, '
                'twitch_community_points_discoveries TO %I',
                role_name
            );
        END IF;
    END LOOP;
END
$$;
