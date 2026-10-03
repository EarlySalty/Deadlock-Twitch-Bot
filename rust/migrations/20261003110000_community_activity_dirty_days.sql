-- Rohänderungen bleiben für den Consent-Sync sichtbar, auch bei Punktedeckeln.
CREATE TABLE twitch_community_points_dirty_days (
    day DATE PRIMARY KEY
);

CREATE FUNCTION twitch_community_mark_dirty_day() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public AS $$
DECLARE stamp timestamptz;
BEGIN
    IF TG_OP <> 'INSERT' THEN
        IF TG_TABLE_NAME = 'twitch_chat_messages' THEN stamp := OLD.message_ts;
        ELSE stamp := OLD.tick_at; END IF;
        INSERT INTO public.twitch_community_points_dirty_days(day)
        VALUES ((stamp AT TIME ZONE 'Europe/Berlin')::date) ON CONFLICT DO NOTHING;
    END IF;
    IF TG_OP <> 'DELETE' THEN
        IF TG_TABLE_NAME = 'twitch_chat_messages' THEN stamp := NEW.message_ts;
        ELSE stamp := NEW.tick_at; END IF;
        INSERT INTO public.twitch_community_points_dirty_days(day)
        VALUES ((stamp AT TIME ZONE 'Europe/Berlin')::date) ON CONFLICT DO NOTHING;
    END IF;
    RETURN NULL;
END $$;
REVOKE ALL ON FUNCTION twitch_community_mark_dirty_day() FROM PUBLIC;

CREATE TRIGGER community_chat_dirty_day AFTER INSERT OR UPDATE OR DELETE ON twitch_chat_messages
FOR EACH ROW EXECUTE FUNCTION twitch_community_mark_dirty_day();
CREATE TRIGGER community_presence_dirty_day AFTER INSERT OR UPDATE OR DELETE ON twitch_viewer_presence_ticks
FOR EACH ROW EXECUTE FUNCTION twitch_community_mark_dirty_day();

DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname='twitchbot') THEN
        GRANT SELECT, DELETE ON twitch_community_points_dirty_days TO twitchbot;
    END IF;
END $$;
