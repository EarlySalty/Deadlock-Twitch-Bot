CREATE TABLE IF NOT EXISTS public.twitch_scam_guard_settings (
    channel_login    TEXT PRIMARY KEY,
    enabled          BOOLEAN NOT NULL DEFAULT TRUE,
    mode             TEXT NOT NULL DEFAULT 'auto_ban',
    threshold        REAL NOT NULL DEFAULT 0.90,
    suggestion_floor REAL NOT NULL DEFAULT 0.70
);

CREATE TABLE IF NOT EXISTS public.twitch_scam_guard_verdicts (
    id                  BIGSERIAL PRIMARY KEY,
    channel_login       TEXT NOT NULL,
    chatter_login       TEXT NOT NULL,
    chatter_id          TEXT,
    verdict             TEXT NOT NULL,
    confidence          REAL NOT NULL,
    category            TEXT NOT NULL,
    reasoning           TEXT NOT NULL,
    transcript_snapshot TEXT NOT NULL,
    action_taken        TEXT NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
