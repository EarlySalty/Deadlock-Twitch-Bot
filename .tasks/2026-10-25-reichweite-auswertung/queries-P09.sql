WITH raids AS (
  SELECT h.id AS raid_id, h.executed_at AS t, lower(h.to_broadcaster_login) AS tl,
         h.to_broadcaster_id AS tid, h.from_broadcaster_login AS frm, h.viewer_count AS sent, h.reason
  FROM twitch_raid_history h
  WHERE h.success AND h.executed_at >= '2026-02-01' AND h.executed_at < '2026-10-25 17:00'
),
ev AS (
  SELECT r.raid_id, 'raid'::text AS kind, 0 AS k, r.t, r.tl, r.tid, r.sent, r.frm FROM raids r
  UNION ALL
  SELECT r.raid_id, 'ctl', k.k, r.t + make_interval(days => 7 * k.k), r.tl, r.tid, r.sent, r.frm
  FROM raids r CROSS JOIN (VALUES (-2),(-1),(1),(2)) AS k(k)
),
pt AS (
  SELECT e.*, s.id AS sid, s.started_at, s.ended_at,
         EXTRACT(EPOCH FROM (e.t - s.started_at)) / 60.0 AS min_into,
         (s.ended_at >= e.t + interval '60 minutes') AS covers60
  FROM ev e
  LEFT JOIN LATERAL (
    SELECT s.id, s.started_at, s.ended_at FROM twitch_stream_sessions s
    WHERE lower(s.streamer_login) = e.tl AND e.t >= s.started_at AND e.t <= COALESCE(s.ended_at, now())
    ORDER BY s.started_at DESC LIMIT 1
  ) s ON TRUE
),
v AS (
  SELECT pt.*,
    v0.viewer_count AS v0, v0.ts_utc AS v0_ts,
    v15.viewer_count AS v15, v15.ts_utc AS v15_ts,
    v30.viewer_count AS v30, v30.ts_utc AS v30_ts,
    v60.viewer_count AS v60, v60.ts_utc AS v60_ts,
    w.avg60, w.n60
  FROM pt
  LEFT JOIN LATERAL (SELECT x.viewer_count, x.ts_utc FROM twitch_session_viewers x WHERE x.session_id = pt.sid AND x.ts_utc <= pt.t ORDER BY x.ts_utc DESC LIMIT 1) v0 ON TRUE
  LEFT JOIN LATERAL (SELECT x.viewer_count, x.ts_utc FROM twitch_session_viewers x WHERE x.session_id = pt.sid AND x.ts_utc <= pt.t + interval '15 minutes' ORDER BY x.ts_utc DESC LIMIT 1) v15 ON TRUE
  LEFT JOIN LATERAL (SELECT x.viewer_count, x.ts_utc FROM twitch_session_viewers x WHERE x.session_id = pt.sid AND x.ts_utc <= pt.t + interval '30 minutes' ORDER BY x.ts_utc DESC LIMIT 1) v30 ON TRUE
  LEFT JOIN LATERAL (SELECT x.viewer_count, x.ts_utc FROM twitch_session_viewers x WHERE x.session_id = pt.sid AND x.ts_utc <= pt.t + interval '60 minutes' ORDER BY x.ts_utc DESC LIMIT 1) v60 ON TRUE
  LEFT JOIN LATERAL (SELECT AVG(x.viewer_count)::float AS avg60, COUNT(*) AS n60 FROM twitch_session_viewers x WHERE x.session_id = pt.sid AND x.ts_utc >= pt.t AND x.ts_utc < pt.t + interval '60 minutes') w ON TRUE
),
c AS (
  SELECT v.*,
    (SELECT COUNT(*) FROM twitch_follow_events f WHERE f.streamer_login = v.tl AND f.followed_at >= v.t AND f.followed_at < v.t + interval '30 minutes') AS fol30,
    (SELECT COUNT(*) FROM twitch_follow_events f WHERE f.streamer_login = v.tl AND f.followed_at >= v.t AND f.followed_at < v.t + interval '60 minutes') AS fol60,
    (SELECT COUNT(*) FROM twitch_raid_history h2 WHERE h2.success AND h2.to_broadcaster_id = v.tid AND h2.executed_at BETWEEN v.t - interval '60 minutes' AND v.t + interval '60 minutes' AND (h2.id <> v.raid_id OR h2.executed_at <> v.t)) AS other_raids_pm60
  FROM v
)
SELECT raid_id, kind, k, to_char(t, 'YYYY-MM-DD"T"HH24:MI:SS') AS t_utc, tid, frm, sent, sid AS session_id,
       ROUND(min_into::numeric, 2) AS min_into, covers60, v0, v0_ts >= t - interval '2 minutes' AS v0_fresh,
       v15, v15_ts >= t + interval '13 minutes' AS v15_fresh, v30, v30_ts >= t + interval '28 minutes' AS v30_fresh,
       v60, v60_ts >= t + interval '58 minutes' AS v60_fresh, ROUND(avg60::numeric,2) AS avg60, n60,
       fol30, fol60, other_raids_pm60
FROM c
ORDER BY raid_id, kind, k;

SELECT lower(s.streamer_login) AS streamer, s.id AS session_id, s.twitch_user_id, s.started_at, s.ended_at,
       s.duration_seconds, s.avg_viewers, s.peak_viewers, s.start_viewers, s.end_viewers, s.samples,
       s.retention_10m, s.unique_chatters, s.first_time_chatters, s.returning_chatters,
       s.followers_start, s.followers_end, s.follower_delta, s.language, s.game_name, s.had_deadlock_in_session
FROM twitch_stream_sessions s
WHERE s.started_at >= '2026-01-09'
ORDER BY s.started_at;

SELECT c.session_id, lower(c.streamer_login) AS streamer, lower(c.chatter_login) AS chatter, c.chatter_id,
       c.first_message_at, c.messages, c.seen_via_chatters_api, c.is_first_time_streamer
FROM twitch_session_chatters c
WHERE c.session_id IN (SELECT id FROM twitch_stream_sessions WHERE started_at >= '2026-01-09')
ORDER BY c.session_id;

SELECT lower(f.streamer_login) AS streamer, f.twitch_user_id, f.follower_id, f.followed_at
FROM twitch_follow_events f
ORDER BY f.followed_at;

SELECT lower(twitch_login) AS login, 'streamers' AS src FROM twitch_streamers WHERE twitch_login IS NOT NULL
UNION ALL SELECT lower(twitch_login), 'partners' FROM twitch_partners WHERE twitch_login IS NOT NULL
UNION ALL SELECT lower(twitch_login), CASE WHEN raid_enabled THEN 'raid_auth_enabled' ELSE 'raid_auth_disabled' END FROM twitch_raid_auth WHERE twitch_login IS NOT NULL
ORDER BY 1, 2;

SELECT s.id AS session_id, s.twitch_user_id,
       COALESCE(m.n, 0) AS msgs, COALESCE(m.sc, 0) AS shared_copies, COALESCE(m.ch, 0) AS chatters_nonshared
FROM twitch_stream_sessions s
LEFT JOIN LATERAL (
  SELECT COUNT(*) AS n,
         COUNT(*) FILTER (WHERE x.shared_chat_copy) AS sc,
         COUNT(DISTINCT x.chatter_user_id) FILTER (WHERE NOT x.shared_chat_copy) AS ch
  FROM category_chat_messages x
  WHERE x.room_user_id = s.twitch_user_id
    AND x.sent_at >= s.started_at AND x.sent_at < COALESCE(s.ended_at, now())
    AND x.sent_at >= '2026-09-27'
) m ON TRUE
WHERE s.started_at >= '2026-09-27' AND s.twitch_user_id IS NOT NULL
ORDER BY s.id;

SELECT count(*), count(*) FILTER (WHERE success), count(DISTINCT (id, executed_at)), count(DISTINCT id)
FROM twitch_raid_history
WHERE executed_at >= '2026-02-01' AND executed_at < '2026-10-25 17:00';

SELECT count(*)
FROM (
  SELECT id FROM twitch_raid_history
  WHERE success AND executed_at >= '2026-02-01' AND executed_at < '2026-10-25 17:00'
  GROUP BY id HAVING count(DISTINCT executed_at) > 1
) x;

SELECT count(*), count(*) FILTER (WHERE h.success), count(*) FILTER (WHERE h.success IS DISTINCT FROM true)
FROM twitch_raid_retention r
LEFT JOIN twitch_raid_history h ON h.id = r.raid_id AND h.executed_at = r.executed_at;

SELECT count(*) FROM partner_effort_shared_chat_observations;

SELECT count(*), count(*) FILTER (WHERE c.messages = 0), count(*) FILTER (WHERE c.messages >= 1),
       count(*) FILTER (WHERE c.messages IS NULL), min(s.started_at) FILTER (WHERE c.messages = 0)
FROM twitch_session_chatters c
JOIN twitch_stream_sessions s ON s.id = c.session_id
WHERE s.started_at >= '2026-02-01';

SELECT count(*), count(*) FILTER (WHERE total_messages = 0)
FROM twitch_chatter_rollup;

SELECT count(*), count(DISTINCT lower(streamer_login)), min(followed_at), max(followed_at)
FROM twitch_follow_events;

SELECT date_trunc('month', s.started_at), count(*), count(*) FILTER (WHERE s.unique_chatters = 0),
       count(*) FILTER (WHERE s.unique_chatters IS NULL)
FROM twitch_stream_sessions s
WHERE s.started_at >= '2026-01-01'
GROUP BY 1 ORDER BY 1;

SELECT date_trunc('month', s.started_at), min(s.follower_delta), max(s.follower_delta),
       count(*) FILTER (WHERE s.follower_delta IS NULL)
FROM twitch_stream_sessions s
WHERE s.started_at >= '2026-01-01'
GROUP BY 1 ORDER BY 1;

SELECT count(*), count(*) FILTER (WHERE s.twitch_user_id IS NULL)
FROM twitch_stream_sessions s
WHERE s.started_at >= '2026-01-09';
