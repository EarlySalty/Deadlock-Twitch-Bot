CREATE TABLE IF NOT EXISTS twitch_promo_timer_settings (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    settings jsonb NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now()
);
INSERT INTO twitch_promo_timer_settings(singleton, settings) VALUES (true,
'{"defaults":{"overallCooldownMinutes":90,"activityCooldownMinMinutes":45,"activityCooldownMaxMinutes":180,"minMessages":16,"newChatters":2,"attemptCooldownMinutes":10,"viewerSpikeCooldownMinutes":60,"pitchCooldownMinutes":10,"pitchMaxPerStream":3},"community":{"broadcasterId":"1367527782","overallCooldownMinutes":20,"activityCooldownMinMinutes":20,"activityCooldownMaxMinutes":30,"minMessages":8,"newChatters":0,"attemptCooldownMinutes":5,"viewerSpikeCooldownMinutes":20,"pitchCooldownMinutes":5,"pitchMaxPerStream":6}}'::jsonb)
ON CONFLICT (singleton) DO NOTHING;

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT ON twitch_promo_timer_settings TO twitchbot;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT, INSERT, UPDATE ON twitch_promo_timer_settings TO twitchdash;
    END IF;
END $$;
