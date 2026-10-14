CREATE TABLE twitch_bot_token_health (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    monitoring_enabled boolean NOT NULL DEFAULT true,
    failure_kind text CHECK (length(failure_kind) <= 32)
);
CREATE TABLE twitch_bot_token_outages (
    id bigserial PRIMARY KEY,
    started_at timestamptz NOT NULL,
    recovered_at timestamptz,
    failure_kind text CHECK (length(failure_kind) <= 32),
    suppressed_at timestamptz,
    CHECK (recovered_at IS NULL OR recovered_at >= started_at)
);
CREATE UNIQUE INDEX twitch_bot_token_one_open_outage
    ON twitch_bot_token_outages ((true)) WHERE recovered_at IS NULL;
ALTER TABLE twitch_watchdog_incidents
    ADD COLUMN token_outage_id bigint UNIQUE REFERENCES twitch_bot_token_outages(id);
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
        GRANT SELECT, INSERT, UPDATE ON twitch_bot_token_health, twitch_bot_token_outages TO twitchbot;
        GRANT USAGE ON SEQUENCE twitch_bot_token_outages_id_seq TO twitchbot;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchcollector') THEN
        GRANT SELECT ON twitch_bot_token_health, twitch_bot_token_outages TO twitchcollector;
    END IF;
END $$;
