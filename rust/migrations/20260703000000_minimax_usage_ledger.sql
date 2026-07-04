-- MiniMax-Usage-Ledger in der zentralen Postgres (vorher separate SQLite-Datei).
-- tb-llm verbucht hier best-effort jeden MiniMax-Call (Tokens pro source/purpose)
-- und liest die rollierende 5h-Budget-Summe. `ts` bleibt bewusst TEXT
-- (ISO-8601 UTC, Sekunden, +00:00) für Byte-Parität mit dem Python-Helfer; die
-- Fensterabfrage vergleicht `ts` deshalb lexikografisch. Integer-Spalten sind
-- BIGINT (1:1 zur 64-Bit-INTEGER-Semantik von SQLite).

CREATE TABLE IF NOT EXISTS public.minimax_usage (
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

CREATE INDEX IF NOT EXISTS idx_mmu_ts     ON public.minimax_usage (ts);
CREATE INDEX IF NOT EXISTS idx_mmu_source ON public.minimax_usage (source);
