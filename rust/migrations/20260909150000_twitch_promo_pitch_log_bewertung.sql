ALTER TABLE public.twitch_promo_pitch_log
    ADD COLUMN IF NOT EXISTS review_message_id BIGINT,
    ADD COLUMN IF NOT EXISTS bewertung TEXT,
    ADD COLUMN IF NOT EXISTS bewertet_at TIMESTAMPTZ;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conname = 'twitch_promo_pitch_log_bewertung_chk'
    ) THEN
        ALTER TABLE public.twitch_promo_pitch_log
            ADD CONSTRAINT twitch_promo_pitch_log_bewertung_chk
            CHECK (bewertung IS NULL OR bewertung IN ('gut', 'schlecht'));
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS idx_twitch_promo_pitch_log_offene_bewertung
    ON public.twitch_promo_pitch_log (sent_at)
    WHERE review_message_id IS NOT NULL AND bewertung IS NULL;
