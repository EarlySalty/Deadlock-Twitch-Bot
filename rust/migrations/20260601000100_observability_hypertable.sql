-- F1 baseline (Timescale-Teil): twitch_observability_events zur Hypertable +
-- Compression machen. Getrennt von der Tabellen-Baseline, da Timescale-DDL
-- raw SQL ist (ADR-0002) und die Tabelle + PK (id, created_at) voraussetzt.
--
-- Voraussetzung: timescaledb-Extension ist im geteilten Schema bereits installiert
-- (CREATE EXTENSION timescaledb muss erster Befehl einer Session sein und kann
-- daher nicht in einer Migration laufen). Idempotent via if_not_exists.

SELECT create_hypertable(
    'twitch_observability_events',
    'created_at',
    if_not_exists => TRUE,
    migrate_data => TRUE,
    chunk_time_interval => INTERVAL '7 days'
);

ALTER TABLE twitch_observability_events SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'flow_type,flow_id',
    timescaledb.compress_orderby = 'created_at DESC'
);

SELECT add_compression_policy(
    'twitch_observability_events',
    INTERVAL '7 days',
    if_not_exists => TRUE
);
