\echo Q0 overview: stream sessions since 2026-01-09
select count(*), count(distinct streamer_login), min(started_at)::date, max(started_at)::date, count(*) filter (where game_name = 'Deadlock') from twitch_stream_sessions where started_at >= '2026-01-09' and started_at < '2026-10-25 17:28:00+00';

\echo Q0 overview: category snapshots by month since 2025-10-01
select date_trunc('month', ts_utc) m, count(*) n, count(distinct streamer) streamers, count(distinct game_name) games, count(*) filter (where game_name ilike '%deadlock%') dl_rows, count(*) filter (where language is null) null_lang, count(*) filter (where language is not null and language not like 'de%') non_de from twitch_stats_category where ts_utc >= '2025-10-01' group by 1 order by 1;

\echo Q0 data trap: chat and follower fields by month
select date_trunc('month',started_at)::date m, count(*) n, count(*) filter (where first_time_chatters=0 and unique_chatters>0) ftc0_but_chatters, count(*) filter (where unique_chatters>0) with_chatters from twitch_stream_sessions where started_at>='2026-01-09' group by 1 order by 1;

\echo Q0 curve coverage: first recorded minute per session
with f as (select v.session_id, min(v.mi) mn0, max(v.mi) mx, count(*) n from (select sv.session_id, sv.ts_utc, sv.viewer_count, floor(extract(epoch from (sv.ts_utc - ss.started_at)) / 60)::int mi from twitch_session_viewers sv join twitch_stream_sessions ss on ss.id = sv.session_id) v where v.ts_utc >= '2026-01-09' and v.ts_utc < '2026-10-25 17:28:00+00' group by 1) select 'first_min_bucket', case when mn0=0 then '0' when mn0<=4 then '1-4' when mn0<=15 then '5-15' when mn0<=60 then '16-60' else '>60' end b, count(*), round(avg(n)) from f group by 2 order by 2;

\echo Q0 size class counts (Deadlock sessions >= 60 min since 2026-01-09)
select case when m<5 then '0-4' when m<20 then '5-19' when m<100 then '20-99' else '100+' end c, count(*) from (select streamer_login, percentile_cont(0.5) within group (order by avg_viewers) m from twitch_stream_sessions where started_at >= '2026-01-09' and started_at < '2026-10-25 17:28:00+00' and game_name='Deadlock' and duration_seconds>=3600 group by 1) x group by 1 order by 1;

\echo Q1a curve summary per session (size class, peak timing, hour means)
with sess as (
  select id, streamer_login, duration_seconds
  from twitch_stream_sessions
  where started_at >= '2026-01-09' and started_at < '2026-10-25 17:28:00+00' and game_name = 'Deadlock' and duration_seconds >= 3600
),
cls as (
  select streamer_login,
    case when m < 5 then '0-4' when m < 20 then '5-19' when m < 100 then '20-99' else '100+' end sc
  from (
    select streamer_login, percentile_cont(0.5) within group (order by avg_viewers) m
    from twitch_stream_sessions
    where started_at >= '2026-01-09' and started_at < '2026-10-25 17:28:00+00' and game_name = 'Deadlock' and duration_seconds >= 3600
    group by 1) x
),
mn as (
  select v.session_id, v.mi mi, avg(v.viewer_count)::float vc, count(*) n
  from (select sv.session_id, sv.ts_utc, sv.viewer_count, floor(extract(epoch from (sv.ts_utc - ss.started_at)) / 60)::int mi from twitch_session_viewers sv join twitch_stream_sessions ss on ss.id = sv.session_id) v
  join sess s on s.id = v.session_id
  where v.ts_utc >= '2026-01-09' and v.ts_utc < '2026-10-25 17:28:00+00' and v.mi between 0 and 480
  group by 1, 2
),
cov as (
  select session_id, min(mi) first_mi, max(mi) last_mi, count(*) n_min from mn group by 1
),
pk as (
  select session_id, max(vc) pk from mn group by 1
),
am as (
  select mn.session_id, min(mn.mi) argmax_mi from mn join pk on pk.session_id = mn.session_id and mn.vc = pk.pk group by 1
),
t95 as (
  select mn.session_id, min(mn.mi) t95_mi from mn join pk on pk.session_id = mn.session_id and mn.vc >= 0.95 * pk.pk group by 1
),
hb as (
  select session_id, (mi / 60) + 1 hr, count(*) hn, avg(vc) hv from mn group by 1, 2
)
select s.id session_id, c.sc, cov.first_mi, cov.last_mi, cov.n_min, round(pk.pk::numeric, 2) pk,
  am.argmax_mi, t95.t95_mi, s.duration_seconds,
  max(case when hb.hr = 1 and hb.hn >= 40 then hb.hv end) h1,
  max(case when hb.hr = 2 and hb.hn >= 40 then hb.hv end) h2,
  max(case when hb.hr = 3 and hb.hn >= 40 then hb.hv end) h3,
  max(case when hb.hr = 4 and hb.hn >= 40 then hb.hv end) h4,
  max(case when hb.hr = 5 and hb.hn >= 40 then hb.hv end) h5,
  max(case when hb.hr = 6 and hb.hn >= 40 then hb.hv end) h6,
  max(case when hb.hr = 7 and hb.hn >= 40 then hb.hv end) h7,
  max(case when hb.hr = 8 and hb.hn >= 40 then hb.hv end) h8
from sess s
join cls c on c.streamer_login = (select streamer_login from twitch_stream_sessions x where x.id = s.id)
join cov on cov.session_id = s.id
join pk on pk.session_id = s.id
left join am on am.session_id = s.id
left join t95 on t95.session_id = s.id
left join hb on hb.session_id = s.id
group by s.id, c.sc, cov.first_mi, cov.last_mi, cov.n_min, pk.pk, am.argmax_mi, t95.t95_mi, s.duration_seconds;

\echo Q1b session-based normalized curve medians per minute and size class
with sess as (
  select id, streamer_login
  from twitch_stream_sessions
  where started_at >= '2026-01-09' and started_at < '2026-10-25 17:28:00+00' and game_name = 'Deadlock' and duration_seconds >= 3600
),
cls as (
  select streamer_login,
    case when m < 5 then '0-4' when m < 20 then '5-19' when m < 100 then '20-99' else '100+' end sc
  from (
    select streamer_login, percentile_cont(0.5) within group (order by avg_viewers) m
    from twitch_stream_sessions
    where started_at >= '2026-01-09' and started_at < '2026-10-25 17:28:00+00' and game_name = 'Deadlock' and duration_seconds >= 3600
    group by 1) x
),
mn as (
  select v.session_id, v.mi mi, avg(v.viewer_count)::float vc
  from (select sv.session_id, sv.ts_utc, sv.viewer_count, floor(extract(epoch from (sv.ts_utc - ss.started_at)) / 60)::int mi from twitch_session_viewers sv join twitch_stream_sessions ss on ss.id = sv.session_id) v
  join sess s on s.id = v.session_id
  where v.ts_utc >= '2026-01-09' and v.ts_utc < '2026-10-25 17:28:00+00' and v.mi between 0 and 480
  group by 1, 2
),
cov as (
  select session_id from mn group by 1 having min(mi) <= 15
),
pk as (
  select mn.session_id, max(vc) pk from mn join cov using (session_id) group by 1 having max(vc) >= 3
),
nz as (
  select mn.session_id, mn.mi, mn.vc / pk.pk nv, c.sc
  from mn join pk using (session_id) join sess s on s.id = mn.session_id join cls c on c.streamer_login = s.streamer_login
)
select sc, mi, count(*) n_sessions, round(percentile_cont(0.5) within group (order by nv)::numeric, 4) med_norm,
  round(percentile_cont(0.25) within group (order by nv)::numeric, 4) p25_norm,
  round(percentile_cont(0.75) within group (order by nv)::numeric, 4) p75_norm
from nz group by sc, mi order by sc, mi;

\echo Q1c snapshot-based normalized curve medians per minute and size class
with f as (
  select twitch_user_id uid, ts_utc ts, viewer_count v
  from twitch_stats_category
  where ts_utc >= '2026-03-11' and ts_utc < '2026-10-26'
    and twitch_user_id is not null
    and not (language is not null and language not like 'de%')
    and not (language is null and ts_utc >= '2026-06-10' and ts_utc < '2026-06-14')
),
g1 as (
  select uid, ts, v,
    extract(epoch from (ts - lag(ts) over w)) pgap,
    extract(epoch from (lead(ts) over w - ts)) ngap
  from f window w as (partition by uid order by ts)
),
g2 as (
  select uid, ts, v,
    sum(case when pgap is null or pgap > 1200 then 1 else 0 end) over (partition by uid order by ts) sid,
    case when ngap is null or ngap > 1200 then 0 else ngap end wgt
  from g1
),
st as (
  select uid, sid, min(ts) st_start, sum(wgt) live_sec, sum(v*wgt)/nullif(sum(wgt),0) mean_v, count(*) snaps
  from g2 group by uid, sid
),
cls_u as (
  select uid,
    case when m < 5 then '0-4' when m < 20 then '5-19' when m < 100 then '20-99' else '100+' end sc
  from (
    select uid, percentile_cont(0.5) within group (order by mean_v) m from st where live_sec >= 3600 group by uid
  ) x
),
mn as (
  select g2.uid, g2.sid, floor(extract(epoch from (g2.ts - st.st_start))/60)::int mi, avg(g2.v)::float vc
  from g2 join st on st.uid = g2.uid and st.sid = g2.sid
  where st.live_sec >= 3600
  group by 1, 2, 3
),
mn2 as (select * from mn where mi between 0 and 480),
cov as (select uid, sid from mn2 group by 1, 2 having min(mi) <= 5),
pk as (
  select mn2.uid, mn2.sid, max(vc) pk from mn2 join cov using (uid, sid) group by 1, 2 having max(vc) >= 3
),
nz as (
  select mn2.uid, mn2.sid, mn2.mi, mn2.vc / pk.pk nv, c.sc
  from mn2 join pk using (uid, sid) join cls_u c on c.uid = mn2.uid
)
select sc, mi, count(distinct (uid, sid)) n_streams, count(distinct uid) n_streamers,
  round(percentile_cont(0.5) within group (order by nv)::numeric, 4) med_norm,
  round(percentile_cont(0.25) within group (order by nv)::numeric, 4) p25_norm,
  round(percentile_cont(0.75) within group (order by nv)::numeric, 4) p75_norm
from nz group by sc, mi order by sc, mi;

\echo Q1d snapshot-based hour means of streams of at least 5 hours
with f as (
  select twitch_user_id uid, ts_utc ts, viewer_count v
  from twitch_stats_category
  where ts_utc >= '2026-03-11' and ts_utc < '2026-10-26'
    and twitch_user_id is not null
    and not (language is not null and language not like 'de%')
    and not (language is null and ts_utc >= '2026-06-10' and ts_utc < '2026-06-14')
),
g1 as (
  select uid, ts, v,
    extract(epoch from (ts - lag(ts) over w)) pgap,
    extract(epoch from (lead(ts) over w - ts)) ngap
  from f window w as (partition by uid order by ts)
),
g2 as (
  select uid, ts, v,
    sum(case when pgap is null or pgap > 1200 then 1 else 0 end) over (partition by uid order by ts) sid,
    case when ngap is null or ngap > 1200 then 0 else ngap end wgt
  from g1
),
st as (
  select uid, sid, min(ts) st_start, sum(wgt) live_sec from g2 group by uid, sid
),
cls_u as (
  select uid, case when m < 5 then '0-4' when m < 20 then '5-19' when m < 100 then '20-99' else '100+' end sc
  from (select uid, percentile_cont(0.5) within group (order by mv) m
        from (select g2.uid, g2.sid, sum(g2.v*g2.wgt)/nullif(sum(g2.wgt),0) mv from g2 join st using (uid, sid) where st.live_sec >= 3600 group by 1, 2) q
        group by uid) x
),
mn as (
  select g2.uid, g2.sid, floor(extract(epoch from (g2.ts - st.st_start))/60)::int mi, avg(g2.v)::float vc
  from g2 join st on st.uid = g2.uid and st.sid = g2.sid
  where st.live_sec >= 18000
  group by 1, 2, 3
),
hb as (
  select uid, sid, (mi/60)+1 hr, count(*) hn, avg(vc) hv from mn where mi between 0 and 480 group by 1, 2, 3
)
select cls_u.sc, hb.uid, hb.sid, hb.hr, hb.hn, hb.hv
from hb join cls_u using (uid);

\echo Q2 stream derivation from category snapshots, one uid shard (run with -v from_ts, -v to_ts, -v k=0..3)
with f as (
  select twitch_user_id uid, ts_utc ts, viewer_count v, game_name g, stream_title t, language l
  from twitch_stats_category
  where ts_utc >= :'from_ts'::timestamptz and ts_utc < :'to_ts'::timestamptz
    and twitch_user_id is not null and (twitch_user_id::bigint % 4) = :k
    and not (language is not null and language not like 'de%')
    and not (language is null and ts_utc >= '2026-06-10' and ts_utc < '2026-06-14')
),
g1 as (
  select uid, ts, v, g, t, l,
    extract(epoch from (ts - lag(ts) over w)) pgap,
    extract(epoch from (lead(ts) over w - ts)) ngap,
    lag(t) over w pt
  from f window w as (partition by uid order by ts)
),
g2 as (
  select *, sum(case when pgap is null or pgap > 1200 then 1 else 0 end) over (partition by uid order by ts) sid,
    case when ngap is null or ngap > 1200 then 0 else ngap end wgt
  from g1
)
select uid, min(ts) stream_start, max(ts) stream_end, count(*) snaps,
  sum(wgt) live_sec, sum(v*wgt) vw_sum, sum(v) v_sum, max(v) peak_v,
  percentile_cont(0.5) within group (order by v) median_v,
  sum(case when pt is not null and t is distinct from pt then 1 else 0 end) title_changes,
  count(distinct t) n_titles,
  mode() within group (order by g) game_mode,
  mode() within group (order by l) lang_mode
from g2 group by uid, sid;

\echo Q3 note: first_time_chatters, unique_chatters and returning_chatters are replaced by chat_ftc, chat_uc and chat_ret from chat-counts.sql, joined by session id, because the session columns also count lurkers
\echo Q3 session export for duration, breaks, consistency and habits (title as hash)
select id session_id, streamer_login, twitch_user_id, started_at, ended_at, duration_seconds, start_viewers, peak_viewers, end_viewers,
  round(avg_viewers::numeric, 3) avg_viewers, samples, dropoff_pct, unique_chatters, first_time_chatters, returning_chatters,
  followers_start, followers_end, follower_delta, game_name, had_deadlock_in_session, language,
  md5(coalesce(stream_title, '')) title_hash, coalesce(stream_title, '') = '' title_empty
from twitch_stream_sessions
where started_at >= '2026-01-09' and started_at < '2026-10-26'
order by streamer_login, started_at;

\echo Q4 returning audience per session from chatter table (present and chatters split)
with s as (
  select id, streamer_login, twitch_user_id, started_at from twitch_stream_sessions
  where started_at >= '2026-01-09' and game_name = 'Deadlock' and duration_seconds >= 1800
),
c as (
  select sc.session_id, sc.streamer_login, coalesce(sc.chatter_id, lower(sc.chatter_login)) k, sc.messages, s.started_at, s.twitch_user_id
  from twitch_session_chatters sc join s on s.id = sc.session_id
  where coalesce(sc.chatter_id, sc.chatter_login) is not null
),
f as (
  select c.*, min(started_at) over (partition by streamer_login, k) first_seen from c
)
select session_id, twitch_user_id, started_at,
  count(*) present_n,
  count(*) filter (where started_at > first_seen) present_returning,
  count(*) filter (where messages >= 1) chatter_n,
  count(*) filter (where messages >= 1 and started_at > first_seen) chatter_returning
from f group by 1, 2, 3 order by 3;
