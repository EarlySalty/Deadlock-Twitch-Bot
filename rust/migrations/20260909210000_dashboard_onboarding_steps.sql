-- Bestehenden Fortschritt behalten; alte Tourstationen bestätigen keine neuen Einstellungen.
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
