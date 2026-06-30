CREATE TABLE IF NOT EXISTS minimax_usage (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    ts         TEXT    NOT NULL,
    source     TEXT    NOT NULL,
    purpose    TEXT,
    model      TEXT,
    tokens_in  INTEGER DEFAULT 0,
    tokens_out INTEGER DEFAULT 0,
    total      INTEGER DEFAULT 0,
    success    INTEGER DEFAULT 1,
    meta       TEXT
);
CREATE INDEX IF NOT EXISTS idx_mmu_ts     ON minimax_usage(ts);
CREATE INDEX IF NOT EXISTS idx_mmu_source ON minimax_usage(source);
