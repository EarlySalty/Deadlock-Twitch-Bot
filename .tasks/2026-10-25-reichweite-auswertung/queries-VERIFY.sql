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
