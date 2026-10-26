with raw as (
  select ts_utc, streamer, viewer_count
  from twitch_stats_category
  where ts_utc >= '__FROM__' and ts_utc < '__TO__'
    and (language like 'de%' or language is null)
    and (ts_utc at time zone 'Europe/Berlin')::date not between date '2026-06-10' and date '2026-06-13'
),
ranked as (
  select ts_utc, viewer_count,
         row_number() over (partition by ts_utc order by viewer_count desc, streamer) rn
  from raw
),
polls as (
  select ts_utc, count(*) n, sum(viewer_count) v,
         coalesce(sum(viewer_count) filter (where rn > 3), 0) v_rest,
         greatest(count(*) - 3, 0) n_rest
  from ranked group by ts_utc
),
timed as (
  select ts_utc, n, v, v_rest, n_rest,
         least(coalesce(extract(epoch from (lead(ts_utc) over (order by ts_utc) - ts_utc)), 20), 1800) w_s
  from polls
)
select to_char(ts_utc at time zone 'Europe/Berlin', 'YYYY-MM-DD HH24:MI:SS') local_ts,
       extract(isodow from ts_utc at time zone 'Europe/Berlin')::int isodow,
       extract(hour from ts_utc at time zone 'Europe/Berlin')::int hr,
       n, v, v_rest, n_rest, w_s
from timed
order by ts_utc;

with x as (
  select (snapshot_at at time zone 'Europe/Berlin') lt, viewer_count v, sample_seconds s, language
  from category_snapshots_normalized
  where snapshot_at >= '2026-10-09' and snapshot_at < '2026-10-26'
),
h as (
  select extract(hour from lt)::int hr, date(lt) d, v, s, (language = 'de') is_de from x
),
agg as (
  select hr, count(distinct d) days,
         sum(v*s)::numeric vs_all, sum(v*s) filter (where is_de)::numeric vs_de,
         sum(s)::numeric ss_all, sum(s) filter (where is_de)::numeric ss_de
  from h group by hr
)
select hr, days,
       round(vs_all/(3600.0*days),1) avg_viewers_all,
       round(vs_de/(3600.0*days),1) avg_viewers_de,
       round(ss_all/(3600.0*days),1) avg_streams_all,
       round(ss_de/(3600.0*days),1) avg_streams_de,
       round(100.0*vs_de/nullif(vs_all,0),1) de_share_viewers_pct,
       round(100.0*ss_de/nullif(ss_all,0),1) de_share_streams_pct,
       round(vs_de/nullif(ss_de,0),1) de_viewers_per_stream,
       round(vs_all/nullif(ss_all,0),1) all_viewers_per_stream
from agg order by hr;

\echo Session base for start hour, weekday and gap tests; first_time_chatters, unique_chatters and returning_chatters are replaced by chat_ftc, chat_uc and chat_ret from chat-counts.sql, joined by session id, because the session columns also count lurkers
with base as (
  select id, streamer_login, started_at, duration_seconds, avg_viewers, peak_viewers, start_viewers,
         first_time_chatters, unique_chatters, returning_chatters, retention_10m, follower_delta,
         extract(isodow from started_at at time zone 'Europe/Berlin')::int dow,
         extract(hour from started_at at time zone 'Europe/Berlin')::int hr,
         to_char(started_at at time zone 'Europe/Berlin', 'YYYY-MM-DD HH24:MI') local_start,
         language, had_deadlock_in_session
  from twitch_stream_sessions
  where started_at >= '2026-01-09' and started_at < '2026-10-26'
    and duration_seconds between 600 and 86400
    and had_deadlock_in_session
    and (language like 'de%' or language is null)
),
med as (
  select streamer_login,
         count(*) n_sess,
         percentile_cont(0.5) within group (order by avg_viewers) med_avg,
         percentile_cont(0.5) within group (order by first_time_chatters) med_ftc,
         percentile_cont(0.5) within group (order by first_time_chatters::float / greatest(duration_seconds/3600.0,0.1)) med_ftc_ph
  from base group by 1
)
select b.id, b.streamer_login, b.local_start, b.dow, b.hr, b.duration_seconds, b.avg_viewers, b.peak_viewers, b.start_viewers,
       b.first_time_chatters, b.unique_chatters, b.returning_chatters, b.retention_10m, b.follower_delta, b.language, b.had_deadlock_in_session,
       m.n_sess, m.med_avg, m.med_ftc, m.med_ftc_ph
from base b join med m using (streamer_login)
order by b.started_at;

select min(snapshot_at) from category_snapshots_normalized where snapshot_at >= '2026-09-01';

select count(*) from category_snapshots_normalized where snapshot_at >= '2026-10-01' and snapshot_at < '2026-10-26';

\echo Coverage by month from the session columns; the reported coverage counts sessions with chat_ftc > 0 from chat-counts.sql
select to_char(started_at at time zone 'Europe/Berlin','YYYY-MM') m,
       count(*) sessions,
       count(distinct streamer_login) streamers,
       count(*) filter (where first_time_chatters > 0) n_ftc_pos,
       count(*) filter (where unique_chatters > 0) n_uc_pos,
       count(*) filter (where follower_delta is not null) n_fd
from twitch_stream_sessions
where started_at >= '2026-01-01' and started_at < '2026-10-26'
  and duration_seconds between 600 and 86400 and had_deadlock_in_session
  and (language like 'de%' or language is null)
group by 1 order by 1;
