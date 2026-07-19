//! Öffentlicher, datenschutzarmer Vergleich aktiver Partner-Streamer.

use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration as StdDuration, Instant},
};

use axum::{
    extract::{Query, State},
    http::{header::CACHE_CONTROL, StatusCode},
    response::IntoResponse,
    Extension, Json,
};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use chrono_tz::Europe::Berlin;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgPool, Row};

const ALLOWED_DAYS: [i64; 3] = [7, 30, 90];
const CACHE_POLICY: &str = "public, max-age=60";
const CACHE_TTL: StdDuration = StdDuration::from_secs(60);
const QUERY_TIMEOUT: StdDuration = StdDuration::from_secs(15);

#[derive(Clone)]
struct CachedResponse {
    created_at: Instant,
    response: StreamerComparisonResponse,
}

/// Prozesslokaler Cache eines einzelnen Routers. Jeder Zeitraum hat ein eigenes
/// Schloss, sodass ein langsamer 90-Tage-Miss keine 7-/30-Tage-Treffer blockiert.
#[derive(Clone)]
pub struct StreamerComparisonCache {
    slots: Arc<[tokio::sync::Mutex<Option<CachedResponse>>; 3]>,
}

impl Default for StreamerComparisonCache {
    fn default() -> Self {
        Self {
            slots: Arc::new(std::array::from_fn(|_| tokio::sync::Mutex::new(None))),
        }
    }
}

impl StreamerComparisonCache {
    fn slot(&self, days: i64) -> &tokio::sync::Mutex<Option<CachedResponse>> {
        let index = ALLOWED_DAYS
            .iter()
            .position(|allowed| *allowed == days)
            .unwrap_or(1);
        &self.slots[index]
    }
}

#[derive(Debug, Deserialize)]
pub struct ComparisonQuery {
    days: Option<i64>,
}

#[derive(Debug, Clone)]
struct SessionMetric {
    login: String,
    display_name: String,
    sessions: i64,
    stream_hours: f64,
    average_viewers: f64,
    peak_viewers: Option<i32>,
    viewer_hours: f64,
    recent_hours: f64,
    recent_average_viewers: Option<f64>,
    previous_hours: f64,
    previous_average_viewers: Option<f64>,
}

#[derive(Debug, Clone, Default)]
struct RaidMetric {
    confirmed_raids: i64,
    raid_viewers_received: i64,
    measured_raids: i64,
    raid_uplift_5m: Option<f64>,
    raid_uplift_30m: Option<f64>,
    positive_raids_30m: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComparisonPeriod {
    days: i64,
    from: String,
    to: String,
    timezone: &'static str,
    trend_days: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComparisonMethodology {
    cohort: &'static str,
    minimum_hours_for_ranking: f64,
    raid_measurement: &'static str,
    privacy: &'static str,
    caveat: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSummary {
    streamer_count: usize,
    qualified_streamer_count: usize,
    stream_hours: f64,
    viewer_hours: f64,
    confirmed_raids: i64,
    viewers_forwarded: i64,
    measured_raids: i64,
    average_raid_uplift_5m: Option<f64>,
    average_raid_uplift_30m: Option<f64>,
    positive_raid_share_30m: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamerRanks {
    stream_hours: Option<usize>,
    average_viewers: Option<usize>,
    viewer_hours: Option<usize>,
    momentum: Option<usize>,
    raid_impact: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NextStep {
    code: &'static str,
    title: &'static str,
    reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamerComparisonRow {
    login: String,
    display_name: String,
    twitch_url: String,
    sample_qualified: bool,
    trend_qualified: bool,
    sessions: i64,
    stream_hours: f64,
    average_viewers: f64,
    peak_viewers: Option<i32>,
    viewer_hours: f64,
    recent_hours: f64,
    recent_average_viewers: Option<f64>,
    previous_hours: f64,
    previous_average_viewers: Option<f64>,
    viewer_growth_pct: Option<f64>,
    confirmed_raids: i64,
    raid_viewers_received: i64,
    measured_raids: i64,
    raid_data_qualified: bool,
    raid_uplift_5m: Option<f64>,
    raid_uplift_30m: Option<f64>,
    positive_raid_share_30m: Option<f64>,
    ranks: StreamerRanks,
    next_step: NextStep,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamerComparisonResponse {
    generated_at: String,
    period: ComparisonPeriod,
    methodology: ComparisonMethodology,
    network: NetworkSummary,
    streamers: Vec<StreamerComparisonRow>,
}

fn ranking_minimum_hours(days: i64) -> f64 {
    match days {
        7 => 5.0,
        90 => 20.0,
        _ => 10.0,
    }
}

fn round(value: f64, digits: i32) -> f64 {
    let factor = 10_f64.powi(digits);
    (value * factor).round() / factor
}

fn growth_pct(metric: &SessionMetric) -> Option<f64> {
    let recent = metric.recent_average_viewers?;
    let previous = metric.previous_average_viewers?;
    if previous <= 0.0 {
        return None;
    }
    Some(round((recent - previous) / previous * 100.0, 1))
}

fn rank_values<I>(values: I) -> HashMap<String, usize>
where
    I: IntoIterator<Item = (String, f64)>,
{
    let mut values: Vec<_> = values
        .into_iter()
        .filter(|(_, value)| value.is_finite())
        .collect();
    values.sort_by(|left, right| right.1.partial_cmp(&left.1).unwrap_or(Ordering::Equal));

    let mut result = HashMap::with_capacity(values.len());
    let mut previous_value: Option<f64> = None;
    let mut current_rank = 0;
    for (index, (login, value)) in values.into_iter().enumerate() {
        if previous_value.is_none_or(|previous| (previous - value).abs() >= 0.005) {
            current_rank = index + 1;
        }
        previous_value = Some(value);
        result.insert(login, current_rank);
    }
    result
}

fn recommendation(
    metric: &SessionMetric,
    raid: &RaidMetric,
    minimum_hours: f64,
    trend_qualified: bool,
) -> NextStep {
    let visible_raid_uplift_5m = round(raid.raid_uplift_5m.unwrap_or_default(), 1);
    let visible_raid_uplift_30m = round(raid.raid_uplift_30m.unwrap_or_default(), 1);

    if round(metric.stream_hours, 1) < minimum_hours {
        return NextStep {
            code: "collect_more_data",
            title: "Erst eine belastbare Basis sammeln",
            reason: format!(
                "{:.1} von mindestens {:.0} Streamstunden sind erfasst; vorher wären Vergleiche zu zufällig.",
                metric.stream_hours, minimum_hours
            ),
        };
    }

    if trend_qualified {
        if let (Some(recent), Some(previous), Some(growth)) = (
            metric.recent_average_viewers,
            metric.previous_average_viewers,
            growth_pct(metric),
        ) {
            if growth >= 20.0 && recent - previous >= 0.5 {
                return NextStep {
                    code: "protect_momentum",
                    title: "Momentum jetzt wiederholbar machen",
                    reason: format!(
                        "Der Zuschauerschnitt liegt im jüngsten Vergleichsfenster {:.0}% höher. Gleiche Slots und Formate zuerst wiederholen.",
                        growth
                    ),
                };
            }
        }
    }

    if raid.measured_raids >= 5 && visible_raid_uplift_30m >= 0.5 {
        return NextStep {
            code: "scale_matching_raids",
            title: "Passende Raids gezielt hochfahren",
            reason: format!(
                "Nach {} messbaren Raids bleiben nach 30 Minuten im Mittel {:+.1} Zuschauer gegenüber der Vorphase.",
                raid.measured_raids,
                visible_raid_uplift_30m
            ),
        };
    }

    if raid.confirmed_raids < 3 {
        return NextStep {
            code: "test_more_raids",
            title: "Mehr Raid-Daten gezielt testen",
            reason: format!(
                "Mit {} bestätigten Raids ist noch offen, welche Community am besten passt. Kleine, wiederholte Tests sind aussagekräftiger.",
                raid.confirmed_raids
            ),
        };
    }

    if raid.measured_raids >= 3 && visible_raid_uplift_5m >= 0.5 && visible_raid_uplift_30m < 0.2 {
        return NextStep {
            code: "strengthen_handoff",
            title: "Die ersten 30 Minuten nach dem Raid stärken",
            reason: "Der erste Ausschlag ist sichtbar, hält aber noch nicht stabil. Begrüßung, Kontext und ein klarer nächster Programmpunkt sind der beste Test.".to_owned(),
        };
    }

    NextStep {
        code: "keep_testing",
        title: "Konstant weiter testen",
        reason: "Die Daten zeigen noch keinen einzelnen dominanten Hebel. Sendezeit, Format und Raid-Quelle jeweils einzeln variieren.".to_owned(),
    }
}

async fn load_session_metrics(
    pool: &PgPool,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    previous_from: DateTime<Utc>,
    recent_from: DateTime<Utc>,
) -> Result<Vec<SessionMetric>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        WITH active AS (
            SELECT LOWER(twitch_login) AS login, MIN(twitch_login) AS display_name
            FROM twitch_streamers_partner_state
            WHERE is_partner_active = 1
            GROUP BY LOWER(twitch_login)
        ), parts AS (
            SELECT
                a.login,
                a.display_name,
                s.id,
                s.started_at,
                COALESCE(s.avg_viewers, 0)::DOUBLE PRECISION AS avg_viewers,
                s.peak_viewers,
                GREATEST(0.0, EXTRACT(EPOCH FROM (
                    LEAST(COALESCE(s.ended_at, $2), $2) - GREATEST(s.started_at, $1)
                )))::DOUBLE PRECISION AS current_sec,
                CASE WHEN s.started_at < $2 AND COALESCE(s.ended_at, $2) > $4 THEN
                    GREATEST(0.0, EXTRACT(EPOCH FROM (
                        LEAST(COALESCE(s.ended_at, $2), $2) - GREATEST(s.started_at, $4)
                    ))) ELSE 0.0 END::DOUBLE PRECISION AS recent_sec,
                CASE WHEN s.started_at < $4 AND COALESCE(s.ended_at, $4) > $3 THEN
                    GREATEST(0.0, EXTRACT(EPOCH FROM (
                        LEAST(COALESCE(s.ended_at, $4), $4) - GREATEST(s.started_at, $3)
                    ))) ELSE 0.0 END::DOUBLE PRECISION AS previous_sec
            FROM active a
            JOIN twitch_stream_sessions s
              ON LOWER(s.streamer_login) = a.login
             AND s.started_at < $2
             AND COALESCE(s.ended_at, $2) > $1
        )
        SELECT
            login,
            display_name,
            COUNT(*) FILTER (WHERE current_sec > 0)::BIGINT AS sessions,
            SUM(current_sec) / 3600.0 AS stream_hours,
            SUM(avg_viewers * current_sec) / NULLIF(SUM(current_sec), 0) AS average_viewers,
            MAX(peak_viewers) FILTER (WHERE started_at >= $1)::INTEGER AS peak_viewers,
            SUM(avg_viewers * current_sec) / 3600.0 AS viewer_hours,
            SUM(recent_sec) / 3600.0 AS recent_hours,
            SUM(avg_viewers * recent_sec) / NULLIF(SUM(recent_sec), 0) AS recent_average_viewers,
            SUM(previous_sec) / 3600.0 AS previous_hours,
            SUM(avg_viewers * previous_sec) / NULLIF(SUM(previous_sec), 0) AS previous_average_viewers
        FROM parts
        GROUP BY login, display_name
        HAVING SUM(current_sec) > 0
        "#,
    )
    .bind(from)
    .bind(to)
    .bind(previous_from)
    .bind(recent_from)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(SessionMetric {
                login: row.try_get::<String, _>("login")?.trim().to_lowercase(),
                display_name: row.try_get::<String, _>("display_name")?,
                sessions: row.try_get("sessions")?,
                stream_hours: row.try_get("stream_hours")?,
                average_viewers: row.try_get("average_viewers")?,
                peak_viewers: row.try_get("peak_viewers")?,
                viewer_hours: row.try_get("viewer_hours")?,
                recent_hours: row.try_get("recent_hours")?,
                recent_average_viewers: row.try_get("recent_average_viewers")?,
                previous_hours: row.try_get("previous_hours")?,
                previous_average_viewers: row.try_get("previous_average_viewers")?,
            })
        })
        .collect()
}

async fn load_raid_metrics(
    pool: &PgPool,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<HashMap<String, RaidMetric>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        WITH active AS (
            SELECT DISTINCT LOWER(twitch_login) AS login
            FROM twitch_streamers_partner_state
            WHERE is_partner_active = 1
        ), raid_candidates AS (
            SELECT
                rr.raid_id,
                rr.executed_at,
                LOWER(rr.to_broadcaster_login) AS target,
                rr.viewer_count_sent
            FROM twitch_raid_retention rr
            JOIN active a ON a.login = LOWER(rr.to_broadcaster_login)
            WHERE rr.executed_at >= $1 - INTERVAL '40 minutes'
              AND rr.executed_at < $2 + INTERVAL '40 minutes'
        ), sequenced AS (
            SELECT
                *,
                LAG(executed_at) OVER (
                    PARTITION BY target ORDER BY executed_at, raid_id
                ) AS previous_raid_at,
                LEAD(executed_at) OVER (
                    PARTITION BY target ORDER BY executed_at, raid_id
                ) AS next_raid_at
            FROM raid_candidates
        ), raids AS (
            SELECT *
            FROM sequenced
            WHERE executed_at >= $1 AND executed_at < $2
        ), windows AS (
            SELECT
                r.raid_id,
                r.executed_at,
                r.target,
                r.viewer_count_sent,
                r.previous_raid_at,
                r.next_raid_at,
                COUNT(st.viewer_count) FILTER (
                    WHERE st.ts_utc >= r.executed_at - INTERVAL '10 minutes'
                      AND st.ts_utc < r.executed_at - INTERVAL '1 minute'
                ) AS pre_n,
                (AVG(st.viewer_count) FILTER (
                    WHERE st.ts_utc >= r.executed_at - INTERVAL '10 minutes'
                      AND st.ts_utc < r.executed_at - INTERVAL '1 minute'
                ))::DOUBLE PRECISION AS pre_avg,
                COUNT(st.viewer_count) FILTER (
                    WHERE st.ts_utc >= r.executed_at
                      AND st.ts_utc < r.executed_at + INTERVAL '5 minutes'
                ) AS post5_n,
                (AVG(st.viewer_count) FILTER (
                    WHERE st.ts_utc >= r.executed_at
                      AND st.ts_utc < r.executed_at + INTERVAL '5 minutes'
                ))::DOUBLE PRECISION AS post5_avg,
                COUNT(st.viewer_count) FILTER (
                    WHERE st.ts_utc >= r.executed_at + INTERVAL '5 minutes'
                      AND st.ts_utc < r.executed_at + INTERVAL '15 minutes'
                ) AS post15_n,
                COUNT(st.viewer_count) FILTER (
                    WHERE st.ts_utc >= r.executed_at + INTERVAL '15 minutes'
                      AND st.ts_utc < r.executed_at + INTERVAL '30 minutes'
                ) AS post30_n,
                (AVG(st.viewer_count) FILTER (
                    WHERE st.ts_utc >= r.executed_at + INTERVAL '15 minutes'
                      AND st.ts_utc < r.executed_at + INTERVAL '30 minutes'
                ))::DOUBLE PRECISION AS post30_avg
            FROM raids r
            LEFT JOIN twitch_stats_tracked st
              ON LOWER(st.streamer) = r.target
             AND st.ts_utc >= r.executed_at - INTERVAL '10 minutes'
             AND st.ts_utc < r.executed_at + INTERVAL '30 minutes'
            GROUP BY r.raid_id, r.executed_at, r.target, r.viewer_count_sent,
                     r.previous_raid_at, r.next_raid_at
        ), marked AS (
            SELECT *, (
                pre_n >= 4 AND post5_n >= 4 AND post15_n >= 4 AND post30_n >= 4
                AND (
                    previous_raid_at IS NULL
                    OR previous_raid_at <= executed_at - INTERVAL '40 minutes'
                )
                AND (
                    next_raid_at IS NULL
                    OR next_raid_at >= executed_at + INTERVAL '40 minutes'
                )
            ) AS measurable
            FROM windows
        )
        SELECT
            target,
            COUNT(*)::BIGINT AS confirmed_raids,
            COALESCE(SUM(viewer_count_sent), 0)::BIGINT AS raid_viewers_received,
            COUNT(*) FILTER (WHERE measurable)::BIGINT AS measured_raids,
            AVG(post5_avg - pre_avg) FILTER (WHERE measurable) AS raid_uplift_5m,
            AVG(post30_avg - pre_avg) FILTER (WHERE measurable) AS raid_uplift_30m,
            COUNT(*) FILTER (WHERE measurable AND post30_avg > pre_avg)::BIGINT AS positive_raids_30m
        FROM marked
        GROUP BY target
        "#,
    )
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            let login: String = row.try_get("target")?;
            Ok((
                login,
                RaidMetric {
                    confirmed_raids: row.try_get("confirmed_raids")?,
                    raid_viewers_received: row.try_get("raid_viewers_received")?,
                    measured_raids: row.try_get("measured_raids")?,
                    raid_uplift_5m: row.try_get("raid_uplift_5m")?,
                    raid_uplift_30m: row.try_get("raid_uplift_30m")?,
                    positive_raids_30m: row.try_get("positive_raids_30m")?,
                },
            ))
        })
        .collect()
}

fn build_response(
    now: DateTime<Utc>,
    days: i64,
    trend_days: i64,
    metrics: Vec<SessionMetric>,
    raid_metrics: HashMap<String, RaidMetric>,
) -> StreamerComparisonResponse {
    let minimum_hours = ranking_minimum_hours(days);
    let qualified = |metric: &SessionMetric| round(metric.stream_hours, 1) >= minimum_hours;
    let trend_qualified = |metric: &SessionMetric| {
        metric.recent_hours >= 2.0
            && metric.previous_hours >= 2.0
            && metric.recent_average_viewers.is_some()
            && metric.previous_average_viewers.is_some()
    };

    let hours_ranks = rank_values(
        metrics
            .iter()
            .filter(|metric| qualified(metric))
            .map(|metric| (metric.login.clone(), round(metric.stream_hours, 1))),
    );
    let average_ranks = rank_values(
        metrics
            .iter()
            .filter(|metric| qualified(metric))
            .map(|metric| (metric.login.clone(), round(metric.average_viewers, 2))),
    );
    let viewer_hour_ranks = rank_values(
        metrics
            .iter()
            .filter(|metric| qualified(metric))
            .map(|metric| (metric.login.clone(), round(metric.viewer_hours, 1))),
    );
    let momentum_ranks = rank_values(
        metrics
            .iter()
            .filter(|metric| qualified(metric) && trend_qualified(metric))
            .filter_map(|metric| growth_pct(metric).map(|growth| (metric.login.clone(), growth))),
    );
    let raid_impact_ranks = rank_values(metrics.iter().filter_map(|metric| {
        let raid = raid_metrics.get(&metric.login)?;
        (qualified(metric) && raid.measured_raids >= 5)
            .then_some((metric.login.clone(), round(raid.raid_uplift_30m?, 2)))
    }));

    let mut rows: Vec<_> = metrics
        .iter()
        .map(|metric| {
            let raid = raid_metrics.get(&metric.login).cloned().unwrap_or_default();
            let is_qualified = qualified(metric);
            let is_trend_qualified = trend_qualified(metric);
            let positive_raid_share = (raid.measured_raids > 0).then(|| {
                round(
                    raid.positive_raids_30m as f64 / raid.measured_raids as f64 * 100.0,
                    1,
                )
            });
            StreamerComparisonRow {
                login: metric.login.clone(),
                display_name: metric.display_name.clone(),
                twitch_url: format!("https://www.twitch.tv/{}", metric.login),
                sample_qualified: is_qualified,
                trend_qualified: is_trend_qualified,
                sessions: metric.sessions,
                stream_hours: round(metric.stream_hours, 1),
                average_viewers: round(metric.average_viewers, 2),
                peak_viewers: metric.peak_viewers,
                viewer_hours: round(metric.viewer_hours, 1),
                recent_hours: round(metric.recent_hours, 1),
                recent_average_viewers: metric.recent_average_viewers.map(|value| round(value, 2)),
                previous_hours: round(metric.previous_hours, 1),
                previous_average_viewers: metric
                    .previous_average_viewers
                    .map(|value| round(value, 2)),
                viewer_growth_pct: is_trend_qualified.then(|| growth_pct(metric)).flatten(),
                confirmed_raids: raid.confirmed_raids,
                raid_viewers_received: raid.raid_viewers_received,
                measured_raids: raid.measured_raids,
                raid_data_qualified: raid.measured_raids >= 5,
                raid_uplift_5m: raid.raid_uplift_5m.map(|value| round(value, 2)),
                raid_uplift_30m: raid.raid_uplift_30m.map(|value| round(value, 2)),
                positive_raid_share_30m: positive_raid_share,
                ranks: StreamerRanks {
                    stream_hours: hours_ranks.get(&metric.login).copied(),
                    average_viewers: average_ranks.get(&metric.login).copied(),
                    viewer_hours: viewer_hour_ranks.get(&metric.login).copied(),
                    momentum: momentum_ranks.get(&metric.login).copied(),
                    raid_impact: raid_impact_ranks.get(&metric.login).copied(),
                },
                next_step: recommendation(metric, &raid, minimum_hours, is_trend_qualified),
            }
        })
        .collect();
    rows.sort_by(|left, right| {
        right
            .viewer_hours
            .partial_cmp(&left.viewer_hours)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.login.cmp(&right.login))
    });

    let cohort_logins: HashSet<&str> = metrics.iter().map(|metric| metric.login.as_str()).collect();
    let mut confirmed_raids = 0;
    let mut viewers_forwarded = 0;
    let mut measured_raids = 0;
    let mut positive_raids = 0;
    let mut weighted_uplift_5m = 0.0;
    let mut weighted_uplift_30m = 0.0;
    for (login, raid) in &raid_metrics {
        if !cohort_logins.contains(login.as_str()) {
            continue;
        }
        confirmed_raids += raid.confirmed_raids;
        viewers_forwarded += raid.raid_viewers_received;
        measured_raids += raid.measured_raids;
        positive_raids += raid.positive_raids_30m;
        weighted_uplift_5m += raid.raid_uplift_5m.unwrap_or_default() * raid.measured_raids as f64;
        weighted_uplift_30m +=
            raid.raid_uplift_30m.unwrap_or_default() * raid.measured_raids as f64;
    }
    let from = now - Duration::days(days);

    let now_berlin = now.with_timezone(&Berlin);
    let from_berlin = from.with_timezone(&Berlin);

    StreamerComparisonResponse {
        generated_at: now_berlin.to_rfc3339_opts(SecondsFormat::Secs, true),
        period: ComparisonPeriod {
            days,
            from: from_berlin.to_rfc3339_opts(SecondsFormat::Secs, true),
            to: now_berlin.to_rfc3339_opts(SecondsFormat::Secs, true),
            timezone: "Europe/Berlin",
            trend_days,
        },
        methodology: ComparisonMethodology {
            cohort: "Aktive Partner mit mindestens einem Stream im Zeitraum",
            minimum_hours_for_ranking: minimum_hours,
            raid_measurement: "Durchschnittliche Zuschauer 10 bis 1 Minute vor dem Raid gegenüber 0 bis 5 und 15 bis 30 Minuten danach; mindestens vier Messpunkte je Fenster. Überlappende 40-Minuten-Fenster werden nicht gewertet.",
            privacy: "Nur aggregierte Stream- und Raid-Daten. Keine Einnahmen, Abos, Discord-IDs oder einzelnen Zuschauer.",
            caveat: "Zeitliche Veränderung ist ein Wirkungshinweis, aber kein Beweis, dass allein der Raid die Veränderung verursacht hat.",
        },
        network: NetworkSummary {
            streamer_count: rows.len(),
            qualified_streamer_count: metrics.iter().filter(|metric| qualified(metric)).count(),
            stream_hours: round(metrics.iter().map(|metric| metric.stream_hours).sum(), 1),
            viewer_hours: round(metrics.iter().map(|metric| metric.viewer_hours).sum(), 1),
            confirmed_raids,
            viewers_forwarded,
            measured_raids,
            average_raid_uplift_5m: (measured_raids > 0)
                .then(|| round(weighted_uplift_5m / measured_raids as f64, 2)),
            average_raid_uplift_30m: (measured_raids > 0)
                .then(|| round(weighted_uplift_30m / measured_raids as f64, 2)),
            positive_raid_share_30m: (measured_raids > 0)
                .then(|| round(positive_raids as f64 / measured_raids as f64 * 100.0, 1)),
        },
        streamers: rows,
    }
}

/// `GET /twitch/api/v2/public/streamer-comparison?days=7|30|90`
pub async fn streamer_comparison_handler(
    State(pool): State<PgPool>,
    Extension(cache): Extension<StreamerComparisonCache>,
    Query(query): Query<ComparisonQuery>,
) -> impl IntoResponse {
    let days = query.days.unwrap_or(30);
    if !ALLOWED_DAYS.contains(&days) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid_days", "allowedDays": ALLOWED_DAYS})),
        )
            .into_response();
    }

    // Der Raid-Vorher-/Nachher-Join ist bewusst nicht bei jedem Seitenaufruf neu
    // fällig. Schloss-Wartezeit und DB-Abfragen teilen sich dieselbe Deadline.
    let deadline = tokio::time::Instant::now() + QUERY_TIMEOUT;
    let mut cache_slot = match tokio::time::timeout_at(deadline, cache.slot(days).lock()).await {
        Ok(cache_slot) => cache_slot,
        Err(_) => {
            tracing::error!(
                timeout_seconds = QUERY_TIMEOUT.as_secs(),
                phase = "cache_wait",
                "public streamer comparison request timed out"
            );
            return (
                StatusCode::GATEWAY_TIMEOUT,
                Json(json!({"error":"query_timeout"})),
            )
                .into_response();
        }
    };
    if let Some(cached) = cache_slot.as_ref() {
        if cached.created_at.elapsed() < CACHE_TTL {
            return (
                [(CACHE_CONTROL, CACHE_POLICY)],
                Json(cached.response.clone()),
            )
                .into_response();
        }
    }

    let now = Utc::now();
    let from = now - Duration::days(days);
    let trend_days = (days / 2).clamp(3, 7);
    let recent_from = now - Duration::days(trend_days);
    let previous_from = recent_from - Duration::days(trend_days);

    let query_result = tokio::time::timeout_at(deadline, async {
        tokio::join!(
            load_session_metrics(&pool, from, now, previous_from, recent_from),
            load_raid_metrics(&pool, from, now),
        )
    })
    .await;
    let (session_metrics, raid_metrics) = match query_result {
        Ok((Ok(session_metrics), Ok(raid_metrics))) => (session_metrics, raid_metrics),
        Ok((Err(error), _)) | Ok((_, Err(error))) => {
            tracing::error!("public streamer comparison query failed: {error}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":"internal_error"})),
            )
                .into_response();
        }
        Err(_) => {
            tracing::error!(
                timeout_seconds = QUERY_TIMEOUT.as_secs(),
                phase = "database",
                "public streamer comparison query timed out"
            );
            return (
                StatusCode::GATEWAY_TIMEOUT,
                Json(json!({"error":"query_timeout"})),
            )
                .into_response();
        }
    };

    let response = build_response(now, days, trend_days, session_metrics, raid_metrics);
    *cache_slot = Some(CachedResponse {
        created_at: Instant::now(),
        response: response.clone(),
    });
    ([(CACHE_CONTROL, CACHE_POLICY)], Json(response)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named_session(
        login: &str,
        hours: f64,
        recent: Option<f64>,
        previous: Option<f64>,
    ) -> SessionMetric {
        SessionMetric {
            login: login.to_owned(),
            display_name: login.to_owned(),
            sessions: 4,
            stream_hours: hours,
            average_viewers: 4.0,
            peak_viewers: Some(8),
            viewer_hours: hours * 4.0,
            recent_hours: 4.0,
            recent_average_viewers: recent,
            previous_hours: 4.0,
            previous_average_viewers: previous,
        }
    }

    fn session(hours: f64, recent: Option<f64>, previous: Option<f64>) -> SessionMetric {
        named_session("test", hours, recent, previous)
    }

    #[test]
    fn recommendation_macht_jede_heuristik_begruendet_sichtbar() {
        let metric = session(20.0, Some(6.0), Some(3.0));
        let next_step = recommendation(&metric, &RaidMetric::default(), 10.0, true);
        assert_eq!(next_step.code, "protect_momentum");
        assert!(next_step.reason.contains("100%"));

        let metric = session(2.0, None, None);
        let next_step = recommendation(&metric, &RaidMetric::default(), 10.0, false);
        assert_eq!(next_step.code, "collect_more_data");
        assert!(next_step.reason.contains("mindestens 10"));
    }

    #[test]
    fn ranking_verwendet_wettbewerbsraenge_bei_gleichstand() {
        let ranks = rank_values([
            ("a".to_owned(), 5.0),
            ("b".to_owned(), 5.0),
            ("c".to_owned(), 3.0),
        ]);
        assert_eq!(ranks["a"], 1);
        assert_eq!(ranks["b"], 1);
        assert_eq!(ranks["c"], 3);
    }

    #[test]
    fn sichtbare_stundenschwelle_und_ranking_nutzen_dieselbe_rundung() {
        let response = build_response(
            Utc::now(),
            30,
            7,
            vec![
                named_session("qualified", 9.96, Some(6.0), Some(3.0)),
                named_session("too_short", 9.94, Some(100.0), Some(1.0)),
            ],
            HashMap::new(),
        );

        let qualified = response
            .streamers
            .iter()
            .find(|row| row.login == "qualified")
            .unwrap();
        assert!(qualified.sample_qualified);
        assert_eq!(qualified.stream_hours, 10.0);
        assert_eq!(qualified.ranks.momentum, Some(1));

        let too_short = response
            .streamers
            .iter()
            .find(|row| row.login == "too_short")
            .unwrap();
        assert!(!too_short.sample_qualified);
        assert_eq!(too_short.stream_hours, 9.9);
        assert_eq!(too_short.ranks.momentum, None);
    }

    #[test]
    fn empfehlung_nutzt_die_auf_der_karte_sichtbare_raid_rundung() {
        let metric = session(20.0, None, None);
        let rounded_up = RaidMetric {
            confirmed_raids: 5,
            measured_raids: 5,
            raid_uplift_30m: Some(0.46),
            ..RaidMetric::default()
        };
        assert_eq!(
            recommendation(&metric, &rounded_up, 10.0, false).code,
            "scale_matching_raids"
        );

        let visible_boundary = RaidMetric {
            confirmed_raids: 3,
            measured_raids: 3,
            raid_uplift_5m: Some(0.54),
            raid_uplift_30m: Some(0.16),
            ..RaidMetric::default()
        };
        assert_eq!(
            recommendation(&metric, &visible_boundary, 10.0, false).code,
            "keep_testing"
        );
    }

    #[tokio::test]
    async fn cache_wartezeit_respektiert_eine_deadline() {
        let cache = StreamerComparisonCache::default();
        let held_slot = cache.slot(30).lock().await;
        let deadline = tokio::time::Instant::now() + StdDuration::from_millis(5);

        let waiting = tokio::time::timeout_at(deadline, cache.slot(30).lock()).await;

        assert!(waiting.is_err());
        drop(held_slot);
    }
}
