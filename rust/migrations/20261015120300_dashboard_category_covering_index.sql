-- no-transaction
CREATE INDEX IF NOT EXISTS idx_dashboard_category_time_covering
    ON public.twitch_stats_category (ts_utc) INCLUDE (streamer, viewer_count, is_partner)
    WITH (timescaledb.transaction_per_chunk);
