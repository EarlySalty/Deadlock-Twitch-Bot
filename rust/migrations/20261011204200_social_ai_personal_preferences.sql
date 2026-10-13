CREATE TABLE public.social_ai_personal_preferences (
    twitch_user_id BIGINT PRIMARY KEY,
    profile JSONB NOT NULL CHECK (jsonb_typeof(profile) = 'object' AND octet_length(profile::text) <= 65536),
    last_event_id BIGINT NOT NULL CHECK (last_event_id >= 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

REVOKE ALL ON TABLE public.social_ai_personal_preferences FROM PUBLIC;

DO $$
DECLARE role_name TEXT;
BEGIN
    FOREACH role_name IN ARRAY ARRAY['twitchbot','twitchdash'] LOOP
        IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = role_name) THEN
            EXECUTE format('GRANT SELECT, INSERT, UPDATE ON TABLE public.social_ai_personal_preferences TO %I', role_name);
        END IF;
    END LOOP;
END
$$;
