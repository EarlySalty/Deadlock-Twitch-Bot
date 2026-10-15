-- no-transaction
CREATE INDEX IF NOT EXISTS idx_dashboard_tracked_login_window
    ON public.twitch_stats_tracked (LOWER(streamer), ts_utc) INCLUDE (streamer, viewer_count, is_partner)
    WITH (timescaledb.transaction_per_chunk);
