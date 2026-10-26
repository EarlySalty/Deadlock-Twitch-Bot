\echo Message-based chatter counts per session, logins seen in 30 or more channels excluded, used by P03, P04 and P11 in place of the session counter columns
with bots as (
  select lower(chatter_login) as login
  from twitch_session_chatters
  group by 1
  having count(distinct lower(streamer_login)) >= 30
)
select c.session_id,
       count(*) filter (where c.messages >= 1) as chat_uc,
       count(*) filter (where c.messages >= 1 and c.is_first_time_streamer) as chat_ftc,
       count(*) filter (where c.messages >= 1 and c.is_first_time_streamer is not true) as chat_ret,
       count(*) as tracked_rows
from twitch_session_chatters c
join twitch_stream_sessions s on s.id = c.session_id
where s.started_at >= '2026-01-09' and s.started_at < '2026-10-26'
  and lower(c.chatter_login) not in (select login from bots)
group by c.session_id
order by c.session_id;
