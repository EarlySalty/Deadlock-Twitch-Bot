use crate::{
    calendar::{berlin_week_start, midnight},
    Engine, Result,
};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use std::collections::BTreeMap;

type Interval = (DateTime<Utc>, DateTime<Utc>);

pub(crate) fn union_seconds(mut intervals: Vec<(DateTime<Utc>, DateTime<Utc>)>) -> i64 {
    intervals.retain(|(a, b)| a < b);
    intervals.sort_unstable();
    let mut total = 0;
    let mut previous: Option<(DateTime<Utc>, DateTime<Utc>)> = None;
    for (start, end) in intervals {
        match previous {
            Some((a, b)) if start <= b => previous = Some((a, b.max(end))),
            Some((a, b)) => {
                total += (b - a).num_seconds();
                previous = Some((start, end));
            }
            None => previous = Some((start, end)),
        }
    }
    total + previous.map_or(0, |(a, b)| (b - a).num_seconds())
}

pub(crate) fn continuous_seconds(
    previous: DateTime<Utc>,
    now: DateTime<Utc>,
    accumulated: i64,
    same_session: bool,
    max_gap: i64,
) -> i64 {
    let delta = (now - previous).num_seconds();
    if same_session && (0..=max_gap).contains(&delta) {
        accumulated + delta
    } else {
        0
    }
}

#[derive(sqlx::FromRow)]
struct Sample {
    user_id: String,
    stream_id: String,
    started_at: DateTime<Utc>,
    snapshot_at: DateTime<Utc>,
    sample_seconds: f64,
}

impl Engine {
    pub(crate) async fn category_collection_coverage(&self, now: DateTime<Utc>) -> Result<()> {
        let week = berlin_week_start(now);
        let baseline_since = midnight(week - Duration::weeks(4));
        self.category_coverage_between(baseline_since, now).await
    }

    pub(crate) async fn category_coverage_between(
        &self,
        baseline_since: DateTime<Utc>,
        until: DateTime<Utc>,
    ) -> Result<()> {
        let since: DateTime<Utc> = sqlx::query_scalar(
            "SELECT GREATEST($1,started_at) FROM partner_effort_program WHERE singleton",
        )
        .bind(baseline_since)
        .fetch_one(&self.pool)
        .await?;
        let covered: bool = sqlx::query_scalar(
            "WITH settings AS (
                SELECT poll_seconds FROM category_collector_config
                WHERE singleton AND enabled AND preserve_raw_data
                    AND EXISTS(SELECT 1 FROM category_collector_status cs WHERE cs.singleton
                        AND cs.heartbeat_at >= $2 - interval '90 seconds'
                        AND COALESCE((cs.details->>'disk_paused')::boolean, TRUE) = FALSE)
            ), runs AS (
                SELECT r.snapshot_at,r.poll_seconds,
                    lag(r.snapshot_at) OVER (ORDER BY r.snapshot_at) AS previous_at,
                    lag(r.poll_seconds) OVER (ORDER BY r.snapshot_at) AS previous_poll_seconds
                FROM category_collection_runs r CROSS JOIN settings s
                WHERE r.snapshot_at >= $1 - make_interval(secs => 2 * s.poll_seconds)
                    AND r.snapshot_at <= $2
            ), coverage AS (
                SELECT
                    max(snapshot_at) FILTER (WHERE snapshot_at <= $1) AS before_start,
                    max(snapshot_at) AS latest,
                    bool_and(snapshot_at - previous_at <= make_interval(
                        secs => 2 * GREATEST(poll_seconds, previous_poll_seconds)
                    )) FILTER (WHERE previous_at IS NOT NULL AND snapshot_at > $1) AS gaps_ok
                FROM runs
            )
            SELECT COALESCE(
                coverage.before_start >= $1 - make_interval(secs => 2 * settings.poll_seconds)
                AND coverage.latest >= $2 - make_interval(secs => 3 * settings.poll_seconds)
                AND COALESCE(coverage.gaps_ok, TRUE),
                FALSE
            )
            FROM coverage
            LEFT JOIN settings ON TRUE",
        )
        .bind(since)
        .bind(until)
        .fetch_one(&self.pool)
        .await?;
        if covered {
            Ok(())
        } else {
            Err(crate::Error::Source("category_collection_incomplete"))
        }
    }

    pub(crate) async fn category_complete_between(
        &self,
        since: DateTime<Utc>,
        until: DateTime<Utc>,
    ) -> Result<bool> {
        match self.category_coverage_between(since, until).await {
            Ok(()) => Ok(true),
            Err(crate::Error::Source("category_collection_incomplete")) => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub(crate) async fn refresh_stream_evidence(&self, now: DateTime<Utc>) -> Result<()> {
        let week = berlin_week_start(now);
        let oldest: Option<NaiveDate> = sqlx::query_scalar("SELECT MIN(q.week_start) FROM partner_effort_weekly_quests q WHERE NOT EXISTS(SELECT 1 FROM partner_effort_events e WHERE e.partner_twitch_user_id=q.partner_twitch_user_id AND e.event_type='quest_done' AND e.source_id='quest:' || q.partner_twitch_user_id || ':' || q.week_start::text || ':all_three')")
            .fetch_one(&self.pool).await?;
        let first_week = oldest.map_or(week - Duration::weeks(5), |old| {
            old.min(week - Duration::weeks(5))
        });
        let since = midnight(first_week);
        let ids: Vec<_> = self
            .active_partners()
            .await?
            .into_iter()
            .map(|p| p.twitch_user_id)
            .collect();
        let samples: Vec<Sample> = sqlx::query_as("SELECT user_id,stream_id,started_at,snapshot_at,sample_seconds FROM category_stream_snapshots WHERE user_id=ANY($1) AND snapshot_at >= $2 AND snapshot_at <= $3 ORDER BY user_id,stream_id,snapshot_at")
            .bind(&ids).bind(since).bind(now).fetch_all(&self.pool).await?;
        type Key = (String, String, NaiveDate);
        let previous: Vec<(String, String, NaiveDate, DateTime<Utc>)> = sqlx::query_as("SELECT partner_twitch_user_id,stream_id,week_start,observed_through FROM partner_effort_stream_weeks WHERE partner_twitch_user_id=ANY($1) AND week_start >= $2")
            .bind(&ids).bind(first_week).fetch_all(&self.pool).await?;
        let observed: BTreeMap<Key, DateTime<Utc>> = previous
            .into_iter()
            .map(|(id, stream, week, through)| ((id, stream, week), through))
            .collect();
        let mut intervals: BTreeMap<Key, (Vec<Interval>, DateTime<Utc>)> = BTreeMap::new();
        for sample in samples {
            if !sample.sample_seconds.is_finite()
                || sample.sample_seconds <= 0.0
                || sample.sample_seconds > 300.0
                || sample.started_at > sample.snapshot_at
                || sample.stream_id.is_empty()
            {
                continue;
            }
            let mut a = (sample.snapshot_at
                - Duration::seconds(sample.sample_seconds.floor() as i64))
            .max(sample.started_at)
            .max(since);
            let b = sample.snapshot_at.min(now);
            while a < b {
                let week = berlin_week_start(a);
                let end = b.min(midnight(week + Duration::weeks(1)));
                let key = (sample.user_id.clone(), sample.stream_id.clone(), week);
                let start = observed.get(&key).map_or(a, |through| a.max(*through));
                if start < end {
                    let entry = intervals
                        .entry(key)
                        .or_insert_with(|| (Vec::new(), sample.snapshot_at));
                    entry.0.push((start, end));
                    entry.1 = entry.1.max(sample.snapshot_at);
                }
                a = end;
            }
        }
        let mut tx = self.pool.begin().await?;
        for ((id, stream, week), (spans, through)) in intervals {
            sqlx::query("INSERT INTO partner_effort_stream_weeks(partner_twitch_user_id,stream_id,week_start,deadlock_seconds,observed_through) VALUES($1,$2,$3,$4,$5) ON CONFLICT(partner_twitch_user_id,stream_id,week_start) DO UPDATE SET deadlock_seconds=partner_effort_stream_weeks.deadlock_seconds+EXCLUDED.deadlock_seconds,observed_through=GREATEST(partner_effort_stream_weeks.observed_through,EXCLUDED.observed_through)")
                .bind(id).bind(stream).bind(week).bind(union_seconds(spans)).bind(through).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn stream_minutes(
        &self,
        id: &str,
        first: NaiveDate,
        until: NaiveDate,
    ) -> Result<i64> {
        let seconds: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(deadlock_seconds),0)::bigint FROM partner_effort_stream_weeks WHERE partner_twitch_user_id=$1 AND week_start >= $2 AND week_start < $3")
            .bind(id).bind(first).bind(until).fetch_one(&self.pool).await?;
        Ok(seconds / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn t(s: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(s, 0).unwrap()
    }
    #[test]
    fn duplicate_and_overlapping_samples_never_multiply_time() {
        assert_eq!(
            union_seconds(vec![
                (t(0), t(60)),
                (t(0), t(60)),
                (t(30), t(90)),
                (t(200), t(210))
            ]),
            100
        );
    }
    #[test]
    fn missing_polls_and_changed_shared_sessions_reset_proof() {
        assert_eq!(continuous_seconds(t(0), t(60), 120, true, 150), 180);
        assert_eq!(continuous_seconds(t(0), t(3600), 120, true, 150), 0);
        assert_eq!(continuous_seconds(t(0), t(60), 120, false, 150), 0);
        assert_eq!(continuous_seconds(t(60), t(0), 120, true, 150), 0);
    }
}
