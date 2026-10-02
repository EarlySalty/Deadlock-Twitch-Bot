CREATE TABLE twitch_watchdog_incidents (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    service text NOT NULL CHECK (service IN ('deadlock-twitch-bot-rust.service', 'tb-category-collector.service')),
    started_at timestamptz NOT NULL,
    recovered_at timestamptz,
    notified_at timestamptz,
    warning_at timestamptz,
    last_attempt_at timestamptz,
    delivery_error_at timestamptz,
    UNIQUE(service, started_at),
    CHECK (recovered_at IS NULL OR recovered_at >= started_at)
);
CREATE UNIQUE INDEX twitch_watchdog_one_open_incident
    ON twitch_watchdog_incidents(service) WHERE recovered_at IS NULL;
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchcollector') THEN
        GRANT SELECT, INSERT, UPDATE ON twitch_watchdog_incidents TO twitchcollector;
        GRANT USAGE, SELECT ON SEQUENCE twitch_watchdog_incidents_id_seq TO twitchcollector;
    END IF;
END $$;
