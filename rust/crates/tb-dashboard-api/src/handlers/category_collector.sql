-- Read-only category metrics. Hours are sample-held estimates ONLY between
-- adjacent successful polls no more than two configured intervals apart.
-- An incomplete poll or an outage never becomes invented offline/zero time.
WITH
params AS (
    SELECT $1::integer AS days, $2::timestamptz AS period_end,
           date_trunc('hour', $2::timestamptz - make_interval(days => $1::integer), 'UTC') AS period_start
),
cfg AS (SELECT * FROM category_collector_config WHERE id = 1),
health AS (SELECT * FROM category_collector_status WHERE id = 1),
ordered_polls AS (
    SELECT p.*, lead(started_at) OVER (ORDER BY started_at, poll_id) AS next_at,
           lead(status) OVER (ORDER BY started_at, poll_id) AS next_status
    FROM category_polls p CROSS JOIN params t CROSS JOIN cfg
    WHERE p.started_at >= t.period_start - make_interval(secs => cfg.discovery_interval_seconds * 2)
      AND p.started_at <= t.period_end
),
weights AS (
    SELECT p.*, CASE
        WHEN p.status = 'complete' AND p.next_status = 'complete'
         AND p.next_at > p.started_at
         AND p.next_at - p.started_at <= make_interval(secs => cfg.discovery_interval_seconds * 2)
        THEN greatest(0.0, extract(epoch FROM
             least(p.next_at, t.period_end) - greatest(p.started_at, t.period_start)))::double precision
        ELSE 0.0::double precision END AS observed_seconds
    FROM ordered_polls p CROSS JOIN params t CROSS JOIN cfg
),
samples AS (
    SELECT s.*, coalesce(nullif(lower(s.language), ''), 'und') AS stream_language,
           w.observed_seconds
    FROM category_stream_snapshots s JOIN weights w ON w.poll_id = s.poll_id CROSS JOIN params t
    WHERE w.status = 'complete' AND (s.snapshot_at >= t.period_start OR w.observed_seconds > 0)
      AND s.snapshot_at <= t.period_end
),
stream_languages AS (
    SELECT stream_language AS language, count(DISTINCT stream_id) AS unique_streams,
           count(DISTINCT user_id) AS unique_channels,
           sum(observed_seconds) / 3600.0 AS broadcast_hours,
           sum(viewer_count * observed_seconds) / 3600.0 AS viewer_hours,
           sum(viewer_count * observed_seconds) / nullif(sum(observed_seconds), 0) AS avg_viewers
    FROM samples GROUP BY stream_language
),
channel_metrics AS (
    SELECT stream_language AS language, user_id,
           (array_agg(user_login ORDER BY snapshot_at DESC))[1] AS login,
           sum(observed_seconds) / 3600.0 AS broadcast_hours,
           sum(viewer_count * observed_seconds) / 3600.0 AS viewer_hours,
           sum(viewer_count * observed_seconds) / nullif(sum(observed_seconds), 0) AS avg_viewers
    FROM samples GROUP BY stream_language, user_id
),
ranked_channels AS (
    SELECT m.*, coalesce(nullif(c.display_name, ''), m.login) AS display_name,
           row_number() OVER (PARTITION BY m.language ORDER BY m.viewer_hours DESC, m.user_id) AS position
    FROM channel_metrics m LEFT JOIN category_channels c ON c.user_id = m.user_id
),
trend AS (
    SELECT date_trunc('hour', p.started_at, 'UTC') AS bucket_at,
           avg(p.stream_count)::double precision AS avg_streams,
           avg(p.viewer_total)::double precision AS avg_viewers,
           count(*) AS poll_samples
    FROM ordered_polls p CROSS JOIN params t
    WHERE p.status = 'complete' AND p.started_at >= t.period_start
    GROUP BY 1
),
chat_buckets AS (
    -- Full hours from the durable rollup. Do not sum distinct_chatters here:
    -- distinct counts are valid only within one channel, hour and language.
    SELECT r.hour_bucket, coalesce(nullif(r.lang, ''), 'und') AS language, r.messages
    FROM category_chat_rollup r CROSS JOIN params t
    WHERE r.hour_bucket >= t.period_start
      AND r.hour_bucket < date_trunc('hour', t.period_end, 'UTC')
    UNION ALL
    -- Exact leading partial hour and the still-open current hour. No raw text
    -- or chatter identifiers leave this query, even for administrators.
    SELECT date_trunc('hour', m.sent_at, 'UTC'), coalesce(nullif(m.detected_lang, ''), 'und'), count(*)
    FROM category_chat_messages m CROSS JOIN params t
    WHERE m.sent_at >= t.period_start AND m.sent_at <= t.period_end
      AND ((t.period_start > date_trunc('hour', t.period_start, 'UTC') AND m.sent_at < date_trunc('hour', t.period_start, 'UTC') + interval '1 hour')
           OR m.sent_at >= date_trunc('hour', t.period_end, 'UTC'))
      AND (m.source_room_id IS NULL OR m.source_room_id = m.room_user_id)
    GROUP BY 1, 2
),
message_languages AS (
    SELECT language, sum(messages)::bigint AS messages FROM chat_buckets GROUP BY language
),
chat_heatmap AS (
    SELECT language, extract(hour FROM hour_bucket AT TIME ZONE 'UTC')::integer AS hour_utc,
           sum(messages)::bigint AS messages FROM chat_buckets GROUP BY 1, 2
),
coverage AS (
    SELECT min(started_at) FILTER (WHERE status = 'complete') AS first_seen_at,
           max(completed_at) FILTER (WHERE status = 'complete') AS last_complete_at
    FROM category_polls
)
SELECT jsonb_build_object(
    'meta', jsonb_build_object(
        'days', t.days, 'period_start', t.period_start, 'period_end', t.period_end, 'timezone', 'UTC',
        'first_seen_at', c.first_seen_at, 'last_completed_poll_at', c.last_complete_at,
        'last_message_at', (SELECT sent_at FROM category_chat_messages ORDER BY sent_at DESC LIMIT 1),
        'complete_polls', (SELECT count(*) FROM ordered_polls WHERE status = 'complete' AND started_at >= t.period_start),
        'incomplete_polls', (SELECT count(*) FROM ordered_polls WHERE status IN ('incomplete', 'failed') AND started_at >= t.period_start),
        'dropped_messages', h.chat_privmsgs_dropped + h.chat_queue_dropped,
        'storage_bytes', (SELECT coalesce(sum(pg_total_relation_size(r.oid)), 0)::bigint
                          FROM pg_class r JOIN pg_namespace n ON n.oid = r.relnamespace
                          WHERE n.nspname = current_schema() AND r.relkind = 'r'
                            AND left(r.relname, 9) = 'category_'),
        'retention_days', cfg.retention_days,
        'collector_status', CASE
            WHEN NOT cfg.enabled THEN 'disabled'
            WHEN h.last_error_at IS NOT NULL AND (c.last_complete_at IS NULL OR h.last_error_at > c.last_complete_at) THEN 'error'
            WHEN c.last_complete_at IS NULL THEN 'not_started'
            WHEN c.last_complete_at < t.period_end - make_interval(secs => greatest(180, cfg.discovery_interval_seconds * 3)) THEN 'stale'
            ELSE 'running' END,
        'measurement_seconds', (SELECT coalesce(sum(observed_seconds), 0) FROM weights),
        'last_rollup_hour', h.last_rollup_hour,
        'last_retention_run_at', h.last_retention_run_at,
        'sampling_method', 'adjacent_complete_polls_max_two_intervals',
        'dropped_messages_scope', 'collector_lifetime'
    ),
    'stream_languages', coalesce((SELECT jsonb_agg(to_jsonb(l) ORDER BY l.viewer_hours DESC, l.language) FROM stream_languages l), '[]'::jsonb),
    'message_languages', coalesce((SELECT jsonb_agg(to_jsonb(m) ORDER BY m.messages DESC, m.language) FROM message_languages m), '[]'::jsonb),
    'trend', coalesce((SELECT jsonb_agg(to_jsonb(p) ORDER BY p.bucket_at) FROM trend p), '[]'::jsonb),
    'top_channels', coalesce((SELECT jsonb_agg(to_jsonb(r) - 'position' ORDER BY r.language, r.position) FROM ranked_channels r WHERE r.position <= 20), '[]'::jsonb),
    'chat_heatmap', coalesce((SELECT jsonb_agg(to_jsonb(hm) ORDER BY hm.language, hm.hour_utc) FROM chat_heatmap hm), '[]'::jsonb)
)
FROM params t CROSS JOIN cfg CROSS JOIN health h CROSS JOIN coverage c
