with base as (
  select streamer, ts_utc, viewer_count, stream_title, game_name, is_partner
  from twitch_stats_category
  where ts_utc >= (:'m0'::timestamptz - interval '1 day') and ts_utc < (:'m1'::timestamptz + interval '1 day')
    and (language like 'de%' or (language is null and ts_utc < '2026-06-10'))
),
l as (
  select streamer, ts_utc, viewer_count, stream_title, game_name, is_partner,
         lead(ts_utc) over w as nts,
         lag(ts_utc) over w as pts,
         lag(stream_title) over w as pt
  from base window w as (partition by streamer order by ts_utc)
),
s as (
  select *, case when pts is null or extract(epoch from ts_utc - pts) > 1800 then 1 else 0 end as newseg
  from l
),
g as (
  select *, sum(newseg) over (partition by streamer order by ts_utc rows unbounded preceding) as segno
  from s
),
wt as (
  select *, case when nts is not null and extract(epoch from nts - ts_utc) <= 600
                 then extract(epoch from nts - ts_utc) else 0 end as w
  from g
),
fa as (
  select streamer, segno, (array_agg(stream_title order by ts_utc))[1] as first_title
  from wt group by 1, 2
)
select wt.streamer || '#' || wt.segno as seg_key,
       wt.streamer,
       min(wt.ts_utc) as seg_start,
       max(wt.ts_utc) as seg_end,
       count(*) as n_rows,
       round((sum(wt.viewer_count * wt.w) / nullif(sum(wt.w), 0))::numeric, 3) as tw_avg_viewers,
       max(wt.viewer_count) as peak_viewers,
       round(sum(wt.w)::numeric, 0) as active_seconds,
       count(*) filter (where wt.newseg = 0 and wt.stream_title is distinct from wt.pt) as n_title_changes,
       round((sum(wt.w) filter (where wt.stream_title is not distinct from fa.first_title) / nullif(sum(wt.w), 0))::numeric, 4) as opening_title_share,
       fa.first_title,
       (array_agg(wt.game_name order by wt.ts_utc))[1] as game_name,
       bool_or(wt.is_partner) as is_partner
from wt join fa on fa.streamer = wt.streamer and fa.segno = wt.segno
group by wt.streamer, wt.segno, fa.first_title
having min(wt.ts_utc) >= :'m0'::timestamptz and min(wt.ts_utc) < :'m1'::timestamptz;

select id, streamer_login, started_at, ended_at, duration_seconds, start_viewers, peak_viewers, end_viewers,
       avg_viewers, samples, retention_10m, dropoff_pct, unique_chatters, first_time_chatters, returning_chatters,
       followers_start, followers_end, follower_delta, stream_title, game_name, is_mature, tags, notification_text
from twitch_stream_sessions
where language = 'de' and had_deadlock_in_session and stream_title is not null
  and started_at >= '2026-01-09'
order by started_at;

with base as (
  select streamer, ts_utc, viewer_count, stream_title, game_name
  from twitch_stats_category
  where ts_utc >= (:'m0'::timestamptz - interval '1 day') and ts_utc < :'m1'::timestamptz
    and (language like 'de%' or (language is null and ts_utc < '2026-06-10'))
),
l as (
  select streamer, ts_utc, viewer_count, stream_title, game_name,
         lag(ts_utc) over w as pts,
         lag(stream_title) over w as pt
  from base window w as (partition by streamer order by ts_utc)
),
s as (
  select *, case when pts is null or extract(epoch from ts_utc - pts) > 1800 then 1 else 0 end as newseg
  from l
),
g as (
  select *, sum(newseg) over (partition by streamer order by ts_utc rows unbounded preceding) as segno
  from s
),
sk as (
  select streamer, segno, min(ts_utc) as st from g group by 1, 2
),
gk as (
  select g.*, g.streamer || '#' || to_char(sk.st at time zone 'UTC', 'YYYYMMDDHH24MI') as seg_key
  from g join sk on sk.streamer = g.streamer and sk.segno = g.segno
),
ev as (
  select 'E'::text as kind, seg_key, ts_utc as t, pt as a, stream_title as b,
         null::numeric as v, null::bigint as n
  from gk
  where newseg = 0 and stream_title is distinct from pt and game_name = 'Deadlock'
    and ts_utc >= :'m0'::timestamptz and ts_utc < :'m1'::timestamptz
),
mi as (
  select 'M'::text as kind, seg_key, date_trunc('minute', ts_utc) as t, null::text as a, null::text as b,
         round(avg(viewer_count)::numeric, 2) as v, count(*) as n
  from gk
  where game_name = 'Deadlock' and ts_utc >= :'m0'::timestamptz and ts_utc < :'m1'::timestamptz
  group by seg_key, date_trunc('minute', ts_utc)
)
select * from ev union all select * from mi;

with s as (
  select id, started_at
  from twitch_stream_sessions
  where started_at >= '2026-07-13' and started_at < '2026-10-26'
    and language = 'de' and had_deadlock_in_session
),
bots as (
  select lower(c.chatter_login) as login
  from twitch_session_chatters c join s on s.id = c.session_id
  group by 1
  having count(distinct c.streamer_login) >= 30
),
agg as (
  select c.session_id,
         count(*) as tracked_rows,
         count(*) filter (where c.messages = 0) as lurker_rows,
         count(*) filter (where c.messages >= 1 and lower(c.chatter_login) not in (select login from bots)) as chat_uc,
         count(*) filter (where c.messages >= 1 and c.is_first_time_streamer
                          and lower(c.chatter_login) not in (select login from bots)) as chat_ftc
  from twitch_session_chatters c join s on s.id = c.session_id
  group by c.session_id
)
select s.id, s.started_at, coalesce(agg.tracked_rows, 0) as tracked_rows,
       coalesce(agg.lurker_rows, 0) as lurker_rows,
       coalesce(agg.chat_uc, 0) as chat_uc, coalesce(agg.chat_ftc, 0) as chat_ftc
from s left join agg on agg.session_id = s.id;

select date_trunc('day', ts_utc), count(*), count(stream_title), count(distinct streamer), count(*) filter (where game_name='Deadlock') from twitch_stats_category where ts_utc >= '2025-11-03' and ts_utc < '2025-11-04' group by 1;

select date_trunc('day', ts_utc), count(*), count(stream_title), count(distinct streamer), count(*) filter (where game_name='Deadlock') from twitch_stats_category where ts_utc >= '2026-03-20' and ts_utc < '2026-03-21' group by 1;

select to_char(date_trunc('week', s.started_at), 'YYYY-MM-DD') as wk, count(*) as sessions,
       count(*) filter (where s.unique_chatters > 0) as s_uc_pos,
       sum(s.unique_chatters) as sum_uc, sum(s.first_time_chatters) as sum_ftc, sum(s.returning_chatters) as sum_ret,
       count(*) filter (where s.follower_delta is not null) as fd_nn
from twitch_stream_sessions s
where s.started_at >= '2026-02-15' and s.started_at < '2026-10-26' and s.language='de' and s.had_deadlock_in_session
group by 1 order by 1;

select to_char(date_trunc('week', s.started_at), 'YYYY-MM-DD') as wk, count(distinct s.id) as sessions_with_m1_chatters, sum(x.n) as chatter_rows_m1
from twitch_stream_sessions s join (
  select session_id, count(*) as n from twitch_session_chatters where messages >= 1 group by 1
) x on x.session_id = s.id
where s.started_at >= '2026-02-15' and s.started_at < '2026-10-26' and s.language='de' and s.had_deadlock_in_session
group by 1 order by 1;

select count(*) from (select lower(c.chatter_login) from twitch_session_chatters c join twitch_stream_sessions s on s.id=c.session_id where s.started_at >= '2026-07-13' and s.started_at < '2026-10-26' and s.language='de' and s.had_deadlock_in_session group by 1 having count(distinct c.streamer_login) >= 30) b;

select count(*), count(distinct source_streamer), count(distinct keywords), min(added_at), max(added_at), count(*) filter (where relative_perf is null), count(*) filter (where engagement_rate is null), count(*) filter (where keywords is null or cardinality(keywords)=0) from title_generator_knowledge;

select id, source_streamer, keywords::text as keywords, relative_perf, engagement_rate, quality_tier, streamer_size, added_at from title_generator_knowledge;
