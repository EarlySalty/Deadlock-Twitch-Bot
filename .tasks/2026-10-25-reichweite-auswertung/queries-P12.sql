-- P12 (when Twitch shows a channel more often): extraction queries.
-- Run through /tmp/tq.sh (read-only, 180 s statement timeout).
-- Python post-processing (stdlib only) is described in FINDINGS-P12.md; scratch CSVs live in /tmp/reichweite-scratch/P12/.

-- Q-A: German minute panel (twitch_stats_category), exported per calendar month.
-- Trap rule: keep language like 'de%', keep NULL language only outside 2026-06-10..2026-06-13.
-- Parameters: from_ts, to_ts (psql -v).
select streamer,
       max(twitch_user_id) as user_id,
       date_trunc('minute', ts_utc) as minute_utc,
       round(avg(viewer_count)::numeric, 2) as viewers_avg,
       count(*) as n_samples,
       md5(max(coalesce(stream_title, ''))) as title_md5,
       max(coalesce(language, 'null')) as lang_seen
from twitch_stats_category
where ts_utc >= :'from_ts'::timestamptz
  and ts_utc <  :'to_ts'::timestamptz
  and (language like 'de%'
       or (language is null and not (ts_utc >= '2026-06-10' and ts_utc < '2026-06-14')))
group by streamer, date_trunc('minute', ts_utc);

-- Q-B: German streams rank among all category channels per snapshot (category_snapshots_normalized).
-- Data starts 2026-10-09 10:40 UTC. Parameters: from_ts, to_ts.
with s as (
  select snapshot_at, user_login, user_id, language, viewer_count,
         count(*) over (partition by snapshot_at) as n_all,
         rank() over (partition by snapshot_at order by viewer_count desc) as rank_all
  from category_snapshots_normalized
  where snapshot_at >= :'from_ts'::timestamptz and snapshot_at < :'to_ts'::timestamptz
)
select user_login, user_id, snapshot_at, viewer_count, n_all, rank_all
from s where language = 'de';

-- Q-C: session list since 2026-01-01 (twitch_stream_sessions).
select id as session_id, streamer_login, twitch_user_id, started_at, ended_at, duration_seconds,
       start_viewers, peak_viewers, end_viewers, avg_viewers, samples, retention_5m, retention_10m,
       retention_20m, unique_chatters, first_time_chatters, returning_chatters, follower_delta,
       language, had_deadlock_in_session, game_name
from twitch_stream_sessions
where started_at >= '2026-01-01'
order by started_at;

-- Q-D: chatter and lurker rows (twitch_session_chatters). first_message_at is a real chat time
-- when messages > 0; for seen_via_chatters_api rows with messages = 0 it is the first poll that saw the viewer.
select session_id, streamer_login, chatter_login, first_message_at, is_first_time_streamer,
       seen_via_chatters_api, messages
from twitch_session_chatters
where first_message_at >= '2026-01-01'
order by first_message_at;

-- Q-E: chat messages per session and minute (twitch_chat_messages).
select session_id, date_trunc('minute', message_ts) as m, count(*) as msgs,
       count(*) filter (where is_command) as cmds, count(distinct chatter_login) as chatters
from twitch_chat_messages
group by 1, 2
order by 1, 2;

-- Q-F: incoming raids (EventSub arrival tracking).
select to_broadcaster_id, to_broadcaster_login, from_broadcaster_id, detected_at, viewer_count,
       classification, primary_signal, correlation_status
from twitch_raid_arrival_tracking
order by detected_at;

-- Q-G: raids executed by the network (incoming and outgoing).
select to_broadcaster_id, to_broadcaster_login, executed_at, viewer_count, success, reason
from twitch_raid_history
where executed_at >= '2026-01-01'
order by executed_at;

select from_broadcaster_id, to_broadcaster_id, executed_at
from twitch_raid_history
where executed_at >= '2026-01-01' and success
order by executed_at;

-- Q-H: profile checks used for the flag semantics.
-- First-time flag: is_first_time_streamer = true rows should have no earlier row for the same streamer and chatter.
select c.is_first_time_streamer, count(*),
       count(*) filter (where exists (
         select 1 from twitch_session_chatters p
         where p.streamer_login = c.streamer_login and p.chatter_login = c.chatter_login
           and p.first_message_at < c.first_message_at - interval '1 minute')) as has_earlier
from twitch_session_chatters c
where c.first_message_at >= '2026-06-01' and c.first_message_at < '2026-07-01'
  and c.messages > 0
group by 1 order by 1;

-- Q-I: lurker rows by month (seen_via_chatters_api and messages = 0) against chat rows.
select date_trunc('month', first_message_at) as m,
       count(*) filter (where seen_via_chatters_api and messages = 0) as lurk_rows,
       count(*) filter (where not seen_via_chatters_api and messages > 0) as chat_rows,
       count(*) filter (where seen_via_chatters_api and messages > 0) as api_and_chat,
       count(*) filter (where not seen_via_chatters_api and messages = 0) as neither
from twitch_session_chatters
where first_message_at >= '2026-02-01'
group by 1 order by 1;

-- Q-J: lurker first_message_at and last_seen_at relation, by flags.
select seen_via_chatters_api, (messages = 0) as no_msgs, is_first_time_streamer, count(*),
       percentile_cont(0.5) within group (order by extract(epoch from last_seen_at - first_message_at) / 60) as med_span_min
from twitch_session_chatters
where first_message_at >= '2026-03-01'
group by 1, 2, 3 order by 1, 2, 3;

-- Q-K: German snapshot coverage check (same 5-minute window in both tables).
select 'stats' as src, count(distinct streamer)
from twitch_stats_category
where ts_utc >= '2026-10-20 19:00' and ts_utc < '2026-10-20 19:05' and (language like 'de%' or language is null);

select 'snap_de' as src, count(distinct user_login), count(distinct snapshot_at)
from category_snapshots_normalized
where snapshot_at >= '2026-10-20 19:00' and snapshot_at < '2026-10-20 19:05' and language = 'de';

-- Q-L: data range and volume checks.
select min(ts_utc), max(ts_utc), count(*) from twitch_stats_category;
select min(snapshot_at), max(snapshot_at), count(*), count(distinct user_id) from category_snapshots_normalized;
select count(*), min(first_message_at), max(first_message_at) from twitch_session_chatters;
select count(*), count(distinct session_id), min(message_ts), max(message_ts) from twitch_chat_messages;
select count(*), min(ts_utc), max(ts_utc) from twitch_session_viewers;
