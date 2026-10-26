with sess as (
  select s.id, lower(s.streamer_login) as login, s.started_at,
         coalesce(s.ended_at, s.started_at + make_interval(secs => s.duration_seconds)) as end_at,
         s.duration_seconds, s.peak_viewers, s.avg_viewers, s.start_viewers, s.stream_id
  from twitch_stream_sessions s
),
fol as (
  select f.id as follow_id, f.streamer_login as login, f.follower_id, f.followed_at
  from twitch_follow_events f
),
matched as (
  select fo.*, s.id as session_id, s.started_at, s.end_at, s.duration_seconds, s.peak_viewers, s.avg_viewers, s.start_viewers
  from fol fo
  left join lateral (
    select * from sess s
    where s.login = fo.login and s.started_at <= fo.followed_at and s.end_at >= fo.followed_at
    order by s.started_at desc, s.duration_seconds asc
    limit 1
  ) s on true
)
select m.follow_id, m.login, m.follower_id, m.followed_at, m.session_id, m.started_at, m.end_at,
       m.peak_viewers, m.avg_viewers, m.start_viewers,
       round((extract(epoch from (m.followed_at - m.started_at)) / 60)::numeric, 2) as minutes_into_stream,
       v.viewer_count as viewers_at_follow, v.gap_sec,
       pk.peak_ts, round((extract(epoch from (pk.peak_ts - m.followed_at)) / 60)::numeric, 2) as minutes_to_peak,
       coalesce(r.raids_in_session, 0) as raids_in_session_before,
       coalesce(r.raid_viewers_session, 0) as raid_viewers_in_session_before,
       coalesce(r10.raids_last10, 0) as raids_last_10min,
       coalesce(r10.raid_viewers_last10, 0) as raid_viewers_last_10min
from matched m
left join lateral (
  select sv.viewer_count, abs(extract(epoch from (sv.ts_utc - m.followed_at))) as gap_sec
  from twitch_session_viewers sv
  where m.session_id is not null and sv.session_id = m.session_id
  order by abs(extract(epoch from (sv.ts_utc - m.followed_at)))
  limit 1
) v on true
left join lateral (
  select sv.ts_utc as peak_ts from twitch_session_viewers sv
  where m.session_id is not null and sv.session_id = m.session_id
  order by sv.viewer_count desc, sv.ts_utc limit 1
) pk on true
left join lateral (
  select count(*) as raids_in_session, sum(rh.viewer_count) as raid_viewers_session
  from twitch_raid_history rh
  where m.session_id is not null and rh.to_broadcaster_login = m.login and rh.success
    and rh.executed_at >= m.started_at and rh.executed_at <= m.followed_at
) r on true
left join lateral (
  select count(*) as raids_last10, sum(rh.viewer_count) as raid_viewers_last10
  from twitch_raid_history rh
  where rh.to_broadcaster_login = m.login and rh.success
    and rh.executed_at <= m.followed_at and rh.executed_at >= m.followed_at - interval '10 minutes'
) r10 on true
order by m.followed_at;

with trk as (
  select lower(streamer_login) as login, min(followed_at) as t0 from twitch_follow_events group by 1
),
ss as (
  select s.id, lower(s.streamer_login) as login, s.started_at, t.t0,
         coalesce(s.ended_at, s.started_at + make_interval(secs => s.duration_seconds)) as end_at
  from twitch_stream_sessions s join trk t on t.login = lower(s.streamer_login)
  where coalesce(s.ended_at, s.started_at + make_interval(secs => s.duration_seconds)) >= t.t0
),
raw as (
  select sv.session_id, sv.ts_utc, floor(extract(epoch from (sv.ts_utc - ss.started_at)) / 60)::int as mi, sv.viewer_count, ss.t0, ss.end_at,
         lead(sv.ts_utc) over (partition by sv.session_id order by sv.ts_utc) as next_ts
  from twitch_session_viewers sv
  join ss on ss.id = sv.session_id
  where sv.ts_utc >= :'m1'::timestamptz - interval '2 days' and sv.ts_utc < :'m2'::timestamptz + interval '2 days'
),
w as (
  select session_id, ts_utc, mi, viewer_count,
         greatest(extract(epoch from (least(coalesce(next_ts, end_at), ts_utc + interval '300 seconds', end_at) - greatest(ts_utc, t0))), 0) as dt
  from raw
  where ts_utc >= :'m1'::timestamptz and ts_utc < :'m2'::timestamptz
)
select floor(mi / 10) * 10 as stream_min_bucket,
       case when viewer_count < 5 then '0-4' when viewer_count < 10 then '5-9' when viewer_count < 20 then '10-19'
            when viewer_count < 50 then '20-49' when viewer_count < 100 then '50-99' when viewer_count < 300 then '100-299' else '300+' end as viewer_band,
       count(*) filter (where dt > 0) as samples, sum(dt) as seconds, sum(dt*viewer_count) as viewer_seconds
from w
group by 1,2
order by 1,2;

with fs as (select lower(streamer_login) login, min(followed_at) first_follow, max(followed_at) last_follow, count(*) n_follow from twitch_follow_events group by 1),
ss as (select lower(streamer_login) login, count(*) n_sess, min(started_at) first_sess, max(started_at) last_sess, sum(follower_delta) sum_delta, sum(case when follower_delta is null then 1 else 0 end) null_delta from twitch_stream_sessions group by 1)
select
  case when fs.login is null then 'no_follow_rows' else 'follow_rows' end grp,
  count(*) streamers,
  sum(ss.n_sess) sessions,
  sum(fs.n_follow) follows,
  sum(ss.sum_delta) sum_follower_delta,
  min(ss.first_sess)::date min_first_sess, max(ss.last_sess)::date max_last_sess
from ss left join fs on fs.login = ss.login
group by 1 order by 1;

with sess as (
  select s.id, lower(s.streamer_login) login, s.started_at, coalesce(s.ended_at, s.started_at + make_interval(secs => s.duration_seconds)) end_at,
         s.follower_delta, s.followers_start, s.followers_end, s.avg_viewers, s.duration_seconds, s.peak_viewers
  from twitch_stream_sessions s
  where lower(s.streamer_login) in (select distinct lower(streamer_login) from twitch_follow_events)
),
cnt as (
  select s.id, count(f.id) n_follow_in_session
  from sess s left join twitch_follow_events f on lower(f.streamer_login)=s.login and f.followed_at >= s.started_at and f.followed_at <= s.end_at
  group by s.id
)
select s.login, s.id, s.started_at::date, s.follower_delta, s.followers_end-s.followers_start as end_minus_start, c.n_follow_in_session, s.avg_viewers, s.duration_seconds
from sess s join cnt c on c.id=s.id
order by s.login, s.started_at;

with trk as (
  select lower(streamer_login) as login, min(followed_at) as t0 from twitch_follow_events group by 1
),
ss as (
  select s.id, lower(s.streamer_login) as login, t.t0,
         coalesce(s.ended_at, s.started_at + make_interval(secs => s.duration_seconds)) as end_at
  from twitch_stream_sessions s join trk t on t.login = lower(s.streamer_login)
  where coalesce(s.ended_at, s.started_at + make_interval(secs => s.duration_seconds)) >= t.t0
),
raw as (
  select sv.session_id, ss.login, sv.ts_utc, sv.viewer_count, ss.t0, ss.end_at,
         lead(sv.ts_utc) over (partition by sv.session_id order by sv.ts_utc) as next_ts
  from twitch_session_viewers sv
  join ss on ss.id = sv.session_id
  where sv.ts_utc >= :'m1'::timestamptz - interval '2 days' and sv.ts_utc < :'m2'::timestamptz + interval '2 days'
),
pk as (
  select distinct on (session_id) session_id, ts_utc as peak_ts from raw order by session_id, viewer_count desc, ts_utc
),
w as (
  select session_id, login, ts_utc, viewer_count,
         greatest(extract(epoch from (least(coalesce(next_ts, end_at), ts_utc + interval '300 seconds', end_at) - greatest(ts_utc, t0))), 0) as dt
  from raw
  where ts_utc >= :'m1'::timestamptz and ts_utc < :'m2'::timestamptz
),
flag as (
  select w.*, pk.peak_ts,
         exists (select 1 from twitch_raid_history rh where rh.to_broadcaster_login = w.login and rh.success
                 and rh.executed_at <= w.ts_utc and rh.executed_at > w.ts_utc - interval '10 minutes') as in_raid_window
  from w join pk using (session_id)
  where w.dt > 0
)
select 'peak' as kind, floor(extract(epoch from (ts_utc - peak_ts))/60/5)*5 as bucket,
       count(*) samples, sum(dt) seconds, sum(dt*viewer_count) viewer_seconds
from flag group by 2
union all
select 'raid10', case when in_raid_window then 1 else 0 end, count(*), sum(dt), sum(dt*viewer_count)
from flag group by 2
order by 1,2;

with fo as (
  select row_number() over (order by f.followed_at, f.streamer_login, f.follower_login) as follow_id, f.streamer_login, f.follower_login, f.followed_at
  from twitch_follow_events f
  where f.followed_at >= '2026-01-31'
),
chat_stats as (
  select fo.follow_id,
         count(*) filter (where m.message_ts < fo.followed_at) as msgs_before,
         count(*) filter (where m.message_ts >= fo.followed_at) as msgs_after,
         min(m.message_ts) as first_msg_ts
  from fo
  left join twitch_chat_messages m
    on m.streamer_login = fo.streamer_login and m.chatter_login = fo.follower_login
  group by fo.follow_id
),
reply as (
  select fo.follow_id,
         exists (
           select 1 from twitch_chat_messages b
           where b.streamer_login = fo.streamer_login and b.chatter_login = fo.streamer_login
             and b.message_ts > cs.first_msg_ts and b.message_ts <= cs.first_msg_ts + interval '3 minutes'
         ) as bc_reply_3m_after_first_msg
  from fo join chat_stats cs using (follow_id)
  where cs.msgs_before > 0
)
select fo.follow_id, fo.streamer_login, fo.follower_login, fo.followed_at,
       cs.msgs_before, cs.msgs_after, cs.first_msg_ts,
       round((extract(epoch from (fo.followed_at - cs.first_msg_ts)) / 60)::numeric, 2) as min_first_chat_to_follow,
       r.bc_reply_3m_after_first_msg
from fo
join chat_stats cs using (follow_id)
left join reply r using (follow_id)
order by fo.followed_at;

with ev as (
  select e.id as event_id, e.session_id, lower(s.streamer_login) as login, e.event_type, e.is_gift, e.tier,
         e.user_login, e.gifter_login, e.total_gifted, e.cumulative_months, e.streak_months, e.received_at,
         s.started_at, s.duration_seconds,
         round((extract(epoch from (e.received_at - s.started_at)) / 60)::numeric, 2) as min_into_stream
  from twitch_subscription_events e
  join twitch_stream_sessions s on s.id = e.session_id
  where e.session_id is not null and e.received_at >= '2026-01-31'
),
win as (
  select ev.event_id, ev.session_id, ev.login, ev.event_type, ev.is_gift, ev.tier, ev.user_login, ev.gifter_login,
         ev.total_gifted, ev.cumulative_months, ev.streak_months, ev.received_at, ev.started_at, ev.duration_seconds, ev.min_into_stream,
         (select count(*) from twitch_chat_messages m
            where m.session_id = ev.session_id and m.message_ts > ev.received_at and m.message_ts <= ev.received_at + interval '10 minutes'
              and lower(m.chatter_login) <> ev.login) as msgs_after10,
         (select count(distinct m.chatter_login) from twitch_chat_messages m
            where m.session_id = ev.session_id and m.message_ts > ev.received_at and m.message_ts <= ev.received_at + interval '10 minutes'
              and lower(m.chatter_login) <> ev.login) as chatters_after10,
         (select count(*) from twitch_chat_messages m
            where m.session_id = ev.session_id and m.message_ts > ev.received_at - interval '10 minutes' and m.message_ts <= ev.received_at
              and lower(m.chatter_login) <> ev.login) as msgs_before10,
         (select count(distinct m.chatter_login) from twitch_chat_messages m
            where m.session_id = ev.session_id and m.message_ts > ev.received_at - interval '10 minutes' and m.message_ts <= ev.received_at
              and lower(m.chatter_login) <> ev.login) as chatters_before10
  from ev
)
select * from win order by received_at;

with fo as (
  select f.id as follow_id, f.streamer_login, f.twitch_user_id as channel_id, f.follower_id, f.followed_at,
         (f.followed_at at time zone 'UTC')::date as follow_day
  from twitch_follow_events f
  where exists (select 1 from twitch_community_points_viewer_daily c where c.channel_twitch_user_id = f.twitch_user_id)
)
select fo.follow_id, fo.streamer_login, fo.followed_at, fo.follow_day,
  coalesce((select sum(c.watch_minutes) from twitch_community_points_viewer_daily c
            where c.channel_twitch_user_id = fo.channel_id and c.twitch_user_id = fo.follower_id
              and c.day >= fo.follow_day - 7 and c.day < fo.follow_day), 0) as watch_min_7d_before,
  coalesce((select count(*) from twitch_community_points_viewer_daily c
            where c.channel_twitch_user_id = fo.channel_id and c.twitch_user_id = fo.follower_id
              and c.day >= fo.follow_day - 7 and c.day < fo.follow_day), 0) as watch_days_7d_before,
  coalesce((select sum(c.chat_messages) from twitch_community_points_viewer_daily c
            where c.channel_twitch_user_id = fo.channel_id and c.twitch_user_id = fo.follower_id
              and c.day >= fo.follow_day - 7 and c.day < fo.follow_day), 0) as chat_7d_before,
  coalesce((select sum(c.watch_minutes) from twitch_community_points_viewer_daily c
            where c.channel_twitch_user_id = fo.channel_id and c.twitch_user_id = fo.follower_id
              and c.day >= fo.follow_day and c.day < fo.follow_day + 7), 0) as watch_min_7d_from_follow_day,
  coalesce((select sum(c.watch_minutes) from twitch_community_points_viewer_daily c
            where c.channel_twitch_user_id = fo.channel_id and c.twitch_user_id = fo.follower_id
              and c.day = fo.follow_day), 0) as watch_min_follow_day
from fo
order by fo.followed_at;

select channel_twitch_user_id, count(*) viewer_days, count(distinct twitch_user_id) viewers,
  percentile_cont(0.5) within group (order by watch_minutes) med_watch_min_per_viewer_day,
  avg(watch_minutes) avg_watch_min, sum(watch_minutes) total_watch_min, sum(chat_messages) total_chat,
  min(day) min_day, max(day) max_day
from twitch_community_points_viewer_daily
group by 1 order by 2 desc;

with trk as (
  select lower(streamer_login) as login, min(followed_at) as t0 from twitch_follow_events group by 1
),
fm as (
  select distinct on (m.session_id, m.chatter_login)
         m.session_id, m.streamer_login, m.chatter_login, m.message_ts as first_ts
  from twitch_chat_messages m
  join trk on trk.login = m.streamer_login
  where m.message_ts >= trk.t0
    and m.chatter_login <> m.streamer_login
    and m.message_ts >= '2026-02-01'
  order by m.session_id, m.chatter_login, m.message_ts
)
select fm.session_id, fm.streamer_login, fm.chatter_login, fm.first_ts,
  exists (select 1 from twitch_chat_messages b
          where b.session_id = fm.session_id and b.chatter_login = fm.streamer_login
            and b.message_ts > fm.first_ts and b.message_ts <= fm.first_ts + interval '3 minutes') as bc_reply_3m,
  exists (select 1 from twitch_follow_events f
          where f.streamer_login = fm.streamer_login and f.follower_login = fm.chatter_login
            and f.followed_at >= fm.first_ts and f.followed_at <= fm.first_ts + interval '60 minutes') as follow_60m,
  exists (select 1 from twitch_follow_events f
          where f.streamer_login = fm.streamer_login and f.follower_login = fm.chatter_login
            and f.followed_at >= fm.first_ts and f.followed_at <= fm.first_ts + interval '24 hours') as follow_24h
from fm
where fm.first_ts <= timestamptz '2026-10-25 17:28:00+00' - interval '24 hours'
    and not (fm.first_ts > timestamptz '2026-03-15 18:00:00+00' - interval '24 hours' and fm.first_ts < timestamptz '2026-06-16 16:45:00+00')
order by fm.first_ts;

with off as (
  select f.id, lower(f.streamer_login) login, f.followed_at
  from twitch_follow_events f
  where not exists (
    select 1 from twitch_stream_sessions s
    where lower(s.streamer_login) = lower(f.streamer_login) and s.started_at <= f.followed_at
      and coalesce(s.ended_at, s.started_at + make_interval(secs => s.duration_seconds)) >= f.followed_at)
)
select
  width_bucket(e.dist_min, array[0,1,5,15,60,240,1440,10080,1000000000]) as dist_bucket,
  count(*) as n,
  count(*) filter (where e.dist_min is null) as no_sessions
from off
left join lateral (
  select min(least(abs(extract(epoch from (s.started_at - off.followed_at))), abs(extract(epoch from (coalesce(s.ended_at, s.started_at + make_interval(secs=>s.duration_seconds)) - off.followed_at))))/60) as dist_min
  from twitch_stream_sessions s where lower(s.streamer_login) = off.login
) e on true
group by 1 order by 1;

select lower(sc.chatter_login) as chatter, count(distinct lower(sc.streamer_login)) as channels
from twitch_session_chatters sc
group by 1
having count(distinct lower(sc.streamer_login)) >= 30
order by 2 desc;

with trk as (
  select lower(streamer_login) as login, min(followed_at) as t0 from twitch_follow_events group by 1
)
select s.id as session_id, lower(s.streamer_login) as login, s.started_at,
  coalesce(s.ended_at, s.started_at + make_interval(secs => s.duration_seconds)) as end_at,
  exists (select 1 from twitch_session_chatters sc where sc.session_id = s.id) as sampled
from twitch_stream_sessions s
join trk on trk.login = lower(s.streamer_login) and s.started_at >= trk.t0
where s.started_at >= '2026-02-19'
order by s.started_at;

with trk as (
  select lower(streamer_login) as login, min(followed_at) as t0 from twitch_follow_events group by 1
)
select sc.session_id, lower(sc.chatter_login) as chatter, sc.seen_via_chatters_api as seen_api,
  sc.messages, sc.first_message_at, sc.last_seen_at
from twitch_session_chatters sc
join twitch_stream_sessions s on s.id = sc.session_id
join trk on trk.login = lower(s.streamer_login) and s.started_at >= trk.t0
where s.started_at >= '2026-02-19';

with ev as (
  select e.id as event_id, lower(e.user_login) as user_login, lower(s.streamer_login) as login, e.event_type, e.is_gift,
    e.received_at
  from twitch_subscription_events e
  join twitch_stream_sessions s on s.id = e.session_id
  where e.session_id is not null and e.received_at >= '2026-03-01' and e.received_at <= '2026-09-25'
    and e.user_login is not null and e.event_type = 'subscribe'
),
cohort as (
  select ev.*, case when ev.is_gift then 'gift recipient' else 'new paid sub' end as grp from ev
)
select c.grp, count(*) as n, count(distinct c.login) as streamers,
  count(*) filter (where exists (select 1 from twitch_chat_messages m
     where lower(m.streamer_login) = c.login and lower(m.chatter_login) = c.user_login
       and m.message_ts > c.received_at and m.message_ts <= c.received_at + interval '30 days')) as chatted_30d,
  count(*) filter (where exists (select 1 from twitch_subscription_events r
     join twitch_stream_sessions rs on rs.id = r.session_id
     where r.event_type = 'resub' and lower(r.user_login) = c.user_login
       and lower(rs.streamer_login) = c.login and r.received_at > c.received_at)) as resub_later_any
from cohort c
group by c.grp order by c.grp;

with fo as (
  select f.streamer_login, f.follower_login, f.followed_at, f.twitch_user_id as channel_id, f.follower_id
  from twitch_follow_events f
  where f.followed_at >= '2026-02-19'
    and exists (select 1 from twitch_community_points_viewer_daily c where c.channel_twitch_user_id = f.twitch_user_id)
),
fc as (
  select fo.*, (select min(m.message_ts) from twitch_chat_messages m
                where m.streamer_login = fo.streamer_login and m.chatter_login = fo.follower_login) as first_msg_ts
  from fo
)
select fc.streamer_login, fc.followed_at, fc.first_msg_ts,
  (fc.first_msg_ts is not null and fc.first_msg_ts < fc.followed_at) as chatted_before_follow,
  coalesce((select sum(c.watch_minutes) from twitch_community_points_viewer_daily c
            where c.channel_twitch_user_id = fc.channel_id and c.twitch_user_id = fc.follower_id
              and fc.first_msg_ts is not null and c.day >= (fc.first_msg_ts at time zone 'UTC')::date - 7
              and c.day < (fc.first_msg_ts at time zone 'UTC')::date), 0) as watch_min_7d_before_first_chat,
  coalesce((select count(*) from twitch_community_points_viewer_daily c
            where c.channel_twitch_user_id = fc.channel_id and c.twitch_user_id = fc.follower_id
              and fc.first_msg_ts is not null and c.day >= (fc.first_msg_ts at time zone 'UTC')::date - 7
              and c.day < (fc.first_msg_ts at time zone 'UTC')::date and c.watch_minutes > 0), 0) as watch_days_7d_before_first_chat,
  coalesce((select sum(c.watch_minutes) from twitch_community_points_viewer_daily c
            where c.channel_twitch_user_id = fc.channel_id and c.twitch_user_id = fc.follower_id
              and c.day >= (fc.followed_at at time zone 'UTC')::date - 7
              and c.day < (fc.followed_at at time zone 'UTC')::date), 0) as watch_min_7d_before_follow
from fc
order by fc.followed_at;

with ss as (
  select s.id as session_id, lower(s.streamer_login) as login, s.twitch_user_id, s.stream_id, s.started_at,
    coalesce(s.ended_at, s.started_at + make_interval(secs => s.duration_seconds)) as end_at,
    s.duration_seconds, s.avg_viewers, s.peak_viewers, s.follower_delta, s.followers_start,
    s.game_name, s.had_deadlock_in_session, s.language, s.stream_title, s.tags
  from twitch_stream_sessions s
  where s.started_at >= '2026-03-01'
),
chat as (
  select m.session_id, count(distinct m.chatter_login) filter (where m.chatter_login <> m.streamer_login) as unique_chatters,
    count(*) filter (where m.chatter_login <> m.streamer_login) as chat_msgs
  from twitch_chat_messages m
  where m.session_id in (select session_id from ss)
  group by m.session_id
)
select ss.session_id, ss.login, ss.started_at, ss.end_at, ss.duration_seconds, ss.avg_viewers, ss.peak_viewers,
  ss.follower_delta, ss.followers_start, ss.game_name, ss.had_deadlock_in_session, ss.language,
  ss.stream_title, ss.tags,
  coalesce(c.unique_chatters, 0) as unique_chatters, coalesce(c.chat_msgs, 0) as chat_msgs,
  (select count(*) from twitch_raid_history r
     where lower(r.to_broadcaster_login) = ss.login and r.success
       and r.executed_at >= ss.started_at and r.executed_at <= ss.end_at) as raids_in_session,
  exists (select 1 from partner_effort_shared_chat_observations p
     where p.partner_twitch_user_id = ss.twitch_user_id and p.stream_id = ss.stream_id) as shared_chat
from ss
left join chat c on c.session_id = ss.session_id
order by ss.started_at;


select max(followed_at) filter (where followed_at < '2026-04-15') as last_before, min(followed_at) filter (where followed_at > '2026-04-15') as first_after from twitch_follow_events;

with trk as (
  select lower(streamer_login) as login, min(followed_at) as t0 from twitch_follow_events group by 1
),
ss as (
  select s.id, lower(s.streamer_login) as login, s.started_at, t.t0,
         coalesce(s.ended_at, s.started_at + make_interval(secs => s.duration_seconds)) as end_at
  from twitch_stream_sessions s join trk t on t.login = lower(s.streamer_login)
  where coalesce(s.ended_at, s.started_at + make_interval(secs => s.duration_seconds)) >= t.t0
),
raw as (
  select sv.session_id, ss.login, ss.started_at, sv.ts_utc, sv.viewer_count, ss.t0, ss.end_at,
         lead(sv.ts_utc) over (partition by sv.session_id order by sv.ts_utc) as next_ts
  from twitch_session_viewers sv
  join ss on ss.id = sv.session_id
  where sv.ts_utc >= :'m1'::timestamptz - interval '2 days' and sv.ts_utc < :'m2'::timestamptz + interval '2 days'
),
pk as (
  select distinct on (session_id) session_id, ts_utc as peak_ts from raw order by session_id, viewer_count desc, ts_utc
),
iv as (
  select session_id, login, started_at, ts_utc, viewer_count,
         greatest(ts_utc, t0) as a,
         least(coalesce(next_ts, end_at), ts_utc + interval '300 seconds', end_at, :'cut'::timestamptz,
               case when ts_utc < :'g1'::timestamptz then :'g1'::timestamptz else 'infinity'::timestamptz end) as b
  from raw
  where ts_utc >= :'m1'::timestamptz and ts_utc < :'m2'::timestamptz and ts_utc < :'cut'::timestamptz
    and not (ts_utc >= :'g1'::timestamptz and ts_utc < :'g2'::timestamptz)
),
iv2 as (
  select iv.*, pk.peak_ts from iv join pk using (session_id) where iv.b > iv.a
),
rd as (
  select lower(rh.to_broadcaster_login) as login, rh.executed_at as r
  from twitch_raid_history rh
  where rh.success
    and rh.executed_at >= :'m1'::timestamptz - interval '1 day' and rh.executed_at < :'m2'::timestamptz + interval '1 day'
),
sb as (
  select session_id, min(login) as login, min(a) as lo, max(b) as hi from iv2 group by session_id
),
rs as (
  select sb.session_id, rd.r
  from sb join rd on rd.login = sb.login and rd.r > sb.lo - interval '10 minutes' and rd.r < sb.hi
),
cuts as (
  select session_id, ts_utc, a as t from iv2
  union
  select iv2.session_id, iv2.ts_utc, x.t
  from iv2 cross join lateral (values
    (iv2.started_at + make_interval(secs => 300 * (floor(extract(epoch from (iv2.a - iv2.started_at)) / 300) + 1))),
    (iv2.started_at + make_interval(secs => 300 * (floor(extract(epoch from (iv2.a - iv2.started_at)) / 300) + 2))),
    (iv2.peak_ts + make_interval(secs => 300 * (floor(extract(epoch from (iv2.a - iv2.peak_ts)) / 300) + 1))),
    (iv2.peak_ts + make_interval(secs => 300 * (floor(extract(epoch from (iv2.a - iv2.peak_ts)) / 300) + 2)))
  ) x(t)
  where x.t > iv2.a and x.t < iv2.b
  union
  select iv2.session_id, iv2.ts_utc, x.t
  from iv2 join rs on rs.session_id = iv2.session_id and rs.r > iv2.a - interval '10 minutes' and rs.r < iv2.b
  cross join lateral (values (rs.r), (rs.r + interval '10 minutes')) x(t)
  where x.t > iv2.a and x.t < iv2.b
),
sub as (
  select iv2.session_id, iv2.login, iv2.started_at, iv2.ts_utc, iv2.viewer_count, iv2.peak_ts,
         c.t as a,
         coalesce(lead(c.t) over (partition by c.session_id, c.ts_utc order by c.t), iv2.b) as b
  from cuts c join iv2 using (session_id, ts_utc)
),
mids as (
  select sub.*, sub.a + (sub.b - sub.a) / 2 as mid from sub where sub.b > sub.a
),
ivf as (
  select m.session_id, m.login, m.started_at, m.a, m.b, m.viewer_count,
         extract(epoch from (m.b - m.a)) as dt,
         floor(extract(epoch from (m.mid - m.started_at)) / 300) * 5 as min5,
         case when m.viewer_count < 5 then '0-4' when m.viewer_count < 10 then '5-9' when m.viewer_count < 20 then '10-19'
              when m.viewer_count < 50 then '20-49' when m.viewer_count < 100 then '50-99' when m.viewer_count < 300 then '100-299' else '300+' end as band,
         floor(extract(epoch from (m.mid - m.peak_ts)) / 300) * 5 as peak5,
         exists (select 1 from rs where rs.session_id = m.session_id and rs.r <= m.mid and rs.r > m.mid - interval '10 minutes') as in_raid
  from mids m
),
fol as (
  select f.id as follow_id, lower(f.streamer_login) as login, f.followed_at
  from twitch_follow_events f
  where f.followed_at >= :'m1'::timestamptz - interval '1 day' and f.followed_at < :'m2'::timestamptz + interval '1 day'
    and f.followed_at < :'cut'::timestamptz
)
select 'exp' as kind, null::bigint as follow_id, null::timestamptz as followed_at, null::timestamptz as session_start, null::numeric as follow_min,
       min5, band, peak5, in_raid, sum(dt) as seconds, sum(dt * viewer_count) as viewer_seconds
from ivf
group by min5, band, peak5, in_raid
union all
select 'fol', fol.follow_id, fol.followed_at, ivf.started_at,
       round((extract(epoch from (fol.followed_at - ivf.started_at)) / 60)::numeric, 2),
       ivf.min5, ivf.band, ivf.peak5, ivf.in_raid, null, null
from fol join ivf on ivf.login = fol.login and fol.followed_at >= ivf.a and fol.followed_at < ivf.b;
