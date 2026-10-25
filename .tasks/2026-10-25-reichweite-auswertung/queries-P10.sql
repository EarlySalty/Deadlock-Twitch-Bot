select min(snapshot_at), max(snapshot_at), count(*), count(distinct user_id), count(distinct stream_id), sum(sample_seconds)/3600.0 as hours, percentile_cont(0.5) within group (order by sample_seconds) as med_sample from category_snapshots_normalized where snapshot_at >= '2026-09-27';

select language, count(*), count(distinct user_id), round((sum(sample_seconds)/3600.0)::numeric,0) hours from category_snapshots_normalized where snapshot_at >= '2026-09-27' group by 1 order by 4 desc limit 30;

select pg_get_viewdef('category_snapshots_normalized'::regclass, true);

select table_name, column_name, data_type from information_schema.columns where table_schema='public' and table_name in ('category_snapshot_samples','category_snapshot_versions') order by table_name, ordinal_position;

select c.relname, c.reltuples::bigint, pg_size_pretty(pg_total_relation_size(c.oid)), c.relkind from pg_class c join pg_namespace n on n.oid=c.relnamespace where n.nspname='public' and c.relname in ('category_snapshot_samples','category_snapshot_versions');

select 'rollup', min(hour_at)::text, max(hour_at)::text, count(*)::text from category_chat_rollup where hour_at >= '2026-09-27';

select 'chan', min(first_seen)::text, max(first_seen)::text, count(*)::text, count(distinct user_id)::text, count(created_at)::text, min(created_at)::text, count(followers_total)::text, max(followers_checked_at)::text from category_channels;

select round(sample_seconds::numeric,0) s, count(*) from category_snapshot_samples where snapshot_at >= '2026-10-09' group by 1 order by 2 desc limit 12;

select min(sent_at), max(sent_at) from category_chat_metrics where sent_at >= '2026-09-27' and sent_at < '2026-09-28';

select s.stream_id, s.version_id, v.user_id, v.language,
  count(*) as n_samples,
  sum(s.sample_seconds) as live_s,
  sum(s.viewer_count*s.sample_seconds)/nullif(sum(s.sample_seconds),0) as wavg_viewers,
  max(s.viewer_count) as peak_viewers,
  min(s.snapshot_at) as first_at,
  max(s.snapshot_at) as last_at
from category_snapshot_samples s
join category_snapshot_versions v on v.id = s.version_id
where s.snapshot_at >= '2026-10-09' and s.snapshot_at < '2026-10-26' and s.sample_seconds > 0
group by 1,2,3,4;

select v.user_id, date_trunc('hour', s.snapshot_at) as hour_at,
  count(*) as n_samples,
  count(distinct s.stream_id) as n_streams,
  sum(s.sample_seconds) as live_s,
  sum(s.viewer_count*s.sample_seconds)/nullif(sum(s.sample_seconds),0) as wavg_viewers,
  max(s.viewer_count) as peak_viewers
from category_snapshot_samples s
join category_snapshot_versions v on v.id = s.version_id
where s.snapshot_at >= '2026-10-09' and s.snapshot_at < '2026-10-26' and s.sample_seconds > 0
group by 1,2;

select v.user_id,
  case when s.snapshot_at < '2026-10-16' then 1 when s.snapshot_at < '2026-10-23' then 2 else 3 end as wk,
  count(*) as n_samples,
  count(distinct s.stream_id) as n_streams,
  sum(s.sample_seconds) as live_s,
  sum(s.viewer_count*s.sample_seconds)/nullif(sum(s.sample_seconds),0) as wavg_viewers,
  percentile_cont(0.5) within group (order by s.viewer_count) as med_viewers,
  percentile_cont(0.25) within group (order by s.viewer_count) as p25_viewers,
  percentile_cont(0.75) within group (order by s.viewer_count) as p75_viewers,
  max(s.viewer_count) as peak_viewers
from category_snapshot_samples s
join category_snapshot_versions v on v.id = s.version_id
where s.snapshot_at >= '2026-10-09' and s.snapshot_at < '2026-10-26' and s.sample_seconds > 0
group by 1,2;

select v.id as version_id, v.stream_id, v.user_id, v.language, v.started_at, v.valid_from, v.title, v.tags, v.is_mature
from category_snapshot_versions v
where v.id in (select distinct s.version_id from category_snapshot_samples s where s.snapshot_at >= '2026-10-09' and s.snapshot_at < '2026-10-26');

select user_id, created_at, broadcaster_type, followers_total, broadcaster_language, first_seen, followers_checked_at, followers_http_status
from category_channels;

select room_user_id, date_trunc('hour', sent_at) as hour_at,
  count(*) filter (where shared_chat_copy is not true) as msgs,
  count(*) filter (where shared_chat_copy is not true and chatter_user_id = room_user_id) as self_msgs,
  count(distinct chatter_user_id) filter (where shared_chat_copy is not true and chatter_user_id <> room_user_id) as distinct_chatters,
  count(*) filter (where shared_chat_copy is true) as shared_copies
from category_chat_metrics
where sent_at >= '2026-10-09' and sent_at < '2026-10-26'
group by 1,2;
