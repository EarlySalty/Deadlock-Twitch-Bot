CREATE TABLE IF NOT EXISTS public.twitch_bot_capabilities (
    id                 SMALLINT PRIMARY KEY DEFAULT 1,
    has_chatters_scope BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO public.twitch_bot_capabilities (id, has_chatters_scope)
VALUES (1, TRUE)
ON CONFLICT (id) DO NOTHING;

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.twitch_bot_capabilities TO twitchbot;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT ON public.twitch_bot_capabilities TO twitchdash;
    END IF;
END $$;
