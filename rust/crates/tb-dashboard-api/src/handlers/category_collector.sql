-- $1: UTC-Stundenbeginn des Auswertungsfensters, $2: Erfassungszeit, $3: Tage.
-- Nur vollständige Beobachtungen. Ein Fehler oder eine größere Lücke trennt
-- die Zeitreihe; nach der letzten Probe wird keine Sendezeit extrapoliert.
WITH cfg AS MATERIALIZED (
    SELECT enabled, discovery_interval_seconds, retention_days
    FROM category_collector_config WHERE id = 1
), state AS MATERIALIZED (
    SELECT last_error_at, updated_at, chat_privmsgs_dropped,
           chat_queue_dropped, chat_commands_dropped
    FROM category_collector_status WHERE id = 1
), poll_chain AS MATERIALIZED (
    SELECT p.*, LEAD(started_at) OVER ordered AS next_at,
           LEAD(status) OVER ordered AS next_status
    FROM category_polls p CROSS JOIN cfg
    WHERE started_at >= $1::timestamptz - make_interval(secs => 2 * cfg.discovery_interval_seconds)
      AND started_at <= $2::timestamptz
    WINDOW ordered AS (ORDER BY started_at, poll_id)
), weighted_polls AS MATERIALIZED (
    SELECT p.*, CASE
        WHEN status = 'complete' AND next_status = 'complete'
         AND next_at > started_at
         AND next_at - started_at <= make_interval(secs => 2 * cfg.discovery_interval_seconds)
        THEN GREATEST(0.0, EXTRACT(EPOCH FROM
             LEAST(next_at, $2::timestamptz) - GREATEST(started_at, $1::timestamptz)))::double precision
        ELSE 0.0::double precision END AS observed_seconds
    FROM poll_chain p CROSS JOIN cfg
), channel_totals AS MATERIALIZED (
    SELECT s.user_id, COALESCE(NULLIF(s.language, ''), 'und') AS language,
           MAX(s.user_login) AS login, COUNT(DISTINCT s.stream_id)::bigint AS unique_streams,
           SUM(p.observed_seconds)::double precision AS seconds,
           SUM(s.viewer_count * p.observed_seconds)::double precision AS viewer_seconds
    FROM weighted_polls p JOIN category_stream_snapshots s ON s.poll_id = p.poll_id
    WHERE p.status = 'complete' AND (p.started_at >= $1 OR p.observed_seconds > 0)
    GROUP BY s.user_id, COALESCE(NULLIF(s.language, ''), 'und')
), language_totals AS (
    SELECT language, SUM(unique_streams)::bigint AS unique_streams,
           COUNT(*)::bigint AS unique_channels,
           SUM(seconds) / 3600.0 AS broadcast_hours,
           SUM(viewer_seconds) / 3600.0 AS viewer_hours,
           SUM(viewer_seconds) / NULLIF(SUM(seconds), 0.0) AS avg_viewers
    FROM channel_totals GROUP BY language
), ranked_channels AS (
    SELECT t.*, COALESCE(c.login, t.login) AS current_login,
           COALESCE(NULLIF(c.display_name, ''), c.login, t.login) AS display_name,
           ROW_NUMBER() OVER (PARTITION BY language ORDER BY viewer_seconds DESC, seconds DESC, t.user_id) AS rank
    FROM channel_totals t LEFT JOIN category_channels c ON c.user_id = t.user_id
), chat_hours AS MATERIALIZED (
    -- Abgeschlossene Stunden kommen ausschließlich aus dem dauerhaften Rollup.
    SELECT hour_bucket, COALESCE(NULLIF(lang, ''), 'und') AS language, messages
    FROM category_chat_rollup
    WHERE hour_bucket >= $1::timestamptz
      AND hour_bucket < date_trunc('hour', $2::timestamptz)
    UNION ALL
    -- Aktuelle Stunde direkt aggregieren, selbst wenn ein Teil-Rollup existiert.
    -- Shared-Chat-Weiterleitungen erzeugen keine zweite originale Aktivität.
    SELECT date_trunc('hour', sent_at), COALESCE(NULLIF(detected_lang, ''), 'und'), COUNT(*)::bigint
    FROM category_chat_messages
    WHERE sent_at >= date_trunc('hour', $2::timestamptz) AND sent_at < $2::timestamptz
      AND (source_message_id IS NULL OR source_message_id = message_id)
    GROUP BY 1, 2
), trend AS (
    SELECT date_trunc('hour', started_at) AS bucket_at,
           AVG(stream_count)::double precision AS avg_streams,
           AVG(viewer_total)::double precision AS avg_viewers,
           COUNT(*)::bigint AS poll_samples
    FROM poll_chain WHERE status = 'complete' AND started_at >= $1
    GROUP BY 1
), bounds AS (
    SELECT
      (SELECT started_at FROM category_polls WHERE status='complete' ORDER BY started_at LIMIT 1) AS first_seen_at,
      (SELECT completed_at FROM category_polls WHERE status='complete' ORDER BY started_at DESC LIMIT 1) AS last_complete_at,
      (SELECT MAX(sent_at) FROM category_chat_messages) AS last_message_at
)
SELECT jsonb_build_object(
    'meta', jsonb_build_object(
        'days', $3::integer, 'period_start', $1::timestamptz,
        'period_end', $2::timestamptz, 'timezone', 'UTC',
        'first_seen_at', bounds.first_seen_at,
        'last_completed_poll_at', bounds.last_complete_at,
        'last_message_at', bounds.last_message_at,
        'complete_polls', (SELECT COUNT(*) FROM poll_chain WHERE started_at >= $1 AND status='complete'),
        'incomplete_polls', (SELECT COUNT(*) FROM poll_chain WHERE started_at >= $1 AND
          (status IN ('failed','incomplete') OR (status='running' AND started_at < $2::timestamptz - make_interval(secs => 2 * cfg.discovery_interval_seconds + 30)))),
        'dropped_messages', state.chat_privmsgs_dropped + state.chat_queue_dropped,
        'dropped_roster_commands', state.chat_commands_dropped,
        'dropped_messages_scope', 'current_process',
        'storage_bytes', COALESCE((
            SELECT SUM(pg_total_relation_size(c.oid))::bigint
            FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
            WHERE n.nspname=current_schema() AND c.relkind IN ('r','m') AND left(c.relname,9)='category_'
        ),0),
        'retention_days', cfg.retention_days,
        'collector_status', CASE
            WHEN NOT cfg.enabled THEN 'disabled'
            WHEN state.last_error_at > COALESCE(bounds.last_complete_at, '-infinity'::timestamptz) THEN 'error'
            WHEN bounds.last_complete_at IS NULL THEN 'not_started'
            WHEN bounds.last_complete_at < $2::timestamptz - make_interval(secs => 2 * cfg.discovery_interval_seconds + 30)
              OR state.updated_at < $2::timestamptz - make_interval(secs => 2 * cfg.discovery_interval_seconds + 30) THEN 'stale'
            ELSE 'running' END,
        'measurement_seconds', COALESCE((SELECT SUM(observed_seconds) FROM weighted_polls),0.0)
    ),
    'stream_languages', COALESCE((SELECT jsonb_agg(to_jsonb(l) ORDER BY viewer_hours DESC, language) FROM language_totals l), '[]'::jsonb),
    'message_languages', COALESCE((SELECT jsonb_agg(to_jsonb(m) ORDER BY messages DESC, language) FROM (
        SELECT language, SUM(messages)::bigint AS messages FROM chat_hours GROUP BY language
    ) m), '[]'::jsonb),
    'trend', COALESCE((SELECT jsonb_agg(to_jsonb(t) ORDER BY bucket_at) FROM trend t), '[]'::jsonb),
    'top_channels', COALESCE((SELECT jsonb_agg(jsonb_build_object(
        'language', language, 'user_id', user_id, 'login', current_login,
        'display_name', display_name, 'broadcast_hours', seconds / 3600.0,
        'viewer_hours', viewer_seconds / 3600.0,
        'avg_viewers', viewer_seconds / NULLIF(seconds,0.0)
    ) ORDER BY viewer_seconds DESC, language, user_id) FROM ranked_channels WHERE rank <= 20), '[]'::jsonb),
    'chat_heatmap', COALESCE((SELECT jsonb_agg(to_jsonb(h) ORDER BY language, hour_utc) FROM (
        SELECT language, EXTRACT(HOUR FROM hour_bucket)::integer AS hour_utc,
               SUM(messages)::bigint AS messages
        FROM chat_hours GROUP BY language, EXTRACT(HOUR FROM hour_bucket)
    ) h), '[]'::jsonb)
) FROM cfg CROSS JOIN state CROSS JOIN bounds;
