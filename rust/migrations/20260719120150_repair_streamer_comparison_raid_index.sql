-- no-transaction
-- Repariert auch einen INVALID-Index nach einem abgebrochenen Concurrent-Build.
REINDEX INDEX CONCURRENTLY public.idx_twitch_raid_retention_target_lower_executed;
