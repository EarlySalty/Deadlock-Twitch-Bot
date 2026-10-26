WITH s AS (
  SELECT t.id, t.streamer_login, t.started_at, t.ended_at, t.duration_seconds,
         t.start_viewers, t.peak_viewers, t.end_viewers, t.avg_viewers, t.samples,
         t.retention_5m, t.retention_10m, t.retention_20m, t.dropoff_pct,
         t.unique_chatters, t.first_time_chatters, t.returning_chatters,
         t.followers_start, t.followers_end, t.follower_delta,
         t.stream_title, t.notification_text, t.game_name, t.had_deadlock_in_session,
         t.language, t.tags, t.twitch_user_id
  FROM twitch_stream_sessions t
  WHERE t.started_at >= '2026-01-09' AND t.started_at < '2026-10-25 17:28:00+00'
),
cv AS (
  SELECT v.session_id,
         min(v.mi) AS first_min,
         max(v.mi) AS last_min,
         avg(v.viewer_count) FILTER (WHERE v.mi = 0)  AS v0,
         avg(v.viewer_count) FILTER (WHERE v.mi = 5)  AS v5,
         avg(v.viewer_count) FILTER (WHERE v.mi = 10) AS v10,
         avg(v.viewer_count) FILTER (WHERE v.mi = 15) AS v15,
         avg(v.viewer_count) FILTER (WHERE v.mi = 30) AS v30,
         max(v.viewer_count) AS curve_max
  FROM (SELECT sv.session_id, sv.ts_utc, sv.viewer_count, floor(extract(epoch FROM (sv.ts_utc - ss.started_at)) / 60)::int AS mi FROM twitch_session_viewers sv JOIN twitch_stream_sessions ss ON ss.id = sv.session_id) v
  JOIN s ON s.id = v.session_id
  WHERE v.ts_utc >= '2026-01-09' AND v.ts_utc < '2026-10-25 17:28:00+00'
  GROUP BY v.session_id
),
tl AS (
  SELECT v.session_id,
         avg(v.viewer_count) FILTER (WHERE v.mi > (cv.last_min - 30)) AS last30_avg,
         avg(v.viewer_count) FILTER (WHERE v.mi > (cv.last_min - 10)) AS last10_avg,
         avg(v.viewer_count) FILTER (WHERE v.mi BETWEEN 15 AND 45)     AS mid_avg
  FROM (SELECT sv.session_id, sv.ts_utc, sv.viewer_count, floor(extract(epoch FROM (sv.ts_utc - ss.started_at)) / 60)::int AS mi FROM twitch_session_viewers sv JOIN twitch_stream_sessions ss ON ss.id = sv.session_id) v
  JOIN cv ON cv.session_id = v.session_id
  WHERE v.ts_utc >= '2026-01-09' AND v.ts_utc < '2026-10-25 17:28:00+00'
  GROUP BY v.session_id
),
ch AS (
  SELECT c.session_id,
         count(*) FILTER (WHERE NOT c.is_command)                          AS msgs15,
         count(DISTINCT c.chatter_login) FILTER (WHERE NOT c.is_command)   AS chatters15,
         count(*) FILTER (WHERE c.chatter_login = s2.streamer_login)       AS own15
  FROM twitch_chat_messages c
  JOIN s s2 ON s2.id = c.session_id
  WHERE c.message_ts >= s2.started_at
    AND c.message_ts < s2.started_at + interval '15 minutes'
    AND c.message_ts >= '2026-02-01'
  GROUP BY c.session_id
),
rd AS (
  SELECT s3.id AS session_id, count(*) AS raid_out_end
  FROM s s3
  JOIN twitch_raid_history r
    ON r.from_broadcaster_login = s3.streamer_login
   AND r.success
   AND r.executed_at >= s3.ended_at - interval '15 minutes'
   AND r.executed_at <= s3.ended_at + interval '5 minutes'
   AND r.executed_at >= '2026-01-09'
  GROUP BY s3.id
),
lg AS (
  SELECT s4.id,
         lag(s4.ended_at)   OVER w AS prev_ended_at,
         lag(s4.start_viewers) OVER w AS prev_start_viewers,
         lag(s4.avg_viewers)   OVER w AS prev_avg_viewers,
         lag(s4.first_time_chatters) OVER w AS prev_ftc,
         lag(s4.peak_viewers)  OVER w AS prev_peak_viewers,
         lag(s4.end_viewers)   OVER w AS prev_end_viewers,
         lead(s4.start_viewers) OVER w AS next_start_viewers,
         lead(s4.started_at)    OVER w AS next_started_at
  FROM s s4
  WINDOW w AS (PARTITION BY s4.streamer_login ORDER BY s4.started_at)
)
SELECT s.id, s.streamer_login, s.started_at, s.ended_at, s.duration_seconds,
       s.start_viewers, s.peak_viewers, s.end_viewers, s.avg_viewers, s.samples,
       s.retention_5m, s.retention_10m, s.retention_20m, s.dropoff_pct,
       s.unique_chatters, s.first_time_chatters, s.returning_chatters,
       s.followers_start, s.followers_end, s.follower_delta,
       replace(replace(coalesce(s.stream_title,''), E'\n', ' '), E'\t', ' ') AS stream_title,
       replace(replace(coalesce(s.notification_text,''), E'\n', ' '), E'\t', ' ') AS notification_text,
       coalesce(s.game_name,'') AS game_name, s.had_deadlock_in_session, s.language,
       coalesce(s.tags,'') AS tags,
       extract(hour FROM s.started_at AT TIME ZONE 'Europe/Berlin') AS start_hour_de,
       extract(isodow FROM s.started_at AT TIME ZONE 'Europe/Berlin') AS start_isodow,
       cv.first_min, cv.last_min, cv.v0, cv.v5, cv.v10, cv.v15, cv.v30, cv.curve_max,
       tl.last30_avg, tl.last10_avg, tl.mid_avg,
       ch.msgs15, ch.chatters15, ch.own15,
       coalesce(rd.raid_out_end, 0) AS raid_out_end,
       EXTRACT(EPOCH FROM (s.started_at - lg.prev_ended_at))/3600.0 AS gap_prev_h,
       lg.prev_start_viewers, lg.prev_avg_viewers, lg.prev_ftc, lg.prev_peak_viewers, lg.prev_end_viewers,
       lg.next_start_viewers,
       EXTRACT(EPOCH FROM (lg.next_started_at - s.ended_at))/3600.0 AS gap_next_h
FROM s
LEFT JOIN cv ON cv.session_id = s.id
LEFT JOIN tl ON tl.session_id = s.id
LEFT JOIN ch ON ch.session_id = s.id
LEFT JOIN rd ON rd.session_id = s.id
LEFT JOIN lg ON lg.id = s.id
ORDER BY s.streamer_login, s.started_at
;

SELECT t.id AS session_id,
       count(c.id) AS total_msgs_session,
       count(c.id) FILTER (WHERE c.message_ts < t.started_at + interval '15 minutes' AND NOT c.is_command) AS msgs15,
       count(DISTINCT c.chatter_login) FILTER (WHERE c.message_ts < t.started_at + interval '15 minutes' AND NOT c.is_command) AS chatters15,
       count(DISTINCT c.chatter_login) FILTER (WHERE c.message_ts < t.started_at + interval '15 minutes' AND NOT c.is_command AND c.chatter_login <> t.streamer_login) AS viewers_chatters15,
       count(DISTINCT c.chatter_login) AS chatters_total
FROM twitch_stream_sessions t
LEFT JOIN twitch_chat_messages c
  ON c.session_id = t.id AND c.message_ts >= t.started_at AND c.message_ts >= '2026-01-31'
WHERE t.started_at >= '2026-02-01' AND t.started_at < '2026-10-25 17:28:00+00'
GROUP BY t.id, t.started_at, t.streamer_login
;

WITH f AS (
  SELECT v.session_id, min(v.mi) AS first_min
  FROM (SELECT sv.session_id, sv.ts_utc, sv.viewer_count, floor(extract(epoch FROM (sv.ts_utc - ss.started_at)) / 60)::int AS mi FROM twitch_session_viewers sv JOIN twitch_stream_sessions ss ON ss.id = sv.session_id) v
  JOIN twitch_stream_sessions t ON t.id = v.session_id
  WHERE t.started_at >= '2026-08-01' AND t.started_at < '2026-10-25 17:28:00+00'
    AND v.ts_utc >= '2026-08-01' AND v.ts_utc < '2026-10-25 17:28:00+00'
  GROUP BY v.session_id
)
SELECT count(*) AS sessions_with_curve,
       round(100.0*avg((first_min > 5)::int),1) AS pct_first_after_5,
       round(100.0*avg((first_min > 15)::int),1) AS pct_first_after_15,
       round(100.0*avg((first_min <= 5)::int),1) AS pct_first_le_5
FROM f
;

WITH f AS (
  SELECT v.session_id, min(v.mi) AS first_min
  FROM (SELECT sv.session_id, sv.ts_utc, sv.viewer_count, floor(extract(epoch FROM (sv.ts_utc - ss.started_at)) / 60)::int AS mi FROM twitch_session_viewers sv JOIN twitch_stream_sessions ss ON ss.id = sv.session_id) v
  JOIN twitch_stream_sessions t ON t.id = v.session_id
  WHERE t.started_at >= '2026-01-01' AND t.started_at < '2026-10-25 17:28:00+00'
    AND v.ts_utc >= '2026-01-01' AND v.ts_utc < '2026-10-25 17:28:00+00'
  GROUP BY v.session_id
)
SELECT count(*) AS sessions_with_curve,
       round(100.0*avg((first_min > 5)::int),1) AS pct_first_after_5,
       round(100.0*avg((first_min > 15)::int),1) AS pct_first_after_15,
       round(100.0*avg((first_min <= 5)::int),1) AS pct_first_le_5
FROM f
;

WITH s AS (
  SELECT t.id, t.started_at, t.ended_at, t.streamer_login FROM twitch_stream_sessions t
  WHERE t.started_at >= '2026-01-09' AND t.started_at < '2026-10-25 17:28:00+00'
),
cm AS (
  SELECT v.session_id, max(v.viewer_count) AS curve_max, max(v.mi) AS last_min
  FROM (SELECT sv.session_id, sv.ts_utc, sv.viewer_count, floor(extract(epoch FROM (sv.ts_utc - ss.started_at)) / 60)::int AS mi FROM twitch_session_viewers sv JOIN twitch_stream_sessions ss ON ss.id = sv.session_id) v JOIN s ON s.id = v.session_id
  WHERE v.ts_utc >= '2026-01-09' AND v.ts_utc < '2026-10-25 17:28:00+00'
  GROUP BY v.session_id
),
hp AS (
  SELECT v.session_id, max(v.mi) AS half_last_min
  FROM (SELECT sv.session_id, sv.ts_utc, sv.viewer_count, floor(extract(epoch FROM (sv.ts_utc - ss.started_at)) / 60)::int AS mi FROM twitch_session_viewers sv JOIN twitch_stream_sessions ss ON ss.id = sv.session_id) v JOIN cm ON cm.session_id = v.session_id
  WHERE v.ts_utc >= '2026-01-09' AND v.ts_utc < '2026-10-25 17:28:00+00'
    AND v.viewer_count >= 0.5 * cm.curve_max
  GROUP BY v.session_id
),
lc AS (
  SELECT c.session_id, max(c.message_ts) AS last_chat_ts, count(*) FILTER (WHERE c.message_ts >= s.ended_at - interval '30 minutes') AS chat_last30
  FROM twitch_chat_messages c JOIN s ON s.id = c.session_id
  WHERE c.message_ts >= '2026-02-01' AND NOT c.is_command AND c.message_ts <= coalesce(s.ended_at, c.message_ts)
  GROUP BY c.session_id, s.ended_at
)
SELECT s.id AS session_id, cm.curve_max, cm.last_min, hp.half_last_min,
       extract(epoch FROM (s.ended_at - lc.last_chat_ts))/60.0 AS min_end_since_last_chat,
       lc.chat_last30
FROM s
LEFT JOIN cm ON cm.session_id = s.id
LEFT JOIN hp ON hp.session_id = s.id
LEFT JOIN lc ON lc.session_id = s.id
ORDER BY s.id
;

WITH s AS (
  SELECT t.id, t.ended_at, t.started_at FROM twitch_stream_sessions t
  WHERE t.started_at >= '2026-01-09' AND t.started_at < '2026-10-25 17:28:00+00' AND t.ended_at IS NOT NULL
),
cm AS (
  SELECT v.session_id, max(v.mi) AS last_min, max(v.viewer_count) AS curve_max
  FROM (SELECT sv.session_id, sv.ts_utc, sv.viewer_count, floor(extract(epoch FROM (sv.ts_utc - ss.started_at)) / 60)::int AS mi FROM twitch_session_viewers sv JOIN twitch_stream_sessions ss ON ss.id = sv.session_id) v JOIN s ON s.id = v.session_id
  WHERE v.ts_utc >= '2026-01-09' AND v.ts_utc < '2026-10-25 17:28:00+00'
  GROUP BY v.session_id
)
SELECT v.session_id, cm.curve_max, cm.last_min,
       floor((cm.last_min - v.mi) / 5.0)::int AS bin_from_end,
       avg(v.viewer_count) AS avg_vc, count(*) AS n
FROM (SELECT sv.session_id, sv.ts_utc, sv.viewer_count, floor(extract(epoch FROM (sv.ts_utc - ss.started_at)) / 60)::int AS mi FROM twitch_session_viewers sv JOIN twitch_stream_sessions ss ON ss.id = sv.session_id) v
JOIN cm ON cm.session_id = v.session_id
WHERE v.ts_utc >= '2026-01-09' AND v.ts_utc < '2026-10-25 17:28:00+00'
  AND cm.last_min - v.mi <= 60
  AND cm.last_min >= 60
GROUP BY v.session_id, cm.curve_max, cm.last_min, bin_from_end
ORDER BY v.session_id, bin_from_end
;

WITH g AS (
  SELECT x.id AS tid, x.exp_session_id, x.streamer, x.from_game, x.to_game, x.viewer_count AS vc_at_switch,
         x.ts_utc::timestamptz AS ts
  FROM exp_game_transitions x
  WHERE x.ts_utc::timestamptz >= '2026-03-01' AND x.ts_utc::timestamptz < '2026-10-25 17:28:00+00'
),
gs AS (
  SELECT g.*, t.id AS session_id, t.started_at AS s_start, t.ended_at AS s_end
  FROM g
  JOIN exp_sessions e ON e.id = g.exp_session_id
  JOIN twitch_stream_sessions t ON t.stream_id = e.stream_id
  WHERE t.started_at >= '2026-03-01'
    AND g.ts >= t.started_at AND g.ts <= coalesce(t.ended_at, g.ts)
)
SELECT gs.tid, gs.exp_session_id, gs.session_id, gs.streamer, gs.from_game, gs.to_game, gs.vc_at_switch,
       gs.ts, extract(epoch from (gs.ts - gs.s_start))/60.0 AS min_into_session,
       b.n_b, b.avg_b, a.n_a, a.avg_a, a2.n_a2, a2.avg_a2, b2.n_b2, b2.avg_b2
FROM gs
LEFT JOIN LATERAL (SELECT count(*) n_b, avg(v.viewer_count) avg_b FROM twitch_session_viewers v
  WHERE v.session_id = gs.session_id AND v.ts_utc >= gs.ts - interval '10 minutes' AND v.ts_utc < gs.ts) b ON true
LEFT JOIN LATERAL (SELECT count(*) n_a, avg(v.viewer_count) avg_a FROM twitch_session_viewers v
  WHERE v.session_id = gs.session_id AND v.ts_utc >= gs.ts AND v.ts_utc < gs.ts + interval '10 minutes') a ON true
LEFT JOIN LATERAL (SELECT count(*) n_a2, avg(v.viewer_count) avg_a2 FROM twitch_session_viewers v
  WHERE v.session_id = gs.session_id AND v.ts_utc >= gs.ts + interval '10 minutes' AND v.ts_utc < gs.ts + interval '30 minutes') a2 ON true
LEFT JOIN LATERAL (SELECT count(*) n_b2, avg(v.viewer_count) avg_b2 FROM twitch_session_viewers v
  WHERE v.session_id = gs.session_id AND v.ts_utc >= gs.ts - interval '30 minutes' AND v.ts_utc < gs.ts - interval '10 minutes') b2 ON true
ORDER BY gs.tid
;

WITH g AS (
  SELECT x.id AS tid, x.exp_session_id, x.streamer, x.from_game, x.to_game, x.viewer_count AS vc_at_switch,
         x.ts_utc::timestamptz AS ts, t.id AS session_id
  FROM exp_game_transitions x
  JOIN exp_sessions e ON e.id = x.exp_session_id
  JOIN twitch_stream_sessions t ON t.stream_id = e.stream_id
  WHERE x.ts_utc::timestamptz >= '2026-03-01' AND x.ts_utc::timestamptz < '2026-10-25 17:28:00+00'
    AND t.started_at >= '2026-03-01'
    AND x.ts_utc::timestamptz >= t.started_at AND x.ts_utc::timestamptz <= coalesce(t.ended_at, x.ts_utc::timestamptz)
)
SELECT g.tid, g.session_id,
       p1.n AS n_pm_b, p1.a AS avg_pm_b, p2.n AS n_pm_a, p2.a AS avg_pm_a,
       p3.n AS n_pp_b, p3.a AS avg_pp_b, p4.n AS n_pp_a, p4.a AS avg_pp_a
FROM g
LEFT JOIN LATERAL (SELECT count(*) n, avg(v.viewer_count) a FROM twitch_session_viewers v
  WHERE v.session_id = g.session_id AND v.ts_utc >= g.ts - interval '40 minutes' AND v.ts_utc < g.ts - interval '30 minutes') p1 ON true
LEFT JOIN LATERAL (SELECT count(*) n, avg(v.viewer_count) a FROM twitch_session_viewers v
  WHERE v.session_id = g.session_id AND v.ts_utc >= g.ts - interval '30 minutes' AND v.ts_utc < g.ts - interval '20 minutes') p2 ON true
LEFT JOIN LATERAL (SELECT count(*) n, avg(v.viewer_count) a FROM twitch_session_viewers v
  WHERE v.session_id = g.session_id AND v.ts_utc >= g.ts + interval '20 minutes' AND v.ts_utc < g.ts + interval '30 minutes') p3 ON true
LEFT JOIN LATERAL (SELECT count(*) n, avg(v.viewer_count) a FROM twitch_session_viewers v
  WHERE v.session_id = g.session_id AND v.ts_utc >= g.ts + interval '30 minutes' AND v.ts_utc < g.ts + interval '40 minutes') p4 ON true
ORDER BY g.tid
;

WITH tr AS (
  SELECT x.exp_session_id, count(*) AS n_tr,
         count(*) FILTER (WHERE x.from_game='Deadlock' OR x.to_game='Deadlock') AS n_tr_dl,
         (array_agg(x.from_game ORDER BY x.ts_utc::timestamptz))[1] AS first_from,
         (array_agg(x.to_game ORDER BY x.ts_utc::timestamptz))[1] AS first_to,
         min(x.ts_utc::timestamptz) AS first_ts
  FROM exp_game_transitions x GROUP BY x.exp_session_id
)
SELECT t.id AS session_id, t.streamer_login, t.started_at, e.game_name AS exp_game, e.id AS exp_id,
       coalesce(tr.n_tr,0) AS n_tr, coalesce(tr.n_tr_dl,0) AS n_tr_dl, tr.first_from, tr.first_to,
       extract(epoch FROM (tr.first_ts - t.started_at))/60.0 AS first_switch_min,
       e.duration_min AS exp_duration_min, e.samples AS exp_samples
FROM twitch_stream_sessions t
JOIN exp_sessions e ON e.stream_id = t.stream_id
LEFT JOIN tr ON tr.exp_session_id = e.id
WHERE t.started_at >= '2026-03-01' AND t.started_at < '2026-10-25 17:28:00+00'
;

