-- no-transaction
-- Die Sessiontabelle ist eine normale Tabelle; CONCURRENTLY hält den Ingest offen.
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_twitch_sessions_login_lower_window
    ON public.twitch_stream_sessions (LOWER(streamer_login), started_at, ended_at);
