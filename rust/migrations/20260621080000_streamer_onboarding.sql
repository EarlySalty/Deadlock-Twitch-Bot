-- Onboarding-Fortschritt pro Streamer (resumierbar).
CREATE TABLE IF NOT EXISTS public.streamer_onboarding (
    twitch_user_id   TEXT PRIMARY KEY,
    twitch_login     TEXT NOT NULL,
    current_step     INTEGER NOT NULL DEFAULT 0,
    completed        BOOLEAN NOT NULL DEFAULT FALSE,
    completed_at     TIMESTAMPTZ,
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
