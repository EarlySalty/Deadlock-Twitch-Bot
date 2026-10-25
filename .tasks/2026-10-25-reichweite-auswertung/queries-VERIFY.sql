with f as (
  select v.session_id, min(v.minutes_from_start) as first_min, min(s.started_at) as st
  from twitch_session_viewers v join twitch_stream_sessions s on s.id = v.session_id
  where v.ts_utc >= '2026-01-01'
  group by v.session_id
)
select case when st < '2026-08-01' then 'before Aug' else 'since Aug' end as era,
       count(*) as sessions,
       round(avg((first_min > 5)::int)::numeric, 3) as after_5,
       round(avg((first_min > 15)::int)::numeric, 3) as after_15,
       round(avg((first_min > 30)::int)::numeric, 3) as after_30
from f group by 1 order by 1;

with nc as (
  select c.session_id, c.streamer_login, c.chatter_login, c.messages
  from twitch_session_chatters c
  where c.is_first_time_streamer and c.messages >= 1
    and c.first_message_at >= '2026-02-20' and c.first_message_at < '2026-09-25'
    and lower(c.chatter_login) <> lower(c.streamer_login)
),
fm as (
  select n.*, (select min(m.message_ts) from twitch_chat_messages m
               where m.session_id = n.session_id and m.chatter_login = n.chatter_login) as t0
  from nc n
),
fl as (
  select f.*,
    exists (select 1 from twitch_chat_messages b
            where b.session_id = f.session_id and lower(b.chatter_login) = lower(f.streamer_login)
              and b.message_ts > f.t0 and b.message_ts <= f.t0 + interval '3 min') as bc_any3,
    exists (select 1 from twitch_chat_messages b
            where b.session_id = f.session_id and lower(b.chatter_login) = lower(f.streamer_login)
              and b.message_ts > f.t0 and b.message_ts <= f.t0 + interval '10 min'
              and position(lower(f.chatter_login) in lower(b.content)) > 0) as bc_mention10,
    exists (select 1 from twitch_session_chatters r
            where r.streamer_login = f.streamer_login and r.chatter_login = f.chatter_login
              and r.session_id <> f.session_id
              and r.first_message_at > f.t0 + interval '1 hour'
              and r.first_message_at <= f.t0 + interval '30 days') as back30,
    exists (select 1 from twitch_session_chatters r
            where r.streamer_login = f.streamer_login and r.chatter_login = f.chatter_login
              and r.session_id <> f.session_id and r.messages >= 1
              and r.first_message_at > f.t0 + interval '1 hour'
              and r.first_message_at <= f.t0 + interval '30 days') as chat30
  from fm f where f.t0 is not null
)
select md5(streamer_login) as s,
       case when messages = 1 then '1' when messages <= 4 then '2-4' else '5+' end as mb,
       bc_any3, bc_mention10, count(*) as n,
       sum(back30::int) as back30, sum(chat30::int) as chat30
from fl group by 1, 2, 3, 4;

with nc as (
  select c.session_id, c.streamer_login, c.chatter_login, c.first_message_at
  from twitch_session_chatters c
  where c.is_first_time_streamer and c.messages = 1
    and c.first_message_at >= '2026-02-20' and c.first_message_at < '2026-09-25'
    and lower(c.chatter_login) <> lower(c.streamer_login)
),
m as (
  select n.*, x.content, x.moderation_action, x.is_command
  from nc n join lateral (
    select content, moderation_action, is_command from twitch_chat_messages m
    where m.session_id = n.session_id and m.chatter_login = n.chatter_login
    order by message_ts limit 1) x on true
),
k as (
  select m.*,
    case when moderation_action is not null then 'moderated'
         when content ~* '(https?://|www\.|\.com|\.ru|promo|viewers|followers|cheap)' then 'spamlike'
         when is_command then 'command'
         else 'other' end as cls,
    exists (select 1 from twitch_session_chatters r
            where r.streamer_login = m.streamer_login and r.chatter_login = m.chatter_login
              and r.session_id <> m.session_id
              and r.first_message_at > m.first_message_at + interval '1 hour'
              and r.first_message_at <= m.first_message_at + interval '30 days') as back30
  from m
)
select cls, count(*), round(avg(back30::int)::numeric, 3) from k group by 1 order by 2 desc;

with base as (
  select c.session_id, c.streamer_login, c.chatter_login, c.first_message_at, c.messages
  from twitch_session_chatters c
  where c.is_first_time_streamer
    and c.first_message_at >= '2026-02-20' and c.first_message_at < '2026-09-25'
    and lower(c.chatter_login) <> lower(c.streamer_login)
),
reach as (
  select chatter_login, count(distinct streamer_login) as nch
  from twitch_session_chatters where first_message_at >= '2026-02-20'
  group by 1
),
k as (
  select b.*, r.nch,
    exists (select 1 from twitch_session_chatters x
            where x.streamer_login = b.streamer_login and x.chatter_login = b.chatter_login
              and x.session_id <> b.session_id
              and x.first_message_at > b.first_message_at + interval '1 hour'
              and x.first_message_at <= b.first_message_at + interval '30 days') as back30
  from base b join reach r using (chatter_login)
)
select case when messages = 0 then 'lurker' when messages = 1 then 'chat1' when messages <= 4 then 'chat2-4' else 'chat5+' end as g,
       case when nch >= 30 then 'c30+' when nch >= 10 then 'c10-29' else 'c<10' end as reachcls,
       count(*), round(avg(back30::int)::numeric, 3)
from k group by 1, 2 order by 1, 2;

-- V6 arrivals by minute since start, new vs returning to the channel (logins in fewer than 10 channels)
with s as (
  select id, streamer_login, started_at, ended_at
  from twitch_stream_sessions
  where started_at >= '2026-03-11' and ended_at is not null
    and (game_name = 'Deadlock' or had_deadlock_in_session)
    and duration_seconds >= 7200
),
reach as (
  select c.chatter_login, count(distinct c.streamer_login) as n_ch
  from twitch_session_chatters c
  where c.first_message_at >= '2026-02-20'
  group by 1
),
firstseen as (
  select c.streamer_login, c.chatter_login, min(ss.started_at) as first_start
  from twitch_session_chatters c
  join twitch_stream_sessions ss on ss.id = c.session_id
  group by 1, 2
),
early as (
  select c.session_id
  from twitch_session_chatters c join s on s.id = c.session_id
  group by c.session_id, s.started_at
  having min(c.first_message_at) <= min(s.started_at) + interval '5 minutes'
),
a as (
  select s.id, extract(epoch from c.first_message_at - s.started_at) / 60 as amin,
         (f.first_start < s.started_at) as is_ret,
         (c.messages > 0) as chatter
  from s
  join early e on e.session_id = s.id
  join twitch_session_chatters c on c.session_id = s.id
  join reach r on r.chatter_login = c.chatter_login and r.n_ch < 10
  join firstseen f on f.streamer_login = c.streamer_login and f.chatter_login = c.chatter_login
  where c.chatter_login <> s.streamer_login
)
select case when amin < 10 then '0-10' when amin < 30 then '10-30' when amin < 60 then '30-60'
            when amin < 120 then '60-120' else '120+' end as bucket,
       count(*) filter (where not is_ret) as new_arr,
       count(*) filter (where is_ret) as ret_arr,
       (select count(*) from early e join s on s.id = e.session_id) as n_sessions
from a
group by 1
order by min(amin);

-- V7a reply within 3 min to follow, first message ever per channel
with trk as (
  select lower(streamer_login) as login, min(followed_at) as t0
  from twitch_follow_events group by 1
),
reach as (
  select chatter_login, count(distinct streamer_login) as n_ch
  from twitch_session_chatters
  where first_message_at >= '2026-02-20'
  group by 1
),
firstever as (
  select distinct on (m.streamer_login, m.chatter_login)
         m.streamer_login, m.chatter_login, m.session_id, m.message_ts as first_ts
  from twitch_chat_messages m
  where m.chatter_login is not null and m.chatter_login <> m.streamer_login
    and m.message_ts >= '2026-01-31'
  order by m.streamer_login, m.chatter_login, m.message_ts
),
ev as (
  select fe.*,
    (select min(f.followed_at) from twitch_follow_events f
      where lower(f.streamer_login) = fe.streamer_login and f.follower_login = fe.chatter_login
        and f.followed_at >= fe.first_ts - interval '30 days') as fol_at,
    exists (select 1 from twitch_chat_messages b
      where b.session_id = fe.session_id and b.chatter_login = fe.streamer_login
        and b.message_ts > fe.first_ts and b.message_ts <= fe.first_ts + interval '3 minutes') as reply3,
    (select count(*) from twitch_chat_messages o
      where o.session_id = fe.session_id and o.chatter_login <> fe.chatter_login
        and o.message_ts >= fe.first_ts - interval '10 minutes' and o.message_ts < fe.first_ts) as act10
  from firstever fe
  join trk on trk.login = fe.streamer_login
  where fe.first_ts >= trk.t0 + interval '14 days'
    and fe.first_ts < now() - interval '1 day'
    and not (extract(month from fe.first_ts) in (4, 5) and extract(year from fe.first_ts) = 2026)
),
clean as (
  select ev.*
  from ev
  left join reach r on r.chatter_login = ev.chatter_login
  where coalesce(r.n_ch, 0) < 10
    and (fol_at is null or fol_at > first_ts + interval '3 minutes')
)
select md5(streamer_login) as stratum_streamer,
       case when act10 < 5 then 'a0-4' when act10 < 20 then 'b5-19' else 'c20+' end as act,
       reply3,
       count(*) as n,
       count(*) filter (where fol_at <= first_ts + interval '60 minutes') as fol60,
       count(*) filter (where fol_at <= first_ts + interval '24 hours') as fol24h
from clean
group by 1, 2, 3
order by 1, 2, 3;

-- V7b same, per-session first message (P08 unit), non-command, all earlier follows dropped
with trk as (
  select lower(streamer_login) as login, min(followed_at) as t0
  from twitch_follow_events group by 1
),
reach as (
  select chatter_login, count(distinct streamer_login) as n_ch
  from twitch_session_chatters
  where first_message_at >= '2026-02-20'
  group by 1
),
firstever as (
  select distinct on (m.session_id, m.chatter_login)
         m.streamer_login, m.chatter_login, m.session_id, m.message_ts as first_ts
  from twitch_chat_messages m
  where m.chatter_login is not null and m.chatter_login <> m.streamer_login
    and m.message_ts >= '2026-02-01' and not coalesce(m.is_command, false)
  order by m.session_id, m.chatter_login, m.message_ts
),
ev as (
  select fe.*,
    (select min(f.followed_at) from twitch_follow_events f
      where lower(f.streamer_login) = fe.streamer_login and f.follower_login = fe.chatter_login
        ) as fol_at,
    exists (select 1 from twitch_chat_messages b
      where b.session_id = fe.session_id and b.chatter_login = fe.streamer_login
        and b.message_ts > fe.first_ts and b.message_ts <= fe.first_ts + interval '3 minutes') as reply3,
    (select count(*) from twitch_chat_messages o
      where o.session_id = fe.session_id and o.chatter_login <> fe.chatter_login
        and o.message_ts >= fe.first_ts - interval '10 minutes' and o.message_ts < fe.first_ts) as act10
  from firstever fe
  join trk on trk.login = fe.streamer_login
  where fe.first_ts >= trk.t0
    and fe.first_ts < now() - interval '1 day'
    and not (extract(month from fe.first_ts) in (4, 5) and extract(year from fe.first_ts) = 2026)
),
clean as (
  select ev.*
  from ev
  left join reach r on r.chatter_login = ev.chatter_login
  where coalesce(r.n_ch, 0) < 10
    and (fol_at is null or fol_at > first_ts + interval '3 minutes')
)
select md5(streamer_login) as stratum_streamer,
       case when act10 < 5 then 'a0-4' when act10 < 20 then 'b5-19' else 'c20+' end as act,
       reply3,
       count(*) as n,
       count(*) filter (where fol_at <= first_ts + interval '60 minutes') as fol60,
       count(*) filter (where fol_at <= first_ts + interval '24 hours') as fol24h
from clean
group by 1, 2, 3
order by 1, 2, 3;

-- V7c reverse-causation check: follows inside the 3-minute window
with trk as (
  select lower(streamer_login) as login, min(followed_at) as t0
  from twitch_follow_events group by 1
),
reach as (
  select chatter_login, count(distinct streamer_login) as n_ch
  from twitch_session_chatters
  where first_message_at >= '2026-02-20'
  group by 1
),
firstever as (
  select distinct on (m.session_id, m.chatter_login)
         m.streamer_login, m.chatter_login, m.session_id, m.message_ts as first_ts
  from twitch_chat_messages m
  where m.chatter_login is not null and m.chatter_login <> m.streamer_login
    and m.message_ts >= '2026-02-01' and not coalesce(m.is_command, false)
  order by m.session_id, m.chatter_login, m.message_ts
),
ev as (
  select fe.*,
    (select min(f.followed_at) from twitch_follow_events f
      where lower(f.streamer_login) = fe.streamer_login and f.follower_login = fe.chatter_login
        ) as fol_at,
    exists (select 1 from twitch_chat_messages b
      where b.session_id = fe.session_id and b.chatter_login = fe.streamer_login
        and b.message_ts > fe.first_ts and b.message_ts <= fe.first_ts + interval '3 minutes') as reply3,
    (select count(*) from twitch_chat_messages o
      where o.session_id = fe.session_id and o.chatter_login <> fe.chatter_login
        and o.message_ts >= fe.first_ts - interval '10 minutes' and o.message_ts < fe.first_ts) as act10
  from firstever fe
  join trk on trk.login = fe.streamer_login
  where fe.first_ts >= trk.t0
    and fe.first_ts < now() - interval '1 day'
    and not (extract(month from fe.first_ts) in (4, 5) and extract(year from fe.first_ts) = 2026)
),
clean as (
  select ev.*
  from ev
  left join reach r on r.chatter_login = ev.chatter_login
  where coalesce(r.n_ch, 0) < 10
    and (fol_at is null or fol_at >= first_ts)
)
select reply3, count(*) as n,
       count(*) filter (where fol_at <= first_ts + interval '3 minutes') as fol_0_3m,
       count(*) filter (where fol_at <= first_ts + interval '60 minutes') as fol60
from clean group by 1 order by 1;
