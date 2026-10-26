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

-- V6 arrivals by minute since start on the 30-second presence poll, new vs returning to the channel (logins in fewer than 10 channels); run with -v m1=2026-05-01 -v m2=2026-11-01
with s as (
  select id, streamer_login, started_at
  from twitch_stream_sessions
  where started_at >= :'m1'::timestamptz and started_at < :'m2'::timestamptz and ended_at is not null
    and (game_name = 'Deadlock' or had_deadlock_in_session)
    and duration_seconds >= 7200
),
obs as (
  select c.streamer_login, min(ss.started_at) as first_obs
  from twitch_session_chatters c join twitch_stream_sessions ss on ss.id = c.session_id
  group by 1
),
s2 as (
  select s.* from s join obs o on o.streamer_login = s.streamer_login
  where s.started_at >= greatest(o.first_obs, '2026-04-04 23:00+00'::timestamptz) + interval '30 days'
),
tk as (
  select t.session_id, t.viewer_login, t.tick_at
  from twitch_viewer_presence_ticks t join s2 on s2.id = t.session_id
  where t.tick_at >= s2.started_at - interval '10 minutes' and t.tick_at < s2.started_at + interval '2 hours'
),
cov as (
  select tk.session_id, min(tk.tick_at) as first_tick
  from tk join s2 on s2.id = tk.session_id
  group by tk.session_id, s2.started_at
  having min(tk.tick_at) <= s2.started_at + interval '5 minutes'
     and count(distinct floor(extract(epoch from tk.tick_at - s2.started_at) / 300)) filter (where tk.tick_at >= s2.started_at) >= 20
),
arr as (
  select tk.session_id, tk.viewer_login, min(tk.tick_at) as first_tick
  from tk join cov using (session_id)
  group by 1, 2
),
reach as (
  select chatter_login, count(distinct streamer_login) as n_ch
  from twitch_session_chatters
  where first_message_at >= '2026-02-20'
  group by 1
),
firstseen as (
  select c.streamer_login, c.chatter_login, min(ss.started_at) as first_start
  from twitch_session_chatters c join twitch_stream_sessions ss on ss.id = c.session_id
  group by 1, 2
),
a as (
  select arr.session_id,
         arr.first_tick = cov.first_tick as at_first,
         greatest(extract(epoch from arr.first_tick - s2.started_at), 0) / 60 as amin,
         coalesce(f.first_start < s2.started_at, false) as is_ret
  from arr
  join cov using (session_id)
  join s2 on s2.id = arr.session_id
  left join reach r on r.chatter_login = arr.viewer_login
  left join firstseen f on f.streamer_login = s2.streamer_login and f.chatter_login = arr.viewer_login
  where lower(arr.viewer_login) <> lower(s2.streamer_login)
    and coalesce(r.n_ch, 0) < 10
)
select case when at_first then 'first_tick' when amin < 10 then '0-10' when amin < 30 then '10-30'
            when amin < 60 then '30-60' else '60-120' end as bucket,
       count(*) filter (where not is_ret) as new_arr,
       count(*) filter (where is_ret) as ret_arr,
       (select count(*) from cov) as n_sessions,
       (select count(distinct s2.streamer_login) from cov join s2 on s2.id = cov.session_id) as n_channels,
       (select round(avg(extract(epoch from cov.first_tick - s2.started_at))::numeric, 1) from cov join s2 on s2.id = cov.session_id) as avg_first_tick_s
from a
group by 1
order by min(case when at_first then -1 else amin end);

-- V7 first messages with broadcaster message and follow delays, order-respecting landmark test in verify-v7-reply-follow.py
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
firstall as (
  select m.streamer_login, m.chatter_login, min(m.message_ts) as first_any
  from twitch_chat_messages m
  where m.chatter_login is not null and m.chatter_login <> m.streamer_login
    and m.message_ts >= '2026-01-31'
  group by 1, 2
),
fe as (
  select distinct on (m.session_id, m.chatter_login)
         m.streamer_login, m.chatter_login, m.session_id, m.message_ts as first_ts
  from twitch_chat_messages m
  where m.chatter_login is not null and m.chatter_login <> m.streamer_login
    and m.message_ts >= '2026-02-01' and not coalesce(m.is_command, false)
  order by m.session_id, m.chatter_login, m.message_ts
),
fol as (
  select lower(streamer_login) as login, follower_login, min(followed_at) as fol_at
  from twitch_follow_events group by 1, 2
),
ev as (
  select fe.*, trk.t0, fa.first_any, fol.fol_at,
    (select min(b.message_ts) from twitch_chat_messages b
      where b.session_id = fe.session_id and b.chatter_login = fe.streamer_login
        and b.message_ts > fe.first_ts and b.message_ts <= fe.first_ts + interval '3 minutes') as bc_at,
    exists (select 1 from twitch_chat_messages p
      where p.session_id = fe.session_id and p.chatter_login = fe.streamer_login
        and p.message_ts >= fe.first_ts - interval '3 minutes' and p.message_ts < fe.first_ts) as bc_pre,
    (select count(*) from twitch_chat_messages o
      where o.session_id = fe.session_id and o.chatter_login <> fe.chatter_login
        and o.message_ts >= fe.first_ts - interval '10 minutes' and o.message_ts < fe.first_ts) as act10
  from fe
  join trk on trk.login = fe.streamer_login
  join firstall fa on fa.streamer_login = fe.streamer_login and fa.chatter_login = fe.chatter_login
  left join fol on fol.login = fe.streamer_login and fol.follower_login = fe.chatter_login
  left join reach r on r.chatter_login = fe.chatter_login
  where fe.first_ts >= trk.t0
    and fe.first_ts <= timestamptz '2026-10-25 17:28:00+00' - interval '24 hours'
    and not (fe.first_ts > timestamptz '2026-03-15 18:00:00+00' - interval '24 hours' and fe.first_ts < timestamptz '2026-06-16 16:45:00+00')
    and coalesce(r.n_ch, 0) < 10
    and (fol.fol_at is null or fol.fol_at >= fe.first_ts)
)
select md5(streamer_login) as st,
       case when act10 < 5 then 'a0-4' when act10 < 20 then 'b5-19' else 'c20+' end as act,
       (first_any = first_ts and first_ts >= t0 + interval '14 days') as first_ever,
       round(extract(epoch from (bc_at - first_ts))::numeric, 1) as bc_delay,
       round(extract(epoch from (fol_at - first_ts))::numeric, 1) as fol_delay,
       bc_pre
from ev;

-- V8 end-to-peak ratio and the next start on merged broadcasts (stream_id deduplicated, sessions within 10 min of the previous end merged), pairs within 48 h (streamer fixed effects in verify-v8-end-next.py)
with base as (
  select id, stream_id, streamer_login, started_at, ended_at,
         coalesce(ended_at, started_at + make_interval(secs => duration_seconds)) as end_at,
         duration_seconds, start_viewers, peak_viewers, end_viewers, avg_viewers,
         row_number() over (partition by stream_id order by duration_seconds desc nulls last, id desc) as rn
  from twitch_stream_sessions
  where started_at >= '2026-01-09'
),
u as (
  select * from base where rn = 1
),
o as (
  select u.*,
         max(end_at) over (partition by streamer_login order by started_at, id rows between unbounded preceding and 1 preceding) as prev_max_end
  from u
),
g as (
  select o.*,
         sum(case when prev_max_end is null or started_at > prev_max_end + interval '10 minutes' then 1 else 0 end)
           over (partition by streamer_login order by started_at, id) as bid
  from o
),
b as (
  select streamer_login, bid,
         min(started_at) as started_at,
         max(end_at) as end_at,
         bool_and(ended_at is not null) as ended,
         (array_agg(id order by started_at, id))[1] as first_id,
         (array_agg(start_viewers order by started_at, id))[1] as start_viewers,
         (array_agg(end_viewers order by end_at desc, id desc))[1] as end_viewers,
         max(peak_viewers) as peak_viewers,
         sum(avg_viewers * duration_seconds) filter (where avg_viewers is not null and duration_seconds > 0)
           / nullif(sum(duration_seconds) filter (where avg_viewers is not null and duration_seconds > 0), 0) as avg_viewers
  from g
  group by streamer_login, bid
),
f as (
  select b.*, extract(epoch from (b.end_at - b.started_at)) as dur_s, fs.first_min
  from b
  left join lateral (
    select v.minutes_from_start as first_min
    from twitch_session_viewers v
    where v.session_id = b.first_id
    order by v.ts_utc
    limit 1
  ) fs on true
),
w as (
  select f.*,
    lead(start_viewers) over p as next_start,
    lead(first_min) over p as next_first_min,
    lead(started_at) over p as next_started_at,
    lag(start_viewers) over p as prev_start,
    lag(first_min) over p as prev_first_min,
    avg(avg_viewers) over (partition by streamer_login order by started_at rows between 3 preceding and 1 preceding) as lvl3,
    count(avg_viewers) over (partition by streamer_login order by started_at rows between 3 preceding and 1 preceding) as lvl3_n
  from f
  window p as (partition by streamer_login order by started_at)
),
e as (
  select * from w
  where ended and dur_s >= 3600 and peak_viewers > 0 and avg_viewers is not null and end_viewers is not null
),
med as (
  select streamer_login, percentile_cont(0.5) within group (order by avg_viewers) as m, count(*) as n
  from e group by 1
)
select md5(e.streamer_login) as st, med.m as med_avg,
       round((e.end_viewers::numeric / e.peak_viewers), 4) as endpeak,
       round((e.start_viewers / med.m)::numeric, 4) as cur_start_n,
       round((e.avg_viewers / med.m)::numeric, 4) as cur_avg_n,
       round((e.peak_viewers / med.m)::numeric, 4) as cur_peak_n,
       round((e.lvl3 / med.m)::numeric, 4) as lvl3_n,
       round((e.next_start / med.m)::numeric, 4) as next_start_n,
       round((e.prev_start / med.m)::numeric, 4) as prev_start_n,
       e.first_min, e.next_first_min, e.prev_first_min,
       round((e.dur_s / 3600.0)::numeric, 2) as dur_h,
       round((extract(epoch from (e.next_started_at - e.end_at)) / 3600.0)::numeric, 2) as gap_h
from e join med using (streamer_login)
where med.m > 0 and med.n >= 6 and e.lvl3_n = 3
  and e.next_start is not null and e.start_viewers is not null
  and e.next_started_at - e.end_at <= interval '48 hours';

-- V9 title features, per-session export (local +-14 day matching in verify-v9-titles.py)
select md5(streamer_login) as st,
       extract(epoch from started_at)::bigint as t,
       avg_viewers,
       (stream_title ~ '(^|\s)![a-zA-Z]{2,}') as bang,
       (stream_title ilike '%discord%') as discord,
       (stream_title ~ '\[[^\]]{1,20}\]') as bracket,
       (length(stream_title) < 30) as short,
       (stream_title ilike '%chill%') as chill
from twitch_stream_sessions
where started_at >= '2026-01-09' and ended_at is not null
  and language = 'de' and stream_title is not null and stream_title <> ''
  and (game_name = 'Deadlock' or had_deadlock_in_session)
  and duration_seconds >= 1800 and avg_viewers is not null;

-- V10 5-minute chat slots per session since 2026-07-01 (tests in verify-v10-slots.py <csv> <trim slots after first tracked slot>, reported with 2 and 6)
with s as (
  select id, streamer_login, started_at, ended_at
  from twitch_stream_sessions
  where started_at >= '2026-07-01' and ended_at is not null
    and duration_seconds between 1800 and 43200
),
bots as (
  select chatter_login
  from twitch_chat_messages
  where message_ts >= '2026-07-01'
  group by 1
  having count(distinct streamer_login) >= 30
),
m as (
  select c.session_id, c.chatter_login, c.message_ts, c.is_command,
         (c.chatter_login = s.streamer_login) as is_bc,
         floor(extract(epoch from c.message_ts - s.started_at) / 300)::int as slot
  from twitch_chat_messages c
  join s on s.id = c.session_id
  where c.message_ts >= s.started_at and c.message_ts <= s.ended_at
    and c.chatter_login is not null
    and not exists (select 1 from bots b where b.chatter_login = c.chatter_login)
),
firsts as (
  select session_id, chatter_login, min(slot) as slot
  from m where not is_bc
  group by 1, 2
),
agg as (
  select session_id, slot,
         count(*) filter (where not is_bc) as vmsg,
         count(*) filter (where is_bc and not coalesce(is_command, false)) as bcmsg
  from m group by 1, 2
),
newc as (
  select session_id, slot, count(*) as newc from firsts group by 1, 2
),
active as (
  select session_id from agg group by 1 having sum(vmsg) >= 5
),
grid as (
  select s.id as session_id, s.streamer_login, g.slot
  from s
  join active a on a.session_id = s.id
  cross join lateral generate_series(0, floor(extract(epoch from s.ended_at - s.started_at) / 300)::int - 1) as g(slot)
)
select md5(g.streamer_login) as st, g.session_id, g.slot,
       coalesce(a.vmsg, 0) as vmsg, coalesce(a.bcmsg, 0) as bcmsg, coalesce(n.newc, 0) as newc
from grid g
left join agg a on a.session_id = g.session_id and a.slot = g.slot
left join newc n on n.session_id = g.session_id and n.slot = g.slot
order by g.session_id, g.slot;

-- V11: chat events for the P07 follow-up recheck (verify-v11-followup.py)
with bots as (
  select chatter_login
  from twitch_chat_messages
  where message_ts >= '2026-02-01' and message_ts < '2026-10-05'
  group by 1
  having count(distinct streamer_login) >= 30
)
select left(md5(c.streamer_login), 12) as st, c.session_id as sid,
       extract(epoch from c.message_ts)::bigint as t,
       left(md5(c.chatter_login), 12) as ch,
       (c.chatter_login = c.streamer_login) as bc,
       coalesce(l.label, '') as label
from twitch_chat_messages c
left join twitch_chat_message_labels l on l.message_id = c.id
where c.message_ts >= '2026-02-01' and c.message_ts < '2026-10-05'
  and c.session_id is not null and c.chatter_login is not null
  and not exists (select 1 from bots b where b.chatter_login = c.chatter_login)
order by 2, 3;
