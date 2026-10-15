-- no-transaction
CREATE INDEX IF NOT EXISTS idx_dashboard_follow_login_window
    ON public.twitch_follow_events (LOWER(streamer_login), followed_at)
    WITH (timescaledb.transaction_per_chunk);
