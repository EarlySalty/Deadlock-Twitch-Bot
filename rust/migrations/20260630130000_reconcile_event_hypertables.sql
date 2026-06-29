-- Schema-Reconcile: Event-/Analytics-Tabellen auf den Prod-Zustand
-- bringen: Hypertables mit aktivierter Timescale-Compression und
-- identischen Compression-Policies.
--
-- Frische Datenbanken enthalten bei mehreren Tabellen noch einen
-- prod-fremden PK(id). Dieser wird vorab entfernt, weil er die
-- Hypertable-Konvertierung blockiert: TimescaleDB verlangt, dass jeder
-- UNIQUE/PRIMARY KEY die Partitionierungsspalte enthaelt. Auf Prod sind
-- die Tabellen bereits Hypertables, bereits komprimiert und ohne PK(id);
-- alle Statements sind dort No-ops.
--
-- Voraussetzung: timescaledb-Extension ist im geteilten Schema bereits
-- installiert. Idempotent via IF EXISTS, if_not_exists und Guard gegen
-- bereits aktivierte Compression.

-- twitch_ad_break_events
ALTER TABLE public.twitch_ad_break_events DROP CONSTRAINT IF EXISTS twitch_ad_break_events_pkey;
SELECT create_hypertable('public.twitch_ad_break_events', 'started_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM timescaledb_information.hypertables
                 WHERE hypertable_schema='public' AND hypertable_name='twitch_ad_break_events' AND compression_enabled) THEN
    EXECUTE 'ALTER TABLE public.twitch_ad_break_events SET (timescaledb.compress, timescaledb.compress_segmentby=''twitch_user_id'', timescaledb.compress_orderby=''started_at DESC'')';
  END IF;
END $$;
SELECT add_compression_policy('public.twitch_ad_break_events', INTERVAL '7 days', if_not_exists => TRUE);

-- twitch_ads_schedule_snapshot
ALTER TABLE public.twitch_ads_schedule_snapshot DROP CONSTRAINT IF EXISTS twitch_ads_schedule_snapshot_pkey;
SELECT create_hypertable('public.twitch_ads_schedule_snapshot', 'snapshot_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM timescaledb_information.hypertables
                 WHERE hypertable_schema='public' AND hypertable_name='twitch_ads_schedule_snapshot' AND compression_enabled) THEN
    EXECUTE 'ALTER TABLE public.twitch_ads_schedule_snapshot SET (timescaledb.compress, timescaledb.compress_segmentby=''twitch_user_id'', timescaledb.compress_orderby=''snapshot_at DESC'')';
  END IF;
END $$;
SELECT add_compression_policy('public.twitch_ads_schedule_snapshot', INTERVAL '7 days', if_not_exists => TRUE);

-- twitch_ban_events
ALTER TABLE public.twitch_ban_events DROP CONSTRAINT IF EXISTS twitch_ban_events_pkey;
SELECT create_hypertable('public.twitch_ban_events', 'received_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM timescaledb_information.hypertables
                 WHERE hypertable_schema='public' AND hypertable_name='twitch_ban_events' AND compression_enabled) THEN
    EXECUTE 'ALTER TABLE public.twitch_ban_events SET (timescaledb.compress, timescaledb.compress_segmentby=''twitch_user_id'', timescaledb.compress_orderby=''received_at DESC'')';
  END IF;
END $$;
SELECT add_compression_policy('public.twitch_ban_events', INTERVAL '7 days', if_not_exists => TRUE);

-- twitch_bits_events
ALTER TABLE public.twitch_bits_events DROP CONSTRAINT IF EXISTS twitch_bits_events_pkey;
SELECT create_hypertable('public.twitch_bits_events', 'received_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM timescaledb_information.hypertables
                 WHERE hypertable_schema='public' AND hypertable_name='twitch_bits_events' AND compression_enabled) THEN
    EXECUTE 'ALTER TABLE public.twitch_bits_events SET (timescaledb.compress, timescaledb.compress_segmentby=''twitch_user_id'', timescaledb.compress_orderby=''received_at DESC'')';
  END IF;
END $$;
SELECT add_compression_policy('public.twitch_bits_events', INTERVAL '7 days', if_not_exists => TRUE);

-- twitch_channel_points_events
ALTER TABLE public.twitch_channel_points_events DROP CONSTRAINT IF EXISTS twitch_channel_points_events_pkey;
SELECT create_hypertable('public.twitch_channel_points_events', 'redeemed_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM timescaledb_information.hypertables
                 WHERE hypertable_schema='public' AND hypertable_name='twitch_channel_points_events' AND compression_enabled) THEN
    EXECUTE 'ALTER TABLE public.twitch_channel_points_events SET (timescaledb.compress, timescaledb.compress_segmentby=''twitch_user_id'', timescaledb.compress_orderby=''redeemed_at DESC'')';
  END IF;
END $$;
SELECT add_compression_policy('public.twitch_channel_points_events', INTERVAL '7 days', if_not_exists => TRUE);

-- twitch_channel_updates
ALTER TABLE public.twitch_channel_updates DROP CONSTRAINT IF EXISTS twitch_channel_updates_pkey;
SELECT create_hypertable('public.twitch_channel_updates', 'recorded_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM timescaledb_information.hypertables
                 WHERE hypertable_schema='public' AND hypertable_name='twitch_channel_updates' AND compression_enabled) THEN
    EXECUTE 'ALTER TABLE public.twitch_channel_updates SET (timescaledb.compress, timescaledb.compress_segmentby=''twitch_user_id'', timescaledb.compress_orderby=''recorded_at DESC'')';
  END IF;
END $$;
SELECT add_compression_policy('public.twitch_channel_updates', INTERVAL '7 days', if_not_exists => TRUE);

-- twitch_chat_messages
ALTER TABLE public.twitch_chat_messages DROP CONSTRAINT IF EXISTS twitch_chat_messages_pkey;
SELECT create_hypertable('public.twitch_chat_messages', 'message_ts', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '1 day');
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM timescaledb_information.hypertables
                 WHERE hypertable_schema='public' AND hypertable_name='twitch_chat_messages' AND compression_enabled) THEN
    EXECUTE 'ALTER TABLE public.twitch_chat_messages SET (timescaledb.compress, timescaledb.compress_segmentby=''streamer_login,session_id'', timescaledb.compress_orderby=''message_ts DESC'')';
  END IF;
END $$;
SELECT add_compression_policy('public.twitch_chat_messages', INTERVAL '7 days', if_not_exists => TRUE);

-- twitch_clips_social_analytics
ALTER TABLE public.twitch_clips_social_analytics DROP CONSTRAINT IF EXISTS twitch_clips_social_analytics_pkey;
SELECT create_hypertable('public.twitch_clips_social_analytics', 'synced_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '30 days');
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM timescaledb_information.hypertables
                 WHERE hypertable_schema='public' AND hypertable_name='twitch_clips_social_analytics' AND compression_enabled) THEN
    EXECUTE 'ALTER TABLE public.twitch_clips_social_analytics SET (timescaledb.compress, timescaledb.compress_segmentby=''platform'', timescaledb.compress_orderby=''synced_at DESC'')';
  END IF;
END $$;
SELECT add_compression_policy('public.twitch_clips_social_analytics', INTERVAL '30 days', if_not_exists => TRUE);

-- twitch_eventsub_capacity_snapshot
ALTER TABLE public.twitch_eventsub_capacity_snapshot DROP CONSTRAINT IF EXISTS twitch_eventsub_capacity_snapshot_pkey;
SELECT create_hypertable('public.twitch_eventsub_capacity_snapshot', 'ts_utc', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM timescaledb_information.hypertables
                 WHERE hypertable_schema='public' AND hypertable_name='twitch_eventsub_capacity_snapshot' AND compression_enabled) THEN
    EXECUTE 'ALTER TABLE public.twitch_eventsub_capacity_snapshot SET (timescaledb.compress, timescaledb.compress_segmentby=''trigger_reason'', timescaledb.compress_orderby=''ts_utc DESC'')';
  END IF;
END $$;
SELECT add_compression_policy('public.twitch_eventsub_capacity_snapshot', INTERVAL '7 days', if_not_exists => TRUE);

-- twitch_follow_events
ALTER TABLE public.twitch_follow_events DROP CONSTRAINT IF EXISTS twitch_follow_events_pkey;
SELECT create_hypertable('public.twitch_follow_events', 'followed_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM timescaledb_information.hypertables
                 WHERE hypertable_schema='public' AND hypertable_name='twitch_follow_events' AND compression_enabled) THEN
    EXECUTE 'ALTER TABLE public.twitch_follow_events SET (timescaledb.compress, timescaledb.compress_segmentby=''streamer_login'', timescaledb.compress_orderby=''followed_at DESC'')';
  END IF;
END $$;
SELECT add_compression_policy('public.twitch_follow_events', INTERVAL '7 days', if_not_exists => TRUE);

-- twitch_hype_train_events
ALTER TABLE public.twitch_hype_train_events DROP CONSTRAINT IF EXISTS twitch_hype_train_events_pkey;
SELECT create_hypertable('public.twitch_hype_train_events', 'started_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM timescaledb_information.hypertables
                 WHERE hypertable_schema='public' AND hypertable_name='twitch_hype_train_events' AND compression_enabled) THEN
    EXECUTE 'ALTER TABLE public.twitch_hype_train_events SET (timescaledb.compress, timescaledb.compress_segmentby=''twitch_user_id'', timescaledb.compress_orderby=''started_at DESC'')';
  END IF;
END $$;
SELECT add_compression_policy('public.twitch_hype_train_events', INTERVAL '7 days', if_not_exists => TRUE);

-- twitch_link_clicks
ALTER TABLE public.twitch_link_clicks DROP CONSTRAINT IF EXISTS twitch_link_clicks_pkey;
SELECT create_hypertable('public.twitch_link_clicks', 'clicked_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM timescaledb_information.hypertables
                 WHERE hypertable_schema='public' AND hypertable_name='twitch_link_clicks' AND compression_enabled) THEN
    EXECUTE 'ALTER TABLE public.twitch_link_clicks SET (timescaledb.compress, timescaledb.compress_segmentby=''streamer_login'', timescaledb.compress_orderby=''clicked_at DESC'')';
  END IF;
END $$;
SELECT add_compression_policy('public.twitch_link_clicks', INTERVAL '7 days', if_not_exists => TRUE);

-- twitch_raid_history
ALTER TABLE public.twitch_raid_history DROP CONSTRAINT IF EXISTS twitch_raid_history_pkey;
SELECT create_hypertable('public.twitch_raid_history', 'executed_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM timescaledb_information.hypertables
                 WHERE hypertable_schema='public' AND hypertable_name='twitch_raid_history' AND compression_enabled) THEN
    EXECUTE 'ALTER TABLE public.twitch_raid_history SET (timescaledb.compress, timescaledb.compress_segmentby=''from_broadcaster_id'', timescaledb.compress_orderby=''id, executed_at DESC, to_broadcaster_id'')';
  END IF;
END $$;
SELECT add_compression_policy('public.twitch_raid_history', INTERVAL '7 days', if_not_exists => TRUE);

-- twitch_session_viewers
SELECT create_hypertable('public.twitch_session_viewers', 'ts_utc', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '1 day');
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM timescaledb_information.hypertables
                 WHERE hypertable_schema='public' AND hypertable_name='twitch_session_viewers' AND compression_enabled) THEN
    EXECUTE 'ALTER TABLE public.twitch_session_viewers SET (timescaledb.compress, timescaledb.compress_segmentby=''session_id'', timescaledb.compress_orderby=''ts_utc DESC'')';
  END IF;
END $$;
SELECT add_compression_policy('public.twitch_session_viewers', INTERVAL '7 days', if_not_exists => TRUE);

-- twitch_shoutout_events
ALTER TABLE public.twitch_shoutout_events DROP CONSTRAINT IF EXISTS twitch_shoutout_events_pkey;
SELECT create_hypertable('public.twitch_shoutout_events', 'received_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM timescaledb_information.hypertables
                 WHERE hypertable_schema='public' AND hypertable_name='twitch_shoutout_events' AND compression_enabled) THEN
    EXECUTE 'ALTER TABLE public.twitch_shoutout_events SET (timescaledb.compress, timescaledb.compress_segmentby=''twitch_user_id'', timescaledb.compress_orderby=''received_at DESC'')';
  END IF;
END $$;
SELECT add_compression_policy('public.twitch_shoutout_events', INTERVAL '7 days', if_not_exists => TRUE);

-- twitch_stats_category
SELECT create_hypertable('public.twitch_stats_category', 'ts_utc', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM timescaledb_information.hypertables
                 WHERE hypertable_schema='public' AND hypertable_name='twitch_stats_category' AND compression_enabled) THEN
    EXECUTE 'ALTER TABLE public.twitch_stats_category SET (timescaledb.compress, timescaledb.compress_segmentby=''streamer'', timescaledb.compress_orderby=''ts_utc DESC'')';
  END IF;
END $$;
SELECT add_compression_policy('public.twitch_stats_category', INTERVAL '7 days', if_not_exists => TRUE);

-- twitch_stats_tracked
SELECT create_hypertable('public.twitch_stats_tracked', 'ts_utc', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM timescaledb_information.hypertables
                 WHERE hypertable_schema='public' AND hypertable_name='twitch_stats_tracked' AND compression_enabled) THEN
    EXECUTE 'ALTER TABLE public.twitch_stats_tracked SET (timescaledb.compress, timescaledb.compress_segmentby=''streamer'', timescaledb.compress_orderby=''ts_utc DESC'')';
  END IF;
END $$;
SELECT add_compression_policy('public.twitch_stats_tracked', INTERVAL '7 days', if_not_exists => TRUE);

-- twitch_subscription_events
ALTER TABLE public.twitch_subscription_events DROP CONSTRAINT IF EXISTS twitch_subscription_events_pkey;
SELECT create_hypertable('public.twitch_subscription_events', 'received_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM timescaledb_information.hypertables
                 WHERE hypertable_schema='public' AND hypertable_name='twitch_subscription_events' AND compression_enabled) THEN
    EXECUTE 'ALTER TABLE public.twitch_subscription_events SET (timescaledb.compress, timescaledb.compress_segmentby=''twitch_user_id'', timescaledb.compress_orderby=''received_at DESC'')';
  END IF;
END $$;
SELECT add_compression_policy('public.twitch_subscription_events', INTERVAL '7 days', if_not_exists => TRUE);

-- twitch_subscriptions_snapshot
ALTER TABLE public.twitch_subscriptions_snapshot DROP CONSTRAINT IF EXISTS twitch_subscriptions_snapshot_pkey;
SELECT create_hypertable('public.twitch_subscriptions_snapshot', 'snapshot_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM timescaledb_information.hypertables
                 WHERE hypertable_schema='public' AND hypertable_name='twitch_subscriptions_snapshot' AND compression_enabled) THEN
    EXECUTE 'ALTER TABLE public.twitch_subscriptions_snapshot SET (timescaledb.compress, timescaledb.compress_segmentby=''twitch_user_id'', timescaledb.compress_orderby=''snapshot_at DESC'')';
  END IF;
END $$;
SELECT add_compression_policy('public.twitch_subscriptions_snapshot', INTERVAL '7 days', if_not_exists => TRUE);
