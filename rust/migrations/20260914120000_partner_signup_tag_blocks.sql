CREATE TABLE IF NOT EXISTS public.twitch_partner_signup_tag_blocks (
    tag            text PRIMARY KEY,
    display_tag    text NOT NULL,
    reason         text NOT NULL,
    public_message text,
    added_by       text NOT NULL,
    added_at       timestamptz NOT NULL DEFAULT now()
);

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.twitch_partner_signup_tag_blocks TO twitchbot;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.twitch_partner_signup_tag_blocks TO twitchdash;
    END IF;
END $$;
