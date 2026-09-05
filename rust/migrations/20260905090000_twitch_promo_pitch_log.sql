CREATE TABLE IF NOT EXISTS public.twitch_promo_pitch_log (
    id             BIGSERIAL PRIMARY KEY,
    channel_login  TEXT NOT NULL,
    target_user_id TEXT,
    pfad           TEXT NOT NULL,
    occasion       TEXT,
    trigger_text   TEXT,
    generated_text TEXT,
    reject_reason  TEXT,
    sent_at        TIMESTAMPTZ,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_twitch_promo_pitch_log_target
    ON public.twitch_promo_pitch_log (target_user_id, sent_at);

CREATE INDEX IF NOT EXISTS idx_twitch_promo_pitch_log_channel
    ON public.twitch_promo_pitch_log (channel_login, sent_at);
