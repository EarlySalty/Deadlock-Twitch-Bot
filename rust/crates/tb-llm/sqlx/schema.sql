-- Referenz-DDL des MiniMax-Usage-Ledgers in der ZENTRALEN Postgres.
-- Autoritative Quelle ist die Migration
-- `rust/migrations/20260703000000_minimax_usage_ledger.sql`; diese Datei
-- spiegelt sie nur zur Dokumentation. `ts` bleibt bewusst TEXT (ISO-8601 UTC,
-- Sekunden, +00:00) für Byte-Parität mit dem Python-Helfer; die rollierende
-- Fensterabfrage vergleicht `ts` deshalb lexikografisch.
CREATE TABLE IF NOT EXISTS minimax_usage (
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
CREATE INDEX IF NOT EXISTS idx_mmu_ts     ON minimax_usage(ts);
CREATE INDEX IF NOT EXISTS idx_mmu_source ON minimax_usage(source);
