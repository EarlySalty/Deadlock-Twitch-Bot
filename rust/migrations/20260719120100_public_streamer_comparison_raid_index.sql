-- no-transaction
-- Raid-Ereignisse werden während des Indexaufbaus weiterhin geschrieben.
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_twitch_raid_retention_target_lower_executed
    ON public.twitch_raid_retention (LOWER(to_broadcaster_login), executed_at);
