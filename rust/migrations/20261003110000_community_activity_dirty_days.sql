-- Rohänderungen bleiben für den Consent-Sync sichtbar, auch bei Punktedeckeln.
CREATE TABLE twitch_community_points_dirty_days (
    day DATE PRIMARY KEY
);

CREATE FUNCTION twitch_community_mark_dirty_day() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public AS $$
DECLARE
    source_table text := TG_ARGV[0];
    stamp timestamptz;
    coverage interval;
    affected_days date[] := ARRAY[]::date[];
BEGIN
    -- Auch spätere Korrekturen der Sessionzuordnung und ihrer Grenzen ändern
    -- historische Rohaktivität. Alten und neuen Berliner Zeitraum markieren.
    -- Der Quellname bleibt auch auf Timescale-Chunks erhalten.
    IF source_table = 'twitch_stream_sessions' THEN
        IF TG_OP <> 'INSERT' THEN
            affected_days := affected_days || ARRAY(SELECT d::date FROM generate_series(
                (OLD.started_at AT TIME ZONE 'Europe/Berlin')::date::timestamp,
                ((COALESCE(OLD.ended_at, clock_timestamp()) + INTERVAL '2 hours')
                    AT TIME ZONE 'Europe/Berlin')::date::timestamp,
                INTERVAL '1 day') d);
        END IF;
        IF TG_OP <> 'DELETE' THEN
            affected_days := affected_days || ARRAY(SELECT d::date FROM generate_series(
                (NEW.started_at AT TIME ZONE 'Europe/Berlin')::date::timestamp,
                ((COALESCE(NEW.ended_at, clock_timestamp()) + INTERVAL '2 hours')
                    AT TIME ZONE 'Europe/Berlin')::date::timestamp,
                INTERVAL '1 day') d);
        END IF;
    ELSE
        -- Chatvorgeschichte und Präsenzintervalle können den Folgetag ändern.
        coverage := CASE source_table
            WHEN 'twitch_chat_messages' THEN INTERVAL '2 hours'
            WHEN 'twitch_viewer_presence_ticks' THEN INTERVAL '60 seconds'
            ELSE INTERVAL '0 seconds'
        END;
        IF TG_OP <> 'INSERT' THEN
            IF source_table = 'twitch_chat_messages' THEN stamp := OLD.message_ts;
            ELSIF source_table = 'twitch_raid_history' THEN stamp := OLD.executed_at;
            ELSE stamp := OLD.tick_at; END IF;
            affected_days := affected_days || ARRAY(SELECT d::date FROM generate_series(
                (stamp AT TIME ZONE 'Europe/Berlin')::date::timestamp,
                ((stamp + coverage) AT TIME ZONE 'Europe/Berlin')::date::timestamp,
                INTERVAL '1 day') d);
        END IF;
        IF TG_OP <> 'DELETE' THEN
            IF source_table = 'twitch_chat_messages' THEN stamp := NEW.message_ts;
            ELSIF source_table = 'twitch_raid_history' THEN stamp := NEW.executed_at;
            ELSE stamp := NEW.tick_at; END IF;
            affected_days := affected_days || ARRAY(SELECT d::date FROM generate_series(
                (stamp AT TIME ZONE 'Europe/Berlin')::date::timestamp,
                ((stamp + coverage) AT TIME ZONE 'Europe/Berlin')::date::timestamp,
                INTERVAL '1 day') d);
        END IF;
    END IF;
    -- Das Update hält die Zeilensperre bis zum Rohdatencommit. Eine parallele
    -- Aggregation liest danach die Änderung oder lässt eine neue Markierung stehen.
    -- Alte und neue Tage gemeinsam sortieren, damit Datumswechsel gleich sperren.
    INSERT INTO public.twitch_community_points_dirty_days(day)
    SELECT DISTINCT d FROM unnest(affected_days) AS days(d) ORDER BY d
    ON CONFLICT (day) DO UPDATE SET day = EXCLUDED.day;
    RETURN NULL;
END $$;
REVOKE ALL ON FUNCTION twitch_community_mark_dirty_day() FROM PUBLIC;

CREATE TRIGGER community_chat_dirty_day AFTER INSERT OR UPDATE OR DELETE ON twitch_chat_messages
FOR EACH ROW EXECUTE FUNCTION twitch_community_mark_dirty_day('twitch_chat_messages');
CREATE TRIGGER community_presence_dirty_day AFTER INSERT OR UPDATE OR DELETE ON twitch_viewer_presence_ticks
FOR EACH ROW EXECUTE FUNCTION twitch_community_mark_dirty_day('twitch_viewer_presence_ticks');
CREATE TRIGGER community_session_dirty_days
AFTER INSERT OR DELETE OR UPDATE OF started_at, ended_at, twitch_user_id ON twitch_stream_sessions
FOR EACH ROW EXECUTE FUNCTION twitch_community_mark_dirty_day('twitch_stream_sessions');
CREATE TRIGGER community_raid_dirty_day
AFTER INSERT OR DELETE OR UPDATE OF executed_at, success, from_broadcaster_id, to_broadcaster_id ON twitch_raid_history
FOR EACH ROW EXECUTE FUNCTION twitch_community_mark_dirty_day('twitch_raid_history');

DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname='twitchbot') THEN
        GRANT SELECT, DELETE ON twitch_community_points_dirty_days TO twitchbot;
    END IF;
END $$;
