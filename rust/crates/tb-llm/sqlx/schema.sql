-- Referenz-DDL des LLM-Usage-Ledgers in der ZENTRALEN Postgres.
-- Autoritative Quelle sind die Migrationen unter `rust/migrations/`
-- (zuletzt `20260905130000_llm_usage_rename.sql`); diese Datei
-- spiegelt sie nur zur Dokumentation. `ts` bleibt bewusst TEXT (ISO-8601 UTC,
-- Sekunden, +00:00) für Byte-Parität mit dem Python-Helfer; die rollierende
-- Fensterabfrage vergleicht `ts` deshalb lexikografisch.
CREATE TABLE IF NOT EXISTS llm_usage (
    id         BIGSERIAL PRIMARY KEY,
    ts         TEXT      NOT NULL,
    source     TEXT      NOT NULL,
    purpose    TEXT,
    model      TEXT,
    tokens_in  BIGINT    DEFAULT 0,
    tokens_out BIGINT    DEFAULT 0,
    total      BIGINT    DEFAULT 0,
    success    BIGINT    DEFAULT 1,
    meta       TEXT
);
CREATE INDEX IF NOT EXISTS idx_llm_usage_ts     ON llm_usage(ts);
CREATE INDEX IF NOT EXISTS idx_llm_usage_source ON llm_usage(source);

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

-- Paket K (`20261012140100_llm_usage_streamer.sql`): Streamerbezug je Versuch.
ALTER TABLE public.llm_usage ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
CREATE INDEX IF NOT EXISTS idx_llm_usage_streamer
    ON public.llm_usage (twitch_user_id, ts)
    WHERE twitch_user_id IS NOT NULL;
