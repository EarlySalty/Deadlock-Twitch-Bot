-- P05 lurkers vs chatters: all SQL relied on. Run read-only via /tmp/tq.sh -f <file> (or --csv -f).
-- Post-processing was done in stdlib python on the CSV exports (kept in /tmp/reichweite-scratch/P05/, not in this folder).
-- Superseded: q_slots.sql (fixed session list) and q_conv_window.sql (fixed login list); replaced by q_base_sessions.sql and q_firstwrite_window.sql.
-- q_conversion.sql window columns use first_message_at and are superseded by q_firstwrite_window.sql; its sighting and pair columns are used.
-- Label coverage: twitch_chat_message_labels covers every message before 2026-10-05 00:00 UTC and 64 to 78% per day after (63.5 to 78.0%, 2026-10-05 to 2026-10-25, q_label_coverage.sql).
-- Every Question count below (q_sessions, q_conversion, q_firstwrite_window, q_velocity and the bot-filtered twins) is used only for sessions with
-- started_at + duration_seconds <= 2026-10-05 00:00 UTC; for later sessions the exported label counts are set to missing in python (lbl_build.py, labelcov.py).

-- ===== q_sessions.sql
-- Q-S1: session-level dataset, chat-tracked window (sessions from 2026-07-01)
with s as (
  select id, streamer_login, started_at, duration_seconds, avg_viewers, peak_viewers, samples,
         unique_chatters, first_time_chatters, returning_chatters, follower_delta,
         stream_title, game_name, language, had_deadlock_in_session, tags
  from twitch_stream_sessions
  where started_at >= '2026-07-01' and started_at < '2026-10-26'
),
sc as (
  select session_id,
         count(*) seen,
         count(*) filter (where messages > 0) chatters,
         sum(messages) msgs_sc
  from twitch_session_chatters
  where session_id in (select id from s)
  group by 1
),
cm as (
  select m.session_id,
         count(*) msgs,
         count(*) filter (where m.streamer_login = m.chatter_login) self_msgs,
         count(*) filter (where m.is_command) cmd_msgs,
         count(*) filter (where l.label = 'Question') question_msgs,
         count(*) filter (where l.label = 'Command') command_label_msgs,
         count(*) filter (where l.label = 'Greeting') greeting_msgs,
         count(*) filter (where l.label = 'Hype') hype_msgs,
         count(distinct m.chatter_login) chat_distinct,
         count(*) filter (where m.message_ts - s.started_at < interval '5 minutes') msgs_first5min
  from twitch_chat_messages m
  join s on s.id = m.session_id
  left join twitch_chat_message_labels l on l.message_id = m.id
  group by 1
)
select s.id session_id, s.streamer_login, s.started_at, s.duration_seconds, s.avg_viewers, s.peak_viewers,
       s.samples, s.unique_chatters, s.first_time_chatters, s.returning_chatters, s.follower_delta,
       s.stream_title, s.game_name, s.language, s.had_deadlock_in_session, s.tags,
       coalesce(sc.seen,0) seen, coalesce(sc.chatters,0) chatters, coalesce(sc.msgs_sc,0) msgs_sc,
       coalesce(cm.msgs,0) msgs, coalesce(cm.self_msgs,0) self_msgs, coalesce(cm.cmd_msgs,0) cmd_msgs,
       coalesce(cm.question_msgs,0) question_msgs, coalesce(cm.command_label_msgs,0) command_label_msgs,
       coalesce(cm.greeting_msgs,0) greeting_msgs, coalesce(cm.hype_msgs,0) hype_msgs,
       coalesce(cm.chat_distinct,0) chat_distinct, coalesce(cm.msgs_first5min,0) msgs_first5min,
       (sc.session_id is not null) has_sc, (cm.session_id is not null) has_msgs,
       coalesce(sc.seen,0) - coalesce((select count(*) from twitch_session_chatters c where c.session_id = s.id and c.chatter_login = s.streamer_login),0) seen_ex,
       coalesce(sc.chatters,0) - (case when coalesce(cm.self_msgs,0) > 0 then 1 else 0 end) chatters_ex,
       coalesce((select count(*) from twitch_session_chatters c where c.session_id = s.id and c.chatter_login = s.streamer_login),0) self_listed
from s
left join sc on sc.session_id = s.id
left join cm on cm.session_id = s.id
order by s.started_at;

-- ===== q_lurk_month.sql
-- Q-L1: lurker vs chatter rows per session by month (broadcaster excluded), full history
with s as (select id, streamer_login, started_at, avg_viewers from twitch_stream_sessions where started_at >= '2026-01-01'),
c as (
  select c.session_id,
         count(*) filter (where c.chatter_login <> s.streamer_login and c.messages = 0) lurk_rows,
         count(*) filter (where c.chatter_login <> s.streamer_login and c.messages > 0) chat_rows
  from twitch_session_chatters c join s on s.id = c.session_id
  group by c.session_id
)
select date_trunc('month', s.started_at)::date mon,
       count(*) sessions,
       count(c.session_id) with_rows,
       sum(c.lurk_rows) lurk_rows, sum(c.chat_rows) chat_rows,
       round((sum(c.lurk_rows)::numeric / nullif(sum(c.lurk_rows)+sum(c.chat_rows),0)),3) lurker_share_rows,
       count(*) filter (where c.session_id is not null and c.chat_rows = 0 and c.lurk_rows > 0) sessions_only_lurkers
from s left join c on c.session_id = s.id
group by 1 order by 1;

-- ===== q_overlap.sql
-- Q-X1: tracked chat (twitch_chat_messages) vs whole-category chat (category_chat_messages) per session, since 2026-09-28, avg>=5
with s as (
  select id, twitch_user_id, started_at, coalesce(ended_at, started_at + make_interval(secs => duration_seconds)) end_at,
         duration_seconds, avg_viewers, peak_viewers
  from twitch_stream_sessions
  where started_at >= '2026-09-28' and started_at < '2026-10-25' and avg_viewers >= 5 and twitch_user_id is not null
),
cat as (
  select s.id, count(*) cat_msgs, count(*) filter (where c.chatter_user_id = c.room_user_id) cat_self,
         count(distinct c.chatter_user_id) cat_chatters
  from s join category_chat_messages c
    on c.room_user_id = s.twitch_user_id and c.sent_at >= s.started_at and c.sent_at < s.end_at
  where c.sent_at >= '2026-09-28' and c.sent_at < '2026-10-25'
  group by s.id
),
tr as (
  select m.session_id id, count(*) tr_msgs, count(distinct m.chatter_login) tr_chatters
  from twitch_chat_messages m join s on s.id = m.session_id
  group by m.session_id
)
select s.id session_id, s.avg_viewers, s.peak_viewers, s.duration_seconds, coalesce(cat.cat_msgs,0) cat_msgs,
       coalesce(cat.cat_self,0) cat_self, coalesce(cat.cat_chatters,0) cat_chatters,
       coalesce(tr.tr_msgs,0) tr_msgs, coalesce(tr.tr_chatters,0) tr_chatters
from s left join cat on cat.id = s.id left join tr on tr.id = s.id
order by s.avg_viewers desc;

-- ===== q_sightings.sql
-- Q-L2: viewer sightings per streamer session (broadcaster excluded), since 2026-04-01 (lurker rows and chat rows)
select c.streamer_login, c.chatter_login, c.session_id, s.started_at, s.avg_viewers, s.duration_seconds,
       c.messages, c.first_message_at, c.seen_via_chatters_api
from twitch_session_chatters c
join twitch_stream_sessions s on s.id = c.session_id
where s.started_at >= '2026-04-01' and c.chatter_login <> s.streamer_login
order by c.streamer_login, c.chatter_login, s.started_at;

-- ===== q_conversion.sql
-- Q-L3: every viewer sighting (broadcaster excluded) with session features; window features for first-write sightings
-- since 2026-04-01. Session features: viewer chat msgs, broadcaster msgs, question labels, command msgs.
with sg as (
  select c.streamer_login, c.chatter_login, c.session_id, s.started_at, s.avg_viewers, s.duration_seconds,
         c.messages, c.first_message_at, c.seen_via_chatters_api,
         row_number() over (partition by c.streamer_login, c.chatter_login order by s.started_at, c.session_id) rn
  from twitch_session_chatters c
  join twitch_stream_sessions s on s.id = c.session_id
  where s.started_at >= '2026-04-01' and c.chatter_login <> s.streamer_login
),
sess as (
  select m.session_id,
         count(*) filter (where m.streamer_login <> m.chatter_login) viewer_msgs,
         count(*) filter (where m.streamer_login = m.chatter_login) self_msgs,
         count(*) filter (where m.is_command) cmd_msgs,
         count(*) filter (where l.label = 'Question') q_msgs
  from twitch_chat_messages m
  left join twitch_chat_message_labels l on l.message_id = m.id
  where m.session_id in (select session_id from sg)
  group by 1
),
fw as (
  select sg.*, row_number() over (partition by sg.streamer_login, sg.chatter_login order by sg.rn) fwr
  from sg where sg.messages > 0
),
lurk_first as (
  select streamer_login, chatter_login from sg where rn = 1 and messages = 0
),
win as (
  select f.streamer_login, f.chatter_login, f.session_id,
         count(*) msgs_10m_before,
         count(*) filter (where m.streamer_login = m.chatter_login) self_10m_before,
         count(*) filter (where l.label = 'Question') q_10m_before,
         count(distinct m.chatter_login) chatters_10m_before
  from fw f
  join twitch_chat_messages m
    on m.session_id = f.session_id
   and m.message_ts >= f.first_message_at - interval '10 minutes'
   and m.message_ts < f.first_message_at
  left join twitch_chat_message_labels l on l.message_id = m.id
  where f.fwr = 1
    and (f.streamer_login, f.chatter_login) in (select streamer_login, chatter_login from lurk_first)
  group by 1,2,3
)
select sg.streamer_login, sg.chatter_login, sg.session_id, sg.started_at, sg.avg_viewers, sg.duration_seconds,
       sg.messages, sg.rn, sg.seen_via_chatters_api,
       (sg.streamer_login, sg.chatter_login) in (select streamer_login, chatter_login from lurk_first) lurk_first,
       coalesce(sess.viewer_msgs,0) sess_viewer_msgs, coalesce(sess.self_msgs,0) sess_self_msgs,
       coalesce(sess.cmd_msgs,0) sess_cmd_msgs, coalesce(sess.q_msgs,0) sess_q_msgs,
       fw.first_message_at, win.msgs_10m_before, win.self_10m_before, win.q_10m_before, win.chatters_10m_before,
       (fw.fwr = 1) is_first_write
from sg
left join sess on sess.session_id = sg.session_id
left join fw on fw.session_id = sg.session_id and fw.streamer_login = sg.streamer_login and fw.chatter_login = sg.chatter_login
left join win on win.session_id = sg.session_id and win.streamer_login = sg.streamer_login and win.chatter_login = sg.chatter_login
where (sg.streamer_login, sg.chatter_login) in (select streamer_login, chatter_login from lurk_first)
order by sg.streamer_login, sg.chatter_login, sg.rn;

-- ===== q_firstwrite_window.sql
-- Q-W1: lurker-first viewers and their first written chat session (since 2026-04-01).
-- Window = viewer messages in the 10 minutes before the first written message (first_ts = min(message_ts)).
with s as (
  select id, streamer_login, started_at, duration_seconds, avg_viewers
  from twitch_stream_sessions
  where started_at >= '2026-04-01' and started_at < '2026-10-26'
),
sg as (
  select c.streamer_login, c.chatter_login, c.session_id, s.started_at, s.duration_seconds, s.avg_viewers, c.messages,
         row_number() over (partition by c.streamer_login, c.chatter_login order by s.started_at, c.session_id) rn
  from twitch_session_chatters c
  join s on s.id = c.session_id
  where c.chatter_login <> s.streamer_login
),
lurk_first as (
  select streamer_login, chatter_login from sg where rn = 1 and messages = 0
),
fw as (
  select sg.streamer_login, sg.chatter_login, sg.session_id, sg.started_at, sg.duration_seconds, sg.avg_viewers,
         row_number() over (partition by sg.streamer_login, sg.chatter_login order by sg.started_at, sg.session_id) fwr
  from sg
  join lurk_first lf on lf.streamer_login = sg.streamer_login and lf.chatter_login = sg.chatter_login
  where sg.messages > 0
),
fm as (
  select fw.streamer_login, fw.chatter_login, fw.session_id, fw.started_at, fw.duration_seconds, fw.avg_viewers,
         min(m.message_ts) first_ts
  from fw
  join twitch_chat_messages m on m.session_id = fw.session_id and m.chatter_login = fw.chatter_login
   and m.streamer_login <> m.chatter_login
  where fw.fwr = 1
  group by 1, 2, 3, 4, 5, 6
)
select fm.session_id, fm.chatter_login, fm.first_ts, fm.duration_seconds, fm.avg_viewers, w.msgs_10m_before,
       w.self_10m_before, w.q_10m_before
from fm
join lateral (
  select count(*) msgs_10m_before,
         count(*) filter (where m.streamer_login = m.chatter_login) self_10m_before,
         count(*) filter (where l.label = 'Question') q_10m_before
  from twitch_chat_messages m
  left join twitch_chat_message_labels l on l.message_id = m.id
  where m.session_id = fm.session_id
    and m.message_ts >= fm.first_ts - interval '10 minutes' and m.message_ts < fm.first_ts
) w on true;

-- ===== q_base_sessions.sql
-- Q-B1: session grid for the empty-room base rate (since 2026-04-01, duration >= 10 min, avg > 0)
with s as (
  select id session_id, started_at, duration_seconds
  from twitch_stream_sessions
  where started_at >= '2026-04-01' and started_at < '2026-10-26'
    and duration_seconds >= 600 and avg_viewers > 0
)
select s.session_id, s.duration_seconds, x.slot, count(*) msgs
from s
join twitch_chat_messages m on m.session_id = s.session_id
  and m.message_ts >= '2026-04-01' and m.message_ts < '2026-10-26'
cross join lateral (select floor(extract(epoch from (m.message_ts - s.started_at)) / 300)::int slot) x
group by 1, 2, 3
order by 1, 3;

-- ===== q_base_sessions_all.sql
-- Q-B2: session list for the base rate (every session since 2026-04-01 with duration >= 10 min)
select id session_id, duration_seconds
from twitch_stream_sessions
where started_at >= '2026-04-01' and started_at < '2026-10-26'
  and duration_seconds >= 600 and avg_viewers > 0;

-- ===== q_velocity.sql
-- Q-V1 (revised): 5-minute slots per session (since 2026-07-01, duration >= 10 min, avg > 0).
-- New chatter = first message of a viewer in the session, timed from twitch_chat_messages.message_ts
-- (twitch_session_chatters.first_message_at is not used: it differs from the real first message for about 29% of chatters).
with s as (
  select id, streamer_login, started_at, duration_seconds, avg_viewers
  from twitch_stream_sessions
  where started_at >= '2026-07-01' and started_at < '2026-10-26' and duration_seconds >= 600 and avg_viewers > 0
),
slots as (
  select s.id session_id, g.slot
  from s, lateral generate_series(0, floor(s.duration_seconds / 300.0)::int - 1) as g(slot)
),
m as (
  select m.session_id, floor(extract(epoch from (m.message_ts - s.started_at)) / 300)::int slot,
         count(*) filter (where m.streamer_login <> m.chatter_login) viewer_msgs,
         count(*) filter (where m.streamer_login = m.chatter_login) self_msgs,
         count(*) filter (where l.label = 'Question') q_msgs,
         count(*) filter (where m.is_command) cmd_msgs
  from twitch_chat_messages m
  join s on s.id = m.session_id
  left join twitch_chat_message_labels l on l.message_id = m.id
  group by 1, 2
),
first_msg as (
  select m.session_id, m.chatter_login, min(m.message_ts) first_ts
  from twitch_chat_messages m
  join s on s.id = m.session_id
  where m.streamer_login <> m.chatter_login
  group by 1, 2
),
f as (
  select fm.session_id, floor(extract(epoch from (fm.first_ts - s.started_at)) / 300)::int slot, count(*) new_starts
  from first_msg fm join s on s.id = fm.session_id
  group by 1, 2
)
select s.id session_id, s.avg_viewers, sl.slot,
       coalesce(m.viewer_msgs,0) viewer_msgs, coalesce(m.self_msgs,0) self_msgs,
       coalesce(m.q_msgs,0) q_msgs, coalesce(m.cmd_msgs,0) cmd_msgs, coalesce(f.new_starts,0) new_starts
from slots sl
join s on s.id = sl.session_id
left join m on m.session_id = sl.session_id and m.slot = sl.slot
left join f on f.session_id = sl.session_id and f.slot = sl.slot
order by s.id, sl.slot;

-- ===== q_cpvd_channel.sql
-- Q-C1: per channel: viewers per day, lurker share per channel-day (viewers with watch>0, chat=0), and channel size from sessions (same window)
with v as (
  select channel_twitch_user_id ch, day, count(*) filter (where watch_minutes > 0) viewers,
         count(*) filter (where watch_minutes > 0 and chat_messages = 0) lurkers,
         count(*) filter (where watch_minutes > 0 and chat_messages > 0) chatters,
         sum(watch_minutes) filter (where watch_minutes > 0) watch_min,
         sum(chat_messages) chat_total
  from twitch_community_points_viewer_daily
  where day >= '2026-08-13' and day <= '2026-10-25'
  group by 1, 2
),
sz as (
  select twitch_user_id ch, count(*) sessions, avg(avg_viewers) mean_avg, percentile_cont(0.5) within group (order by avg_viewers) med_avg
  from twitch_stream_sessions
  where started_at >= '2026-08-13' and started_at < '2026-10-26' and twitch_user_id is not null
  group by 1
)
select v.ch, count(*) days_with_data, sum(v.viewers) viewer_days, sum(v.lurkers) lurker_days, sum(v.chatters) chatter_days,
       round(sum(v.lurkers)::numeric/nullif(sum(v.viewers),0),3) lurker_share,
       round(avg(v.viewers),1) avg_daily_viewers, round(percentile_cont(0.5) within group (order by v.viewers)::numeric,1) med_daily_viewers,
       sum(v.chat_total) chat_total, sz.sessions, round(sz.med_avg::numeric,1) med_avg_viewers, round(sz.mean_avg::numeric,1) mean_avg_viewers
from v left join sz on sz.ch = v.ch
group by v.ch, sz.sessions, sz.med_avg, sz.mean_avg
order by med_avg_viewers desc nulls last;

-- ===== q_cpvd_seq.sql
-- Q-C2: viewer x channel x day sequence (watch_minutes > 0 days only) for lurker-to-chatter conversion
select twitch_user_id, channel_twitch_user_id, day, watch_minutes, chat_messages
from twitch_community_points_viewer_daily
where watch_minutes > 0
order by channel_twitch_user_id, twitch_user_id, day;

-- ===== q_cat_snap_hour.sql
-- Q-C1: live channel-hours from category snapshots (since 2026-10-09, first snapshots), sample_seconds as time weight
select date_trunc('hour', snapshot_at) hour_at, user_id room_user_id,
       sum(viewer_count * sample_seconds) / nullif(sum(sample_seconds), 0) avg_viewers,
       sum(sample_seconds) live_seconds,
       mode() within group (order by language) snap_lang
from category_snapshots_normalized
where snapshot_at >= '2026-10-09' and snapshot_at < '2026-10-26'
group by 1, 2;

-- ===== q_cat_roll_hour.sql
-- Q-C2: chat rollup per channel-hour, summed over languages (since 2026-10-09)
select hour_at, room_user_id, sum(messages) messages, sum(distinct_chatter) distinct_chatter,
       string_agg(language || ':' || messages, ',' order by messages desc) lang_split
from category_chat_rollup
where hour_at >= '2026-10-09' and hour_at < '2026-10-26'
group by 1, 2;

-- ===== q_cat_self_hour.sql
-- Q-C3: broadcaster self-chat per channel-hour (chatter = room owner), since 2026-10-09
select date_trunc('hour', sent_at) hour_at, room_user_id, count(*) self_msgs
from category_chat_messages
where sent_at >= '2026-10-09' and sent_at < '2026-10-26'
  and chatter_user_id = room_user_id
group by 1, 2;

-- ===== q_cat_exact_day.sql
-- Q-C7: exact per channel-hour chat counts from raw category chat, one day (shared-chat copies excluded)
select date_trunc('hour', sent_at) hour_at, room_user_id,
       count(distinct chatter_user_id) distinct_all,
       count(distinct chatter_user_id) filter (where chatter_user_id <> room_user_id) distinct_viewers,
       count(*) msgs,
       count(*) filter (where chatter_user_id = room_user_id) self_msgs
from category_chat_messages
where sent_at >= :'d'::timestamptz and sent_at < :'d'::timestamptz + interval '1 day'
  and not shared_chat_copy
group by 1, 2;

-- ===== q_cat_check.sql
-- Q-C4: validity check, rollup message totals vs raw category chat for one day, with and without shared-chat copies
select 'rollup_total' k, sum(messages)::text v from category_chat_rollup where hour_at >= '2026-10-20' and hour_at < '2026-10-21'
union all
select 'raw_total', count(*)::text from category_chat_messages where sent_at >= '2026-10-20' and sent_at < '2026-10-21'
union all
select 'raw_total_not_shared', count(*)::text from category_chat_messages where sent_at >= '2026-10-20' and sent_at < '2026-10-21' and not shared_chat_copy
union all
select 'raw_self', count(*)::text from category_chat_messages where sent_at >= '2026-10-20' and sent_at < '2026-10-21' and chatter_user_id = room_user_id;

-- ===== q_cat_distinct_check.sql
-- Q-C5: validity check, does rollup distinct_chatter include the broadcaster? (room-hours with self chat, one day)
with h as (
  select distinct date_trunc('hour', sent_at) hour_at, room_user_id
  from category_chat_messages
  where sent_at >= '2026-10-20' and sent_at < '2026-10-21' and chatter_user_id = room_user_id
  limit 300
),
raw as (
  select date_trunc('hour', m.sent_at) hour_at, m.room_user_id,
         count(distinct m.chatter_user_id) d_all,
         count(distinct m.chatter_user_id) filter (where m.chatter_user_id <> m.room_user_id) d_ex
  from category_chat_messages m
  join h on h.room_user_id = m.room_user_id and h.hour_at = date_trunc('hour', m.sent_at)
  where m.sent_at >= '2026-10-20' and m.sent_at < '2026-10-21' and not m.shared_chat_copy
  group by 1, 2
),
roll as (
  select hour_at, room_user_id, sum(distinct_chatter) dc
  from category_chat_rollup
  where hour_at >= '2026-10-20' and hour_at < '2026-10-21'
  group by 1, 2
)
select count(*) room_hours,
       count(*) filter (where roll.dc = raw.d_all) match_incl_owner,
       count(*) filter (where roll.dc = raw.d_ex) match_excl_owner,
       count(*) filter (where roll.dc is null) no_rollup_row
from raw left join roll on roll.room_user_id = raw.room_user_id and roll.hour_at = raw.hour_at;

-- ===== q_cat_distinct_diff.sql
-- Q-C6: distribution of rollup distinct_chatter minus raw distinct chatters (same day as Q-C5)
with h as (
  select distinct date_trunc('hour', sent_at) hour_at, room_user_id
  from category_chat_messages
  where sent_at >= '2026-10-20' and sent_at < '2026-10-21' and chatter_user_id = room_user_id
  limit 300
),
raw as (
  select date_trunc('hour', m.sent_at) hour_at, m.room_user_id,
         count(distinct m.chatter_user_id) d_all,
         count(distinct m.chatter_user_id) filter (where m.chatter_user_id <> m.room_user_id) d_ex,
         count(*) n_msg
  from category_chat_messages m
  join h on h.room_user_id = m.room_user_id and h.hour_at = date_trunc('hour', m.sent_at)
  where m.sent_at >= '2026-10-20' and m.sent_at < '2026-10-21' and not m.shared_chat_copy
  group by 1, 2
),
roll as (
  select hour_at, room_user_id, sum(distinct_chatter) dc, sum(messages) msgs, count(*) lang_rows
  from category_chat_rollup
  where hour_at >= '2026-10-20' and hour_at < '2026-10-21'
  group by 1, 2
)
select (roll.dc - raw.d_all) diff_vs_all, (roll.dc - raw.d_ex) diff_vs_ex, count(*) n,
       round(avg(roll.msgs::numeric / nullif(raw.n_msg, 0)), 3) msg_ratio, round(avg(roll.lang_rows), 2) avg_lang_rows
from raw left join roll on roll.room_user_id = raw.room_user_id and roll.hour_at = raw.hour_at
group by 1, 2
order by n desc
limit 12;


-- ===== Bot exclusion (added after the first run)
-- Bots = logins seen as chatters or viewers in 30+ channels (chatter_login with count(distinct streamer_login) >= 30, sessions since 2026-07-01).
-- The bot list is computed inside each query below (q_bot_ids.sql, q_bots_session.sql, q_cpvd_bots.sql, q_late_sampling.sql, and the bots CTEs of the *_nobot queries).
-- Python post-filters on exports (stdlib, scratch only): conv_nobot.csv = conv.csv rows whose chatter_login is in the bot list (q_bots_session.sql);
-- cpvd_seq_nobot.csv = cpvd_seq.csv rows whose twitch_user_id is in the bot ID list (q_bot_ids.sql);
-- sessions_nobot.csv = sessions_jul_oct.csv with chatters_ex reduced by bot chatters with messages (bots_session.csv), msgs and cmd_msgs reduced by bot_msgs / bot_cmd_msgs (q_bot_msgs_session.sql), question_msgs reduced by bot_question_msgs (q_bot_q_session.sql).
-- Per-day category query: run with -v d=YYYY-MM-DD for 2026-10-09 to 2026-10-25 and concatenate.

-- ===== q_bot_ids.sql
-- Q-K3: twitch user IDs of logins seen in 30+ channels since 2026-07-01 (bot list, scratch only)
with s as (select id, duration_seconds, avg_viewers, started_at from twitch_stream_sessions where started_at >= '2026-07-01' and started_at < '2026-10-26' and duration_seconds >= 600 and avg_viewers > 0),
b as (select c.chatter_login, max(c.chatter_id) chatter_id, count(distinct c.streamer_login) n_ch from twitch_session_chatters c join s on s.id = c.session_id where c.chatter_login <> (select streamer_login from twitch_stream_sessions t where t.id = c.session_id) group by 1)
select distinct chatter_id from b where n_ch >= 30 and chatter_id is not null;

-- ===== q_bots_session.sql
-- Q-K1: logins seen in 30+ channels since 2026-07-01 (bot list) and per-session lurker/chatter rows split by bot flag
with s as (
  select id, streamer_login, started_at, duration_seconds, avg_viewers
  from twitch_stream_sessions
  where started_at >= '2026-07-01' and started_at < '2026-10-26' and duration_seconds >= 600 and avg_viewers > 0
),
sc as (
  select c.session_id, c.chatter_login, c.streamer_login, c.messages, c.seen_via_chatters_api, c.last_seen_at
  from twitch_session_chatters c
  join s on s.id = c.session_id
  where c.chatter_login <> s.streamer_login
),
bots as (
  select chatter_login from sc group by 1 having count(distinct streamer_login) >= 30
)
select sc.session_id,
       count(*) filter (where sc.messages = 0 and sc.chatter_login not in (select chatter_login from bots)) lurk_nb,
       count(*) filter (where sc.messages > 0 and sc.chatter_login not in (select chatter_login from bots)) chat_nb,
       count(*) filter (where sc.chatter_login in (select chatter_login from bots)) bot_rows,
       count(*) filter (where sc.messages = 0 and sc.chatter_login in (select chatter_login from bots)) bot_lurk_rows,
       (select count(*) from bots) bot_logins
from sc
group by 1;

-- ===== q_cpvd_bots.sql
-- Q-C3: community points channel lurker shares without bot logins (seen in 30+ channels since 2026-07-01), 2026-08-13 to 2026-10-25
with bots as (
  select chatter_id from (
    select c.chatter_login, max(c.chatter_id) chatter_id, count(distinct c.streamer_login) n_ch
    from twitch_session_chatters c
    join twitch_stream_sessions s on s.id = c.session_id
    where s.started_at >= '2026-07-01' and s.started_at < '2026-10-26' and s.duration_seconds >= 600 and s.avg_viewers > 0
      and c.chatter_login <> s.streamer_login
    group by 1
  ) b where n_ch >= 30 and chatter_id is not null
),
v as (
  select channel_twitch_user_id ch, day, twitch_user_id, chat_messages,
         (twitch_user_id in (select chatter_id from bots)) is_bot
  from twitch_community_points_viewer_daily
  where day >= '2026-08-13' and day <= '2026-10-25' and watch_minutes > 0
),
sz as (
  select twitch_user_id ch, percentile_cont(0.5) within group (order by avg_viewers) med_avg
  from twitch_stream_sessions
  where started_at >= '2026-08-13' and started_at < '2026-10-26' and twitch_user_id is not null
  group by 1
)
select v.ch, count(*) filter (where not v.is_bot) viewer_days_nb,
       count(*) filter (where not v.is_bot and v.chat_messages = 0) lurker_days_nb,
       count(*) filter (where v.is_bot) bot_viewer_days,
       (select count(*) from bots) bot_ids,
       round(sz.med_avg::numeric, 1) med_avg_viewers
from v left join sz on sz.ch = v.ch
group by v.ch, sz.med_avg;

-- ===== q_late_sampling.sql
-- Q-K2: lurker rows (non-bot, sessions since 2026-07-01) by when they were last seen relative to session start
with s as (
  select id, started_at, duration_seconds
  from twitch_stream_sessions
  where started_at >= '2026-07-01' and started_at < '2026-10-26' and duration_seconds >= 600 and avg_viewers > 0
),
sc as (
  select c.session_id, c.chatter_login, c.chatter_id, c.streamer_login, c.messages, c.last_seen_at, s.started_at, s.duration_seconds
  from twitch_session_chatters c
  join s on s.id = c.session_id
  where c.chatter_login <> c.streamer_login and c.messages = 0
),
bots as (
  select chatter_login from (
    select c.chatter_login, count(distinct c.streamer_login) n_ch
    from twitch_session_chatters c join s on s.id = c.session_id
    where c.chatter_login <> c.streamer_login
    group by 1
  ) b where n_ch >= 30
)
select count(*) lurker_rows_nb,
       count(*) filter (where sc.last_seen_at is null) no_last_seen,
       round(avg((extract(epoch from (sc.last_seen_at - sc.started_at)) / 60.0 > 15)::int)::numeric, 3) share_last_seen_after_min15,
       round(avg(least(extract(epoch from (sc.last_seen_at - sc.started_at)) / sc.duration_seconds, 1))::numeric, 3) mean_last_seen_fraction,
       round(percentile_cont(0.5) within group (order by extract(epoch from (sc.last_seen_at - sc.started_at)) / sc.duration_seconds)::numeric, 3) median_last_seen_fraction
from sc
where sc.chatter_login not in (select chatter_login from bots);

-- ===== q_bot_msgs_session.sql
with s as (
  select id from twitch_stream_sessions
  where started_at >= '2026-07-01' and started_at < '2026-10-26' and duration_seconds >= 600 and avg_viewers > 0
),
bots as (
  select c.chatter_login from twitch_session_chatters c join s on s.id = c.session_id
  where c.chatter_login <> (select t.streamer_login from twitch_stream_sessions t where t.id = c.session_id)
  group by 1 having count(distinct c.streamer_login) >= 30
)
select m.session_id,
       count(*) filter (where m.chatter_login in (select chatter_login from bots) and m.streamer_login <> m.chatter_login) bot_msgs,
       count(*) filter (where m.chatter_login in (select chatter_login from bots) and m.streamer_login <> m.chatter_login and m.is_command) bot_cmd_msgs
from twitch_chat_messages m join s on s.id = m.session_id
where m.message_ts >= '2026-07-01' and m.message_ts < '2026-10-26'
group by 1 order by 1;

-- ===== q_bot_q_session.sql
with s as (
  select id from twitch_stream_sessions
  where started_at >= '2026-07-01' and started_at < '2026-10-26' and duration_seconds >= 600 and avg_viewers > 0
),
bots as (
  select c.chatter_login from twitch_session_chatters c join s on s.id = c.session_id
  where c.chatter_login <> (select t.streamer_login from twitch_stream_sessions t where t.id = c.session_id)
  group by 1 having count(distinct c.streamer_login) >= 30
)
select m.session_id,
       count(*) filter (where l.label = 'Question' and m.chatter_login in (select chatter_login from bots) and m.streamer_login <> m.chatter_login) bot_question_msgs
from twitch_chat_messages m
join s on s.id = m.session_id
left join twitch_chat_message_labels l on l.message_id = m.id
where m.message_ts >= '2026-07-01' and m.message_ts < '2026-10-26'
group by 1 order by 1;

-- ===== q_velocity_nobot.sql
with s as (
  select id, streamer_login, started_at, duration_seconds, avg_viewers
  from twitch_stream_sessions
  where started_at >= '2026-07-01' and started_at < '2026-10-26' and duration_seconds >= 600 and avg_viewers > 0
),
bots as (
  select c.chatter_login from twitch_session_chatters c join s on s.id = c.session_id
  where c.chatter_login <> (select t.streamer_login from twitch_stream_sessions t where t.id = c.session_id)
  group by 1 having count(distinct c.streamer_login) >= 30
),
slots as (
  select s.id session_id, g.slot
  from s, lateral generate_series(0, floor(s.duration_seconds / 300.0)::int - 1) as g(slot)
),
m as (
  select m.session_id, floor(extract(epoch from (m.message_ts - s.started_at)) / 300)::int slot,
         count(*) filter (where m.streamer_login <> m.chatter_login and b.chatter_login is null) viewer_msgs,
         count(*) filter (where m.streamer_login = m.chatter_login) self_msgs,
         count(*) filter (where l.label = 'Question' and b.chatter_login is null) q_msgs,
         count(*) filter (where m.is_command and b.chatter_login is null) cmd_msgs
  from twitch_chat_messages m
  join s on s.id = m.session_id
  left join bots b on b.chatter_login = m.chatter_login
  left join twitch_chat_message_labels l on l.message_id = m.id
  group by 1, 2
),
first_msg as (
  select m.session_id, m.chatter_login, min(m.message_ts) first_ts
  from twitch_chat_messages m
  join s on s.id = m.session_id
  left join bots b on b.chatter_login = m.chatter_login
  where m.streamer_login <> m.chatter_login and b.chatter_login is null
  group by 1, 2
),
f as (
  select fm.session_id, floor(extract(epoch from (fm.first_ts - s.started_at)) / 300)::int slot, count(*) new_starts
  from first_msg fm join s on s.id = fm.session_id
  group by 1, 2
)
select s.id session_id, s.avg_viewers, sl.slot,
       coalesce(m.viewer_msgs,0) viewer_msgs, coalesce(m.self_msgs,0) self_msgs,
       coalesce(m.q_msgs,0) q_msgs, coalesce(m.cmd_msgs,0) cmd_msgs, coalesce(f.new_starts,0) new_starts
from slots sl
join s on s.id = sl.session_id
left join m on m.session_id = sl.session_id and m.slot = sl.slot
left join f on f.session_id = sl.session_id and f.slot = sl.slot
order by s.id, sl.slot;

-- ===== q_base_sessions_nobot.sql
with bots as (
  select c.chatter_login from twitch_session_chatters c
  join twitch_stream_sessions s on s.id = c.session_id
  where s.started_at >= '2026-07-01' and s.started_at < '2026-10-26' and s.duration_seconds >= 600 and s.avg_viewers > 0
    and c.chatter_login <> s.streamer_login
  group by 1 having count(distinct c.streamer_login) >= 30
),
s as (
  select id session_id, started_at, duration_seconds
  from twitch_stream_sessions
  where started_at >= '2026-04-01' and started_at < '2026-10-26'
    and duration_seconds >= 600 and avg_viewers > 0
)
select s.session_id, s.duration_seconds, x.slot, count(*) msgs
from s
join twitch_chat_messages m on m.session_id = s.session_id
  and m.message_ts >= '2026-04-01' and m.message_ts < '2026-10-26'
left join bots b on b.chatter_login = m.chatter_login
cross join lateral (select floor(extract(epoch from (m.message_ts - s.started_at)) / 300)::int slot) x
where b.chatter_login is null
group by 1, 2, 3
order by 1, 3;

-- ===== q_firstwrite_window_nobot.sql
with s as (
  select id, streamer_login, started_at, duration_seconds, avg_viewers
  from twitch_stream_sessions
  where started_at >= '2026-04-01' and started_at < '2026-10-26'
),
bots as (
  select c.chatter_login from twitch_session_chatters c join s on s.id = c.session_id
  where c.chatter_login <> s.streamer_login
  group by 1 having count(distinct c.streamer_login) >= 30
),
sg as (
  select c.streamer_login, c.chatter_login, c.session_id, s.started_at, s.duration_seconds, s.avg_viewers, c.messages,
         row_number() over (partition by c.streamer_login, c.chatter_login order by s.started_at, c.session_id) rn
  from twitch_session_chatters c
  join s on s.id = c.session_id
  where c.chatter_login <> s.streamer_login
),
lurk_first as (
  select sg.streamer_login, sg.chatter_login from sg
  where sg.rn = 1 and sg.messages = 0
    and sg.chatter_login not in (select chatter_login from bots)
),
fw as (
  select sg.streamer_login, sg.chatter_login, sg.session_id, sg.started_at, sg.duration_seconds, sg.avg_viewers,
         row_number() over (partition by sg.streamer_login, sg.chatter_login order by sg.started_at, sg.session_id) fwr
  from sg
  join lurk_first lf on lf.streamer_login = sg.streamer_login and lf.chatter_login = sg.chatter_login
  where sg.messages > 0
),
fm as (
  select fw.streamer_login, fw.chatter_login, fw.session_id, fw.started_at, fw.duration_seconds, fw.avg_viewers,
         min(m.message_ts) first_ts
  from fw
  join twitch_chat_messages m on m.session_id = fw.session_id and m.chatter_login = fw.chatter_login
   and m.streamer_login <> m.chatter_login
  where fw.fwr = 1
  group by 1, 2, 3, 4, 5, 6
)
select fm.session_id, fm.chatter_login, fm.first_ts, fm.duration_seconds, fm.avg_viewers, w.msgs_10m_before,
       w.self_10m_before, w.q_10m_before
from fm
join lateral (
  select count(*) filter (where b.chatter_login is null) msgs_10m_before,
         count(*) filter (where m.streamer_login = m.chatter_login) self_10m_before,
         count(*) filter (where l.label = 'Question' and b.chatter_login is null) q_10m_before
  from twitch_chat_messages m
  left join bots b on b.chatter_login = m.chatter_login
  left join twitch_chat_message_labels l on l.message_id = m.id
  where m.session_id = fm.session_id
    and m.message_ts >= fm.first_ts - interval '10 minutes' and m.message_ts < fm.first_ts
) w on true;

-- ===== q_cat_exact_day_nobot.sql
with bots as (
  select max(c.chatter_id) chatter_id
  from twitch_session_chatters c join twitch_stream_sessions s on s.id = c.session_id
  where s.started_at >= '2026-07-01' and s.started_at < '2026-10-26' and s.duration_seconds >= 600 and s.avg_viewers > 0
    and c.chatter_login <> s.streamer_login
  group by c.chatter_login
  having count(distinct c.streamer_login) >= 30
),
bot_ids as (select chatter_id from bots where chatter_id is not null)
select date_trunc('hour', m.sent_at) hour_at, m.room_user_id,
       count(distinct m.chatter_user_id) filter (where m.chatter_user_id <> m.room_user_id and m.chatter_user_id not in (select chatter_id from bot_ids)) distinct_viewers_nb,
       count(distinct m.chatter_user_id) filter (where m.chatter_user_id <> m.room_user_id) distinct_viewers_all,
       count(*) filter (where m.chatter_user_id in (select chatter_id from bot_ids)) bot_msgs,
       count(*) filter (where m.chatter_user_id = m.room_user_id) self_msgs,
       count(*) msgs
from category_chat_messages m
where m.sent_at >= :'d'::timestamptz and m.sent_at < :'d'::timestamptz + interval '1 day'
  and not m.shared_chat_copy
group by 1, 2;

-- ===== q_label_coverage.sql
-- Q-LC: share of tracked chat messages with a label, per UTC day
select (m.message_ts at time zone 'UTC')::date d, count(*) msgs, count(l.message_id) labelled,
       round(100.0*count(l.message_id)/count(*),1) pct
from twitch_chat_messages m
left join twitch_chat_message_labels l on l.message_id = m.id
where m.message_ts >= '2026-09-28' and m.message_ts < '2026-10-26'
group by 1 order by 1;
