-- Go-Live-Tipp-System: pro-Streamer Opt-out + Cap-Timestamp, Tipp-Historie,
-- Feature-Nutzungs-Events. Keyed auf twitch_user_id (stabil).

CREATE TABLE IF NOT EXISTS public.twitch_tip_settings (
    twitch_user_id     TEXT PRIMARY KEY,
    opt_out            BOOLEAN     NOT NULL DEFAULT FALSE,
    last_tip_sent_at   TIMESTAMPTZ,
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS public.twitch_tip_history (
    id              BIGSERIAL PRIMARY KEY,
    twitch_user_id  TEXT        NOT NULL,
    tip_slug        TEXT        NOT NULL,
    shown_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_tip_history_user ON public.twitch_tip_history (twitch_user_id, shown_at DESC);

CREATE TABLE IF NOT EXISTS public.twitch_feature_usage (
    twitch_user_id  TEXT        NOT NULL,
    feature         TEXT        NOT NULL,
    last_used_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    use_count       INTEGER     NOT NULL DEFAULT 1,
    PRIMARY KEY (twitch_user_id, feature)
);
