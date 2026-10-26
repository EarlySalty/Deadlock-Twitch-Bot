SELECT 'chat_messages_window' AS item, count(*)::text AS n, count(DISTINCT streamer_login)::text AS streamers,
  count(DISTINCT session_id)::text AS sessions, count(DISTINCT chatter_login)::text AS chatters,
  min(message_ts)::text AS first_ts, max(message_ts)::text AS last_ts
FROM twitch_chat_messages WHERE message_ts >= '2026-02-01' AND message_ts < '2026-10-05'
UNION ALL
SELECT 'labelled_messages_window', count(*)::text, count(DISTINCT m.streamer_login)::text, count(DISTINCT m.session_id)::text,
  count(DISTINCT m.chatter_login)::text, min(m.message_ts)::text, max(m.message_ts)::text
FROM twitch_chat_messages m JOIN twitch_chat_message_labels l ON l.message_id = m.id
WHERE m.message_ts >= '2026-02-01' AND m.message_ts < '2026-10-05'
UNION ALL
SELECT 'word_groups_rows', count(*)::text, NULL, NULL, NULL, NULL, NULL FROM twitch_chat_word_groups
UNION ALL
SELECT 'category_chat_de_window', count(*)::text, count(DISTINCT room_user_id)::text, NULL, count(DISTINCT chatter_user_id)::text,
  min(sent_at)::text, max(sent_at)::text
FROM category_chat_messages
WHERE sent_at >= '2026-09-27' AND sent_at < '2026-10-25' AND stream_language LIKE 'de%' AND NOT shared_chat_copy AND chatter_user_id <> room_user_id
;

WITH per_login AS (
  SELECT chatter_login, count(DISTINCT streamer_login) AS channels, count(*) AS msgs
  FROM twitch_chat_messages
  WHERE message_ts >= '2026-02-01' AND message_ts < '2026-10-05'
  GROUP BY chatter_login
)
SELECT
  count(*) AS logins_total,
  count(*) FILTER (WHERE channels >= 30) AS logins_30plus_channels,
  sum(msgs) FILTER (WHERE channels >= 30) AS msgs_from_30plus,
  sum(msgs) AS msgs_total,
  round(100.0 * sum(msgs) FILTER (WHERE channels >= 30) / sum(msgs), 2) AS pct_msgs_from_30plus,
  count(*) FILTER (WHERE channels >= 30 AND (chatter_login LIKE '%bot' OR chatter_login IN ('streamelements','streamlabs'))) AS of_which_name_pattern_bots,
  max(channels) AS max_channels
FROM per_login
;

WITH msg AS (
  SELECT m.id, m.session_id, m.streamer_login, m.chatter_login, m.message_ts,
    CASE WHEN m.chatter_login = m.streamer_login THEN 'broadcaster'
         WHEN m.chatter_login LIKE '%bot' OR m.chatter_login IN ('streamelements','streamlabs') THEN 'bot'
         ELSE 'viewer' END AS role
  FROM twitch_chat_messages m
  WHERE m.message_ts >= '2026-02-01' AND m.message_ts < '2026-10-05'
),
firsts AS (
  SELECT msg.*, (message_ts = min(message_ts) OVER (PARTITION BY session_id, chatter_login)) AS is_first
  FROM msg
),
win AS (
  SELECT f.id, f.session_id, f.streamer_login, f.chatter_login, f.role, f.message_ts,
    count(*) FILTER (WHERE role <> 'bot') OVER s2 - count(*) FILTER (WHERE role <> 'bot') OVER c2 AS f2,
    count(*) FILTER (WHERE role <> 'bot') OVER s5 - count(*) FILTER (WHERE role <> 'bot') OVER c5 AS f5,
    count(*) FILTER (WHERE is_first AND role <> 'bot') OVER s5 AS new5,
    count(*) FILTER (WHERE role = 'broadcaster') OVER s2 - count(*) FILTER (WHERE role = 'broadcaster') OVER c2 AS bc2
  FROM firsts f
  WINDOW
    s2 AS (PARTITION BY session_id ORDER BY message_ts RANGE BETWEEN INTERVAL '1 microsecond' FOLLOWING AND INTERVAL '2 minutes' FOLLOWING),
    c2 AS (PARTITION BY session_id, chatter_login ORDER BY message_ts RANGE BETWEEN INTERVAL '1 microsecond' FOLLOWING AND INTERVAL '2 minutes' FOLLOWING),
    s5 AS (PARTITION BY session_id ORDER BY message_ts RANGE BETWEEN INTERVAL '1 microsecond' FOLLOWING AND INTERVAL '5 minutes' FOLLOWING),
    c5 AS (PARTITION BY session_id, chatter_login ORDER BY message_ts RANGE BETWEEN INTERVAL '1 microsecond' FOLLOWING AND INTERVAL '5 minutes' FOLLOWING)
),
scored AS (
  SELECT w.*, l.label
  FROM win w JOIN twitch_chat_message_labels l ON l.message_id = w.id
),
sess_all AS (
  SELECT session_id, streamer_login, count(*) AS n_all, avg(f2) AS f2_all, avg(f5) AS f5_all,
         avg((f2 > 0)::int) AS r2_all, avg(new5) AS new5_all
  FROM scored GROUP BY 1, 2 HAVING count(*) >= 30
),
sess_type AS (
  SELECT s.session_id, s.streamer_login, s.role, s.label, count(*) AS n,
         avg(s.f2) AS f2, avg(s.f5) AS f5, avg((s.f2 > 0)::int) AS r2, avg(s.new5) AS new5
  FROM scored s JOIN sess_all a ON a.session_id = s.session_id
  GROUP BY 1, 2, 3, 4 HAVING count(*) >= 3
),
lift AS (
  SELECT t.streamer_login, t.role, t.label, t.session_id, t.n,
         t.f2 / NULLIF(a.f2_all, 0) AS lift_f2, t.f5 / NULLIF(a.f5_all, 0) AS lift_f5,
         t.r2 / NULLIF(a.r2_all, 0) AS lift_r2, t.new5 / NULLIF(a.new5_all, 0) AS lift_new5
  FROM sess_type t JOIN sess_all a ON a.session_id = t.session_id
),
streamer_lift AS (
  SELECT streamer_login, role, label, count(*) AS sessions, sum(n) AS msgs,
    percentile_cont(0.5) WITHIN GROUP (ORDER BY lift_f2) AS s_lift_f2,
    percentile_cont(0.5) WITHIN GROUP (ORDER BY lift_f5) AS s_lift_f5,
    percentile_cont(0.5) WITHIN GROUP (ORDER BY lift_r2) AS s_lift_r2,
    percentile_cont(0.5) WITHIN GROUP (ORDER BY lift_new5) AS s_lift_new5
  FROM lift WHERE lift_f2 IS NOT NULL AND lift_f5 IS NOT NULL AND lift_r2 IS NOT NULL AND lift_new5 IS NOT NULL
  GROUP BY 1, 2, 3
)
SELECT role, label, count(*) AS streamers, sum(sessions) AS sessions, sum(msgs) AS msgs,
  round(percentile_cont(0.5) WITHIN GROUP (ORDER BY s_lift_f2)::numeric, 2) AS med_lift_f2_2m,
  round(avg((s_lift_f2 > 1)::int)::numeric, 2) AS share_str_gt1_f2,
  round(percentile_cont(0.5) WITHIN GROUP (ORDER BY s_lift_f5)::numeric, 2) AS med_lift_f5_5m,
  round(avg((s_lift_f5 > 1)::int)::numeric, 2) AS share_str_gt1_f5,
  round(percentile_cont(0.5) WITHIN GROUP (ORDER BY s_lift_r2)::numeric, 2) AS med_lift_reply_share_2m,
  round(percentile_cont(0.5) WITHIN GROUP (ORDER BY s_lift_new5)::numeric, 2) AS med_lift_new_chatters_5m
FROM streamer_lift
GROUP BY 1, 2
HAVING count(*) >= 5
ORDER BY 1, 2
;

WITH multi30 AS (SELECT chatter_login FROM twitch_chat_messages WHERE message_ts >= '2026-02-01' AND message_ts < '2026-10-05' GROUP BY chatter_login HAVING count(DISTINCT streamer_login) >= 30), msg AS (
  SELECT m.id, m.session_id, m.streamer_login, m.chatter_login, m.message_ts,
    CASE WHEN m.chatter_login = m.streamer_login THEN 'broadcaster'
         WHEN m.chatter_login IN (SELECT chatter_login FROM multi30) OR m.chatter_login LIKE '%bot' OR m.chatter_login IN ('streamelements','streamlabs') THEN 'bot'
         ELSE 'viewer' END AS role,
    l.label
  FROM twitch_chat_messages m
  JOIN twitch_chat_message_labels l ON l.message_id = m.id
  WHERE m.message_ts >= '2026-02-01' AND m.message_ts < '2026-10-05'
),
chan AS (
  SELECT msg.*, (message_ts = min(message_ts) OVER (PARTITION BY streamer_login, chatter_login)) AS is_chan_first,
    min(message_ts) OVER (PARTITION BY session_id) AS sess_start
  FROM msg
),
ctx AS (
  SELECT c.*,
    count(*) FILTER (WHERE role <> 'bot') OVER p3 AS hum3,
    count(*) FILTER (WHERE role = 'broadcaster') OVER p3 AS bc3,
    count(*) FILTER (WHERE role = 'broadcaster') OVER p1 AS bc1,
    count(*) FILTER (WHERE role <> 'bot' AND label = 'Question') OVER p3 AS q3,
    count(*) FILTER (WHERE role <> 'bot' AND label = 'Social') OVER p3 AS soc3,
    count(*) FILTER (WHERE role <> 'bot' AND label = 'Hype') OVER p3 AS hype3,
    count(*) FILTER (WHERE role <> 'bot' AND label = 'Game-Related') OVER p3 AS game3,
    count(*) FILTER (WHERE role <> 'bot' AND label = 'Greeting') OVER p3 AS greet3,
    count(*) FILTER (WHERE role <> 'bot' AND label = 'Feedback') OVER p3 AS fb3,
    count(*) FILTER (WHERE role <> 'bot' AND label = 'Command') OVER p3 AS cmd3,
    count(*) FILTER (WHERE role <> 'bot' AND label = 'Statement') OVER p3 AS stmt3,
    count(*) FILTER (WHERE role <> 'bot' AND label = 'Reaction') OVER p3 AS react3
  FROM chan c
  WINDOW
    p3 AS (PARTITION BY session_id ORDER BY message_ts RANGE BETWEEN INTERVAL '3 minutes' PRECEDING AND INTERVAL '1 microsecond' PRECEDING),
    p1 AS (PARTITION BY session_id ORDER BY message_ts RANGE BETWEEN INTERVAL '1 minute' PRECEDING AND INTERVAL '1 microsecond' PRECEDING)
),
sel AS (
  SELECT session_id, streamer_login, 'newcomer' AS grp, hum3::float / 3 AS speed,
    (bc3 > 0)::int AS bc3f, (bc1 > 0)::int AS bc1f,
    q3::float / NULLIF(hum3, 0) AS sh_question, soc3::float / NULLIF(hum3, 0) AS sh_social,
    hype3::float / NULLIF(hum3, 0) AS sh_hype, game3::float / NULLIF(hum3, 0) AS sh_game,
    greet3::float / NULLIF(hum3, 0) AS sh_greeting, fb3::float / NULLIF(hum3, 0) AS sh_feedback,
    cmd3::float / NULLIF(hum3, 0) AS sh_command, stmt3::float / NULLIF(hum3, 0) AS sh_statement,
    react3::float / NULLIF(hum3, 0) AS sh_reaction
  FROM ctx WHERE role = 'viewer' AND is_chan_first AND message_ts >= '2026-03-01' AND message_ts >= sess_start + interval '10 minutes'
  UNION ALL
  SELECT session_id, streamer_login, 'regular' AS grp, hum3::float / 3 AS speed,
    (bc3 > 0)::int AS bc3f, (bc1 > 0)::int AS bc1f,
    q3::float / NULLIF(hum3, 0), soc3::float / NULLIF(hum3, 0),
    hype3::float / NULLIF(hum3, 0), game3::float / NULLIF(hum3, 0),
    greet3::float / NULLIF(hum3, 0), fb3::float / NULLIF(hum3, 0),
    cmd3::float / NULLIF(hum3, 0), stmt3::float / NULLIF(hum3, 0),
    react3::float / NULLIF(hum3, 0)
  FROM ctx WHERE role = 'viewer' AND NOT is_chan_first AND message_ts >= sess_start + interval '10 minutes'
),
sess_grp AS (
  SELECT session_id, streamer_login, grp, count(*) AS n, avg(speed) AS speed, avg(bc3f) AS bc3, avg(bc1f) AS bc1,
    avg(sh_question) AS sh_question, avg(sh_social) AS sh_social, avg(sh_hype) AS sh_hype, avg(sh_game) AS sh_game,
    avg(sh_greeting) AS sh_greeting, avg(sh_feedback) AS sh_feedback, avg(sh_command) AS sh_command,
    avg(sh_statement) AS sh_statement, avg(sh_reaction) AS sh_reaction
  FROM sel GROUP BY 1, 2, 3
),
sess AS (
  SELECT session_id, streamer_login, min(message_ts) AS s0, max(message_ts) AS s1
  FROM twitch_chat_messages
  WHERE message_ts >= '2026-02-01' AND message_ts < '2026-10-05'
  GROUP BY 1, 2
  HAVING max(message_ts) - min(message_ts) >= interval '20 minutes'
),
grid AS (
  SELECT s.session_id, s.streamer_login, g AS mt
  FROM sess s CROSS JOIN LATERAL generate_series(date_trunc('minute', s.s0), date_trunc('minute', s.s1), interval '1 minute') g
),
mb AS (
  SELECT m.session_id, date_trunc('minute', m.message_ts) AS mt,
    count(*) FILTER (WHERE m.chatter_login NOT LIKE '%bot' AND m.chatter_login NOT IN ('streamelements','streamlabs') AND m.chatter_login NOT IN (SELECT chatter_login FROM multi30)) AS hum,
    count(*) FILTER (WHERE m.chatter_login = m.streamer_login) AS bc,
    count(*) FILTER (WHERE l.label = 'Question' AND m.chatter_login NOT LIKE '%bot' AND m.chatter_login NOT IN ('streamelements','streamlabs') AND m.chatter_login NOT IN (SELECT chatter_login FROM multi30)) AS q,
    count(*) FILTER (WHERE l.label = 'Social' AND m.chatter_login NOT LIKE '%bot' AND m.chatter_login NOT IN ('streamelements','streamlabs') AND m.chatter_login NOT IN (SELECT chatter_login FROM multi30)) AS soc,
    count(*) FILTER (WHERE l.label = 'Hype' AND m.chatter_login NOT LIKE '%bot' AND m.chatter_login NOT IN ('streamelements','streamlabs') AND m.chatter_login NOT IN (SELECT chatter_login FROM multi30)) AS hype,
    count(*) FILTER (WHERE l.label = 'Game-Related' AND m.chatter_login NOT LIKE '%bot' AND m.chatter_login NOT IN ('streamelements','streamlabs') AND m.chatter_login NOT IN (SELECT chatter_login FROM multi30)) AS game,
    count(*) FILTER (WHERE l.label = 'Greeting' AND m.chatter_login NOT LIKE '%bot' AND m.chatter_login NOT IN ('streamelements','streamlabs') AND m.chatter_login NOT IN (SELECT chatter_login FROM multi30)) AS greet,
    count(*) FILTER (WHERE l.label = 'Feedback' AND m.chatter_login NOT LIKE '%bot' AND m.chatter_login NOT IN ('streamelements','streamlabs') AND m.chatter_login NOT IN (SELECT chatter_login FROM multi30)) AS fb,
    count(*) FILTER (WHERE l.label = 'Command' AND m.chatter_login NOT LIKE '%bot' AND m.chatter_login NOT IN ('streamelements','streamlabs') AND m.chatter_login NOT IN (SELECT chatter_login FROM multi30)) AS cmd,
    count(*) FILTER (WHERE l.label = 'Statement' AND m.chatter_login NOT LIKE '%bot' AND m.chatter_login NOT IN ('streamelements','streamlabs') AND m.chatter_login NOT IN (SELECT chatter_login FROM multi30)) AS stmt,
    count(*) FILTER (WHERE l.label = 'Reaction' AND m.chatter_login NOT LIKE '%bot' AND m.chatter_login NOT IN ('streamelements','streamlabs') AND m.chatter_login NOT IN (SELECT chatter_login FROM multi30)) AS react
  FROM twitch_chat_messages m
  JOIN twitch_chat_message_labels l ON l.message_id = m.id
  WHERE m.message_ts >= '2026-02-01' AND m.message_ts < '2026-10-05'
  GROUP BY 1, 2
),
filled AS (
  SELECT g.session_id, g.streamer_login, g.mt,
    coalesce(b.hum, 0) AS hum, coalesce(b.bc, 0) AS bc, coalesce(b.q, 0) AS q, coalesce(b.soc, 0) AS soc,
    coalesce(b.hype, 0) AS hype, coalesce(b.game, 0) AS game, coalesce(b.greet, 0) AS greet, coalesce(b.fb, 0) AS fb,
    coalesce(b.cmd, 0) AS cmd, coalesce(b.stmt, 0) AS stmt, coalesce(b.react, 0) AS react
  FROM grid g LEFT JOIN mb b ON b.session_id = g.session_id AND b.mt = g.mt
),
roll AS (
  SELECT f.*,
    sum(hum) OVER w3 AS hum3, sum(bc) OVER w3 AS bc3, sum(q) OVER w3 AS q3, sum(soc) OVER w3 AS soc3,
    sum(hype) OVER w3 AS hype3, sum(game) OVER w3 AS game3, sum(greet) OVER w3 AS greet3, sum(fb) OVER w3 AS fb3,
    sum(cmd) OVER w3 AS cmd3, sum(stmt) OVER w3 AS stmt3, sum(react) OVER w3 AS react3,
    row_number() OVER (PARTITION BY f.session_id ORDER BY f.mt) AS rn
  FROM filled f
  WINDOW w3 AS (PARTITION BY session_id ORDER BY mt ROWS BETWEEN 2 PRECEDING AND CURRENT ROW)
),
sess_base AS (
  SELECT session_id, streamer_login, count(*) AS n_grid, avg(hum3)::float / 3 AS speed, avg((bc3 > 0)::int) AS bc3f,
    avg(q3::float / NULLIF(hum3, 0)) AS sh_question, avg(soc3::float / NULLIF(hum3, 0)) AS sh_social,
    avg(hype3::float / NULLIF(hum3, 0)) AS sh_hype, avg(game3::float / NULLIF(hum3, 0)) AS sh_game,
    avg(greet3::float / NULLIF(hum3, 0)) AS sh_greeting, avg(fb3::float / NULLIF(hum3, 0)) AS sh_feedback,
    avg(cmd3::float / NULLIF(hum3, 0)) AS sh_command, avg(stmt3::float / NULLIF(hum3, 0)) AS sh_statement,
    avg(react3::float / NULLIF(hum3, 0)) AS sh_reaction
  FROM roll WHERE rn > 10 GROUP BY 1, 2
),
dg AS (
  SELECT n.streamer_login, n.session_id, m.metric, 'time_grid' AS base, m.d
  FROM sess_grp n
  JOIN sess_base g ON g.session_id = n.session_id
  CROSS JOIN LATERAL (VALUES ('speed', n.speed - g.speed), ('bc3', n.bc3 - g.bc3f), ('question', n.sh_question - g.sh_question), ('social', n.sh_social - g.sh_social), ('hype', n.sh_hype - g.sh_hype), ('game_related', n.sh_game - g.sh_game), ('greeting', n.sh_greeting - g.sh_greeting), ('feedback', n.sh_feedback - g.sh_feedback), ('command', n.sh_command - g.sh_command), ('statement', n.sh_statement - g.sh_statement), ('reaction', n.sh_reaction - g.sh_reaction)) AS m(metric, d)
  WHERE n.grp = 'newcomer' AND n.n >= 1
),
dr AS (
  SELECT n.streamer_login, n.session_id, m.metric, 'regular_msgs' AS base, m.d
  FROM sess_grp n
  JOIN sess_grp r ON r.session_id = n.session_id AND r.grp = 'regular'
  CROSS JOIN LATERAL (VALUES ('speed', n.speed - r.speed), ('bc3', n.bc3 - r.bc3), ('question', n.sh_question - r.sh_question), ('social', n.sh_social - r.sh_social), ('hype', n.sh_hype - r.sh_hype), ('game_related', n.sh_game - r.sh_game), ('greeting', n.sh_greeting - r.sh_greeting), ('feedback', n.sh_feedback - r.sh_feedback), ('command', n.sh_command - r.sh_command), ('statement', n.sh_statement - r.sh_statement), ('reaction', n.sh_reaction - r.sh_reaction)) AS m(metric, d)
  WHERE n.grp = 'newcomer' AND n.n >= 1 AND r.n >= 10
),
per_str AS (
  SELECT base, metric, streamer_login, count(*) AS sessions, percentile_cont(0.5) WITHIN GROUP (ORDER BY d) AS med_d
  FROM (SELECT * FROM dg UNION ALL SELECT * FROM dr) u
  WHERE d IS NOT NULL
  GROUP BY 1, 2, 3
)
SELECT base, metric, count(*) AS streamers, sum(sessions) AS sessions,
  round(percentile_cont(0.5) WITHIN GROUP (ORDER BY med_d)::numeric, 3) AS med_of_streamer_median_diff,
  round(avg((med_d > 0)::int)::numeric, 2) AS share_streamers_positive
FROM per_str
GROUP BY 1, 2
ORDER BY 1, 2
;

WITH msg AS (
  SELECT m.id, m.session_id, m.streamer_login, m.chatter_login, m.message_ts,
    CASE WHEN m.chatter_login = m.streamer_login THEN 'broadcaster'
         WHEN m.chatter_login LIKE '%bot' OR m.chatter_login IN ('streamelements','streamlabs') THEN 'bot'
         ELSE 'viewer' END AS role,
    l.label
  FROM twitch_chat_messages m
  JOIN twitch_chat_message_labels l ON l.message_id = m.id
  WHERE m.message_ts >= '2026-02-01' AND m.message_ts < '2026-10-05'
),
chan AS (
  SELECT msg.*, (message_ts = min(message_ts) OVER (PARTITION BY streamer_login, chatter_login)) AS is_chan_first,
    min(message_ts) OVER (PARTITION BY session_id) AS sess_start
  FROM msg
),
ctx AS (
  SELECT c.*,
    count(*) FILTER (WHERE role <> 'bot') OVER p3 AS hum3,
    count(*) FILTER (WHERE role = 'broadcaster') OVER p3 AS bc3,
    count(*) FILTER (WHERE role = 'broadcaster') OVER p1 AS bc1,
    count(*) FILTER (WHERE role <> 'bot' AND label = 'Question') OVER p3 AS q3,
    count(*) FILTER (WHERE role <> 'bot' AND label = 'Social') OVER p3 AS soc3,
    count(*) FILTER (WHERE role <> 'bot' AND label = 'Hype') OVER p3 AS hype3,
    count(*) FILTER (WHERE role <> 'bot' AND label = 'Game-Related') OVER p3 AS game3,
    count(*) FILTER (WHERE role <> 'bot' AND label = 'Greeting') OVER p3 AS greet3,
    count(*) FILTER (WHERE role <> 'bot' AND label = 'Feedback') OVER p3 AS fb3,
    count(*) FILTER (WHERE role <> 'bot' AND label = 'Command') OVER p3 AS cmd3,
    count(*) FILTER (WHERE role <> 'bot' AND label = 'Statement') OVER p3 AS stmt3,
    count(*) FILTER (WHERE role <> 'bot' AND label = 'Reaction') OVER p3 AS react3
  FROM chan c
  WINDOW
    p3 AS (PARTITION BY session_id ORDER BY message_ts RANGE BETWEEN INTERVAL '3 minutes' PRECEDING AND INTERVAL '1 microsecond' PRECEDING),
    p1 AS (PARTITION BY session_id ORDER BY message_ts RANGE BETWEEN INTERVAL '1 minute' PRECEDING AND INTERVAL '1 microsecond' PRECEDING)
),
sel AS (
  SELECT session_id, streamer_login, 'newcomer' AS grp, hum3::float / 3 AS speed,
    (bc3 > 0)::int AS bc3f, (bc1 > 0)::int AS bc1f,
    q3::float / NULLIF(hum3, 0) AS sh_question, soc3::float / NULLIF(hum3, 0) AS sh_social,
    hype3::float / NULLIF(hum3, 0) AS sh_hype, game3::float / NULLIF(hum3, 0) AS sh_game,
    greet3::float / NULLIF(hum3, 0) AS sh_greeting, fb3::float / NULLIF(hum3, 0) AS sh_feedback,
    cmd3::float / NULLIF(hum3, 0) AS sh_command, stmt3::float / NULLIF(hum3, 0) AS sh_statement,
    react3::float / NULLIF(hum3, 0) AS sh_reaction
  FROM ctx WHERE role = 'viewer' AND is_chan_first AND message_ts >= '2026-03-01' AND message_ts >= sess_start + interval '10 minutes'
  UNION ALL
  SELECT session_id, streamer_login, 'regular' AS grp, hum3::float / 3 AS speed,
    (bc3 > 0)::int AS bc3f, (bc1 > 0)::int AS bc1f,
    q3::float / NULLIF(hum3, 0), soc3::float / NULLIF(hum3, 0),
    hype3::float / NULLIF(hum3, 0), game3::float / NULLIF(hum3, 0),
    greet3::float / NULLIF(hum3, 0), fb3::float / NULLIF(hum3, 0),
    cmd3::float / NULLIF(hum3, 0), stmt3::float / NULLIF(hum3, 0),
    react3::float / NULLIF(hum3, 0)
  FROM ctx WHERE role = 'viewer' AND NOT is_chan_first AND message_ts >= sess_start + interval '10 minutes'
),
sess_grp AS (
  SELECT session_id, streamer_login, grp, count(*) AS n, avg(speed) AS speed, avg(bc3f) AS bc3, avg(bc1f) AS bc1,
    avg(sh_question) AS sh_question, avg(sh_social) AS sh_social, avg(sh_hype) AS sh_hype, avg(sh_game) AS sh_game,
    avg(sh_greeting) AS sh_greeting, avg(sh_feedback) AS sh_feedback, avg(sh_command) AS sh_command,
    avg(sh_statement) AS sh_statement, avg(sh_reaction) AS sh_reaction
  FROM sel GROUP BY 1, 2, 3
),
sess AS (
  SELECT session_id, streamer_login, min(message_ts) AS s0, max(message_ts) AS s1
  FROM twitch_chat_messages
  WHERE message_ts >= '2026-02-01' AND message_ts < '2026-10-05'
  GROUP BY 1, 2
  HAVING max(message_ts) - min(message_ts) >= interval '20 minutes'
),
grid AS (
  SELECT s.session_id, s.streamer_login, g AS mt
  FROM sess s CROSS JOIN LATERAL generate_series(date_trunc('minute', s.s0), date_trunc('minute', s.s1), interval '1 minute') g
),
mb AS (
  SELECT m.session_id, date_trunc('minute', m.message_ts) AS mt,
    count(*) FILTER (WHERE m.chatter_login NOT LIKE '%bot' AND m.chatter_login NOT IN ('streamelements','streamlabs')) AS hum,
    count(*) FILTER (WHERE m.chatter_login = m.streamer_login) AS bc,
    count(*) FILTER (WHERE l.label = 'Question' AND m.chatter_login NOT LIKE '%bot' AND m.chatter_login NOT IN ('streamelements','streamlabs')) AS q,
    count(*) FILTER (WHERE l.label = 'Social' AND m.chatter_login NOT LIKE '%bot' AND m.chatter_login NOT IN ('streamelements','streamlabs')) AS soc,
    count(*) FILTER (WHERE l.label = 'Hype' AND m.chatter_login NOT LIKE '%bot' AND m.chatter_login NOT IN ('streamelements','streamlabs')) AS hype,
    count(*) FILTER (WHERE l.label = 'Game-Related' AND m.chatter_login NOT LIKE '%bot' AND m.chatter_login NOT IN ('streamelements','streamlabs')) AS game,
    count(*) FILTER (WHERE l.label = 'Greeting' AND m.chatter_login NOT LIKE '%bot' AND m.chatter_login NOT IN ('streamelements','streamlabs')) AS greet,
    count(*) FILTER (WHERE l.label = 'Feedback' AND m.chatter_login NOT LIKE '%bot' AND m.chatter_login NOT IN ('streamelements','streamlabs')) AS fb,
    count(*) FILTER (WHERE l.label = 'Command' AND m.chatter_login NOT LIKE '%bot' AND m.chatter_login NOT IN ('streamelements','streamlabs')) AS cmd,
    count(*) FILTER (WHERE l.label = 'Statement' AND m.chatter_login NOT LIKE '%bot' AND m.chatter_login NOT IN ('streamelements','streamlabs')) AS stmt,
    count(*) FILTER (WHERE l.label = 'Reaction' AND m.chatter_login NOT LIKE '%bot' AND m.chatter_login NOT IN ('streamelements','streamlabs')) AS react
  FROM twitch_chat_messages m
  JOIN twitch_chat_message_labels l ON l.message_id = m.id
  WHERE m.message_ts >= '2026-02-01' AND m.message_ts < '2026-10-05'
  GROUP BY 1, 2
),
filled AS (
  SELECT g.session_id, g.streamer_login, g.mt,
    coalesce(b.hum, 0) AS hum, coalesce(b.bc, 0) AS bc, coalesce(b.q, 0) AS q, coalesce(b.soc, 0) AS soc,
    coalesce(b.hype, 0) AS hype, coalesce(b.game, 0) AS game, coalesce(b.greet, 0) AS greet, coalesce(b.fb, 0) AS fb,
    coalesce(b.cmd, 0) AS cmd, coalesce(b.stmt, 0) AS stmt, coalesce(b.react, 0) AS react
  FROM grid g LEFT JOIN mb b ON b.session_id = g.session_id AND b.mt = g.mt
),
roll AS (
  SELECT f.*,
    sum(hum) OVER w3 AS hum3, sum(bc) OVER w3 AS bc3, sum(q) OVER w3 AS q3, sum(soc) OVER w3 AS soc3,
    sum(hype) OVER w3 AS hype3, sum(game) OVER w3 AS game3, sum(greet) OVER w3 AS greet3, sum(fb) OVER w3 AS fb3,
    sum(cmd) OVER w3 AS cmd3, sum(stmt) OVER w3 AS stmt3, sum(react) OVER w3 AS react3,
    row_number() OVER (PARTITION BY f.session_id ORDER BY f.mt) AS rn
  FROM filled f
  WINDOW w3 AS (PARTITION BY session_id ORDER BY mt ROWS BETWEEN 2 PRECEDING AND CURRENT ROW)
),
sess_base AS (
  SELECT session_id, streamer_login, count(*) AS n_grid, avg(hum3)::float / 3 AS speed, avg((bc3 > 0)::int) AS bc3f,
    avg(q3::float / NULLIF(hum3, 0)) AS sh_question, avg(soc3::float / NULLIF(hum3, 0)) AS sh_social,
    avg(hype3::float / NULLIF(hum3, 0)) AS sh_hype, avg(game3::float / NULLIF(hum3, 0)) AS sh_game,
    avg(greet3::float / NULLIF(hum3, 0)) AS sh_greeting, avg(fb3::float / NULLIF(hum3, 0)) AS sh_feedback,
    avg(cmd3::float / NULLIF(hum3, 0)) AS sh_command, avg(stmt3::float / NULLIF(hum3, 0)) AS sh_statement,
    avg(react3::float / NULLIF(hum3, 0)) AS sh_reaction
  FROM roll WHERE rn > 10 GROUP BY 1, 2
),
dg AS (
  SELECT n.streamer_login, n.session_id, m.metric, 'time_grid' AS base, m.d
  FROM sess_grp n
  JOIN sess_base g ON g.session_id = n.session_id
  CROSS JOIN LATERAL (VALUES ('speed', n.speed - g.speed), ('bc3', n.bc3 - g.bc3f), ('question', n.sh_question - g.sh_question), ('social', n.sh_social - g.sh_social), ('hype', n.sh_hype - g.sh_hype), ('game_related', n.sh_game - g.sh_game), ('greeting', n.sh_greeting - g.sh_greeting), ('feedback', n.sh_feedback - g.sh_feedback), ('command', n.sh_command - g.sh_command), ('statement', n.sh_statement - g.sh_statement), ('reaction', n.sh_reaction - g.sh_reaction)) AS m(metric, d)
  WHERE n.grp = 'newcomer' AND n.n >= 1
),
dr AS (
  SELECT n.streamer_login, n.session_id, m.metric, 'regular_msgs' AS base, m.d
  FROM sess_grp n
  JOIN sess_grp r ON r.session_id = n.session_id AND r.grp = 'regular'
  CROSS JOIN LATERAL (VALUES ('speed', n.speed - r.speed), ('bc3', n.bc3 - r.bc3), ('question', n.sh_question - r.sh_question), ('social', n.sh_social - r.sh_social), ('hype', n.sh_hype - r.sh_hype), ('game_related', n.sh_game - r.sh_game), ('greeting', n.sh_greeting - r.sh_greeting), ('feedback', n.sh_feedback - r.sh_feedback), ('command', n.sh_command - r.sh_command), ('statement', n.sh_statement - r.sh_statement), ('reaction', n.sh_reaction - r.sh_reaction)) AS m(metric, d)
  WHERE n.grp = 'newcomer' AND n.n >= 1 AND r.n >= 10
)
SELECT 'newcomer_context' AS src, count(*) AS sessions,
  round(avg(speed)::numeric, 3) AS mean_speed_per_min,
  round(avg(sh_question)::numeric, 4) AS mean_share_question,
  round(avg(sh_statement)::numeric, 4) AS mean_share_statement,
  round(avg(sh_game)::numeric, 4) AS mean_share_game_related
FROM sess_grp WHERE grp = 'newcomer'
UNION ALL
SELECT 'regular_context', count(*), round(avg(speed)::numeric, 3), round(avg(sh_question)::numeric, 4),
  round(avg(sh_statement)::numeric, 4), round(avg(sh_game)::numeric, 4)
FROM sess_grp WHERE grp = 'regular'
UNION ALL
SELECT 'time_grid_minutes', count(*), round(avg(speed)::numeric, 3), round(avg(sh_question)::numeric, 4),
  round(avg(sh_statement)::numeric, 4), round(avg(sh_game)::numeric, 4)
FROM sess_base
;

WITH multi30 AS (SELECT chatter_login FROM twitch_chat_messages WHERE message_ts >= '2026-02-01' AND message_ts < '2026-10-05' GROUP BY chatter_login HAVING count(DISTINCT streamer_login) >= 30), base AS (
  SELECT m.id, m.session_id, m.streamer_login, m.chatter_login, m.message_ts,
    CASE WHEN m.chatter_login = m.streamer_login THEN 'broadcaster'
         WHEN m.chatter_login IN (SELECT chatter_login FROM multi30) OR m.chatter_login LIKE '%bot' OR m.chatter_login IN ('streamelements','streamlabs') THEN 'bot'
         ELSE 'viewer' END AS role,
    l.label
  FROM twitch_chat_messages m
  LEFT JOIN twitch_chat_message_labels l ON l.message_id = m.id
  WHERE m.message_ts >= '2026-02-01' AND m.message_ts < '2026-10-05'
),
chan AS (
  SELECT b.*,
    (message_ts = min(message_ts) OVER (PARTITION BY streamer_login, chatter_login)) AS is_chan_first,
    count(*) FILTER (WHERE role <> 'bot') OVER (PARTITION BY session_id, chatter_login) AS own_sess,
    count(*) OVER (PARTITION BY streamer_login, chatter_login) AS own_chan
  FROM base b
),
nc AS (
  SELECT * FROM chan
  WHERE is_chan_first AND role = 'viewer' AND message_ts >= '2026-03-01' AND label IS NOT NULL
),
ns AS (
  SELECT session_id, streamer_login, label, own_sess, (own_chan > own_sess)::int AS returned, (own_sess >= 3)::int AS stayed3
  FROM nc
),
sess_mean AS (
  SELECT session_id, streamer_login, count(*) AS n, avg(own_sess) AS m_stay, avg(returned) AS m_ret, avg(stayed3) AS m_st3
  FROM ns GROUP BY 1, 2 HAVING count(*) >= 2
),
lab AS (
  SELECT ns.session_id, ns.streamer_login, ns.label, count(*) AS n_l,
    avg(ns.own_sess) AS stay_l, avg(ns.returned) AS ret_l, avg(ns.stayed3) AS st3_l,
    s.m_stay, s.m_ret, s.m_st3
  FROM ns JOIN sess_mean s ON s.session_id = ns.session_id
  GROUP BY ns.session_id, ns.streamer_login, ns.label, s.m_stay, s.m_ret, s.m_st3
),
per_str AS (
  SELECT streamer_login, label, count(*) AS sessions, sum(n_l) AS n,
    percentile_cont(0.5) WITHIN GROUP (ORDER BY stay_l / NULLIF(m_stay, 0)) AS s_stay_rel
  FROM lab WHERE n_l >= 1
  GROUP BY 1, 2
),
label_out AS (
  SELECT label, count(DISTINCT streamer_login) AS streamers, sum(sessions) AS sessions,
    round(percentile_cont(0.5) WITHIN GROUP (ORDER BY s_stay_rel)::numeric, 2) AS med_stay_rel_session
  FROM per_str GROUP BY label
),
pooled AS (
  SELECT label, count(*) AS n, round(avg(own_sess)::numeric, 2) AS mean_msgs_in_session,
    round(avg(stayed3)::numeric, 3) AS share_3plus_in_session, round(avg(returned)::numeric, 3) AS share_returned_later
  FROM ns GROUP BY label
),
share_all AS (
  SELECT label, count(*) AS n FROM chan WHERE role = 'viewer' AND message_ts >= '2026-03-01' AND label IS NOT NULL GROUP BY label
)
SELECT p.label, p.n AS newcomer_first_msgs, round(100.0 * p.n / sum(p.n) OVER (), 1) AS pct_first_msgs,
  round(100.0 * a.n / sum(a.n) OVER (), 1) AS pct_all_viewer_msgs,
  p.mean_msgs_in_session, p.share_3plus_in_session, p.share_returned_later,
  o.med_stay_rel_session, o.sessions, o.streamers
FROM pooled p
LEFT JOIN label_out o ON o.label = p.label
LEFT JOIN share_all a ON a.label = p.label
ORDER BY p.n DESC
;

WITH base AS (
  SELECT m.id, m.session_id, m.streamer_login, m.chatter_login, m.message_ts,
    CASE WHEN m.chatter_login = m.streamer_login THEN 'broadcaster'
         WHEN m.chatter_login LIKE '%bot' OR m.chatter_login IN ('streamelements','streamlabs') THEN 'bot'
         ELSE 'viewer' END AS role,
    l.label
  FROM twitch_chat_messages m
  LEFT JOIN twitch_chat_message_labels l ON l.message_id = m.id
  WHERE m.message_ts >= '2026-02-01' AND m.message_ts < '2026-10-05'
),
chan AS (
  SELECT b.*,
    (message_ts = min(message_ts) OVER (PARTITION BY streamer_login, chatter_login)) AS is_chan_first,
    count(*) FILTER (WHERE role <> 'bot') OVER (PARTITION BY session_id, chatter_login) AS own_sess,
    count(*) OVER (PARTITION BY streamer_login, chatter_login) AS own_chan
  FROM base b
),
nc AS (
  SELECT * FROM chan
  WHERE is_chan_first AND role = 'viewer' AND message_ts >= '2026-03-01' AND message_ts < '2026-09-05' AND label IS NOT NULL
),
ns AS (
  SELECT session_id, streamer_login, label, own_sess, (own_chan > own_sess)::int AS returned, (own_sess >= 3)::int AS stayed3
  FROM nc
),
sess_mean AS (
  SELECT session_id, streamer_login, count(*) AS n, avg(own_sess) AS m_stay, avg(returned) AS m_ret, avg(stayed3) AS m_st3
  FROM ns GROUP BY 1, 2 HAVING count(*) >= 2
),
lab AS (
  SELECT ns.session_id, ns.streamer_login, ns.label, count(*) AS n_l,
    avg(ns.own_sess) AS stay_l, avg(ns.returned) AS ret_l, avg(ns.stayed3) AS st3_l,
    s.m_stay, s.m_ret, s.m_st3
  FROM ns JOIN sess_mean s ON s.session_id = ns.session_id
  GROUP BY ns.session_id, ns.streamer_login, ns.label, s.m_stay, s.m_ret, s.m_st3
),
per_str AS (
  SELECT streamer_login, label, count(*) AS sessions, sum(n_l) AS n,
    percentile_cont(0.5) WITHIN GROUP (ORDER BY stay_l / NULLIF(m_stay, 0)) AS s_stay_rel
  FROM lab WHERE n_l >= 1
  GROUP BY 1, 2
),
label_out AS (
  SELECT label, count(DISTINCT streamer_login) AS streamers, sum(sessions) AS sessions,
    round(percentile_cont(0.5) WITHIN GROUP (ORDER BY s_stay_rel)::numeric, 2) AS med_stay_rel_session
  FROM per_str GROUP BY label
),
pooled AS (
  SELECT label, count(*) AS n, round(avg(own_sess)::numeric, 2) AS mean_msgs_in_session,
    round(avg(stayed3)::numeric, 3) AS share_3plus_in_session, round(avg(returned)::numeric, 3) AS share_returned_later
  FROM ns GROUP BY label
),
share_all AS (
  SELECT label, count(*) AS n FROM chan WHERE role = 'viewer' AND message_ts >= '2026-03-01' AND label IS NOT NULL GROUP BY label
)
SELECT p.label, p.n AS newcomer_first_msgs, round(100.0 * p.n / sum(p.n) OVER (), 1) AS pct_first_msgs,
  round(100.0 * a.n / sum(a.n) OVER (), 1) AS pct_all_viewer_msgs,
  p.mean_msgs_in_session, p.share_3plus_in_session, p.share_returned_later,
  o.med_stay_rel_session, o.sessions, o.streamers
FROM pooled p
LEFT JOIN label_out o ON o.label = p.label
LEFT JOIN share_all a ON a.label = p.label
ORDER BY p.n DESC
;

WITH base AS (
  SELECT m.id, m.session_id, m.streamer_login, m.chatter_login, m.message_ts, m.content,
    CASE WHEN m.chatter_login = m.streamer_login THEN 'broadcaster'
         WHEN m.chatter_login LIKE '%bot' OR m.chatter_login IN ('streamelements','streamlabs') THEN 'bot'
         ELSE 'viewer' END AS role,
    l.label
  FROM twitch_chat_messages m
  LEFT JOIN twitch_chat_message_labels l ON l.message_id = m.id
  WHERE m.message_ts >= '2026-02-01'
),
win AS (
  SELECT b.*,
    count(*) FILTER (WHERE role = 'broadcaster') OVER (PARTITION BY session_id ORDER BY message_ts
      RANGE BETWEEN INTERVAL '1 microsecond' FOLLOWING AND INTERVAL '2 minutes' FOLLOWING) AS bc2,
    count(*) FILTER (WHERE role <> 'bot') OVER (PARTITION BY session_id, chatter_login ORDER BY message_ts
      RANGE BETWEEN INTERVAL '10 minutes' PRECEDING AND INTERVAL '1 microsecond' PRECEDING) AS own_before10,
    count(*) FILTER (WHERE role <> 'bot') OVER (PARTITION BY session_id, chatter_login ORDER BY message_ts
      RANGE BETWEEN INTERVAL '1 microsecond' FOLLOWING AND INTERVAL '10 minutes' FOLLOWING) AS own_after10
  FROM base b
),
q AS (
  SELECT w.*
  FROM win w
  WHERE w.label = 'Question' AND w.role = 'viewer' AND w.message_ts >= '2026-02-01' AND w.message_ts < '2026-10-05'
),
qa AS (
  SELECT q.id, q.session_id, q.streamer_login, q.message_ts, q.own_before10, q.own_after10, q.bc2,
    bcr.ts AS bc_ts, mr.ts AS mention_ts
  FROM q
  LEFT JOIN LATERAL (
    SELECT min(b.message_ts) AS ts FROM twitch_chat_messages b
    WHERE b.session_id = q.session_id AND b.message_ts > q.message_ts AND b.message_ts <= q.message_ts + interval '5 minutes'
      AND b.chatter_login = b.streamer_login
  ) bcr ON true
  LEFT JOIN LATERAL (
    SELECT min(b.message_ts) AS ts FROM twitch_chat_messages b
    WHERE b.session_id = q.session_id AND b.message_ts > q.message_ts AND b.message_ts <= q.message_ts + interval '5 minutes'
      AND b.chatter_login <> q.chatter_login AND b.chatter_login <> b.streamer_login
      AND NOT (b.chatter_login LIKE '%bot' OR b.chatter_login IN ('streamelements','streamlabs'))
      AND position(lower('@' || q.chatter_login) IN lower(b.content)) > 0
  ) mr ON true
),
qf AS (
  SELECT *,
    (bc_ts IS NOT NULL AND bc_ts <= message_ts + interval '2 minutes') AS ans_bc2,
    ((bc_ts IS NOT NULL AND bc_ts <= message_ts + interval '2 minutes')
      OR (mention_ts IS NOT NULL AND mention_ts <= message_ts + interval '2 minutes')) AS ans_any2,
    extract(epoch FROM LEAST(bc_ts, mention_ts) - message_ts) AS ans_secs_any,
    extract(epoch FROM bc_ts - message_ts) AS ans_secs_bc,
    (own_after10 - own_before10) AS delta10
  FROM qa
),
overall AS (
  SELECT count(*) AS questions, count(DISTINCT session_id) AS sessions, count(DISTINCT streamer_login) AS streamers,
    round(avg(ans_bc2::int)::numeric, 3) AS share_bc_reply_2m,
    round(avg(ans_any2::int)::numeric, 3) AS share_any_reply_2m,
    round(avg((bc_ts IS NOT NULL OR mention_ts IS NOT NULL)::int)::numeric, 3) AS share_any_reply_5m,
    round(percentile_cont(0.5) WITHIN GROUP (ORDER BY ans_secs_any)::numeric, 1) AS med_secs_first_reply_any,
    round(percentile_cont(0.5) WITHIN GROUP (ORDER BY ans_secs_bc)::numeric, 1) AS med_secs_first_bc_reply,
    round(avg(own_after10)::numeric, 3) AS mean_asker_msgs_next10,
    round(avg(own_before10)::numeric, 3) AS mean_asker_msgs_prev10
  FROM qf
),
sess_cmp AS (
  SELECT session_id, streamer_login,
    avg(own_after10) FILTER (WHERE ans_any2) AS after_ans, avg(own_after10) FILTER (WHERE NOT ans_any2) AS after_unans,
    avg(delta10) FILTER (WHERE ans_any2) AS delta_ans, avg(delta10) FILTER (WHERE NOT ans_any2) AS delta_unans,
    count(*) FILTER (WHERE ans_any2) AS n_ans, count(*) FILTER (WHERE NOT ans_any2) AS n_unans
  FROM qf GROUP BY 1, 2
),
sc AS (
  SELECT streamer_login, count(*) AS sessions,
    percentile_cont(0.5) WITHIN GROUP (ORDER BY after_ans - after_unans) AS d_after,
    percentile_cont(0.5) WITHIN GROUP (ORDER BY delta_ans - delta_unans) AS d_delta
  FROM sess_cmp WHERE n_ans >= 1 AND n_unans >= 1 GROUP BY 1
)
SELECT 'overall' AS part, o.questions::text AS a, o.sessions::text AS b, o.streamers::text AS c,
  o.share_bc_reply_2m::text AS d, o.share_any_reply_2m::text AS e, o.share_any_reply_5m::text AS f,
  o.med_secs_first_reply_any::text AS g, o.med_secs_first_bc_reply::text AS h,
  o.mean_asker_msgs_prev10::text AS i, o.mean_asker_msgs_next10::text AS j
FROM overall o
UNION ALL
SELECT 'within_session_answered_minus_unanswered', count(*)::text, sum(sessions)::text, NULL,
  round(percentile_cont(0.5) WITHIN GROUP (ORDER BY d_after)::numeric, 3)::text,
  round(avg((d_after > 0)::int)::numeric, 2)::text,
  round(percentile_cont(0.5) WITHIN GROUP (ORDER BY d_delta)::numeric, 3)::text,
  round(avg((d_delta > 0)::int)::numeric, 2)::text, NULL, NULL, NULL
FROM sc
;

WITH multi30 AS (SELECT chatter_login FROM twitch_chat_messages WHERE message_ts >= '2026-02-01' AND message_ts < '2026-10-05' GROUP BY chatter_login HAVING count(DISTINCT streamer_login) >= 30), cmd AS (
  SELECT lower(split_part(substr(m.content, 2), ' ', 1)) AS word, m.streamer_login,
    CASE WHEN m.chatter_login = m.streamer_login THEN 'broadcaster'
         WHEN m.chatter_login IN (SELECT chatter_login FROM multi30) OR m.chatter_login LIKE '%bot' OR m.chatter_login IN ('streamelements','streamlabs') THEN 'bot'
         ELSE 'viewer' END AS role
  FROM twitch_chat_messages m
  WHERE m.content LIKE '!%' AND length(m.content) > 1 AND m.message_ts >= '2026-02-01'
)
SELECT word, count(*) AS msgs, count(DISTINCT streamer_login) AS channels,
  round(avg((role = 'viewer')::int)::numeric, 3) AS share_viewer,
  round(avg((role = 'broadcaster')::int)::numeric, 3) AS share_broadcaster,
  round(avg((role = 'bot')::int)::numeric, 3) AS share_bot
FROM cmd
WHERE word <> ''
GROUP BY word
HAVING count(DISTINCT streamer_login) >= 5
ORDER BY msgs DESC
LIMIT 40
;

WITH multi30 AS (SELECT chatter_login FROM twitch_chat_messages WHERE message_ts >= '2026-02-01' AND message_ts < '2026-10-05' GROUP BY chatter_login HAVING count(DISTINCT streamer_login) >= 30), msgs AS (
  SELECT m.session_id, m.streamer_login, m.chatter_login, m.message_ts,
    CASE WHEN m.chatter_login = m.streamer_login THEN 'broadcaster'
         WHEN m.chatter_login IN (SELECT chatter_login FROM multi30) OR m.chatter_login LIKE '%bot' OR m.chatter_login IN ('streamelements','streamlabs') THEN 'bot'
         ELSE 'viewer' END AS role,
    (m.content LIKE '!%') AS is_bang
  FROM twitch_chat_messages m
  WHERE m.message_ts >= '2026-02-01' AND m.message_ts < '2026-10-25'
),
sess AS (
  SELECT session_id, streamer_login, min(message_ts) AS s0,
    count(*) FILTER (WHERE role <> 'bot' AND NOT is_bang) AS hum_msgs,
    count(DISTINCT chatter_login) FILTER (WHERE role <> 'bot' AND NOT is_bang) AS hum_chatters,
    count(*) FILTER (WHERE role = 'viewer' AND is_bang) AS viewer_cmds
  FROM msgs GROUP BY 1, 2
),
sess_size AS (
  SELECT s.session_id, s.streamer_login, s.hum_msgs, s.hum_chatters, s.viewer_cmds, ts.avg_viewers
  FROM sess s LEFT JOIN twitch_stream_sessions ts ON ts.id = s.session_id
  WHERE s.hum_msgs >= 1
),
chan AS (
  SELECT streamer_login, count(*) AS sessions,
    count(*) FILTER (WHERE viewer_cmds > 0) AS cmd_sessions,
    percentile_cont(0.5) WITHIN GROUP (ORDER BY hum_msgs) AS med_hum,
    percentile_cont(0.5) WITHIN GROUP (ORDER BY hum_chatters) AS med_chatters,
    percentile_cont(0.5) WITHIN GROUP (ORDER BY avg_viewers) AS med_avg_viewers
  FROM sess_size GROUP BY streamer_login HAVING count(*) >= 3
),
classed AS (
  SELECT *,
    CASE WHEN med_avg_viewers < 5 THEN '0-4' WHEN med_avg_viewers < 20 THEN '5-19'
         WHEN med_avg_viewers < 100 THEN '20-99' WHEN med_avg_viewers IS NULL THEN 'unknown' ELSE '100+' END AS size_class,
    CASE WHEN cmd_sessions >= 3 AND cmd_sessions::float / sessions >= 0.2 THEN 'interactive' ELSE 'no_viewer_commands' END AS grp
  FROM chan
)
SELECT size_class, grp, count(*) AS channels, sum(sessions) AS sessions,
  round(percentile_cont(0.5) WITHIN GROUP (ORDER BY med_hum)::numeric, 1) AS med_of_channel_median_human_msgs,
  round(percentile_cont(0.5) WITHIN GROUP (ORDER BY med_chatters)::numeric, 1) AS med_of_channel_median_chatters
FROM classed
GROUP BY 1, 2
ORDER BY 1, 2
;

WITH msgs AS (
  SELECT m.session_id, m.streamer_login, m.chatter_login, m.message_ts,
    CASE WHEN m.chatter_login = m.streamer_login THEN 'broadcaster'
         WHEN m.chatter_login LIKE '%bot' OR m.chatter_login IN ('streamelements','streamlabs') THEN 'bot'
         ELSE 'viewer' END AS role,
    (m.content LIKE '!%') AS is_bang
  FROM twitch_chat_messages m
  WHERE m.message_ts >= '2026-02-01' AND m.message_ts < '2026-10-25'
),
sess AS (
  SELECT session_id, streamer_login, min(message_ts) AS s0,
    count(*) FILTER (WHERE role <> 'bot' AND NOT is_bang) AS hum_msgs,
    count(*) FILTER (WHERE role = 'viewer' AND is_bang) AS viewer_cmds
  FROM msgs GROUP BY 1, 2
),
sess_w AS (
  SELECT *, date_trunc('week', s0) AS wk FROM sess WHERE hum_msgs >= 1
),
wk_med AS (
  SELECT wk, percentile_cont(0.5) WITHIN GROUP (ORDER BY hum_msgs) AS med_week FROM sess_w GROUP BY wk
),
rel AS (
  SELECT s.session_id, s.streamer_login, s.s0, s.viewer_cmds, s.hum_msgs::float / NULLIF(w.med_week, 0) AS rel_msgs
  FROM sess_w s JOIN wk_med w ON w.wk = s.wk
),
chan_first AS (
  SELECT streamer_login, min(s0) FILTER (WHERE viewer_cmds > 0) AS first_cmd FROM rel GROUP BY 1
),
ba AS (
  SELECT r.streamer_login,
    count(*) FILTER (WHERE r.s0 < c.first_cmd) AS n_before,
    count(*) FILTER (WHERE r.s0 >= c.first_cmd) AS n_after,
    percentile_cont(0.5) WITHIN GROUP (ORDER BY r.rel_msgs) FILTER (WHERE r.s0 < c.first_cmd) AS med_before,
    percentile_cont(0.5) WITHIN GROUP (ORDER BY r.rel_msgs) FILTER (WHERE r.s0 >= c.first_cmd) AS med_after
  FROM rel r JOIN chan_first c ON c.streamer_login = r.streamer_login
  WHERE c.first_cmd IS NOT NULL
  GROUP BY r.streamer_login
)
SELECT count(*) AS channels_with_viewer_command_adoption,
  round(percentile_cont(0.5) WITHIN GROUP (ORDER BY med_before)::numeric, 3) AS med_rel_before,
  round(percentile_cont(0.5) WITHIN GROUP (ORDER BY med_after)::numeric, 3) AS med_rel_after,
  round(percentile_cont(0.5) WITHIN GROUP (ORDER BY med_after - med_before)::numeric, 3) AS med_diff_after_minus_before,
  round(avg((med_after > med_before)::int)::numeric, 2) AS share_channels_after_higher
FROM ba WHERE n_before >= 3 AND n_after >= 3
;

WITH msgs AS (
  SELECT m.session_id, m.streamer_login, m.chatter_login, m.message_ts,
    CASE WHEN m.chatter_login = m.streamer_login THEN 'broadcaster'
         WHEN m.chatter_login LIKE '%bot' OR m.chatter_login IN ('streamelements','streamlabs') THEN 'bot'
         ELSE 'viewer' END AS role,
    (m.content LIKE '!%') AS is_bang
  FROM twitch_chat_messages m
  WHERE m.message_ts >= '2026-02-01' AND m.message_ts < '2026-10-25'
),
sess AS (
  SELECT session_id, streamer_login, min(message_ts) AS s0,
    count(*) FILTER (WHERE role <> 'bot' AND NOT is_bang) AS hum_msgs,
    count(*) FILTER (WHERE role = 'viewer' AND is_bang) AS viewer_cmds
  FROM msgs GROUP BY 1, 2
),
sess_w AS (
  SELECT *, date_trunc('week', s0) AS wk FROM sess WHERE hum_msgs >= 1
),
wk_med AS (
  SELECT wk, percentile_cont(0.5) WITHIN GROUP (ORDER BY hum_msgs) AS med_week FROM sess_w GROUP BY wk
),
rel AS (
  SELECT s.session_id, s.streamer_login, s.s0, s.viewer_cmds, s.hum_msgs::float / NULLIF(w.med_week, 0) AS rel_msgs
  FROM sess_w s JOIN wk_med w ON w.wk = s.wk
),
no_cmd AS (
  SELECT streamer_login, min(s0) AS first_s, max(s0) AS last_s, count(*) AS n
  FROM rel GROUP BY streamer_login
  HAVING sum(viewer_cmds) = 0 AND count(*) >= 6
),
mid AS (
  SELECT r.streamer_login, r.rel_msgs, r.s0,
    (r.s0 < n.first_s + (n.last_s - n.first_s) / 2) AS first_half
  FROM rel r JOIN no_cmd n ON n.streamer_login = r.streamer_login
),
ba AS (
  SELECT streamer_login,
    count(*) FILTER (WHERE first_half) AS n_before, count(*) FILTER (WHERE NOT first_half) AS n_after,
    percentile_cont(0.5) WITHIN GROUP (ORDER BY rel_msgs) FILTER (WHERE first_half) AS med_before,
    percentile_cont(0.5) WITHIN GROUP (ORDER BY rel_msgs) FILTER (WHERE NOT first_half) AS med_after
  FROM mid GROUP BY streamer_login
)
SELECT count(*) AS channels_without_viewer_commands,
  round(percentile_cont(0.5) WITHIN GROUP (ORDER BY med_before)::numeric, 3) AS med_rel_first_half,
  round(percentile_cont(0.5) WITHIN GROUP (ORDER BY med_after)::numeric, 3) AS med_rel_second_half,
  round(percentile_cont(0.5) WITHIN GROUP (ORDER BY med_after - med_before)::numeric, 3) AS med_diff_second_minus_first,
  round(avg((med_after > med_before)::int)::numeric, 2) AS share_second_higher
FROM ba WHERE n_before >= 3 AND n_after >= 3
;

WITH base AS (
  SELECT room_user_id AS room, chatter_user_id AS chatter, sent_at AS ts, emote_count, message_len
  FROM category_chat_messages
  WHERE sent_at >= '2026-09-27' AND sent_at < '2026-10-25'
    AND stream_language LIKE 'de%'
    AND NOT shared_chat_copy
    AND chatter_user_id <> room_user_id
),
fu AS (
  SELECT room, ts, emote_count, message_len,
    count(*) OVER w AS f2
  FROM base
  WINDOW w AS (PARTITION BY room ORDER BY ts
    RANGE BETWEEN INTERVAL '1 microsecond' FOLLOWING AND INTERVAL '2 minutes' FOLLOWING)
),
rm AS (
  SELECT room, avg(f2) AS room_avg
  FROM fu GROUP BY room
  HAVING count(*) >= 20 AND avg(f2) > 0
),
feat AS (
  SELECT fu.room, fu.f2 / rm.room_avg AS rel, (fu.emote_count > 0) AS has_emote,
    CASE WHEN fu.message_len <= 10 THEN 'a_1_10'
         WHEN fu.message_len <= 25 THEN 'b_11_25'
         WHEN fu.message_len <= 60 THEN 'c_26_60'
         ELSE 'd_61_plus' END AS len_bucket
  FROM fu JOIN rm USING (room)
),
emote_room AS (
  SELECT room,
    avg(rel) FILTER (WHERE has_emote) AS rel_emote,
    avg(rel) FILTER (WHERE NOT has_emote) AS rel_plain,
    count(*) FILTER (WHERE has_emote) AS n_emote,
    count(*) FILTER (WHERE NOT has_emote) AS n_plain
  FROM feat GROUP BY room
  HAVING count(*) FILTER (WHERE has_emote) >= 5 AND count(*) FILTER (WHERE NOT has_emote) >= 5
),
len_room AS (
  SELECT room, len_bucket, avg(rel) AS rel_bucket, count(*) AS n
  FROM feat GROUP BY room, len_bucket
  HAVING count(*) >= 5
)
SELECT 'emote_vs_plain' AS feature, count(*) AS rooms,
  sum(n_emote + n_plain) AS msgs_in_rooms,
  round(percentile_cont(0.5) WITHIN GROUP (ORDER BY rel_emote / NULLIF(rel_plain, 0))::numeric, 3) AS median_ratio,
  round(avg((rel_emote > rel_plain)::int)::numeric, 2) AS share_positive
FROM emote_room
UNION ALL
SELECT 'len_' || len_bucket, count(*), sum(n),
  round(percentile_cont(0.5) WITHIN GROUP (ORDER BY rel_bucket)::numeric, 3),
  round(avg((rel_bucket > 1)::int)::numeric, 2)
FROM len_room GROUP BY len_bucket
ORDER BY 1
;

WITH multi30 AS (SELECT chatter_login FROM twitch_chat_messages WHERE message_ts >= '2026-02-01' AND message_ts < '2026-10-05' GROUP BY chatter_login HAVING count(DISTINCT streamer_login) >= 30), msgs AS (
  SELECT m.id, m.session_id, m.streamer_login,
    CASE WHEN m.chatter_login = m.streamer_login THEN 'broadcaster'
         WHEN m.chatter_login IN (SELECT chatter_login FROM multi30) OR m.chatter_login LIKE '%bot' OR m.chatter_login IN ('streamelements','streamlabs') THEN 'bot'
         ELSE 'viewer' END AS role,
    (m.content LIKE '!%') AS is_bang
  FROM twitch_chat_messages m
  WHERE m.message_ts >= '2026-02-01' AND m.message_ts < '2026-10-05'
),
sess AS (
  SELECT session_id, streamer_login, count(*) FILTER (WHERE role = 'viewer' AND NOT is_bang) AS vmsgs
  FROM msgs GROUP BY session_id, streamer_login
  HAVING count(*) FILTER (WHERE role = 'viewer' AND NOT is_bang) >= 10
),
st AS (
  SELECT streamer_login, percentile_cont(0.5) WITHIN GROUP (ORDER BY vmsgs) AS med, count(*) AS n_sess
  FROM sess GROUP BY streamer_login HAVING count(*) >= 3
),
sq AS (
  SELECT s.session_id, s.streamer_login, s.vmsgs::float / st.med AS rel,
    ntile(4) OVER (ORDER BY s.vmsgs::float / st.med) AS quartile
  FROM sess s JOIN st ON st.streamer_login = s.streamer_login
),
lab AS (
  SELECT m.session_id, l.label, count(*) AS n
  FROM msgs m JOIN sq ON sq.session_id = m.session_id
  JOIN twitch_chat_message_labels l ON l.message_id = m.id
  WHERE m.role = 'viewer' AND NOT m.is_bang
  GROUP BY m.session_id, l.label
),
tot AS (
  SELECT sq.quartile, sq.session_id, sum(lab.n) AS labelled
  FROM sq JOIN lab ON lab.session_id = sq.session_id
  GROUP BY sq.quartile, sq.session_id
)
SELECT t.quartile, lab.label,
  count(DISTINCT t.session_id) AS sessions,
  sum(lab.n) AS msgs,
  round((sum(lab.n)::numeric / sum(sum(lab.n)) OVER (PARTITION BY t.quartile)), 4) AS share_in_quartile
FROM tot t JOIN lab ON lab.session_id = t.session_id
GROUP BY t.quartile, lab.label
ORDER BY t.quartile, msgs DESC
;

WITH multi30 AS (SELECT chatter_login FROM twitch_chat_messages WHERE message_ts >= '2026-02-01' AND message_ts < '2026-10-05' GROUP BY chatter_login HAVING count(DISTINCT streamer_login) >= 30), msgs AS (
  SELECT m.id, m.session_id, m.streamer_login,
    CASE WHEN m.chatter_login = m.streamer_login THEN 'broadcaster'
         WHEN m.chatter_login IN (SELECT chatter_login FROM multi30) OR m.chatter_login LIKE '%bot' OR m.chatter_login IN ('streamelements','streamlabs') THEN 'bot'
         ELSE 'viewer' END AS role,
    (m.content LIKE '!%') AS is_bang
  FROM twitch_chat_messages m
  WHERE m.message_ts >= '2026-02-01' AND m.message_ts < '2026-10-05'
),
sess AS (
  SELECT session_id, streamer_login, count(*) FILTER (WHERE role = 'viewer' AND NOT is_bang) AS vmsgs
  FROM msgs GROUP BY session_id, streamer_login
  HAVING count(*) FILTER (WHERE role = 'viewer' AND NOT is_bang) >= 10
),
st AS (
  SELECT streamer_login, percentile_cont(0.5) WITHIN GROUP (ORDER BY vmsgs) AS med, count(*) AS n_sess
  FROM sess GROUP BY streamer_login HAVING count(*) >= 3
),
sq AS (
  SELECT s.session_id, s.streamer_login, s.vmsgs::float / st.med AS rel,
    ntile(4) OVER (ORDER BY s.vmsgs::float / st.med) AS quartile
  FROM sess s JOIN st ON st.streamer_login = s.streamer_login
)
SELECT sq.quartile, count(*) AS sessions, count(DISTINCT sq.streamer_login) AS streamers,
  round(min(sq.rel)::numeric, 2) AS rel_min, round(max(sq.rel)::numeric, 2) AS rel_max,
  (SELECT count(*) FROM msgs m JOIN sq s2 ON s2.session_id = m.session_id
     WHERE s2.quartile = sq.quartile AND m.role = 'viewer' AND NOT m.is_bang
       AND EXISTS (SELECT 1 FROM twitch_chat_message_labels l WHERE l.message_id = m.id)) AS labelled_viewer_msgs
FROM sq
GROUP BY sq.quartile
ORDER BY sq.quartile
;


WITH multi30 AS (SELECT chatter_login FROM twitch_chat_messages WHERE message_ts >= '2026-02-01' AND message_ts < '2026-10-05' GROUP BY chatter_login HAVING count(DISTINCT streamer_login) >= 30), msg AS (
  SELECT m.id, m.session_id, m.streamer_login, m.chatter_login, m.message_ts,
    CASE WHEN m.chatter_login = m.streamer_login THEN 'broadcaster'
         WHEN m.chatter_login IN (SELECT chatter_login FROM multi30) OR m.chatter_login LIKE '%bot' OR m.chatter_login IN ('streamelements','streamlabs') THEN 'bot'
         ELSE 'viewer' END AS role
  FROM twitch_chat_messages m
  WHERE m.message_ts >= '2026-02-01' AND m.message_ts < '2026-10-05'
),
firsts AS (
  SELECT msg.*, (message_ts = min(message_ts) OVER (PARTITION BY session_id, chatter_login)) AS is_first
  FROM msg
),
win AS (
  SELECT f.id, f.session_id, f.streamer_login, f.chatter_login, f.role, f.message_ts,
    count(*) FILTER (WHERE role <> 'bot') OVER s2 - count(*) FILTER (WHERE role <> 'bot') OVER c2 AS f2,
    count(*) FILTER (WHERE role <> 'bot') OVER s5 - count(*) FILTER (WHERE role <> 'bot') OVER c5 AS f5,
    count(*) FILTER (WHERE is_first AND role <> 'bot') OVER s5 AS new5,
    count(*) FILTER (WHERE role = 'broadcaster') OVER s2 - count(*) FILTER (WHERE role = 'broadcaster') OVER c2 AS bc2
  FROM firsts f
  WINDOW
    s2 AS (PARTITION BY session_id ORDER BY message_ts RANGE BETWEEN INTERVAL '1 microsecond' FOLLOWING AND INTERVAL '2 minutes' FOLLOWING),
    c2 AS (PARTITION BY session_id, chatter_login ORDER BY message_ts RANGE BETWEEN INTERVAL '1 microsecond' FOLLOWING AND INTERVAL '2 minutes' FOLLOWING),
    s5 AS (PARTITION BY session_id ORDER BY message_ts RANGE BETWEEN INTERVAL '1 microsecond' FOLLOWING AND INTERVAL '5 minutes' FOLLOWING),
    c5 AS (PARTITION BY session_id, chatter_login ORDER BY message_ts RANGE BETWEEN INTERVAL '1 microsecond' FOLLOWING AND INTERVAL '5 minutes' FOLLOWING)
),
scored AS (
  SELECT w.*, l.label
  FROM win w JOIN twitch_chat_message_labels l ON l.message_id = w.id
),
sess_all AS (
  SELECT session_id, streamer_login, count(*) AS n_all, avg(f2) AS f2_all, avg(f5) AS f5_all,
         avg((f2 > 0)::int) AS r2_all, avg(new5) AS new5_all
  FROM scored GROUP BY 1, 2 HAVING count(*) >= 30
),
sess_type AS (
  SELECT s.session_id, s.streamer_login, s.role, s.label, count(*) AS n,
         avg(s.f2) AS f2, avg(s.f5) AS f5, avg((s.f2 > 0)::int) AS r2, avg(s.new5) AS new5
  FROM scored s JOIN sess_all a ON a.session_id = s.session_id
  GROUP BY 1, 2, 3, 4 HAVING count(*) >= 3
),
lift AS (
  SELECT t.streamer_login, t.role, t.label, t.session_id, t.n,
         t.f2 / NULLIF(a.f2_all, 0) AS lift_f2, t.f5 / NULLIF(a.f5_all, 0) AS lift_f5,
         t.r2 / NULLIF(a.r2_all, 0) AS lift_r2, t.new5 / NULLIF(a.new5_all, 0) AS lift_new5
  FROM sess_type t JOIN sess_all a ON a.session_id = t.session_id
),
streamer_lift AS (
  SELECT streamer_login, role, label, count(*) AS sessions, sum(n) AS msgs,
    percentile_cont(0.5) WITHIN GROUP (ORDER BY lift_f2) AS s_lift_f2,
    percentile_cont(0.5) WITHIN GROUP (ORDER BY lift_f5) AS s_lift_f5,
    percentile_cont(0.5) WITHIN GROUP (ORDER BY lift_r2) AS s_lift_r2,
    percentile_cont(0.5) WITHIN GROUP (ORDER BY lift_new5) AS s_lift_new5
  FROM lift WHERE lift_f2 IS NOT NULL AND lift_f5 IS NOT NULL AND lift_r2 IS NOT NULL AND lift_new5 IS NOT NULL
  GROUP BY 1, 2, 3
)
SELECT role, label, count(*) AS streamers, sum(sessions) AS sessions, sum(msgs) AS msgs,
  round(percentile_cont(0.5) WITHIN GROUP (ORDER BY s_lift_f2)::numeric, 2) AS med_lift_f2_2m,
  round(avg((s_lift_f2 > 1)::int)::numeric, 2) AS share_str_gt1_f2,
  round(percentile_cont(0.5) WITHIN GROUP (ORDER BY s_lift_f5)::numeric, 2) AS med_lift_f5_5m,
  round(avg((s_lift_f5 > 1)::int)::numeric, 2) AS share_str_gt1_f5,
  round(percentile_cont(0.5) WITHIN GROUP (ORDER BY s_lift_r2)::numeric, 2) AS med_lift_reply_share_2m,
  round(percentile_cont(0.5) WITHIN GROUP (ORDER BY s_lift_new5)::numeric, 2) AS med_lift_new_chatters_5m
FROM streamer_lift
GROUP BY 1, 2
HAVING count(*) >= 5
ORDER BY 1, 2
;

WITH s AS (
  SELECT id, started_at FROM twitch_stream_sessions
  WHERE started_at >= '2026-02-01' AND started_at < '2026-10-05'
),
m AS (
  SELECT session_id, message_ts FROM twitch_chat_messages
  WHERE message_ts >= '2026-02-01' AND message_ts < '2026-10-05'
),
mo AS (
  SELECT m.session_id, extract(epoch FROM m.message_ts - s.started_at) / 60.0 AS off_min
  FROM m JOIN s ON s.id = m.session_id
),
fs AS (
  SELECT session_id, min(off_min) AS first_off, count(*) AS n_msgs FROM mo GROUP BY session_id
)
SELECT
  CASE WHEN n_msgs < 10 THEN 'a_under_10_msgs'
       WHEN n_msgs < 50 THEN 'b_10_to_49_msgs'
       WHEN n_msgs < 200 THEN 'c_50_to_199_msgs'
       ELSE 'd_200_plus_msgs' END AS size_class,
  count(*) AS sessions,
  round(100.0 * avg((first_off > 15)::int), 1) AS pct_first_msg_after_min15,
  round(100.0 * avg((first_off > 5)::int), 1) AS pct_first_msg_after_min5,
  round(percentile_cont(0.5) WITHIN GROUP (ORDER BY first_off)::numeric, 2) AS median_first_msg_min
FROM fs
GROUP BY 1
ORDER BY 1
;

WITH s AS (
  SELECT id, started_at FROM twitch_stream_sessions
  WHERE started_at >= '2026-02-01' AND started_at < '2026-10-05'
),
m AS (
  SELECT session_id, message_ts FROM twitch_chat_messages
  WHERE message_ts >= '2026-02-01' AND message_ts < '2026-10-05'
),
mo AS (
  SELECT m.session_id, extract(epoch FROM m.message_ts - s.started_at) / 60.0 AS off_min
  FROM m JOIN s ON s.id = m.session_id
),
fs AS (
  SELECT session_id, min(off_min) AS first_off, count(*) AS n_msgs FROM mo GROUP BY session_id
)
SELECT
  CASE WHEN first_off < 0 THEN 'a_negative'
       WHEN first_off < 1 THEN 'b_0_to_1'
       WHEN first_off < 2 THEN 'c_1_to_2'
       WHEN first_off < 5 THEN 'd_2_to_5'
       WHEN first_off < 10 THEN 'e_5_to_10'
       WHEN first_off < 15 THEN 'f_10_to_15'
       WHEN first_off < 20 THEN 'g_15_to_20'
       WHEN first_off < 30 THEN 'h_20_to_30'
       WHEN first_off < 60 THEN 'i_30_to_60'
       ELSE 'j_60_plus' END AS first_msg_bin,
  count(*) AS sessions,
  count(*) FILTER (WHERE n_msgs >= 50) AS sessions_50_plus_msgs,
  round(100.0 * count(*) / sum(count(*)) OVER (), 1) AS pct_all
FROM fs
GROUP BY 1
ORDER BY 1
;

WITH s AS (
  SELECT id, started_at FROM twitch_stream_sessions
  WHERE started_at >= '2026-02-01' AND started_at < '2026-10-05'
),
m AS (
  SELECT session_id, message_ts FROM twitch_chat_messages
  WHERE message_ts >= '2026-02-01' AND message_ts < '2026-10-05'
),
mo AS (
  SELECT m.session_id, extract(epoch FROM m.message_ts - s.started_at) / 60.0 AS off_min
  FROM m JOIN s ON s.id = m.session_id
),
fs AS (
  SELECT session_id, min(off_min) AS first_off FROM mo GROUP BY session_id
)
SELECT
  (SELECT count(*) FROM m) AS msgs_window,
  (SELECT count(*) FROM mo) AS msgs_matched_to_session,
  round(100.0 * (SELECT count(*) FROM mo WHERE off_min > 15) / (SELECT count(*) FROM mo), 1) AS pct_msgs_after_min15,
  round(100.0 * (SELECT count(*) FROM mo WHERE off_min < 0) / (SELECT count(*) FROM mo), 2) AS pct_msgs_before_start,
  (SELECT count(*) FROM s) AS sessions_window,
  (SELECT count(*) FROM fs) AS sessions_with_chat_matched,
  round(100.0 * (SELECT count(*) FROM fs WHERE first_off > 15) / (SELECT count(*) FROM fs), 1) AS pct_sessions_first_msg_after_min15,
  round(100.0 * (SELECT count(*) FROM fs WHERE first_off > 5) / (SELECT count(*) FROM fs), 1) AS pct_sessions_first_msg_after_min5
;

WITH multi30 AS (SELECT chatter_login FROM twitch_chat_messages WHERE message_ts >= '2026-02-01' AND message_ts < '2026-10-05' GROUP BY chatter_login HAVING count(DISTINCT streamer_login) >= 30), base AS (
  SELECT m.id, m.session_id, m.streamer_login, m.chatter_login, m.message_ts,
    CASE WHEN m.chatter_login = m.streamer_login THEN 'broadcaster'
         WHEN m.chatter_login IN (SELECT chatter_login FROM multi30) OR m.chatter_login LIKE '%bot' OR m.chatter_login IN ('streamelements','streamlabs') THEN 'bot'
         ELSE 'viewer' END AS role,
    l.label
  FROM twitch_chat_messages m
  LEFT JOIN twitch_chat_message_labels l ON l.message_id = m.id
  WHERE m.message_ts >= '2026-02-01' AND m.message_ts < '2026-10-05'
),
chan AS (
  SELECT b.*,
    (message_ts = min(message_ts) OVER (PARTITION BY streamer_login, chatter_login)) AS is_chan_first,
    count(*) FILTER (WHERE role <> 'bot') OVER (PARTITION BY session_id, chatter_login) AS own_sess,
    count(*) OVER (PARTITION BY streamer_login, chatter_login) AS own_chan
  FROM base b
),
nc AS (
  SELECT * FROM chan
  WHERE is_chan_first AND role = 'viewer' AND message_ts >= '2026-03-01' AND message_ts < '2026-09-05' AND label IS NOT NULL
),
ns AS (
  SELECT session_id, streamer_login, label, own_sess, (own_chan > own_sess)::int AS returned, (own_sess >= 3)::int AS stayed3
  FROM nc
),
sess_mean AS (
  SELECT session_id, streamer_login, count(*) AS n, avg(own_sess) AS m_stay, avg(returned) AS m_ret, avg(stayed3) AS m_st3
  FROM ns GROUP BY 1, 2 HAVING count(*) >= 2
),
lab AS (
  SELECT ns.session_id, ns.streamer_login, ns.label, count(*) AS n_l,
    avg(ns.own_sess) AS stay_l, avg(ns.returned) AS ret_l, avg(ns.stayed3) AS st3_l,
    s.m_stay, s.m_ret, s.m_st3
  FROM ns JOIN sess_mean s ON s.session_id = ns.session_id
  GROUP BY ns.session_id, ns.streamer_login, ns.label, s.m_stay, s.m_ret, s.m_st3
),
per_str AS (
  SELECT streamer_login, label, count(*) AS sessions, sum(n_l) AS n,
    percentile_cont(0.5) WITHIN GROUP (ORDER BY stay_l / NULLIF(m_stay, 0)) AS s_stay_rel
  FROM lab WHERE n_l >= 1
  GROUP BY 1, 2
),
label_out AS (
  SELECT label, count(DISTINCT streamer_login) AS streamers, sum(sessions) AS sessions,
    round(percentile_cont(0.5) WITHIN GROUP (ORDER BY s_stay_rel)::numeric, 2) AS med_stay_rel_session
  FROM per_str GROUP BY label
),
pooled AS (
  SELECT label, count(*) AS n, round(avg(own_sess)::numeric, 2) AS mean_msgs_in_session,
    round(avg(stayed3)::numeric, 3) AS share_3plus_in_session, round(avg(returned)::numeric, 3) AS share_returned_later
  FROM ns GROUP BY label
),
share_all AS (
  SELECT label, count(*) AS n FROM chan WHERE role = 'viewer' AND message_ts >= '2026-03-01' AND label IS NOT NULL GROUP BY label
)
SELECT p.label, p.n AS newcomer_first_msgs, round(100.0 * p.n / sum(p.n) OVER (), 1) AS pct_first_msgs,
  round(100.0 * a.n / sum(a.n) OVER (), 1) AS pct_all_viewer_msgs,
  p.mean_msgs_in_session, p.share_3plus_in_session, p.share_returned_later,
  o.med_stay_rel_session, o.sessions, o.streamers
FROM pooled p
LEFT JOIN label_out o ON o.label = p.label
LEFT JOIN share_all a ON a.label = p.label
ORDER BY p.n DESC
;
