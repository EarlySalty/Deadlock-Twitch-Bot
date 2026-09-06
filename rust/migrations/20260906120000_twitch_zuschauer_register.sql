CREATE TABLE IF NOT EXISTS public.twitch_zuschauer_register (
    twitch_user_id         TEXT PRIMARY KEY,
    twitch_login           TEXT,
    discord_user_id        TEXT,
    community_probability   DOUBLE PRECISION NOT NULL,
    signals                JSONB NOT NULL DEFAULT '{}'::jsonb,
    first_partner_channel   TEXT,
    first_seen_at          TIMESTAMPTZ,
    computed_at            TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_twitch_zuschauer_register_discord
    ON public.twitch_zuschauer_register (discord_user_id);

CREATE INDEX IF NOT EXISTS idx_twitch_zuschauer_register_computed
    ON public.twitch_zuschauer_register (computed_at);

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.twitch_zuschauer_register TO twitchbot;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.twitch_zuschauer_register TO twitchdash;
    END IF;
END $$;
