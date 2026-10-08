ALTER TABLE twitch_watchdog_incidents ADD COLUMN IF NOT EXISTS superseded_at timestamptz;
ALTER TABLE twitch_watchdog_incidents DROP CONSTRAINT twitch_watchdog_incidents_service_check;
ALTER TABLE twitch_watchdog_incidents ADD CONSTRAINT twitch_watchdog_incidents_service_check
    CHECK (service IN ('deadlock-twitch-bot-rust.service', 'tb-category-collector.service', 'category-collector'));

CREATE TABLE category_watchdog_suspensions (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    reason text NOT NULL CHECK (reason IN ('disabled', 'disk', 'unobserved')),
    started_at timestamptz NOT NULL,
    last_seen_at timestamptz NOT NULL,
    ended_at timestamptz,
    CHECK (last_seen_at >= started_at),
    CHECK (ended_at IS NULL OR ended_at >= started_at)
);
INSERT INTO category_watchdog_suspensions(reason,started_at,last_seen_at,ended_at)
    VALUES('unobserved',now()-interval '2 days',now(),now());
UPDATE twitch_watchdog_incidents SET superseded_at=now()
    WHERE service='tb-category-collector.service' AND superseded_at IS NULL;

CREATE UNIQUE INDEX category_watchdog_one_suspension
    ON category_watchdog_suspensions(reason) WHERE ended_at IS NULL;

CREATE TABLE category_watchdog_storage_incidents (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    started_at timestamptz NOT NULL,
    last_paused_at timestamptz NOT NULL,
    clear_since timestamptz,
    recovered_at timestamptz,
    reason text NOT NULL CHECK (reason IN ('disk', 'budget')),
    repetitions integer NOT NULL DEFAULT 0 CHECK (repetitions >= 0),
    CHECK (last_paused_at >= started_at),
    CHECK (recovered_at IS NULL OR recovered_at >= started_at)
);
CREATE UNIQUE INDEX category_watchdog_one_storage_incident
    ON category_watchdog_storage_incidents((true)) WHERE recovered_at IS NULL;

CREATE TABLE category_watchdog_storage_notifications (
    notification_day date PRIMARY KEY,
    incident_id bigint NOT NULL UNIQUE REFERENCES category_watchdog_storage_incidents(id),
    content text NOT NULL,
    last_attempt_at timestamptz,
    notified_at timestamptz,
    delivery_error_at timestamptz
);

DO $$
DECLARE relation_name text; role_name text;
BEGIN
    FOREACH relation_name IN ARRAY ARRAY['twitch_watchdog_incidents','category_watchdog_suspensions',
        'category_watchdog_storage_incidents','category_watchdog_storage_notifications'] LOOP
        EXECUTE format('REVOKE ALL ON TABLE %I FROM PUBLIC',relation_name);
        FOREACH role_name IN ARRAY ARRAY['twitchbot','twitchdash','twitchlegacy','twitchcollector'] LOOP
            IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname=role_name) THEN
                EXECUTE format('REVOKE ALL ON TABLE %I FROM %I',relation_name,role_name);
            END IF;
        END LOOP;
        IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname='twitchbot') THEN
            EXECUTE format('GRANT SELECT,INSERT,UPDATE ON TABLE %I TO twitchbot',relation_name);
        END IF;
    END LOOP;
    FOREACH relation_name IN ARRAY ARRAY['twitch_watchdog_incidents_id_seq','category_watchdog_suspensions_id_seq',
        'category_watchdog_storage_incidents_id_seq'] LOOP
        EXECUTE format('REVOKE ALL ON SEQUENCE %I FROM PUBLIC',relation_name);
        FOREACH role_name IN ARRAY ARRAY['twitchbot','twitchdash','twitchlegacy','twitchcollector'] LOOP
            IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname=role_name) THEN
                EXECUTE format('REVOKE ALL ON SEQUENCE %I FROM %I',relation_name,role_name);
            END IF;
        END LOOP;
        IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname='twitchbot') THEN
            EXECUTE format('GRANT USAGE,SELECT ON SEQUENCE %I TO twitchbot',relation_name);
        END IF;
    END LOOP;
END $$;
