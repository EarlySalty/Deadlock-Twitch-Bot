select column_name, data_type, table_name
from information_schema.columns
where table_name in ('twitch_stats_category', 'twitch_stream_sessions', 'category_snapshots_normalized')
order by table_name, ordinal_position;

select count(*), count(*) filter (where game_name = 'Deadlock'), count(*) filter (where language like 'de%')
from twitch_stats_category
where ts_utc >= '2025-10-10' and ts_utc < '2025-11-01';

with s as (
  select id, stream_id, streamer_login, started_at, duration_seconds, start_viewers, peak_viewers, avg_viewers, end_viewers, samples,
         retention_10m, dropoff_pct,
         followers_start, followers_end, follower_delta, is_mature, tags, game_name, had_deadlock_in_session, language,
         row_number() over (partition by coalesce(stream_id, id::text) order by duration_seconds desc nulls last, id desc) rn
  from twitch_stream_sessions
  where started_at >= '2026-03-12' and started_at < '2026-10-26'
    and (game_name = 'Deadlock' or had_deadlock_in_session)
    and language like 'de%'
    and tags is not null
)
select id as session_id, left(md5(streamer_login), 12) as streamer_key, started_at, duration_seconds, start_viewers, peak_viewers,
       avg_viewers, end_viewers, samples, retention_10m, dropoff_pct,
       followers_start, followers_end, follower_delta, is_mature, coalesce(tags, '') as tags, game_name, had_deadlock_in_session
from s
where rn = 1
order by started_at;

select distinct s.streamer, lower(btrim(t.tag)) as tg
from twitch_stats_category s
cross join lateral json_array_elements_text(s.tags::json) as t(tag)
where s.ts_utc >= :'from_ts' and s.ts_utc < :'to_ts'
  and s.game_name = 'Deadlock'
  and (s.language like 'de%' or (s.language is null and s.ts_utc < '2026-06-10'))
  and s.tags is not null and s.tags <> '[]';

select user_id, stream_id, coalesce(language, 'NULL') as language, is_mature,
       array_to_string(tags, '|') as tagset,
       sum(sample_seconds) as secs,
       sum(viewer_count * sample_seconds) as vsecs,
       max(viewer_count) as peak,
       min(snapshot_at) as first_seen,
       min(started_at) as started_at
from category_snapshots_normalized
where snapshot_at >= :'d1' and snapshot_at < :'d2'
group by user_id, stream_id, coalesce(language, 'NULL'), is_mature, array_to_string(tags, '|');

with s0 as (
  select id, stream_id, duration_seconds,
         row_number() over (partition by coalesce(stream_id, id::text) order by duration_seconds desc nulls last, id desc) rn
  from twitch_stream_sessions
  where started_at >= '2026-06-22' and started_at < '2026-10-26'
    and (game_name = 'Deadlock' or had_deadlock_in_session)
    and language like 'de%'
    and tags is not null
),
s as (select id from s0 where rn = 1),
bots as (
  select lower(c.chatter_login) as login
  from twitch_session_chatters c join s on s.id = c.session_id
  group by 1
  having count(distinct c.streamer_login) >= 30
),
agg as (
  select c.session_id,
         count(*) filter (where c.messages >= 1 and lower(c.chatter_login) not in (select login from bots)) as chat_uc,
         count(*) filter (where c.messages >= 1 and c.is_first_time_streamer
                          and lower(c.chatter_login) not in (select login from bots)) as chat_ftc,
         count(*) as tracked_rows
  from twitch_session_chatters c join s on s.id = c.session_id
  group by c.session_id
)
select s.id as session_id, coalesce(agg.chat_uc, 0) as chat_uc, coalesce(agg.chat_ftc, 0) as chat_ftc,
       coalesce(agg.tracked_rows, 0) as tracked_rows
from s left join agg on agg.session_id = s.id;
