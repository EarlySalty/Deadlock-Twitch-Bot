-- Reconcile fresh migrations with the live Rust schema contract.
-- Every change is guarded so live systems that already match are no-ops.

DO $$
DECLARE
    target RECORD;
    clip_media_id_needs_fix BOOLEAN;
BEGIN
    SELECT EXISTS (
        SELECT 1
          FROM pg_attribute
         WHERE attrelid = to_regclass('public.twitch_clips_social_media')
           AND attname = 'id'
           AND NOT attisdropped
           AND atttypid <> 'int8'::regtype
    ) INTO clip_media_id_needs_fix;

    IF clip_media_id_needs_fix THEN
        CREATE TEMP TABLE IF NOT EXISTS live_schema_type_reconcile_clip_media_fks (
            table_schema TEXT NOT NULL,
            table_name TEXT NOT NULL,
            constraint_name TEXT NOT NULL,
            constraint_def TEXT NOT NULL
        ) ON COMMIT DROP;

        TRUNCATE live_schema_type_reconcile_clip_media_fks;

        INSERT INTO live_schema_type_reconcile_clip_media_fks
            (table_schema, table_name, constraint_name, constraint_def)
        SELECT n.nspname,
               rel.relname,
               c.conname,
               pg_get_constraintdef(c.oid)
          FROM pg_constraint c
          JOIN pg_class rel ON rel.oid = c.conrelid
          JOIN pg_namespace n ON n.oid = rel.relnamespace
         WHERE c.contype = 'f'
           AND c.confrelid = to_regclass('public.twitch_clips_social_media')
           AND (
               SELECT a.attnum
                 FROM pg_attribute a
                WHERE a.attrelid = c.confrelid
                  AND a.attname = 'id'
                  AND NOT a.attisdropped
           ) = ANY (c.confkey);

        FOR target IN
            SELECT * FROM live_schema_type_reconcile_clip_media_fks
        LOOP
            EXECUTE format(
                'ALTER TABLE %I.%I DROP CONSTRAINT IF EXISTS %I',
                target.table_schema,
                target.table_name,
                target.constraint_name
            );
        END LOOP;

        CREATE TEMP TABLE IF NOT EXISTS live_schema_type_reconcile_clip_media_pk (
            table_schema TEXT NOT NULL,
            table_name TEXT NOT NULL,
            constraint_name TEXT NOT NULL,
            constraint_def TEXT NOT NULL
        ) ON COMMIT DROP;

        TRUNCATE live_schema_type_reconcile_clip_media_pk;

        INSERT INTO live_schema_type_reconcile_clip_media_pk
            (table_schema, table_name, constraint_name, constraint_def)
        SELECT n.nspname,
               rel.relname,
               c.conname,
               pg_get_constraintdef(c.oid)
          FROM pg_constraint c
          JOIN pg_class rel ON rel.oid = c.conrelid
          JOIN pg_namespace n ON n.oid = rel.relnamespace
         WHERE c.contype = 'p'
           AND c.conrelid = to_regclass('public.twitch_clips_social_media');

        FOR target IN
            SELECT * FROM live_schema_type_reconcile_clip_media_pk
        LOOP
            EXECUTE format(
                'ALTER TABLE %I.%I DROP CONSTRAINT IF EXISTS %I',
                target.table_schema,
                target.table_name,
                target.constraint_name
            );
        END LOOP;

        ALTER TABLE public.twitch_clips_social_media
            ALTER COLUMN id DROP DEFAULT;

        ALTER SEQUENCE IF EXISTS public.twitch_clips_social_media_id_seq AS bigint;

        ALTER TABLE public.twitch_clips_social_media
            ALTER COLUMN id TYPE bigint USING id::bigint;

        IF to_regclass('public.twitch_clips_social_media_id_seq') IS NOT NULL THEN
            ALTER TABLE public.twitch_clips_social_media
                ALTER COLUMN id SET DEFAULT nextval('public.twitch_clips_social_media_id_seq'::regclass);
        END IF;

        FOR target IN
            SELECT * FROM live_schema_type_reconcile_clip_media_pk
        LOOP
            EXECUTE format(
                'ALTER TABLE %I.%I ADD CONSTRAINT %I %s',
                target.table_schema,
                target.table_name,
                target.constraint_name,
                target.constraint_def
            );
        END LOOP;

        IF EXISTS (
            SELECT 1
              FROM pg_attribute
             WHERE attrelid = to_regclass('public.twitch_clips_social_analytics')
               AND attname = 'clip_id'
               AND NOT attisdropped
               AND atttypid <> 'int8'::regtype
        ) THEN
            ALTER TABLE public.twitch_clips_social_analytics
                ALTER COLUMN clip_id TYPE bigint USING clip_id::bigint;
        END IF;

        IF EXISTS (
            SELECT 1
              FROM pg_attribute
             WHERE attrelid = to_regclass('public.twitch_clips_upload_queue')
               AND attname = 'clip_id'
               AND NOT attisdropped
               AND atttypid <> 'int8'::regtype
        ) THEN
            ALTER TABLE public.twitch_clips_upload_queue
                ALTER COLUMN clip_id TYPE bigint USING clip_id::bigint;
        END IF;

        FOR target IN
            SELECT * FROM live_schema_type_reconcile_clip_media_fks
        LOOP
            EXECUTE format(
                'ALTER TABLE %I.%I ADD CONSTRAINT %I %s',
                target.table_schema,
                target.table_name,
                target.constraint_name,
                target.constraint_def
            );
        END LOOP;
    ELSIF EXISTS (
        SELECT 1
          FROM pg_sequence
         WHERE seqrelid = to_regclass('public.twitch_clips_social_media_id_seq')
           AND seqtypid <> 'int8'::regtype
    ) THEN
        ALTER SEQUENCE public.twitch_clips_social_media_id_seq AS bigint;
    END IF;

    FOR target IN
        SELECT * FROM (VALUES
            ('clip_fetch_history', 'clip_fetch_history_id_seq'),
            ('clip_templates_global', 'clip_templates_global_id_seq'),
            ('clip_templates_streamer', 'clip_templates_streamer_id_seq'),
            ('twitch_ad_break_events', 'twitch_ad_break_events_id_seq'),
            ('twitch_ads_schedule_snapshot', 'twitch_ads_schedule_snapshot_id_seq'),
            ('twitch_ban_events', 'twitch_ban_events_id_seq'),
            ('twitch_bits_events', 'twitch_bits_events_id_seq'),
            ('twitch_channel_points_events', 'twitch_channel_points_events_id_seq'),
            ('twitch_channel_updates', 'twitch_channel_updates_id_seq'),
            ('twitch_chat_messages', 'twitch_chat_messages_id_seq'),
            ('twitch_clips_social_analytics', 'twitch_clips_social_analytics_id_seq'),
            ('twitch_clips_upload_queue', 'twitch_clips_upload_queue_id_seq'),
            ('twitch_eventsub_capacity_snapshot', 'twitch_eventsub_capacity_snapshot_id_seq'),
            ('twitch_follow_events', 'twitch_follow_events_id_seq'),
            ('twitch_hype_train_events', 'twitch_hype_train_events_id_seq'),
            ('twitch_link_clicks', 'twitch_link_clicks_id_seq'),
            ('twitch_shoutout_events', 'twitch_shoutout_events_id_seq'),
            ('twitch_subscription_events', 'twitch_subscription_events_id_seq'),
            ('twitch_subscriptions_snapshot', 'twitch_subscriptions_snapshot_id_seq')
        ) AS t(table_name, seq_name)
    LOOP
        IF EXISTS (
            SELECT 1
              FROM pg_attribute
             WHERE attrelid = to_regclass(format('public.%I', target.table_name))
               AND attname = 'id'
               AND NOT attisdropped
               AND atttypid <> 'int8'::regtype
        ) THEN
            EXECUTE format(
                'ALTER TABLE public.%I ALTER COLUMN id DROP DEFAULT',
                target.table_name
            );

            EXECUTE format(
                'ALTER SEQUENCE IF EXISTS public.%I AS bigint',
                target.seq_name
            );

            EXECUTE format(
                'ALTER TABLE public.%I ALTER COLUMN id TYPE bigint USING id::bigint',
                target.table_name
            );

            IF to_regclass(format('public.%I', target.seq_name)) IS NOT NULL THEN
                EXECUTE format(
                    'ALTER TABLE public.%I ALTER COLUMN id SET DEFAULT nextval(%L::regclass)',
                    target.table_name,
                    'public.' || target.seq_name
                );
            END IF;
        ELSIF EXISTS (
            SELECT 1
              FROM pg_sequence
             WHERE seqrelid = to_regclass(format('public.%I', target.seq_name))
               AND seqtypid <> 'int8'::regtype
        ) THEN
            EXECUTE format(
                'ALTER SEQUENCE public.%I AS bigint',
                target.seq_name
            );
        END IF;
    END LOOP;

    FOR target IN
        SELECT * FROM (VALUES
            ('twitch_ad_break_events', 'session_id'),
            ('twitch_ban_events', 'session_id'),
            ('twitch_bits_events', 'session_id'),
            ('twitch_channel_points_events', 'session_id'),
            ('twitch_hype_train_events', 'session_id'),
            ('twitch_subscription_events', 'session_id')
        ) AS t(table_name, column_name)
    LOOP
        IF EXISTS (
            SELECT 1
              FROM pg_attribute
             WHERE attrelid = to_regclass(format('public.%I', target.table_name))
               AND attname = target.column_name
               AND NOT attisdropped
               AND atttypid <> 'int8'::regtype
        ) THEN
            EXECUTE format(
                'ALTER TABLE public.%I ALTER COLUMN %I TYPE bigint USING %I::bigint',
                target.table_name,
                target.column_name,
                target.column_name
            );
        END IF;
    END LOOP;

    FOR target IN
        SELECT * FROM (VALUES
            ('clip_fetch_history', 'fetched_at', TRUE),
            ('clip_last_hashtags', 'last_used_at', FALSE),
            ('clip_templates_global', 'created_at', TRUE),
            ('clip_templates_streamer', 'created_at', TRUE),
            ('clip_templates_streamer', 'updated_at', TRUE),
            ('twitch_ad_break_events', 'started_at', TRUE),
            ('twitch_ads_schedule_snapshot', 'last_ad_at', FALSE),
            ('twitch_ads_schedule_snapshot', 'next_ad_at', FALSE),
            ('twitch_ads_schedule_snapshot', 'snapshot_at', TRUE),
            ('twitch_ads_schedule_snapshot', 'snooze_refresh_at', FALSE),
            ('twitch_ban_events', 'ends_at', FALSE),
            ('twitch_ban_events', 'received_at', TRUE),
            ('twitch_bits_events', 'received_at', TRUE),
            ('twitch_channel_points_events', 'redeemed_at', FALSE),
            ('twitch_channel_updates', 'recorded_at', TRUE),
            ('twitch_clips_social_analytics', 'posted_at', FALSE),
            ('twitch_clips_social_analytics', 'synced_at', FALSE),
            ('twitch_clips_social_media', 'downloaded_at', FALSE),
            ('twitch_clips_social_media', 'instagram_uploaded_at', FALSE),
            ('twitch_clips_social_media', 'last_analytics_sync', FALSE),
            ('twitch_clips_social_media', 'tiktok_uploaded_at', FALSE),
            ('twitch_clips_social_media', 'youtube_uploaded_at', FALSE),
            ('twitch_clips_upload_queue', 'completed_at', FALSE),
            ('twitch_clips_upload_queue', 'created_at', TRUE),
            ('twitch_clips_upload_queue', 'last_attempt_at', FALSE),
            ('twitch_clips_upload_queue', 'scheduled_at', FALSE),
            ('twitch_eventsub_capacity_snapshot', 'ts_utc', TRUE),
            ('twitch_follow_events', 'followed_at', TRUE),
            ('twitch_hype_train_events', 'ended_at', FALSE),
            ('twitch_hype_train_events', 'started_at', FALSE),
            ('twitch_link_clicks', 'clicked_at', TRUE),
            ('twitch_raid_auth', 'authorized_at', TRUE),
            ('twitch_raid_auth', 'created_at', TRUE),
            ('twitch_raid_auth', 'enc_migrated_at', FALSE),
            ('twitch_raid_auth', 'last_refreshed_at', FALSE),
            ('twitch_raid_auth', 'reauth_notified_at', FALSE),
            ('twitch_raid_auth', 'token_expires_at', FALSE),
            ('twitch_shoutout_events', 'received_at', TRUE),
            ('twitch_streamers', 'created_at', TRUE),
            ('twitch_subscription_events', 'received_at', TRUE),
            ('twitch_subscriptions_snapshot', 'snapshot_at', TRUE)
        ) AS t(table_name, column_name, set_default_now)
    LOOP
        IF EXISTS (
            SELECT 1
              FROM pg_attribute
             WHERE attrelid = to_regclass(format('public.%I', target.table_name))
               AND attname = target.column_name
               AND NOT attisdropped
               AND atttypid <> 'timestamptz'::regtype
        ) THEN
            EXECUTE format(
                'ALTER TABLE public.%I '
                || 'ALTER COLUMN %I DROP DEFAULT, '
                || 'ALTER COLUMN %I TYPE timestamp with time zone USING CASE '
                || 'WHEN %I IS NULL OR BTRIM(%I::text) = '''' THEN NULL '
                || 'ELSE %I::text::timestamptz END',
                target.table_name,
                target.column_name,
                target.column_name,
                target.column_name,
                target.column_name,
                target.column_name
            );

            IF target.set_default_now THEN
                EXECUTE format(
                    'ALTER TABLE public.%I ALTER COLUMN %I SET DEFAULT now()',
                    target.table_name,
                    target.column_name
                );
            END IF;
        END IF;
    END LOOP;

    FOR target IN
        SELECT * FROM (VALUES
            ('clip_templates_streamer', 'is_default'),
            ('twitch_clips_social_media', 'uploaded_instagram'),
            ('twitch_clips_social_media', 'uploaded_tiktok'),
            ('twitch_clips_social_media', 'uploaded_youtube')
        ) AS t(table_name, column_name)
    LOOP
        IF EXISTS (
            SELECT 1
              FROM pg_attribute
             WHERE attrelid = to_regclass(format('public.%I', target.table_name))
               AND attname = target.column_name
               AND NOT attisdropped
               AND atttypid <> 'boolean'::regtype
        ) THEN
            EXECUTE format(
                'ALTER TABLE public.%I '
                || 'ALTER COLUMN %I DROP DEFAULT, '
                || 'ALTER COLUMN %I TYPE boolean USING CASE '
                || 'WHEN %I IS NULL THEN FALSE '
                || 'WHEN %I::text IN (''1'', ''true'', ''t'', ''yes'', ''on'') THEN TRUE '
                || 'ELSE FALSE END, '
                || 'ALTER COLUMN %I SET DEFAULT false',
                target.table_name,
                target.column_name,
                target.column_name,
                target.column_name,
                target.column_name,
                target.column_name
            );
        END IF;
    END LOOP;

    FOR target IN
        SELECT * FROM (VALUES
            ('twitch_clips_social_analytics', 'completion_rate', FALSE),
            ('twitch_clips_social_analytics', 'ctr', FALSE),
            ('twitch_clips_social_analytics', 'engagement_rate', FALSE),
            ('twitch_clips_social_analytics', 'watch_time_avg', FALSE),
            ('twitch_clips_social_media', 'duration_seconds', FALSE),
            ('twitch_eventsub_capacity_snapshot', 'utilization_pct', TRUE),
            ('twitch_stream_sessions', 'dropoff_pct', FALSE),
            ('twitch_stream_sessions', 'retention_10m', FALSE),
            ('twitch_stream_sessions', 'retention_20m', FALSE),
            ('twitch_stream_sessions', 'retention_5m', FALSE)
        ) AS t(table_name, column_name, set_default_zero)
    LOOP
        IF EXISTS (
            SELECT 1
              FROM pg_attribute
             WHERE attrelid = to_regclass(format('public.%I', target.table_name))
               AND attname = target.column_name
               AND NOT attisdropped
               AND atttypid <> 'float8'::regtype
        ) THEN
            EXECUTE format(
                'ALTER TABLE public.%I '
                || 'ALTER COLUMN %I DROP DEFAULT, '
                || 'ALTER COLUMN %I TYPE double precision USING %I::double precision',
                target.table_name,
                target.column_name,
                target.column_name,
                target.column_name
            );

            IF target.set_default_zero THEN
                EXECUTE format(
                    'ALTER TABLE public.%I ALTER COLUMN %I SET DEFAULT 0',
                    target.table_name,
                    target.column_name
                );
            END IF;
        END IF;
    END LOOP;

    FOR target IN
        SELECT * FROM (VALUES
            ('twitch_ads_schedule_snapshot', 'snapshot_at', FALSE),
            ('twitch_eventsub_capacity_snapshot', 'ts_utc', FALSE),
            ('twitch_hype_train_events', 'started_at', FALSE),
            ('twitch_link_clicks', 'clicked_at', FALSE),
            ('twitch_live_announcement_configs', 'updated_at', TRUE),
            ('twitch_stats_category', 'streamer', FALSE),
            ('twitch_stats_category', 'ts_utc', FALSE),
            ('twitch_stats_tracked', 'streamer', FALSE),
            ('twitch_stats_tracked', 'ts_utc', FALSE),
            ('twitch_subscriptions_snapshot', 'snapshot_at', FALSE)
        ) AS t(table_name, column_name, drop_default)
    LOOP
        IF EXISTS (
            SELECT 1
              FROM pg_attribute
             WHERE attrelid = to_regclass(format('public.%I', target.table_name))
               AND attname = target.column_name
               AND NOT attisdropped
               AND NOT attnotnull
        ) THEN
            IF target.drop_default THEN
                EXECUTE format(
                    'ALTER TABLE public.%I ALTER COLUMN %I DROP DEFAULT, ALTER COLUMN %I SET NOT NULL',
                    target.table_name,
                    target.column_name,
                    target.column_name
                );
            ELSE
                EXECUTE format(
                    'ALTER TABLE public.%I ALTER COLUMN %I SET NOT NULL',
                    target.table_name,
                    target.column_name
                );
            END IF;
        END IF;
    END LOOP;
END $$;
