CREATE TABLE partner_effort_weekly_quest_evaluations (
    partner_twitch_user_id TEXT NOT NULL CHECK (partner_twitch_user_id ~ '^[0-9]+$'),
    week_start DATE NOT NULL CHECK (extract(isodow FROM week_start)=1),
    evaluated_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (partner_twitch_user_id,week_start)
);

CREATE TRIGGER partner_effort_quest_evaluations_immutable
    BEFORE UPDATE OR DELETE OR TRUNCATE ON partner_effort_weekly_quest_evaluations
    FOR EACH STATEMENT EXECUTE FUNCTION partner_effort_reject_mutation();

DO $$
DECLARE
    role_name TEXT;
BEGIN
    FOREACH role_name IN ARRAY ARRAY['twitchbot','twitchdash','twitchlegacy','twitchcontest'] LOOP
        IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname=role_name) THEN
            EXECUTE format('REVOKE ALL ON TABLE partner_effort_weekly_quest_evaluations FROM %I',role_name);
        END IF;
    END LOOP;

    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname='twitchbot') THEN
        GRANT SELECT, INSERT ON partner_effort_weekly_quest_evaluations TO twitchbot;
    END IF;
    FOREACH role_name IN ARRAY ARRAY['twitchdash','twitchlegacy'] LOOP
        IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname=role_name) THEN
            EXECUTE format('GRANT SELECT ON partner_effort_weekly_quest_evaluations TO %I',role_name);
        END IF;
    END LOOP;
END
$$;
