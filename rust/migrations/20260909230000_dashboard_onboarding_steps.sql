-- Bestehenden Fortschritt behalten; alte Tourstationen bestätigen keine neuen Einstellungen.
-- Ein erneuter manueller Lauf darf neue Fortschritte nicht auf den Altstand setzen.
DO $onboarding$
BEGIN
IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_schema='public' AND table_name='streamer_onboarding' AND column_name='active_step') THEN
ALTER TABLE public.streamer_onboarding
    ADD COLUMN active_step TEXT NOT NULL DEFAULT 'bookmark',
    ADD COLUMN completed_step_ids TEXT[] NOT NULL DEFAULT '{}',
    ADD COLUMN paused BOOLEAN NOT NULL DEFAULT TRUE;

UPDATE public.streamer_onboarding
SET active_step = CASE current_step
    WHEN 1 THEN 'discord'
    WHEN 2 THEN 'steam'
    WHEN 3 THEN 'chat'
    WHEN 4 THEN 'bot'
    ELSE 'bookmark'
END;

ALTER TABLE public.streamer_onboarding
    ADD CONSTRAINT streamer_onboarding_active_step_valid CHECK
        (active_step IN ('bookmark', 'discord', 'steam', 'chat', 'bot', 'overlay', 'advertising', 'feedback')),
    ADD CONSTRAINT streamer_onboarding_completed_steps_valid CHECK
        (completed_step_ids <@ ARRAY['bookmark', 'chat', 'bot', 'overlay', 'advertising', 'feedback']::TEXT[]);
END IF;
END
$onboarding$;
