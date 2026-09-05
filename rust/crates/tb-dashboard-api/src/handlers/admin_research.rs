//! Admin-Research für den Onboarding-Wert eines Twitch-Streamers.

use std::collections::{BTreeMap, HashSet};

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;

use crate::{
    auth::level::DashboardAuthLevel,
    handlers::category_comparison::{get_tier, percentile_of},
    query_int::parse_bounded_query_int,
};

const SESSION_GAP_SECONDS: i64 = 30 * 60;

#[derive(Deserialize)]
pub struct ResearchQuery {
    pub days: Option<String>,
}

#[derive(Serialize)]
struct ResearchResponse {
    login: String,
    days: i64,
    found: bool,
    is_already_partner: bool,
    partner_status: Option<String>,
    subject: SubjectMetrics,
    baseline: Baseline,
    score: Score,
}

#[derive(Serialize)]
struct ResearchSuggestionsResponse {
    days: i64,
    baseline: Baseline,
    items: Vec<ResearchSuggestion>,
}

#[derive(Serialize)]
struct ResearchSuggestion {
    login: String,
    subject: SubjectMetrics,
    score: Score,
}

#[derive(Serialize)]
struct SubjectMetrics {
    sessions_count: usize,
    total_hours: f64,
    active_days: usize,
    avg_viewers: f64,
    median_viewers: f64,
    peak_viewers: i32,
    sample_count: usize,
    last_seen: Option<DateTime<Utc>>,
    dominant_language: Option<String>,
    de_share: f64,
    recent_titles: Vec<String>,
}

impl Default for SubjectMetrics {
    fn default() -> Self {
        Self {
            sessions_count: 0,
            total_hours: 0.0,
            active_days: 0,
            avg_viewers: 0.0,
            median_viewers: 0.0,
            peak_viewers: 0,
            sample_count: 0,
            last_seen: None,
            dominant_language: None,
            de_share: 0.0,
            recent_titles: Vec::new(),
        }
    }
}

#[derive(Serialize)]
struct Baseline {
    partner_count: usize,
    avg_viewers: Distribution,
    total_hours: Distribution,
    active_days: Distribution,
}

#[derive(Serialize)]
struct Distribution {
    median: f64,
    p25: f64,
    p75: f64,
}

#[derive(Serialize)]
struct Score {
    total: i32,
    components: ScoreComponents,
    tier: Tier,
}

#[derive(Serialize)]
struct ScoreComponents {
    viewers: ScoreComponent,
    hours: ScoreComponent,
    consistency: ScoreComponent,
}

#[derive(Serialize)]
struct ScoreComponent {
    value: f64,
    percentile: i32,
    weight: f64,
}

#[derive(Serialize)]
struct Tier {
    key: &'static str,
    label: &'static str,
}

type SubjectTick = (DateTime<Utc>, i32, Option<String>, Option<String>);
type PartnerAggregate = (String, f64, f64, i64);

#[derive(sqlx::FromRow)]
struct SuggestionAggregate {
    login: String,
    sessions_count: i64,
    total_hours: f64,
    active_days: i64,
    avg_viewers: f64,
    median_viewers: f64,
    peak_viewers: i32,
    sample_count: i64,
    last_seen: DateTime<Utc>,
    dominant_language: Option<String>,
    de_share: f64,
}

const PARTNER_BASELINE_SQL: &str = r#"WITH partner_ticks AS (
       SELECT LOWER(streamer) AS streamer, ts_utc, viewer_count,
              LAG(ts_utc) OVER (PARTITION BY LOWER(streamer) ORDER BY ts_utc) AS previous_ts
       FROM twitch_stats_category
       WHERE ts_utc >= $1 AND is_partner = TRUE
   )
   SELECT streamer,
          AVG(viewer_count)::float8 AS avg_viewers,
          (SUM(CASE
              WHEN previous_ts IS NULL OR ts_utc - previous_ts > INTERVAL '30 minutes' THEN 0
              ELSE LEAST(EXTRACT(EPOCH FROM (ts_utc - previous_ts)), 1800)
          END)::float8 / 3600.0) AS total_hours,
          COUNT(DISTINCT (ts_utc AT TIME ZONE 'UTC')::date)::bigint AS active_days
   FROM partner_ticks
   GROUP BY streamer
   ORDER BY streamer"#;

const SUGGESTIONS_SQL: &str = r#"WITH candidate_ticks AS (
       SELECT LOWER(s.streamer) AS login, s.ts_utc, s.viewer_count, s.language,
              LAG(s.ts_utc) OVER (
                  PARTITION BY LOWER(s.streamer) ORDER BY s.ts_utc
              ) AS previous_ts
       FROM twitch_stats_category s
       WHERE s.ts_utc >= $1
         AND s.is_partner = FALSE
         AND NOT EXISTS (
             SELECT 1 FROM twitch_partners p
             WHERE LOWER(p.twitch_login) = LOWER(s.streamer)
         )
   )
   SELECT login,
          COUNT(*) FILTER (
              WHERE previous_ts IS NULL OR ts_utc - previous_ts > INTERVAL '30 minutes'
          )::bigint AS sessions_count,
          (SUM(CASE
              WHEN previous_ts IS NULL OR ts_utc - previous_ts > INTERVAL '30 minutes' THEN 0
              ELSE LEAST(EXTRACT(EPOCH FROM (ts_utc - previous_ts)), 1800)
          END)::float8 / 3600.0) AS total_hours,
          COUNT(DISTINCT (ts_utc AT TIME ZONE 'UTC')::date)::bigint AS active_days,
          AVG(viewer_count)::float8 AS avg_viewers,
          PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY viewer_count)::float8 AS median_viewers,
          MAX(viewer_count)::int4 AS peak_viewers,
          COUNT(*)::bigint AS sample_count,
          MAX(ts_utc) AS last_seen,
          MODE() WITHIN GROUP (ORDER BY NULLIF(LOWER(TRIM(language)), '')) AS dominant_language,
          AVG(CASE WHEN LOWER(TRIM(COALESCE(language, ''))) = 'de' THEN 1.0 ELSE 0.0 END)::float8 AS de_share
   FROM candidate_ticks
   GROUP BY login"#;

fn valid_login(login: &str) -> bool {
    (1..=25).contains(&login.len())
        && login
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn quantile(sorted: &[f64], percentile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let position = (sorted.len() - 1) as f64 * percentile;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    sorted[lower] + (sorted[upper] - sorted[lower]) * position.fract()
}

fn distribution(mut values: Vec<f64>) -> Distribution {
    values.sort_by(f64::total_cmp);
    Distribution {
        median: quantile(&values, 0.5),
        p25: quantile(&values, 0.25),
        p75: quantile(&values, 0.75),
    }
}

fn baseline_from(partners: &[PartnerAggregate]) -> Baseline {
    Baseline {
        partner_count: partners.len(),
        avg_viewers: distribution(partners.iter().map(|row| row.1).collect()),
        total_hours: distribution(partners.iter().map(|row| row.2).collect()),
        active_days: distribution(partners.iter().map(|row| row.3 as f64).collect()),
    }
}

fn parse_days(params: &ResearchQuery) -> Result<i64, Box<Response>> {
    let days = parse_bounded_query_int(params.days.as_deref(), "days", 30, 7, 365)
        .map_err(|error| Box::new(error.into_response()))?;
    if params
        .days
        .as_deref()
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .and_then(|raw| raw.parse::<i64>().ok())
        .is_some_and(|raw| !(7..=365).contains(&raw))
    {
        return Err(Box::new(
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "days must be between 7 and 365"})),
            )
                .into_response(),
        ));
    }
    Ok(days)
}

fn aggregate_subject(ticks: &[SubjectTick]) -> SubjectMetrics {
    if ticks.is_empty() {
        return SubjectMetrics::default();
    }

    let mut sessions_count = 1;
    let mut total_seconds = 0i64;
    let mut active_days = HashSet::new();
    let mut viewers = Vec::with_capacity(ticks.len());
    let mut languages = BTreeMap::<String, usize>::new();
    let mut de_ticks = 0usize;

    for (index, (timestamp, viewer_count, _, language)) in ticks.iter().enumerate() {
        if let Some((previous, _, _, _)) = index.checked_sub(1).and_then(|i| ticks.get(i)) {
            let gap = timestamp.signed_duration_since(*previous).num_seconds();
            if gap > SESSION_GAP_SECONDS {
                sessions_count += 1;
            } else {
                total_seconds += gap.clamp(0, SESSION_GAP_SECONDS);
            }
        }
        active_days.insert(timestamp.date_naive());
        viewers.push(f64::from(*viewer_count));
        if let Some(language) = language.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let language = language.to_lowercase();
            if language == "de" {
                de_ticks += 1;
            }
            *languages.entry(language).or_default() += 1;
        }
    }

    let mut sorted_viewers = viewers.clone();
    sorted_viewers.sort_by(f64::total_cmp);
    let peak_viewers = ticks
        .iter()
        .map(|(_, viewers, _, _)| *viewers)
        .max()
        .unwrap_or_default();
    let dominant_language = languages
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(language, _)| language);
    let mut seen_titles = HashSet::new();
    let recent_titles = ticks
        .iter()
        .rev()
        .filter_map(|(_, _, title, _)| title.as_deref().map(str::trim))
        .filter(|title| !title.is_empty() && seen_titles.insert((*title).to_string()))
        .take(3)
        .map(str::to_string)
        .collect();

    SubjectMetrics {
        sessions_count,
        total_hours: total_seconds as f64 / 3600.0,
        active_days: active_days.len(),
        avg_viewers: viewers.iter().sum::<f64>() / viewers.len() as f64,
        median_viewers: quantile(&sorted_viewers, 0.5),
        peak_viewers,
        sample_count: ticks.len(),
        last_seen: ticks.last().map(|(timestamp, _, _, _)| *timestamp),
        dominant_language,
        de_share: de_ticks as f64 / ticks.len() as f64,
        recent_titles,
    }
}

fn build_score(subject: &SubjectMetrics, partners: &[PartnerAggregate]) -> Score {
    let (tier_key, tier_label) = get_tier(subject.avg_viewers);
    if subject.sample_count == 0 {
        return Score {
            total: 0,
            components: ScoreComponents {
                viewers: ScoreComponent {
                    value: 0.0,
                    percentile: 0,
                    weight: 0.5,
                },
                hours: ScoreComponent {
                    value: 0.0,
                    percentile: 0,
                    weight: 0.3,
                },
                consistency: ScoreComponent {
                    value: 0.0,
                    percentile: 0,
                    weight: 0.2,
                },
            },
            tier: Tier {
                key: tier_key,
                label: tier_label,
            },
        };
    }

    let mut viewer_values: Vec<f64> = partners.iter().map(|row| row.1).collect();
    let mut hour_values: Vec<f64> = partners.iter().map(|row| row.2).collect();
    let mut day_values: Vec<f64> = partners.iter().map(|row| row.3 as f64).collect();
    viewer_values.sort_by(f64::total_cmp);
    hour_values.sort_by(f64::total_cmp);
    day_values.sort_by(f64::total_cmp);
    // Ohne Vergleichsgruppe liefert percentile_of 50 („Mittelmaß") — bei
    // leerer Partner-Baseline irreführend, daher Percentile 0.
    let (viewers_pct, hours_pct, days_pct) = if partners.is_empty() {
        (0, 0, 0)
    } else {
        (
            percentile_of(&viewer_values, subject.avg_viewers),
            percentile_of(&hour_values, subject.total_hours),
            percentile_of(&day_values, subject.active_days as f64),
        )
    };

    Score {
        total: (0.5 * f64::from(viewers_pct)
            + 0.3 * f64::from(hours_pct)
            + 0.2 * f64::from(days_pct))
        .round() as i32,
        components: ScoreComponents {
            viewers: ScoreComponent {
                value: subject.avg_viewers,
                percentile: viewers_pct,
                weight: 0.5,
            },
            hours: ScoreComponent {
                value: subject.total_hours,
                percentile: hours_pct,
                weight: 0.3,
            },
            consistency: ScoreComponent {
                value: subject.active_days as f64,
                percentile: days_pct,
                weight: 0.2,
            },
        },
        tier: Tier {
            key: tier_key,
            label: tier_label,
        },
    }
}

fn internal_error(error: sqlx::Error) -> Response {
    tracing::error!(%error, "Admin-Research-Abfrage fehlgeschlagen");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": "internal_error"})),
    )
        .into_response()
}

/// `GET /twitch/api/admin/research/:login?days=30`
pub async fn handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(raw_login): Path<String>,
    Query(params): Query<ResearchQuery>,
) -> Response {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return err.into_response();
    }

    let days = match parse_days(&params) {
        Ok(days) => days,
        Err(response) => return *response,
    };

    let login = raw_login.trim().to_lowercase();
    if !valid_login(&login) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid login"})),
        )
            .into_response();
    }

    let since = Utc::now() - Duration::days(days);
    let subject_query = sqlx::query_as::<_, SubjectTick>(
        r#"SELECT ts_utc, viewer_count, stream_title, language
           FROM twitch_stats_category
           WHERE ts_utc >= $1 AND LOWER(streamer) = $2
           ORDER BY ts_utc"#,
    )
    .bind(since)
    .bind(&login)
    .fetch_all(&pool);
    // `is_partner` setzt der Rust-Poller (tb-monitoring poller/engine.rs,
    // `sample_of`) per Abgleich gegen das Partner-Login-Set aus der DB — NICHT
    // aus dem Helix-Stream-Objekt. Der Python-Poller
    // (bot/monitoring/monitoring.py) ist seit dem Twitch-Cutover außer
    // Betrieb; sein `stream.get("is_partner")` ist tote Altlast. Live-Check
    // 2026-07-11: 26 Partner-Streamer mit is_partner-Ticks in 30 Tagen.
    let baseline_query = sqlx::query_as::<_, PartnerAggregate>(PARTNER_BASELINE_SQL)
        .bind(since)
        .fetch_all(&pool);
    let partner_query = sqlx::query_as::<_, (Option<String>,)>(
        "SELECT status FROM twitch_partners WHERE LOWER(twitch_login) = $1 LIMIT 1",
    )
    .bind(&login)
    .fetch_optional(&pool);

    let (ticks, partners, partner_row) =
        match tokio::try_join!(subject_query, baseline_query, partner_query) {
            Ok(result) => result,
            Err(error) => return internal_error(error),
        };
    let subject = aggregate_subject(&ticks);
    let baseline = baseline_from(&partners);
    let score = build_score(&subject, &partners);
    let is_already_partner = partner_row.is_some();
    let partner_status = partner_row.and_then(|row| row.0);

    Json(ResearchResponse {
        login,
        days,
        found: subject.sample_count > 0,
        is_already_partner,
        partner_status,
        subject,
        baseline,
        score,
    })
    .into_response()
}

/// `GET /twitch/api/admin/research/suggestions?days=30`
pub async fn suggestions_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<ResearchQuery>,
) -> Response {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return err.into_response();
    }

    let days = match parse_days(&params) {
        Ok(days) => days,
        Err(response) => return *response,
    };
    let since = Utc::now() - Duration::days(days);
    let candidates_query = sqlx::query_as::<_, SuggestionAggregate>(SUGGESTIONS_SQL)
        .bind(since)
        .fetch_all(&pool);
    let baseline_query = sqlx::query_as::<_, PartnerAggregate>(PARTNER_BASELINE_SQL)
        .bind(since)
        .fetch_all(&pool);
    let (candidates, partners) = match tokio::try_join!(candidates_query, baseline_query) {
        Ok(result) => result,
        Err(error) => return internal_error(error),
    };

    let baseline = baseline_from(&partners);
    let mut items: Vec<ResearchSuggestion> = candidates
        .into_iter()
        .map(|candidate| {
            let subject = SubjectMetrics {
                sessions_count: usize::try_from(candidate.sessions_count).unwrap_or_default(),
                total_hours: candidate.total_hours,
                active_days: usize::try_from(candidate.active_days).unwrap_or_default(),
                avg_viewers: candidate.avg_viewers,
                median_viewers: candidate.median_viewers,
                peak_viewers: candidate.peak_viewers,
                sample_count: usize::try_from(candidate.sample_count).unwrap_or_default(),
                last_seen: Some(candidate.last_seen),
                dominant_language: candidate.dominant_language,
                de_share: candidate.de_share,
                recent_titles: Vec::new(),
            };
            let score = build_score(&subject, &partners);
            ResearchSuggestion {
                login: candidate.login,
                subject,
                score,
            }
        })
        .collect();
    items.sort_by(|left, right| {
        right
            .score
            .total
            .cmp(&left.score.total)
            .then_with(|| {
                right
                    .subject
                    .avg_viewers
                    .total_cmp(&left.subject.avg_viewers)
            })
            .then_with(|| left.login.cmp(&right.login))
    });
    items.truncate(12);

    Json(ResearchSuggestionsResponse {
        days,
        baseline,
        items,
    })
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use serde_json::Value;
    use sqlx::{postgres::PgPoolOptions, PgPool};

    async fn pool_or_skip(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .expect("connect test database");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&pool)
            .await
            .expect("drop schema");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .expect("create schema");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("set search path");
        sqlx::query(
            r#"CREATE TABLE twitch_stats_category (
                ts_utc TIMESTAMPTZ NOT NULL,
                streamer TEXT NOT NULL,
                viewer_count INTEGER NOT NULL,
                is_partner BOOLEAN NOT NULL DEFAULT FALSE,
                game_name TEXT,
                stream_title TEXT,
                tags TEXT,
                language TEXT
            )"#,
        )
        .execute(&pool)
        .await
        .expect("create twitch_stats_category");
        sqlx::query(
            r#"CREATE TABLE twitch_partners (
                twitch_login TEXT NOT NULL,
                status TEXT
            )"#,
        )
        .execute(&pool)
        .await
        .expect("create twitch_partners");
        Some(pool)
    }

    async fn request(
        auth: DashboardAuthLevel,
        pool: PgPool,
        login: &str,
        days: Option<&str>,
    ) -> (StatusCode, Value) {
        let response = handler(
            auth,
            State(pool),
            Path(login.into()),
            Query(ResearchQuery {
                days: days.map(str::to_owned),
            }),
        )
        .await;
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("body");
        (status, serde_json::from_slice(&body).expect("json"))
    }

    async fn suggestions_request(
        auth: DashboardAuthLevel,
        pool: PgPool,
        days: Option<&str>,
    ) -> (StatusCode, Value) {
        let response = suggestions_handler(
            auth,
            State(pool),
            Query(ResearchQuery {
                days: days.map(str::to_owned),
            }),
        )
        .await;
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("body");
        (status, serde_json::from_slice(&body).expect("json"))
    }

    async fn insert_baseline(pool: &PgPool) {
        sqlx::query(
            r#"INSERT INTO twitch_stats_category
                (ts_utc, streamer, viewer_count, is_partner, stream_title, language)
            VALUES
                (NOW() - INTERVAL '2 hours', 'partner_a', 20, TRUE, 'A', 'de'),
                (NOW() - INTERVAL '110 minutes', 'partner_a', 40, TRUE, 'A', 'de')"#,
        )
        .execute(pool)
        .await
        .expect("insert baseline");
    }

    #[tokio::test]
    async fn unauth_returns_auth_required_401() {
        let Some(pool) = pool_or_skip("admin_research_unauth").await else {
            return;
        };

        let (status, body) = request(DashboardAuthLevel::None, pool, "subject", None).await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            body,
            serde_json::json!({"error": "auth_required", "required": "admin"})
        );
    }

    #[tokio::test]
    async fn unknown_login_returns_found_false_with_baseline() {
        let Some(pool) = pool_or_skip("admin_research_unknown").await else {
            return;
        };
        insert_baseline(&pool).await;

        let (status, body) = request(DashboardAuthLevel::admin(), pool, "missing", None).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["login"], "missing");
        assert_eq!(body["days"], 30);
        assert_eq!(body["found"], false);
        assert_eq!(body["baseline"]["partner_count"], 1);
        assert_eq!(body["score"]["total"], 0);
    }

    #[tokio::test]
    async fn aggregates_sessions_capped_intervals_and_context() {
        let Some(pool) = pool_or_skip("admin_research_aggregate").await else {
            return;
        };
        insert_baseline(&pool).await;
        sqlx::query(
            r#"INSERT INTO twitch_stats_category
                (ts_utc, streamer, viewer_count, stream_title, language)
            VALUES
                (date_trunc('day', NOW()) - INTERVAL '1 day' + INTERVAL '10 hours',
                    'subject', 10, 'First', 'de'),
                (date_trunc('day', NOW()) - INTERVAL '1 day' + INTERVAL '10 hours 10 min',
                    'subject', 20, 'First', 'de'),
                (date_trunc('day', NOW()) - INTERVAL '1 day' + INTERVAL '10 hours 50 min',
                    'subject', 30, 'Second', 'en'),
                (date_trunc('day', NOW()) - INTERVAL '1 day' + INTERVAL '11 hours 20 min',
                    'subject', 40, 'Third', 'de')"#,
        )
        .execute(&pool)
        .await
        .expect("insert subject");

        let (status, body) = request(DashboardAuthLevel::admin(), pool, "subject", Some("7")).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["found"], true);
        assert_eq!(body["subject"]["sessions_count"], 2);
        assert!(
            (body["subject"]["total_hours"].as_f64().expect("hours") - 2.0 / 3.0).abs() < 0.001
        );
        assert_eq!(body["subject"]["active_days"], 1);
        assert_eq!(body["subject"]["avg_viewers"], 25.0);
        assert_eq!(body["subject"]["median_viewers"], 25.0);
        assert_eq!(body["subject"]["peak_viewers"], 40);
        assert_eq!(body["subject"]["sample_count"], 4);
        assert_eq!(body["subject"]["dominant_language"], "de");
        assert_eq!(body["subject"]["de_share"], 0.75);
        assert_eq!(
            body["subject"]["recent_titles"],
            serde_json::json!(["Third", "Second", "First"])
        );
        assert!(body["subject"]["last_seen"].is_string());
    }

    #[tokio::test]
    async fn higher_average_viewers_has_higher_viewer_percentile() {
        let Some(pool) = pool_or_skip("admin_research_score").await else {
            return;
        };
        sqlx::query(
            r#"INSERT INTO twitch_stats_category
                (ts_utc, streamer, viewer_count, is_partner)
            VALUES
                (NOW() - INTERVAL '1 hour', 'p1', 10, TRUE),
                (NOW() - INTERVAL '1 hour', 'p2', 20, TRUE),
                (NOW() - INTERVAL '1 hour', 'p3', 30, TRUE),
                (NOW() - INTERVAL '1 hour', 'low',  5, FALSE),
                (NOW() - INTERVAL '1 hour', 'high', 50, FALSE)"#,
        )
        .execute(&pool)
        .await
        .expect("insert score rows");

        let (_, low) = request(DashboardAuthLevel::admin(), pool.clone(), "low", None).await;
        let (_, high) = request(DashboardAuthLevel::admin(), pool, "high", None).await;

        assert!(
            high["score"]["components"]["viewers"]["percentile"].as_i64()
                > low["score"]["components"]["viewers"]["percentile"].as_i64()
        );
    }

    #[tokio::test]
    async fn suggestions_rank_non_partners_and_exclude_existing_partners() {
        let Some(pool) = pool_or_skip("admin_research_suggestions").await else {
            return;
        };
        insert_baseline(&pool).await;
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, status) VALUES ('known', 'active')",
        )
        .execute(&pool)
        .await
        .expect("insert existing partner");
        sqlx::query(
            r#"INSERT INTO twitch_stats_category
                (ts_utc, streamer, viewer_count, stream_title, language)
            VALUES
                (NOW() - INTERVAL '2 hours', 'low',   5, 'Low',   'de'),
                (NOW() - INTERVAL '1 hour',  'low',  10, 'Low',   'de'),
                (NOW() - INTERVAL '2 hours', 'high', 80, 'High',  'de'),
                (NOW() - INTERVAL '1 hour',  'high', 90, 'High',  'de'),
                (NOW() - INTERVAL '1 hour',  'known', 999, 'Known', 'de')"#,
        )
        .execute(&pool)
        .await
        .expect("insert candidates");

        let (status, body) =
            suggestions_request(DashboardAuthLevel::admin(), pool, Some("30")).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["days"], 30);
        assert_eq!(body["items"].as_array().expect("items").len(), 2);
        assert_eq!(body["items"][0]["login"], "high");
        assert_eq!(body["items"][1]["login"], "low");
        assert!(
            body["items"][0]["score"]["total"].as_i64()
                > body["items"][1]["score"]["total"].as_i64()
        );
        assert!(body["items"]
            .as_array()
            .expect("items")
            .iter()
            .all(|item| item["login"] != "known"));
    }

    #[tokio::test]
    async fn empty_partner_baseline_scores_zero_not_fifty() {
        let Some(pool) = pool_or_skip("admin_research_empty_baseline").await else {
            return;
        };
        sqlx::query(
            r#"INSERT INTO twitch_stats_category (ts_utc, streamer, viewer_count)
            VALUES (NOW() - INTERVAL '1 hour', 'solo', 40)"#,
        )
        .execute(&pool)
        .await
        .expect("insert solo subject");

        let (status, body) = request(DashboardAuthLevel::admin(), pool, "solo", None).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["found"], true);
        assert_eq!(body["baseline"]["partner_count"], 0);
        assert_eq!(body["score"]["total"], 0);
        assert_eq!(body["score"]["components"]["viewers"]["percentile"], 0);
    }

    #[tokio::test]
    async fn existing_partner_is_flagged_with_status() {
        let Some(pool) = pool_or_skip("admin_research_partner").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, status) VALUES ('known', 'archived')",
        )
        .execute(&pool)
        .await
        .expect("insert partner");

        let (status, body) = request(DashboardAuthLevel::admin(), pool, "known", None).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["is_already_partner"], true);
        assert_eq!(body["partner_status"], "archived");
    }

    #[tokio::test]
    async fn invalid_days_return_json_400() {
        let Some(pool) = pool_or_skip("admin_research_days").await else {
            return;
        };

        let (invalid_status, invalid_body) = request(
            DashboardAuthLevel::admin(),
            pool.clone(),
            "test",
            Some("abc"),
        )
        .await;
        let (range_status, range_body) =
            request(DashboardAuthLevel::admin(), pool, "test", Some("500")).await;

        assert_eq!(invalid_status, StatusCode::BAD_REQUEST);
        assert_eq!(
            invalid_body,
            serde_json::json!({"error": "days must be an integer"})
        );
        assert_eq!(range_status, StatusCode::BAD_REQUEST);
        assert_eq!(
            range_body,
            serde_json::json!({"error": "days must be between 7 and 365"})
        );
    }

    #[test]
    fn parse_days_akzeptiert_365_und_lehnt_366_ab() {
        let ok = parse_days(&ResearchQuery {
            days: Some("365".into()),
        });
        assert_eq!(ok.ok(), Some(365));

        let zu_gross = parse_days(&ResearchQuery {
            days: Some("366".into()),
        });
        assert!(zu_gross.is_err());
    }

    #[tokio::test]
    async fn login_is_normalized_and_invalid_characters_are_rejected() {
        let Some(pool) = pool_or_skip("admin_research_login").await else {
            return;
        };

        let (normalized_status, normalized) = request(
            DashboardAuthLevel::admin(),
            pool.clone(),
            "UPPER_Case ",
            None,
        )
        .await;
        let (invalid_status, invalid) =
            request(DashboardAuthLevel::admin(), pool, "bad-name", None).await;

        assert_eq!(normalized_status, StatusCode::OK);
        assert_eq!(normalized["login"], "upper_case");
        assert_eq!(invalid_status, StatusCode::BAD_REQUEST);
        assert_eq!(invalid, serde_json::json!({"error": "invalid login"}));
    }
}
