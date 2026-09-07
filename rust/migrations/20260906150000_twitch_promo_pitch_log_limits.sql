CREATE INDEX IF NOT EXISTS idx_twitch_promo_pitch_log_target_pfad_sent
    ON public.twitch_promo_pitch_log (target_user_id, pfad, sent_at)
    WHERE sent_at IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_twitch_promo_pitch_log_channel_pfad_sent
    ON public.twitch_promo_pitch_log (channel_login, pfad, sent_at)
    WHERE sent_at IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_twitch_promo_pitch_log_pfad_sent
    ON public.twitch_promo_pitch_log (pfad, sent_at)
    WHERE sent_at IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_twitch_promo_pitch_log_reject_dedupe
    ON public.twitch_promo_pitch_log (channel_login, target_user_id, reject_reason, created_at)
    WHERE sent_at IS NULL AND reject_reason IS NOT NULL;
