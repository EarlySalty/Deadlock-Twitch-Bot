WITH logged AS (
  SELECT ss.streamer_login, ss.started_at, ss.id
  FROM twitch_stream_sessions ss
  WHERE ss.started_at >= '2026-01-01'
    AND (EXISTS (SELECT 1 FROM twitch_session_chatters p WHERE p.session_id = ss.id)
         OR EXISTS (SELECT 1 FROM twitch_chat_messages m WHERE m.session_id = ss.id))
)
SELECT streamer_login, min(started_at) AS first_logged, max(started_at) AS last_logged, count(*) AS logged_sessions,
       (SELECT count(*) FROM twitch_stream_sessions s2 WHERE s2.streamer_login = logged.streamer_login AND s2.started_at >= '2026-01-01') AS all_sessions
FROM logged GROUP BY streamer_login;

SELECT id AS session_id, streamer_login, started_at, duration_seconds, avg_viewers,
       (EXISTS (SELECT 1 FROM twitch_chat_messages m WHERE m.session_id = s.id)) AS has_chat
FROM twitch_stream_sessions s
WHERE started_at >= '2026-06-25' AND started_at < '2026-10-26';

WITH fm AS (
  SELECT DISTINCT ON (streamer_login, chatter_login)
         streamer_login AS s, chatter_login AS c, id AS mid, session_id AS sid, message_ts AS t0
  FROM twitch_chat_messages
  WHERE message_ts >= '2026-07-01' AND message_ts < '2026-09-26'
  ORDER BY streamer_login, chatter_login, message_ts, id
),
fm2 AS (
  SELECT fm.* FROM fm
  WHERE NOT EXISTS (SELECT 1 FROM twitch_chat_messages x
                    WHERE x.streamer_login = fm.s AND x.chatter_login = fm.c AND x.message_ts < fm.t0)
),
pp AS (
  SELECT p.streamer_login AS s, p.chatter_login AS c, min(sp.started_at) AS first_pres
  FROM twitch_session_chatters p
  JOIN twitch_stream_sessions sp ON sp.id = p.session_id
  GROUP BY 1, 2
),
nc AS (
  SELECT fm2.s, fm2.c, fm2.mid, fm2.sid, fm2.t0,
         ss.started_at AS s_start,
         ss.started_at + make_interval(secs => coalesce(ss.duration_seconds, 0)) AS s_end,
         ss.unique_chatters, ss.peak_viewers, ss.avg_viewers,
         coalesce(pp.first_pres < ss.started_at, false) AS prior_silent
  FROM fm2
  JOIN twitch_stream_sessions ss ON ss.id = fm2.sid
  LEFT JOIN pp ON pp.s = fm2.s AND pp.c = fm2.c
)
, co AS (
  SELECT nc.s, nc.c, nc.sid, nc.t0, x.session_id AS xs, x.message_ts AS xt
  FROM nc
  JOIN twitch_chat_messages x ON x.streamer_login = nc.s AND x.chatter_login = nc.c
  WHERE x.message_ts > nc.t0 AND x.message_ts <= nc.t0 + interval '60 days'
    AND x.message_ts >= '2026-07-01' AND x.session_id <> nc.sid
),
coagg AS (
  SELECT s, c, sid, t0,
         count(DISTINCT xs) FILTER (WHERE xt <= t0 + interval '7 days') AS later_sess_7d,
         count(DISTINCT xs) FILTER (WHERE xt <= t0 + interval '30 days') AS later_sess_30d,
         count(DISTINCT xs) AS later_sess_60d,
         min(xt) AS first_return_ts
  FROM co GROUP BY s, c, sid, t0
),
pl AS (
  SELECT nc.s, nc.c, nc.sid, nc.t0,
         count(DISTINCT lp.session_id) FILTER (WHERE lp.started_at <= nc.t0 + interval '30 days') AS pres_later_sess_30d
  FROM nc
  JOIN (SELECT p.streamer_login AS s, p.chatter_login AS c, p.session_id, sp.started_at
        FROM twitch_session_chatters p
        JOIN twitch_stream_sessions sp ON sp.id = p.session_id
        WHERE sp.started_at >= '2026-07-01') lp
    ON lp.s = nc.s AND lp.c = nc.c AND lp.started_at > nc.s_start AND lp.started_at <= nc.t0 + interval '30 days'
  GROUP BY nc.s, nc.c, nc.sid, nc.t0
),
chan AS (
  SELECT streamer_login, percentile_cont(0.5) WITHIN GROUP (ORDER BY avg_viewers) AS med_avg_viewers,
         count(*) AS sessions_jul_oct
  FROM twitch_stream_sessions
  WHERE started_at >= '2026-07-01' AND started_at < '2026-10-26'
  GROUP BY streamer_login
)
SELECT nc.s AS streamer_login, nc.c AS chatter_login, nc.sid AS session_id, nc.t0, nc.prior_silent,
       nc.s_start, nc.s_end, nc.unique_chatters, nc.peak_viewers, nc.avg_viewers,
       chan.med_avg_viewers, chan.sessions_jul_oct,
       extract(epoch FROM nc.t0 - nc.s_start) / 60.0 AS min_into_stream,
       extract(epoch FROM nc.s_end - nc.t0) / 60.0 AS min_left_in_stream,
       coagg.later_sess_7d, coagg.later_sess_30d, coagg.later_sess_60d, coagg.first_return_ts,
       pl.pres_later_sess_30d
FROM nc
LEFT JOIN coagg ON coagg.s = nc.s AND coagg.c = nc.c AND coagg.sid = nc.sid
LEFT JOIN pl ON pl.s = nc.s AND pl.c = nc.c AND pl.sid = nc.sid
LEFT JOIN chan ON chan.streamer_login = nc.s
ORDER BY nc.t0;

WITH fm AS (
  SELECT DISTINCT ON (streamer_login, chatter_login)
         streamer_login AS s, chatter_login AS c, id AS mid, session_id AS sid, message_ts AS t0
  FROM twitch_chat_messages
  WHERE message_ts >= '2026-07-01' AND message_ts < '2026-09-26'
  ORDER BY streamer_login, chatter_login, message_ts, id
),
fm2 AS (
  SELECT fm.* FROM fm
  WHERE NOT EXISTS (SELECT 1 FROM twitch_chat_messages x
                    WHERE x.streamer_login = fm.s AND x.chatter_login = fm.c AND x.message_ts < fm.t0)
),
pp AS (
  SELECT p.streamer_login AS s, p.chatter_login AS c, min(sp.started_at) AS first_pres
  FROM twitch_session_chatters p
  JOIN twitch_stream_sessions sp ON sp.id = p.session_id
  GROUP BY 1, 2
),
nc AS (
  SELECT fm2.s, fm2.c, fm2.mid, fm2.sid, fm2.t0,
         ss.started_at AS s_start,
         ss.started_at + make_interval(secs => coalesce(ss.duration_seconds, 0)) AS s_end,
         ss.unique_chatters, ss.peak_viewers, ss.avg_viewers
  FROM fm2
  JOIN twitch_stream_sessions ss ON ss.id = fm2.sid
  LEFT JOIN pp ON pp.s = fm2.s AND pp.c = fm2.c
)
SELECT nc.s AS streamer_login, nc.c AS chatter_login, nc.sid AS session_id,
       count(y.id) FILTER (WHERE y.message_ts > nc.t0 - interval '5 minutes' AND y.message_ts <= nc.t0) AS msgs_pre5,
       bool_or(lower(y.chatter_login) = lower(nc.s) AND y.message_ts > nc.t0 AND y.message_ts <= nc.t0 + interval '3 minutes') AS bc_reply_3m,
       bool_or(lower(y.chatter_login) = lower(nc.s) AND y.message_ts > nc.t0 AND y.message_ts <= nc.t0 + interval '5 minutes') AS bc_reply_5m,
       bool_or(y.chatter_login <> nc.c AND y.message_ts > nc.t0 AND y.message_ts <= nc.t0 + interval '1 minute') AS others_1m,
       bool_or(y.chatter_login <> nc.c AND y.message_ts > nc.t0 AND y.message_ts <= nc.t0 + interval '10 minutes'
               AND y.content ~* ('(^|[^a-z0-9_])@?' || nc.c || '($|[^a-z0-9_])')) AS mention_10m
FROM nc
LEFT JOIN twitch_chat_messages y
  ON y.session_id = nc.sid AND y.message_ts > nc.t0 - interval '5 minutes' AND y.message_ts <= nc.t0 + interval '10 minutes'
GROUP BY nc.s, nc.c, nc.sid;

\echo first_label is read only for the burn-in 30 d rows used in section 6; their first messages all fall before 2026-10-05 00:00 UTC (latest 2026-09-25), where twitch_chat_message_labels is complete
WITH fm AS (
  SELECT DISTINCT ON (streamer_login, chatter_login)
         streamer_login AS s, chatter_login AS c, id AS mid, session_id AS sid, message_ts AS t0
  FROM twitch_chat_messages
  WHERE message_ts >= '2026-07-01' AND message_ts < '2026-09-26'
  ORDER BY streamer_login, chatter_login, message_ts, id
),
fm2 AS (
  SELECT fm.* FROM fm
  WHERE NOT EXISTS (SELECT 1 FROM twitch_chat_messages x
                    WHERE x.streamer_login = fm.s AND x.chatter_login = fm.c AND x.message_ts < fm.t0)
),
pp AS (
  SELECT p.streamer_login AS s, p.chatter_login AS c, min(sp.started_at) AS first_pres
  FROM twitch_session_chatters p
  JOIN twitch_stream_sessions sp ON sp.id = p.session_id
  GROUP BY 1, 2
),
nc AS (
  SELECT fm2.s, fm2.c, fm2.mid, fm2.sid, fm2.t0,
         ss.started_at AS s_start,
         ss.started_at + make_interval(secs => coalesce(ss.duration_seconds, 0)) AS s_end,
         ss.unique_chatters, ss.peak_viewers, ss.avg_viewers
  FROM fm2
  JOIN twitch_stream_sessions ss ON ss.id = fm2.sid
  LEFT JOIN pp ON pp.s = fm2.s AND pp.c = fm2.c
)
SELECT nc.s AS streamer_login, nc.c AS chatter_login, nc.sid AS session_id,
       (SELECT l.label FROM twitch_chat_message_labels l WHERE l.message_id = nc.mid) AS first_label,
       (SELECT count(*) FROM twitch_chat_messages x WHERE x.session_id = nc.sid AND x.streamer_login = nc.s AND x.chatter_login = nc.c) AS msgs_s0,
       (SELECT extract(epoch FROM max(x.message_ts) - min(x.message_ts)) / 60.0 FROM twitch_chat_messages x WHERE x.session_id = nc.sid AND x.streamer_login = nc.s AND x.chatter_login = nc.c) AS span_min_s0,
       (SELECT avg(v.viewer_count) FROM twitch_session_viewers v WHERE v.session_id = nc.sid AND v.ts_utc BETWEEN nc.t0 - interval '3 minutes' AND nc.t0 + interval '3 minutes') AS viewers_at_t0,
       EXISTS (SELECT 1 FROM twitch_greeted_chatters g WHERE g.streamer_login = nc.s AND g.chatter_login = nc.c
                 AND g.greeted_at BETWEEN nc.t0 - interval '2 minutes' AND nc.t0 + interval '5 minutes') AS greeted_near,
       EXISTS (SELECT 1 FROM twitch_follow_events f WHERE f.streamer_login = nc.s AND f.follower_login = nc.c AND f.followed_at <= nc.t0) AS followed_before,
       EXISTS (SELECT 1 FROM twitch_follow_events f WHERE f.streamer_login = nc.s AND f.follower_login = nc.c
                 AND f.followed_at > nc.t0 AND f.followed_at <= nc.t0 + interval '30 days') AS followed_after30,
       EXISTS (SELECT 1 FROM twitch_session_chatters p WHERE p.session_id = nc.sid AND p.chatter_login = nc.c AND p.is_first_time_streamer) AS flag_first_time
FROM nc
ORDER BY nc.t0;

WITH pres AS (
  SELECT p.streamer_login AS s, p.chatter_login AS c, p.session_id AS sid,
         coalesce(p.messages, 0) AS msgs, sp.started_at AS s_start,
         row_number() OVER (PARTITION BY p.streamer_login, p.chatter_login ORDER BY sp.started_at, p.session_id) AS rn
  FROM twitch_session_chatters p
  JOIN twitch_stream_sessions sp ON sp.id = p.session_id
  WHERE sp.started_at >= '2026-02-01' AND sp.started_at < '2026-09-26'
),
pp AS (
  SELECT p.streamer_login AS s, p.chatter_login AS c, min(sp.started_at) AS first_pres
  FROM twitch_session_chatters p JOIN twitch_stream_sessions sp ON sp.id = p.session_id
  GROUP BY 1, 2
),
pc AS (
  SELECT streamer_login AS s, chatter_login AS c, min(message_ts) AS first_chat
  FROM twitch_chat_messages
  GROUP BY 1, 2
),
nc AS (
  SELECT pres.* FROM pres
  LEFT JOIN pp ON pp.s = pres.s AND pp.c = pres.c
  LEFT JOIN pc ON pc.s = pres.s AND pc.c = pres.c
  WHERE pres.rn = 1
    AND (pp.first_pres IS NULL OR pp.first_pres >= pres.s_start)
    AND (pc.first_chat IS NULL OR pc.first_chat >= pres.s_start)
),
lp AS (
  SELECT p.streamer_login AS s, p.chatter_login AS c, p.session_id AS sid, sp.started_at
  FROM twitch_session_chatters p JOIN twitch_stream_sessions sp ON sp.id = p.session_id
  WHERE sp.started_at >= '2026-02-01'
),
pl AS (
  SELECT nc.s, nc.c, nc.sid, count(DISTINCT lp.sid) AS pres_later_30d
  FROM nc JOIN lp ON lp.s = nc.s AND lp.c = nc.c
   AND lp.started_at > nc.s_start AND lp.started_at <= nc.s_start + interval '30 days'
  GROUP BY nc.s, nc.c, nc.sid
),
cl AS (
  SELECT nc.s, nc.c, nc.sid, count(DISTINCT x.session_id) AS chat_later_30d
  FROM nc JOIN twitch_chat_messages x ON x.streamer_login = nc.s AND x.chatter_login = nc.c
   AND x.session_id <> nc.sid
   AND x.message_ts > nc.s_start AND x.message_ts <= nc.s_start + interval '30 days'
   AND x.message_ts >= '2026-02-01'
  GROUP BY nc.s, nc.c, nc.sid
)
SELECT nc.s AS streamer_login, nc.c AS chatter_login, nc.sid AS session_id, nc.s_start, nc.msgs AS msgs_first_session,
       coalesce(pl.pres_later_30d, 0) AS pres_later_30d,
       coalesce(cl.chat_later_30d, 0) AS chat_later_30d
FROM nc
LEFT JOIN pl ON pl.s = nc.s AND pl.c = nc.c AND pl.sid = nc.sid
LEFT JOIN cl ON cl.s = nc.s AND cl.c = nc.c AND cl.sid = nc.sid
ORDER BY nc.s_start;

WITH pres AS (
  SELECT p.streamer_login AS s, p.chatter_login AS c, p.session_id AS sid,
         coalesce(p.messages, 0) AS msgs, sp.started_at AS s_start,
         row_number() OVER (PARTITION BY p.streamer_login, p.chatter_login ORDER BY sp.started_at, p.session_id) AS rn
  FROM twitch_session_chatters p
  JOIN twitch_stream_sessions sp ON sp.id = p.session_id
  WHERE sp.started_at >= '2026-07-01' AND sp.started_at < '2026-09-26'
),
pp AS (
  SELECT p.streamer_login AS s, p.chatter_login AS c, min(sp.started_at) AS first_pres
  FROM twitch_session_chatters p JOIN twitch_stream_sessions sp ON sp.id = p.session_id
  GROUP BY 1, 2
),
pc AS (
  SELECT streamer_login AS s, chatter_login AS c, min(message_ts) AS first_chat
  FROM twitch_chat_messages
  GROUP BY 1, 2
),
nc AS (
  SELECT pres.* FROM pres
  LEFT JOIN pp ON pp.s = pres.s AND pp.c = pres.c
  LEFT JOIN pc ON pc.s = pres.s AND pc.c = pres.c
  WHERE pres.rn = 1
    AND (pp.first_pres IS NULL OR pp.first_pres >= pres.s_start)
    AND (pc.first_chat IS NULL OR pc.first_chat >= pres.s_start)
),
lp AS (
  SELECT p.streamer_login AS s, p.chatter_login AS c, p.session_id AS sid, sp.started_at
  FROM twitch_session_chatters p JOIN twitch_stream_sessions sp ON sp.id = p.session_id
  WHERE sp.started_at >= '2026-07-01'
),
pl AS (
  SELECT nc.s, nc.c, nc.sid, count(DISTINCT lp.sid) AS pres_later_30d
  FROM nc JOIN lp ON lp.s = nc.s AND lp.c = nc.c
   AND lp.started_at > nc.s_start AND lp.started_at <= nc.s_start + interval '30 days'
  GROUP BY nc.s, nc.c, nc.sid
),
cl AS (
  SELECT nc.s, nc.c, nc.sid, count(DISTINCT x.session_id) AS chat_later_30d
  FROM nc JOIN twitch_chat_messages x ON x.streamer_login = nc.s AND x.chatter_login = nc.c
   AND x.session_id <> nc.sid
   AND x.message_ts > nc.s_start AND x.message_ts <= nc.s_start + interval '30 days'
   AND x.message_ts >= '2026-07-01'
  GROUP BY nc.s, nc.c, nc.sid
)
SELECT nc.s AS streamer_login, nc.c AS chatter_login, nc.sid AS session_id, nc.s_start, nc.msgs AS msgs_first_session,
       coalesce(pl.pres_later_30d, 0) AS pres_later_30d,
       coalesce(cl.chat_later_30d, 0) AS chat_later_30d
FROM nc
LEFT JOIN pl ON pl.s = nc.s AND pl.c = nc.c AND pl.sid = nc.sid
LEFT JOIN cl ON cl.s = nc.s AND cl.c = nc.c AND cl.sid = nc.sid
ORDER BY nc.s_start;

WITH pres AS (
  SELECT p.streamer_login AS s, p.chatter_login AS c, p.session_id AS sid,
         coalesce(p.messages, 0) AS msgs, sp.started_at AS s_start,
         row_number() OVER (PARTITION BY p.streamer_login, p.chatter_login ORDER BY sp.started_at, p.session_id) AS rn
  FROM twitch_session_chatters p
  JOIN twitch_stream_sessions sp ON sp.id = p.session_id
  WHERE sp.started_at >= '2026-02-01' AND sp.started_at < '2026-09-26'
),
pp AS (
  SELECT p.streamer_login AS s, p.chatter_login AS c, min(sp.started_at) AS first_pres
  FROM twitch_session_chatters p JOIN twitch_stream_sessions sp ON sp.id = p.session_id
  GROUP BY 1, 2
),
pc AS (
  SELECT streamer_login AS s, chatter_login AS c, min(message_ts) AS first_chat
  FROM twitch_chat_messages
  GROUP BY 1, 2
),
nc AS (
  SELECT pres.* FROM pres
  LEFT JOIN pp ON pp.s = pres.s AND pp.c = pres.c
  LEFT JOIN pc ON pc.s = pres.s AND pc.c = pres.c
  WHERE pres.rn = 1
    AND (pp.first_pres IS NULL OR pp.first_pres >= pres.s_start)
    AND (pc.first_chat IS NULL OR pc.first_chat >= pres.s_start)
),
lp AS (
  SELECT p.streamer_login AS s, p.chatter_login AS c, p.session_id AS sid, sp.started_at
  FROM twitch_session_chatters p JOIN twitch_stream_sessions sp ON sp.id = p.session_id
  WHERE sp.started_at >= '2026-02-01'
),
pl AS (
  SELECT nc.s, nc.c, nc.sid, count(DISTINCT lp.sid) AS pres_later_30d
  FROM nc JOIN lp ON lp.s = nc.s AND lp.c = nc.c
   AND lp.started_at > nc.s_start AND lp.started_at <= nc.s_start + interval '30 days'
  GROUP BY nc.s, nc.c, nc.sid
),
cl AS (
  SELECT nc.s, nc.c, nc.sid, count(DISTINCT x.session_id) AS chat_later_30d
  FROM nc JOIN twitch_chat_messages x ON x.streamer_login = nc.s AND x.chatter_login = nc.c
   AND x.session_id <> nc.sid
   AND x.message_ts > nc.s_start AND x.message_ts <= nc.s_start + interval '30 days'
   AND x.message_ts >= '2026-02-01'
  GROUP BY nc.s, nc.c, nc.sid
)
SELECT nc.s AS streamer_login, nc.c AS chatter_login, nc.sid AS session_id, nc.s_start, nc.msgs AS msgs_first_session, EXISTS (SELECT 1 FROM twitch_session_chatters q WHERE q.session_id = nc.sid AND q.chatter_login = nc.c AND q.is_first_time_streamer) AS flag_first,
       coalesce(pl.pres_later_30d, 0) AS pres_later_30d,
       coalesce(cl.chat_later_30d, 0) AS chat_later_30d
FROM nc
LEFT JOIN pl ON pl.s = nc.s AND pl.c = nc.c AND pl.sid = nc.sid
LEFT JOIN cl ON cl.s = nc.s AND cl.c = nc.c AND cl.sid = nc.sid
ORDER BY nc.s_start;

SELECT sc.streamer_login, sc.chatter_login, sc.session_id, s.started_at, sc.messages, sc.seen_via_chatters_api
FROM twitch_session_chatters sc
JOIN twitch_stream_sessions s ON s.id = sc.session_id
WHERE s.started_at >= '2026-02-19' AND s.started_at < '2026-10-26';

WITH fm AS (
  SELECT DISTINCT ON (streamer_login, chatter_login) streamer_login, chatter_login, message_ts
  FROM twitch_chat_messages
  WHERE chatter_login IS NOT NULL
  ORDER BY streamer_login, chatter_login, message_ts
), bc AS (
  SELECT streamer_login, message_ts
  FROM twitch_chat_messages
  WHERE lower(chatter_login) = lower(streamer_login)
)
SELECT f.streamer_login,
       count(*) AS newcomers,
       count(*) FILTER (WHERE EXISTS (
         SELECT 1 FROM bc b
         WHERE b.streamer_login = f.streamer_login
           AND b.message_ts > f.message_ts
           AND b.message_ts <= f.message_ts + interval '3 minutes')) AS bc3
FROM fm f
GROUP BY f.streamer_login;

WITH m AS (
  SELECT room_user_id, chatter_user_id, chatter_login, sent_at, lower(message_text) AS txt,
         stream_language, (chatter_user_id = room_user_id) AS is_bc
  FROM category_chat_messages
  WHERE sent_at >= :'D'::timestamptz AND sent_at < :'D'::timestamptz + interval '1 day'
    AND NOT coalesce(shared_chat_copy, false)
), fp AS (
  SELECT room_user_id, chatter_user_id, min(chatter_login) AS login, min(sent_at) AS first_ts,
         count(*) AS n, max(stream_language) AS lang
  FROM m WHERE NOT is_bc GROUP BY 1, 2
), bc AS (
  SELECT room_user_id, sent_at, txt FROM m WHERE is_bc
)
SELECT :'D'::date AS day, f.room_user_id, f.chatter_user_id, f.login, f.first_ts, f.n, f.lang,
       coalesce(bool_or(strpos(b.txt, '@' || lower(f.login)) > 0) FILTER (WHERE b.sent_at > f.first_ts), false) AS mention,
       coalesce(bool_or(true) FILTER (WHERE b.sent_at > f.first_ts AND b.sent_at <= f.first_ts + interval '2 minutes'), false) AS bc2
FROM fp f LEFT JOIN bc b ON b.room_user_id = f.room_user_id
GROUP BY f.room_user_id, f.chatter_user_id, f.login, f.first_ts, f.n, f.lang;

-- Live days per room in the Deadlock category (all languages), for p06-live-days.py
SELECT user_id, snapshot_at::date AS day, count(*) AS samples
FROM category_snapshots_normalized
WHERE snapshot_at >= '2026-10-09'
GROUP BY 1, 2;

SELECT count(*), min(message_ts)::date, max(message_ts)::date, count(distinct streamer_login), count(distinct session_id) FROM twitch_chat_messages;

SELECT count(*), count(distinct streamer_login), count(distinct session_id), count(*) FILTER (WHERE seen_via_chatters_api AND messages = 0) FROM twitch_session_chatters;
