CREATE TABLE IF NOT EXISTS public.social_ai_browser_passkeys (
    name TEXT PRIMARY KEY CHECK (name ~ '^[a-z_]{1,64}$'),
    passkey_enc BYTEA NOT NULL CHECK (octet_length(passkey_enc) BETWEEN 1 AND 65536),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT, INSERT ON public.social_ai_browser_passkeys TO twitchbot;
    END IF;
END
$$;
