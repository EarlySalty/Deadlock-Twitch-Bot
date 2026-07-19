-- no-transaction
-- Repariert auch einen INVALID-Index nach einem abgebrochenen Concurrent-Build.
REINDEX INDEX CONCURRENTLY public.idx_twitch_sessions_login_lower_window;
