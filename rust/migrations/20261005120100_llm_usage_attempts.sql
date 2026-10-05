-- Jeder bezahlte Versuch erhält vor dem Senden eine Zeile.
ALTER TABLE public.llm_usage
    ADD COLUMN project TEXT,
    ADD COLUMN service TEXT,
    ADD COLUMN provider TEXT,
    ADD COLUMN attempt_state TEXT NOT NULL DEFAULT 'legacy',
    ADD COLUMN request_id TEXT,
    ADD COLUMN finished_at TIMESTAMPTZ,
    ADD COLUMN error_code TEXT,
    ADD COLUMN http_status INTEGER,
    ADD COLUMN latency_ms BIGINT;
ALTER TABLE public.llm_usage ALTER COLUMN tokens_in DROP NOT NULL;
ALTER TABLE public.llm_usage ALTER COLUMN tokens_out DROP NOT NULL;
ALTER TABLE public.llm_usage ALTER COLUMN total DROP NOT NULL;
ALTER TABLE public.llm_usage ADD CONSTRAINT llm_usage_attempt_state_check
    CHECK (attempt_state IN ('legacy','started','succeeded','failed'));
CREATE INDEX idx_llm_usage_open_attempts ON public.llm_usage (ts) WHERE attempt_state='started';
CREATE INDEX idx_llm_usage_origin ON public.llm_usage (project,service,purpose,ts);
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname='twitchanalysis') THEN
        GRANT SELECT, INSERT, UPDATE ON public.llm_usage TO twitchanalysis;
        GRANT USAGE, SELECT ON SEQUENCE public.llm_usage_id_seq TO twitchanalysis;
    END IF;
END $$;
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='twitchaudit') THEN
        CREATE ROLE twitchaudit LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS;
    END IF;
END $$;
GRANT CONNECT ON DATABASE twitch_analytics TO twitchaudit;
GRANT USAGE ON SCHEMA public TO twitchaudit;
GRANT SELECT, INSERT, UPDATE ON public.llm_usage TO twitchaudit;
GRANT USAGE, SELECT ON SEQUENCE public.llm_usage_id_seq TO twitchaudit;
