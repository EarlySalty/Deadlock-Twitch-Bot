CREATE TABLE twitch_bot_token_health (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    monitoring_enabled boolean NOT NULL DEFAULT true,
    last_ok_at timestamptz,
    last_failure_at timestamptz,
    failure_kind text CHECK (length(failure_kind) <= 32)
);
ALTER TABLE twitch_watchdog_incidents DROP CONSTRAINT twitch_watchdog_incidents_service_check;
ALTER TABLE twitch_watchdog_incidents ADD CONSTRAINT twitch_watchdog_incidents_service_check
    CHECK (service IN ('deadlock-twitch-bot-rust.service', 'tb-category-collector.service', 'category-collector', 'bot-token'));
ALTER TABLE twitch_watchdog_incidents
    ADD COLUMN recovery_notified_at timestamptz,
    ADD COLUMN last_recovery_attempt_at timestamptz,
    ADD COLUMN recovery_delivery_error_at timestamptz;
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT, INSERT, UPDATE ON twitch_bot_token_health TO twitchbot;
    END IF;
END $$;
