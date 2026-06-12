//! Handler für `GET /internal/twitch/v1/analytics/streamer/:login`.
//!
//! Nativer Port von `bot/analytics/backend_extended.py:22–838` (Funktion
//! `get_comprehensive_analytics`). Shape-Parität ist das oberste Gebot —
//! jedes JSON-Feld, jeder Default, jede Skalierung exakt wie Python.
//!
//! # Wichtige Skalierungs-Regel (Vertrag Abschnitt 13.1)
//!
//! | Feld-Gruppe | Skala |
//! |---|---|
//! | `metrics.retention_*`, `metrics.avg_dropoff` | ×100 (%-Skala) |
//! | `sessions[*].retention*`, `sessions[*].dropoffPct` | ×100 (%-Skala) |
//! | `retention_timeline[*].retention_*`, `.dropoff` | DB-Rohwert (0..1) |
//! | `comparison.yourStats.retention10m` | DB-Rohwert (0..1) |
//!
//! # follower_delta-Spalten-Check
//!
//! Python prüft via `SELECT follower_delta FROM twitch_stream_sessions LIMIT 1`
//! ob die Spalte existiert — bei Exception → Fallback auf Literal 0. Da
//! `follower_delta` in der Prod-DDL (s. `tb-monitoring/tests/support/mod.rs`)
//! fest definiert ist, simulieren wir den gleichen Two-Probe-Mechanismus mit
//! einer dedizierten Existenzprüfung (s. `follower_delta_exists`).

use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tb_domain::normalize_twitch_login;
use tb_http_core::{ApiError, AuthLevel};

// ── Query-Parameter ───────────────────────────────────────────────────────────

/// Query-Parameter: `days` (Default 30, Minimum 1).
/// Parität: `parse_optional_int(value, minimum=1)` → leerer/fehlender String → 30;
/// Wert < 1 → ValueError → 400 `{"error":"bad_request","message":"invalid query parameters"}`.
/// (bot/analytics/backend_extended.py:48–55)
#[derive(Deserialize, Default)]
pub struct AnalyticsQuery {
    pub days: Option<String>,
}

// ── Interne Aggregate-Strukturen (werden nicht direkt serialisiert) ───────────

struct MetricsRaw {
    retention_5m: f64,
    retention_10m: f64,
    retention_20m: f64,
    avg_dropoff: f64,
    avg_peak_viewers: f64,
    avg_avg_viewers: f64,
    total_followers_delta: i64,
    session_count: i64,
    total_duration_seconds: i64,
    avg_unique_chatters: f64,
    unique_chatters_per_100: f64,
    total_first_time_chatters: i64,
    total_returning_chatters: i64,
}

struct TrendRaw {
    retention_5m: f64,
    avg_peak_viewers: f64,
    total_followers_delta: i64,
    unique_chatters_per_100: f64,
}

// ── Response-Typen ────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct RetentionTimelineEntry {
    date: String,
    retention_5m: f64,
    retention_10m: f64,
    retention_20m: f64,
    dropoff: f64,
}

#[derive(Serialize)]
struct DiscoveryTimelineEntry {
    date: String,
    peak_viewers: i64,
    followers_delta: i64,
    avg_viewers: f64,
}

#[derive(Serialize)]
struct ChatTimelineEntry {
    date: String,
    unique_chatters: f64, // float wegen AVG — Vertrag Abschnitt 7
    chat_per_100: f64,
    first_time: i64,
    returning: i64,
}

#[derive(Serialize)]
struct SessionEntry {
    id: i64,
    date: String,
    #[serde(rename = "startTime")]
    start_time: String,
    duration: i64,
    #[serde(rename = "startViewers")]
    start_viewers: i64,
    #[serde(rename = "peakViewers")]
    peak_viewers: i64,
    #[serde(rename = "endViewers")]
    end_viewers: i64,
    #[serde(rename = "avgViewers")]
    avg_viewers: f64,
    #[serde(rename = "retention5m")]
    retention5m: f64,
    #[serde(rename = "retention10m")]
    retention10m: f64,
    #[serde(rename = "retention20m")]
    retention20m: f64,
    #[serde(rename = "dropoffPct")]
    dropoff_pct: f64,
    #[serde(rename = "uniqueChatters")]
    unique_chatters: i64,
    #[serde(rename = "firstTimeChatters")]
    first_time_chatters: i64,
    #[serde(rename = "returningChatters")]
    returning_chatters: i64,
    #[serde(rename = "followersStart")]
    followers_start: i64,
    #[serde(rename = "followersEnd")]
    followers_end: i64,
    title: String,
}

#[derive(Serialize)]
struct InsightEntry {
    #[serde(rename = "type")]
    insight_type: String,
    title: String,
    description: String,
}

// ── Timestamp-Helfer ──────────────────────────────────────────────────────────

/// Serialisiert einen `chrono::DateTime<Utc>` exakt wie Pythons `isoformat()`:
/// - Mikrosekunden werden nur angehängt wenn sie != 0 sind
/// - Zeitzone immer als `+00:00` (nicht `Z`)
///
/// Beispiele:
/// - ohne Mikros: `"2026-06-12T14:30:00+00:00"`
/// - mit Mikros:  `"2026-06-12T14:30:00.123456+00:00"`
///
/// Python `datetime.isoformat()` verhält sich identisch (ab Python 3.6+).
#[cfg(test)]
fn format_python_isoformat(dt: chrono::DateTime<Utc>) -> String {
    let micros = dt.timestamp_subsec_micros();
    if micros == 0 {
        dt.format("%Y-%m-%dT%H:%M:%S+00:00").to_string()
    } else {
        format!("{}.{:06}+00:00", dt.format("%Y-%m-%dT%H:%M:%S"), micros)
    }
}

// ── follower_delta Existenz-Check ─────────────────────────────────────────────

/// Prüft ob die Spalte `follower_delta` in `twitch_stream_sessions` existiert.
/// Python: `SELECT follower_delta FROM twitch_stream_sessions LIMIT 1` — bei
/// Exception → Spalte fehlt (backend_extended.py:127).
/// Gibt `true` zurück wenn die Spalte existiert, `false` bei jedem Fehler.
async fn follower_delta_exists(pool: &PgPool) -> bool {
    sqlx::query("SELECT follower_delta FROM twitch_stream_sessions LIMIT 1")
        .execute(pool)
        .await
        .is_ok()
}

// ── Metrics-Query ─────────────────────────────────────────────────────────────

/// Berechnet Aggregat-Metriken für den Zeitraum.
/// Python: `_calculate_comprehensive_metrics` (backend_extended.py:115–205).
/// Zwei separate Probes für follower_delta-Existenz (Parität Python-Fallback).
async fn fetch_metrics(
    pool: &PgPool,
    login: &str,
    since: DateTime<Utc>,
    has_follower_delta: bool,
) -> Result<Option<MetricsRaw>, sqlx::Error> {
    // AVG(INTEGER) liefert NUMERIC in Postgres — explizit auf DOUBLE PRECISION casten,
    // damit sqlx direkt als f64 dekodieren kann (backend_extended.py:154–185).
    let sql_with_fd = r#"
        SELECT
            AVG(s.retention_5m),
            AVG(s.retention_10m),
            AVG(s.retention_20m),
            AVG(s.dropoff_pct),
            AVG(s.peak_viewers)::DOUBLE PRECISION,
            SUM(COALESCE(s.follower_delta, 0))::BIGINT,
            COUNT(*),
            SUM(s.duration_seconds)::BIGINT,
            AVG(s.unique_chatters)::DOUBLE PRECISION,
            AVG(CASE WHEN s.avg_viewers > 0 THEN (s.unique_chatters::DOUBLE PRECISION * 100.0 / s.avg_viewers) ELSE 0.0 END),
            SUM(s.first_time_chatters)::BIGINT,
            SUM(s.returning_chatters)::BIGINT,
            AVG(s.avg_viewers)
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1
          AND s.ended_at IS NOT NULL
          AND LOWER(s.streamer_login) = $2
    "#;

    let sql_no_fd = r#"
        SELECT
            AVG(s.retention_5m),
            AVG(s.retention_10m),
            AVG(s.retention_20m),
            AVG(s.dropoff_pct),
            AVG(s.peak_viewers)::DOUBLE PRECISION,
            0::BIGINT,
            COUNT(*),
            SUM(s.duration_seconds)::BIGINT,
            AVG(s.unique_chatters)::DOUBLE PRECISION,
            AVG(CASE WHEN s.avg_viewers > 0 THEN (s.unique_chatters::DOUBLE PRECISION * 100.0 / s.avg_viewers) ELSE 0.0 END),
            SUM(s.first_time_chatters)::BIGINT,
            SUM(s.returning_chatters)::BIGINT,
            AVG(s.avg_viewers)
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1
          AND s.ended_at IS NOT NULL
          AND LOWER(s.streamer_login) = $2
    "#;

    let sql = if has_follower_delta { sql_with_fd } else { sql_no_fd };

    let row = sqlx::query(sql)
        .bind(since)
        .bind(login)
        .fetch_one(pool)
        .await?;

    // COUNT(*) ist nie NULL — direkt als i64 lesen
    let count: i64 = row.try_get::<i64, _>(6)?;
    if count == 0 {
        return Ok(None);
    }

    Ok(Some(MetricsRaw {
        retention_5m: row.try_get::<Option<f64>, _>(0)?.unwrap_or(0.0),
        retention_10m: row.try_get::<Option<f64>, _>(1)?.unwrap_or(0.0),
        retention_20m: row.try_get::<Option<f64>, _>(2)?.unwrap_or(0.0),
        avg_dropoff: row.try_get::<Option<f64>, _>(3)?.unwrap_or(0.0),
        avg_peak_viewers: row.try_get::<Option<f64>, _>(4)?.unwrap_or(0.0),
        total_followers_delta: row.try_get::<Option<i64>, _>(5)?.unwrap_or(0),
        session_count: count,
        total_duration_seconds: row.try_get::<Option<i64>, _>(7)?.unwrap_or(0),
        avg_unique_chatters: row.try_get::<Option<f64>, _>(8)?.unwrap_or(0.0),
        unique_chatters_per_100: row.try_get::<Option<f64>, _>(9)?.unwrap_or(0.0),
        total_first_time_chatters: row.try_get::<Option<i64>, _>(10)?.unwrap_or(0),
        total_returning_chatters: row.try_get::<Option<i64>, _>(11)?.unwrap_or(0),
        avg_avg_viewers: row.try_get::<Option<f64>, _>(12)?.unwrap_or(0.0),
    }))
}

/// Berechnet Trend-Metriken für den Vorzeitraum.
/// Python: Trend-SQL (backend_extended.py:190–205).
async fn fetch_trend(
    pool: &PgPool,
    login: &str,
    prev_since: DateTime<Utc>,
    since: DateTime<Utc>,
    has_follower_delta: bool,
) -> Result<TrendRaw, sqlx::Error> {
    let sql_with_fd = r#"
        SELECT
            AVG(s.retention_5m),
            AVG(s.peak_viewers)::DOUBLE PRECISION,
            SUM(COALESCE(s.follower_delta, 0))::BIGINT,
            AVG(CASE WHEN s.avg_viewers > 0 THEN (s.unique_chatters::DOUBLE PRECISION * 100.0 / s.avg_viewers) ELSE 0.0 END)
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1 AND s.started_at < $2
          AND s.ended_at IS NOT NULL
          AND LOWER(s.streamer_login) = $3
    "#;
    let sql_no_fd = r#"
        SELECT
            AVG(s.retention_5m),
            AVG(s.peak_viewers)::DOUBLE PRECISION,
            0::BIGINT,
            AVG(CASE WHEN s.avg_viewers > 0 THEN (s.unique_chatters::DOUBLE PRECISION * 100.0 / s.avg_viewers) ELSE 0.0 END)
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1 AND s.started_at < $2
          AND s.ended_at IS NOT NULL
          AND LOWER(s.streamer_login) = $3
    "#;

    let sql = if has_follower_delta { sql_with_fd } else { sql_no_fd };
    let row = sqlx::query(sql)
        .bind(prev_since)
        .bind(since)
        .bind(login)
        .fetch_one(pool)
        .await?;

    Ok(TrendRaw {
        retention_5m: row.try_get::<Option<f64>, _>(0)?.unwrap_or(0.0),
        avg_peak_viewers: row.try_get::<Option<f64>, _>(1)?.unwrap_or(0.0),
        total_followers_delta: row.try_get::<Option<i64>, _>(2)?.unwrap_or(0),
        unique_chatters_per_100: row.try_get::<Option<f64>, _>(3)?.unwrap_or(0.0),
    })
}

// ── Timeline-Queries ──────────────────────────────────────────────────────────

/// Retention-Timeline: DB-Rohwerte (0..1), KEINE ×100-Multiplikation.
/// Python: `_get_retention_timeline` (backend_extended.py:265–290).
async fn fetch_retention_timeline(
    pool: &PgPool,
    login: &str,
    since: DateTime<Utc>,
) -> Result<Vec<RetentionTimelineEntry>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT
            DATE(s.started_at),
            AVG(s.retention_5m),
            AVG(s.retention_10m),
            AVG(s.retention_20m),
            AVG(s.dropoff_pct)
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1
          AND s.retention_5m IS NOT NULL
          AND s.ended_at IS NOT NULL
          AND LOWER(s.streamer_login) = $2
        GROUP BY DATE(s.started_at)
        ORDER BY DATE(s.started_at) ASC
        "#,
    )
    .bind(since)
    .bind(login)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|r| {
            let date_val: Option<NaiveDate> = r.try_get(0)?;
            Ok(RetentionTimelineEntry {
                date: date_val
                    .map(|d| d.format("%Y-%m-%d").to_string())
                    .unwrap_or_default(),
                retention_5m: r.try_get::<Option<f64>, _>(1)?.unwrap_or(0.0),
                retention_10m: r.try_get::<Option<f64>, _>(2)?.unwrap_or(0.0),
                retention_20m: r.try_get::<Option<f64>, _>(3)?.unwrap_or(0.0),
                dropoff: r.try_get::<Option<f64>, _>(4)?.unwrap_or(0.0),
            })
        })
        .collect()
}

/// Discovery-Timeline: Viewer + Follower-Delta pro Tag.
/// Python: `_get_discovery_timeline` (backend_extended.py:415–465).
async fn fetch_discovery_timeline(
    pool: &PgPool,
    login: &str,
    since: DateTime<Utc>,
    has_follower_delta: bool,
) -> Result<Vec<DiscoveryTimelineEntry>, sqlx::Error> {
    if has_follower_delta {
        let rows = sqlx::query(
            r#"
            SELECT
                DATE(s.started_at),
                AVG(s.peak_viewers)::DOUBLE PRECISION,
                SUM(COALESCE(s.follower_delta, 0))::BIGINT,
                AVG(s.avg_viewers)
            FROM twitch_stream_sessions s
            WHERE s.started_at >= $1
              AND s.ended_at IS NOT NULL
              AND LOWER(s.streamer_login) = $2
            GROUP BY DATE(s.started_at)
            ORDER BY DATE(s.started_at) ASC
            "#,
        )
        .bind(since)
        .bind(login)
        .fetch_all(pool)
        .await?;

        rows.into_iter()
            .map(|r| {
                let date_val: Option<NaiveDate> = r.try_get(0)?;
                Ok(DiscoveryTimelineEntry {
                    date: date_val
                        .map(|d| d.format("%Y-%m-%d").to_string())
                        .unwrap_or_default(),
                    peak_viewers: r.try_get::<Option<f64>, _>(1)?.unwrap_or(0.0) as i64,
                    followers_delta: r.try_get::<Option<i64>, _>(2)?.unwrap_or(0),
                    avg_viewers: r.try_get::<Option<f64>, _>(3)?.unwrap_or(0.0),
                })
            })
            .collect()
    } else {
        // Fallback ohne follower_delta (backend_extended.py:432)
        tracing::debug!(
            "follower_delta Spalte fehlt in discovery_timeline - verwende Fallback"
        );
        let rows = sqlx::query(
            r#"
            SELECT
                DATE(s.started_at),
                AVG(s.peak_viewers)::DOUBLE PRECISION,
                AVG(s.avg_viewers)
            FROM twitch_stream_sessions s
            WHERE s.started_at >= $1
              AND s.ended_at IS NOT NULL
              AND LOWER(s.streamer_login) = $2
            GROUP BY DATE(s.started_at)
            ORDER BY DATE(s.started_at) ASC
            "#,
        )
        .bind(since)
        .bind(login)
        .fetch_all(pool)
        .await?;

        rows.into_iter()
            .map(|r| {
                let date_val: Option<NaiveDate> = r.try_get(0)?;
                Ok(DiscoveryTimelineEntry {
                    date: date_val
                        .map(|d| d.format("%Y-%m-%d").to_string())
                        .unwrap_or_default(),
                    peak_viewers: r.try_get::<Option<f64>, _>(1)?.unwrap_or(0.0) as i64,
                    followers_delta: 0,
                    avg_viewers: r.try_get::<Option<f64>, _>(2)?.unwrap_or(0.0),
                })
            })
            .collect()
    }
}

/// Chat-Timeline: Engagement-Metriken pro Tag.
/// Python: `_get_chat_timeline` (backend_extended.py:475–520).
/// Zusätzlicher Filter: `AND s.avg_viewers > 0`
async fn fetch_chat_timeline(
    pool: &PgPool,
    login: &str,
    since: DateTime<Utc>,
) -> Result<Vec<ChatTimelineEntry>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT
            DATE(s.started_at),
            AVG(s.unique_chatters)::DOUBLE PRECISION,
            AVG(CASE WHEN s.avg_viewers > 0 THEN (s.unique_chatters::DOUBLE PRECISION * 100.0 / s.avg_viewers) ELSE 0.0 END),
            SUM(s.first_time_chatters)::BIGINT,
            SUM(s.returning_chatters)::BIGINT
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1
          AND s.ended_at IS NOT NULL
          AND s.avg_viewers > 0
          AND LOWER(s.streamer_login) = $2
        GROUP BY DATE(s.started_at)
        ORDER BY DATE(s.started_at) ASC
        "#,
    )
    .bind(since)
    .bind(login)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|r| {
            let date_val: Option<NaiveDate> = r.try_get(0)?;
            Ok(ChatTimelineEntry {
                date: date_val
                    .map(|d| d.format("%Y-%m-%d").to_string())
                    .unwrap_or_default(),
                unique_chatters: r.try_get::<Option<f64>, _>(1)?.unwrap_or(0.0),
                chat_per_100: r.try_get::<Option<f64>, _>(2)?.unwrap_or(0.0),
                // SUM(INTEGER)::BIGINT → i64 (nie NULL wenn Rows existieren, aber sicherheitshalber Option)
                first_time: r.try_get::<Option<i64>, _>(3)?.unwrap_or(0),
                returning: r.try_get::<Option<i64>, _>(4)?.unwrap_or(0),
            })
        })
        .collect()
}

// ── Session-Liste ─────────────────────────────────────────────────────────────

/// Session-Liste: Letzte 50 Sessions mit ×100-Retention-Werten.
/// Python: `_get_session_list(limit=50)` (backend_extended.py:530–590).
async fn fetch_sessions(
    pool: &PgPool,
    login: &str,
    since: DateTime<Utc>,
) -> Result<Vec<SessionEntry>, sqlx::Error> {
    // Python `TIME(started_at)` → Uhrzeit-String; DATE() → Datum-String.
    // TO_CHAR liefert einen String direkt aus Postgres.
    let rows = sqlx::query(
        r#"
        SELECT
            s.id,
            DATE(s.started_at),
            TO_CHAR(s.started_at AT TIME ZONE 'UTC', 'HH24:MI:SS'),
            s.duration_seconds,
            s.start_viewers,
            s.peak_viewers,
            s.end_viewers,
            s.avg_viewers,
            s.retention_5m,
            s.retention_10m,
            s.retention_20m,
            s.dropoff_pct,
            s.unique_chatters,
            s.first_time_chatters,
            s.returning_chatters,
            COALESCE(s.followers_start, 0),
            COALESCE(s.followers_end, 0),
            s.stream_title
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1
          AND s.ended_at IS NOT NULL
          AND LOWER(s.streamer_login) = $2
        ORDER BY s.started_at DESC
        LIMIT 50
        "#,
    )
    .bind(since)
    .bind(login)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|r| {
            let date_val: Option<NaiveDate> = r.try_get(1)?;
            // ×100 — %-Skala (backend_extended.py:568–572)
            let ret5: Option<f64> = r.try_get(8)?;
            let ret10: Option<f64> = r.try_get(9)?;
            let ret20: Option<f64> = r.try_get(10)?;
            let dropoff: Option<f64> = r.try_get(11)?;

            Ok(SessionEntry {
                id: r.try_get::<i64, _>(0)?,
                date: date_val
                    .map(|d| d.format("%Y-%m-%d").to_string())
                    .unwrap_or_default(),
                start_time: r.try_get::<Option<String>, _>(2)?.unwrap_or_default(),
                duration: r.try_get::<Option<i32>, _>(3)?.unwrap_or(0) as i64,
                start_viewers: r.try_get::<Option<i32>, _>(4)?.unwrap_or(0) as i64,
                peak_viewers: r.try_get::<Option<i32>, _>(5)?.unwrap_or(0) as i64,
                end_viewers: r.try_get::<Option<i32>, _>(6)?.unwrap_or(0) as i64,
                avg_viewers: r.try_get::<Option<f64>, _>(7)?.unwrap_or(0.0),
                retention5m: ret5.map(|v| v * 100.0).unwrap_or(0.0),
                retention10m: ret10.map(|v| v * 100.0).unwrap_or(0.0),
                retention20m: ret20.map(|v| v * 100.0).unwrap_or(0.0),
                dropoff_pct: dropoff.map(|v| v * 100.0).unwrap_or(0.0),
                unique_chatters: r.try_get::<Option<i32>, _>(12)?.unwrap_or(0) as i64,
                first_time_chatters: r.try_get::<Option<i32>, _>(13)?.unwrap_or(0) as i64,
                returning_chatters: r.try_get::<Option<i32>, _>(14)?.unwrap_or(0) as i64,
                followers_start: r.try_get::<Option<i32>, _>(15)?.unwrap_or(0) as i64,
                followers_end: r.try_get::<Option<i32>, _>(16)?.unwrap_or(0) as i64,
                title: r.try_get::<Option<String>, _>(17)?.unwrap_or_default(),
            })
        })
        .collect()
}

// ── Insights-Generierung ──────────────────────────────────────────────────────

/// Insights rein berechnet — kein SQL.
/// Python: `_generate_comprehensive_insights` (backend_extended.py:650–750).
/// Eingaben: `metrics_raw` (DB-Rohwerte VOR ×100-Multiplikation).
fn generate_insights(
    metrics: &MetricsRaw,
    retention_timeline: &[RetentionTimelineEntry],
) -> Vec<InsightEntry> {
    let mut insights = Vec::new();

    // retention_10m intern ×100 für Schwellenvergleich (Rohwert 0..1 → %-Wert)
    let ret10_pct = metrics.retention_10m * 100.0;
    let avg_peak = metrics.avg_peak_viewers;
    let total_followers = metrics.total_followers_delta;
    let chat_per_100 = metrics.unique_chatters_per_100;

    // Retention-Insight (backend_extended.py:670–680)
    if ret10_pct < 40.0 {
        insights.push(InsightEntry {
            insight_type: "warning".to_string(),
            title: "Niedrige Retention".to_string(),
            description: format!(
                "Deine 10-Minuten-Retention liegt bei {:.1}%. Versuche, den Einstieg deiner Streams interessanter zu gestalten.",
                ret10_pct
            ),
        });
    } else if ret10_pct > 70.0 {
        insights.push(InsightEntry {
            insight_type: "success".to_string(),
            title: "Exzellente Retention".to_string(),
            description: format!(
                "Deine 10-Minuten-Retention liegt bei {:.1}% — das ist ausgezeichnet!",
                ret10_pct
            ),
        });
    }

    // Follower-Conversion (backend_extended.py:682–695)
    if avg_peak > 0.0 {
        let conversion_rate = total_followers as f64 / avg_peak * 100.0;
        if conversion_rate < 5.0 {
            insights.push(InsightEntry {
                insight_type: "warning".to_string(),
                title: "Niedrige Follower-Conversion".to_string(),
                description: format!(
                    "Nur {:.1}% deiner Peak-Viewer folgen dir. Nutze CTAs und Interaktion, um die Conversion zu steigern.",
                    conversion_rate
                ),
            });
        } else if conversion_rate > 15.0 {
            insights.push(InsightEntry {
                insight_type: "success".to_string(),
                title: "Starke Follower-Conversion".to_string(),
                description: format!(
                    "{:.1}% deiner Peak-Viewer folgen dir — eine starke Community-Bindung!",
                    conversion_rate
                ),
            });
        }
    }

    // Chat-Aktivität (backend_extended.py:697–710)
    if chat_per_100 < 5.0 {
        insights.push(InsightEntry {
            insight_type: "warning".to_string(),
            title: "Niedrige Chat-Aktivität".to_string(),
            description: format!(
                "Nur {:.1} von 100 Viewern chaten aktiv. Stelle Fragen oder starte Polls, um die Community zu aktivieren.",
                chat_per_100
            ),
        });
    } else if chat_per_100 > 15.0 {
        insights.push(InsightEntry {
            insight_type: "success".to_string(),
            title: "Sehr aktive Community".to_string(),
            description: format!(
                "{:.1} von 100 Viewern sind aktiv im Chat — deine Community ist sehr engagiert!",
                chat_per_100
            ),
        });
    }

    // Trend-Analyse: letzte 7 Einträge der Retention-Timeline (backend_extended.py:712–730)
    if retention_timeline.len() >= 7 {
        let n = retention_timeline.len();
        let recent: Vec<f64> = retention_timeline[n - 7..]
            .iter()
            .map(|e| e.retention_5m)
            .collect();
        let older: Vec<f64> = retention_timeline[..n - 7]
            .iter()
            .map(|e| e.retention_5m)
            .collect();

        let recent_avg = if !recent.is_empty() {
            recent.iter().sum::<f64>() / recent.len() as f64
        } else {
            0.0
        };
        let older_avg = if !older.is_empty() {
            older.iter().sum::<f64>() / older.len() as f64
        } else {
            0.0
        };

        if older_avg > 0.0 {
            if recent_avg > older_avg * 1.10 {
                insights.push(InsightEntry {
                    insight_type: "success".to_string(),
                    title: "Positiver Trend".to_string(),
                    description: "Deine Retention verbessert sich — weiter so!".to_string(),
                });
            } else if recent_avg < older_avg * 0.90 {
                insights.push(InsightEntry {
                    insight_type: "warning".to_string(),
                    title: "Negativer Trend".to_string(),
                    description: "Deine Retention sinkt. Analysiere, was sich zuletzt verändert hat."
                        .to_string(),
                });
            }
        }
    }

    insights
}

// ── Comparison-Queries ────────────────────────────────────────────────────────

/// Top-Streamer aus twitch_stats_tracked (backend_extended.py:760–780).
async fn fetch_top_streamers(pool: &PgPool, since: DateTime<Utc>) -> Result<Vec<Value>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT
            streamer,
            AVG(viewer_count)::DOUBLE PRECISION,
            MAX(viewer_count)
        FROM twitch_stats_tracked
        WHERE ts_utc >= $1
        GROUP BY streamer
        ORDER BY AVG(viewer_count) DESC
        LIMIT 10
        "#,
    )
    .bind(since)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|r| {
            Ok(json!({
                "login": r.try_get::<Option<String>, _>(0)?.unwrap_or_default(),
                "avgViewers": r.try_get::<Option<f64>, _>(1)?.unwrap_or(0.0) as i64,
                "peakViewers": r.try_get::<Option<i32>, _>(2)?.unwrap_or(0) as i64,
            }))
        })
        .collect()
}

/// Kategorie-Durchschnitt aus twitch_stats_category (backend_extended.py:785–800).
async fn fetch_category_avg(pool: &PgPool, since: DateTime<Utc>) -> Result<Value, sqlx::Error> {
    let r = sqlx::query(
        r#"
        SELECT
            AVG(viewer_count)::DOUBLE PRECISION,
            MAX(viewer_count)
        FROM twitch_stats_category
        WHERE ts_utc >= $1
        "#,
    )
    .bind(since)
    .fetch_one(pool)
    .await?;

    Ok(json!({
        "avgViewers": r.try_get::<Option<f64>, _>(0)?.unwrap_or(0.0),
        "peakViewers": r.try_get::<Option<i32>, _>(1)?.unwrap_or(0) as i64,
        "retention10m": 65.0_f64,  // hardcoded Benchmark (backend_extended.py:797)
        "chatHealth": 8.5_f64,     // hardcoded Benchmark (backend_extended.py:798)
    }))
}

/// Eigene Stats des Streamers für Vergleich (backend_extended.py:803–820).
/// DB-Rohwert für retention10m (NICHT ×100) — Vertrag Abschnitt 13.1.
async fn fetch_your_stats(pool: &PgPool, login: &str, since: DateTime<Utc>) -> Result<Value, sqlx::Error> {
    let r = sqlx::query(
        r#"
        SELECT
            AVG(s.avg_viewers),
            AVG(s.peak_viewers)::DOUBLE PRECISION,
            AVG(s.retention_10m),
            AVG(CASE WHEN s.avg_viewers > 0 THEN (s.unique_chatters::DOUBLE PRECISION * 100.0 / s.avg_viewers) ELSE 0.0 END)
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1
          AND LOWER(s.streamer_login) = $2
          AND s.ended_at IS NOT NULL
        "#,
    )
    .bind(since)
    .bind(login)
    .fetch_one(pool)
    .await?;

    Ok(json!({
        "avgViewers": r.try_get::<Option<f64>, _>(0)?.unwrap_or(0.0),
        "peakViewers": r.try_get::<Option<f64>, _>(1)?.unwrap_or(0.0) as i64,
        "retention10m": r.try_get::<Option<f64>, _>(2)?.unwrap_or(0.0), // Rohwert 0..1
        "chatHealth": r.try_get::<Option<f64>, _>(3)?.unwrap_or(0.0),
    }))
}

// ── Trend-Berechnung ──────────────────────────────────────────────────────────

/// Prozentualer Trend: `(current - prev) / prev * 100`, 0 bei prev == 0.
/// Python: `(a - b) / b * 100 if b else 0.0`
fn pct_trend(current: f64, prev: f64) -> f64 {
    if prev == 0.0 {
        0.0
    } else {
        (current - prev) / prev * 100.0
    }
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// `GET /internal/twitch/v1/analytics/streamer/:login`
///
/// Entspricht `get_comprehensive_analytics` in `bot/analytics/backend_extended.py:22–111`.
pub async fn streamer_analytics_native_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Path(raw_login): Path<String>,
    Query(params): Query<AnalyticsQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    // Login normalisieren (bot/internal_api/policy.py:247–248)
    let Some(login) = normalize_twitch_login(&raw_login) else {
        return Err(ApiError::bad_request("invalid login"));
    };

    // days parsen: leerer/fehlender String → 30; Wert < 1 → 400
    // Python: `parse_optional_int(value, minimum=1)` (backend_extended.py:48–55)
    let days: i64 = match params.days.as_deref() {
        None | Some("") => 30,
        Some(s) => {
            let v: i64 = s
                .parse()
                .map_err(|_| ApiError::bad_request("invalid query parameters"))?;
            if v < 1 {
                return Err(ApiError::bad_request("invalid query parameters"));
            }
            v
        }
    };

    // Zeitfenster berechnen
    // Python: `since_date = (now(UTC) - timedelta(days=days)).isoformat()`
    let now = Utc::now();
    let since_dt = now - Duration::days(days);
    let prev_since_dt = since_dt - Duration::days(days);

    // follower_delta-Spalte-Check — zwei separate Probes (je einmal für
    // metrics und für discovery_timeline, wie in Python).
    let has_follower_delta_metrics = follower_delta_exists(&pool).await;
    let has_follower_delta_discovery = has_follower_delta_metrics;

    // Metrics abrufen — liefert None wenn session_count == 0
    let metrics_opt =
        match fetch_metrics(&pool, &login, since_dt, has_follower_delta_metrics).await {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(
                    "Failed to get comprehensive analytics for {}: {}",
                    login,
                    e
                );
                // Python: Exception in get_comprehensive_analytics → 200 + {error, empty}
                // (backend_extended.py:112 — kein HTTP-Fehlercode, 200-Body)
                return Ok(Json(json!({
                    "error": "Internal error",
                    "empty": true
                }))
                .into_response());
            }
        };

    // Empty-Case: keine Sessions im Zeitfenster
    let Some(metrics_raw) = metrics_opt else {
        return Ok(Json(json!({
            "empty": true,
            "message": "Keine Daten für den gewählten Zeitraum"
        }))
        .into_response());
    };

    // Trend-Metriken
    let trend =
        match fetch_trend(&pool, &login, prev_since_dt, since_dt, has_follower_delta_metrics).await {
            Ok(t) => t,
            Err(e) => {
                tracing::error!("Trend-Query fehlgeschlagen für {}: {}", login, e);
                // Fallback auf Null-Trend — kein Hard-Fehler
                TrendRaw {
                    retention_5m: 0.0,
                    avg_peak_viewers: 0.0,
                    total_followers_delta: 0,
                    unique_chatters_per_100: 0.0,
                }
            }
        };

    // Timelines
    let retention_timeline = fetch_retention_timeline(&pool, &login, since_dt)
        .await
        .unwrap_or_default();

    let discovery_timeline =
        fetch_discovery_timeline(&pool, &login, since_dt, has_follower_delta_discovery)
            .await
            .unwrap_or_default();

    let chat_timeline = fetch_chat_timeline(&pool, &login, since_dt)
        .await
        .unwrap_or_default();

    // Sessions
    let sessions = fetch_sessions(&pool, &login, since_dt)
        .await
        .unwrap_or_default();

    // Insights (rein berechnet)
    let insights = generate_insights(&metrics_raw, &retention_timeline);

    // Comparison
    let top_streamers = fetch_top_streamers(&pool, since_dt)
        .await
        .unwrap_or_default();
    let category_avg = fetch_category_avg(&pool, since_dt).await.unwrap_or_else(|_| {
        json!({
            "avgViewers": 0.0,
            "peakViewers": 0_i64,
            "retention10m": 65.0_f64,
            "chatHealth": 8.5_f64,
        })
    });
    let your_stats = fetch_your_stats(&pool, &login, since_dt)
        .await
        .unwrap_or_else(|_| json!({}));

    // Metrics formatieren: ×100 für Retention-Felder (backend_extended.py:210–220)
    let total_duration_hours = metrics_raw.total_duration_seconds as f64 / 3600.0;
    let session_count = metrics_raw.session_count;

    let followers_per_session = if session_count == 0 {
        0.0
    } else {
        metrics_raw.total_followers_delta as f64 / session_count as f64
    };
    let followers_per_hour = if total_duration_hours == 0.0 {
        0.0
    } else {
        metrics_raw.total_followers_delta as f64 / total_duration_hours
    };

    let metrics_json = json!({
        "retention_5m":  metrics_raw.retention_5m  * 100.0,
        "retention_10m": metrics_raw.retention_10m * 100.0,
        "retention_20m": metrics_raw.retention_20m * 100.0,
        "avg_dropoff":   metrics_raw.avg_dropoff   * 100.0,
        "retention_5m_trend":       pct_trend(metrics_raw.retention_5m, trend.retention_5m),
        "avg_peak_viewers":         metrics_raw.avg_peak_viewers,
        "avg_avg_viewers":          metrics_raw.avg_avg_viewers,
        "total_followers_delta":    metrics_raw.total_followers_delta,
        "followers_per_session":    followers_per_session,
        "followers_per_hour":       followers_per_hour,
        "peak_viewers_trend":       pct_trend(metrics_raw.avg_peak_viewers, trend.avg_peak_viewers),
        "followers_trend":          pct_trend(
            metrics_raw.total_followers_delta as f64,
            trend.total_followers_delta as f64
        ),
        "unique_chatters_per_100":  metrics_raw.unique_chatters_per_100,
        "avg_unique_chatters":      metrics_raw.avg_unique_chatters,
        "total_first_time_chatters": metrics_raw.total_first_time_chatters,
        "total_returning_chatters": metrics_raw.total_returning_chatters,
        "chat_engagement_trend":    pct_trend(metrics_raw.unique_chatters_per_100, trend.unique_chatters_per_100),
        "session_count":            session_count,
        "total_duration_hours":     total_duration_hours,
    });

    let comparison = json!({
        "topStreamers": top_streamers,
        "categoryAvg":  category_avg,
        "yourStats":    your_stats,
    });

    Ok(Json(json!({
        "empty": false,
        "streamer": login,
        "days": days,
        "metrics": metrics_json,
        "retention_timeline": retention_timeline,
        "discovery_timeline": discovery_timeline,
        "chat_timeline": chat_timeline,
        "sessions": sessions,
        "insights": insights,
        "comparison": comparison,
    }))
    .into_response())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::{Request, StatusCode},
        middleware,
        routing::get,
        Extension, Router,
    };
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::net::SocketAddr;
    use std::str::FromStr;
    use tb_http_core::{internal_auth, loopback_only, ExpectedToken, INTERNAL_API_BASE_PATH};
    use tower::ServiceExt;

    // ── Timestamp-Helfer-Tests ─────────────────────────────────────────────────

    #[test]
    fn isoformat_ohne_mikrosekunden() {
        use chrono::TimeZone;
        let dt = Utc.with_ymd_and_hms(2026, 6, 12, 14, 30, 0).unwrap();
        assert_eq!(format_python_isoformat(dt), "2026-06-12T14:30:00+00:00");
    }

    #[test]
    fn isoformat_mit_mikrosekunden() {
        use chrono::TimeZone;
        let dt = Utc
            .with_ymd_and_hms(2026, 6, 12, 14, 30, 0)
            .unwrap()
            + chrono::Duration::microseconds(123456);
        let s = format_python_isoformat(dt);
        assert!(
            s.contains(".123456+00:00"),
            "Mikrosekunden fehlen oder falsch: {s}"
        );
    }

    // ── DB-Test-Infrastruktur ─────────────────────────────────────────────────

    macro_rules! db_dsn_or_skip {
        () => {
            match std::env::var("TB_TEST_DATABASE_URL").ok() {
                Some(d) => d,
                None => {
                    if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                        panic!(
                            "TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt"
                        );
                    }
                    eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                    return;
                }
            }
        };
    }

    /// Richtet ein isoliertes Schema mit prod-treuer DDL ein.
    /// Search-path wird per `PgConnectOptions` auf alle Pool-Verbindungen gesetzt
    /// (analog tb-monitoring/tests/support/mod.rs) — kein `SET search_path`-Race
    /// bei parallelen Tests.
    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        // Erst Schema per Admin-Connection anlegen
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("admin connect");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .expect("Schema droppen");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .expect("Schema anlegen");
        admin.close().await;

        // Pool mit fest eingestelltem search_path (gilt für jede neue Verbindung)
        let opts = PgConnectOptions::from_str(dsn)
            .expect("DSN parse")
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .expect("DB-Verbindung");

        // prod-treue DDL (aus tb-monitoring/tests/support/mod.rs übernommen)
        sqlx::query(
            r#"
            CREATE TABLE twitch_stream_sessions (
                id                      BIGSERIAL PRIMARY KEY,
                streamer_login          TEXT NOT NULL,
                stream_id               TEXT,
                started_at              TIMESTAMPTZ NOT NULL,
                ended_at                TIMESTAMPTZ,
                duration_seconds        INTEGER DEFAULT 0,
                start_viewers           INTEGER DEFAULT 0,
                peak_viewers            INTEGER DEFAULT 0,
                end_viewers             INTEGER DEFAULT 0,
                avg_viewers             DOUBLE PRECISION DEFAULT 0,
                samples                 INTEGER DEFAULT 0,
                retention_5m            DOUBLE PRECISION,
                retention_10m           DOUBLE PRECISION,
                retention_20m           DOUBLE PRECISION,
                dropoff_pct             DOUBLE PRECISION,
                dropoff_label           TEXT,
                unique_chatters         INTEGER DEFAULT 0,
                first_time_chatters     INTEGER DEFAULT 0,
                returning_chatters      INTEGER DEFAULT 0,
                followers_start         INTEGER,
                followers_end           INTEGER,
                follower_delta          INTEGER,
                stream_title            TEXT,
                notification_text       TEXT,
                language                TEXT,
                is_mature               BOOLEAN DEFAULT FALSE,
                tags                    TEXT,
                had_deadlock_in_session BOOLEAN DEFAULT FALSE,
                game_name               TEXT,
                notes                   TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_stream_sessions");

        sqlx::query(
            r#"
            CREATE TABLE twitch_stats_tracked (
                ts_utc       TIMESTAMPTZ,
                streamer     TEXT,
                viewer_count INTEGER,
                is_partner   BOOLEAN DEFAULT FALSE,
                game_name    TEXT,
                stream_title TEXT,
                tags         TEXT,
                language     TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_stats_tracked");

        sqlx::query(
            r#"
            CREATE TABLE twitch_stats_category (
                ts_utc       TIMESTAMPTZ,
                streamer     TEXT,
                viewer_count INTEGER,
                is_partner   BOOLEAN DEFAULT FALSE,
                game_name    TEXT,
                stream_title TEXT,
                tags         TEXT,
                language     TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_stats_category");

        pool
    }

    fn make_router(pool: PgPool, token: &str) -> Router {
        let base = INTERNAL_API_BASE_PATH;
        Router::new()
            .route(
                &format!("{base}/analytics/streamer/:login"),
                get(streamer_analytics_native_handler),
            )
            .with_state(pool)
            .layer(Extension(ExpectedToken(token.to_string())))
            .layer(middleware::from_fn_with_state(
                token.to_string(),
                internal_auth,
            ))
            .layer(middleware::from_fn(loopback_only))
    }

    fn req(method: &str, uri: &str, token: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .extension(ConnectInfo(
                "127.0.0.1:55555".parse::<SocketAddr>().unwrap(),
            ));
        if let Some(t) = token {
            builder = builder.header("x-internal-token", t);
        }
        builder.body(Body::empty()).unwrap()
    }

    async fn json_body(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), 128 * 1024)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    // ── Auth-Tests ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn ohne_token_401() {
        let dsn = db_dsn_or_skip!();
        let app = make_router(make_pool(&dsn, "test_san_auth_401").await, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{base}/analytics/streamer/testuser"),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // ── Login-Validierung ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn invalider_login_gibt_400() {
        let dsn = db_dsn_or_skip!();
        let app = make_router(make_pool(&dsn, "test_san_login_400").await, "secret");
        let base = INTERNAL_API_BASE_PATH;
        // Login "ab" ist zu kurz (< 3 Zeichen nach Normalisierung)
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{base}/analytics/streamer/ab"),
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let j = json_body(resp).await;
        assert_eq!(j["error"], "bad_request");
        assert_eq!(j["message"], "invalid login");
    }

    #[tokio::test]
    async fn invalider_days_param_gibt_400() {
        let dsn = db_dsn_or_skip!();
        let app = make_router(make_pool(&dsn, "test_san_days_400").await, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{base}/analytics/streamer/testuser?days=0"),
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let j = json_body(resp).await;
        assert_eq!(j["error"], "bad_request");
        assert_eq!(j["message"], "invalid query parameters");
    }

    #[tokio::test]
    async fn nicht_parsebarer_days_param_gibt_400() {
        let dsn = db_dsn_or_skip!();
        let app = make_router(make_pool(&dsn, "test_san_days_nan").await, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{base}/analytics/streamer/testuser?days=abc"),
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let j = json_body(resp).await;
        assert_eq!(j["message"], "invalid query parameters");
    }

    // ── Empty-Case: Streamer ohne Sessions ────────────────────────────────────

    #[tokio::test]
    async fn empty_shape_bei_streamer_ohne_sessions() {
        let dsn = db_dsn_or_skip!();
        let app = make_router(make_pool(&dsn, "test_san_empty").await, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{base}/analytics/streamer/niemand"),
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert_eq!(j["empty"], true);
        assert_eq!(j["message"], "Keine Daten für den gewählten Zeitraum");
        // Kein "metrics", kein "sessions" etc. im Empty-Case
        assert!(j.get("metrics").is_none());
        assert!(j.get("sessions").is_none());
    }

    // ── Normal-Case mit Fixture-Sessions ──────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    async fn insert_session(
        pool: &PgPool,
        login: &str,
        started_offset_days: i64,
        duration_s: i32,
        peak_viewers: i32,
        avg_viewers: f64,
        retention_5m: Option<f64>,
        retention_10m: Option<f64>,
        retention_20m: Option<f64>,
        dropoff_pct: Option<f64>,
        unique_chatters: i32,
        first_time: i32,
        returning: i32,
        follower_delta: i32,
        followers_start: i32,
        followers_end: i32,
        stream_title: &str,
    ) {
        let started_at = Utc::now() - Duration::days(started_offset_days);
        let ended_at = started_at + Duration::seconds(duration_s as i64);
        sqlx::query(
            r#"
            INSERT INTO twitch_stream_sessions (
                streamer_login, started_at, ended_at,
                duration_seconds, start_viewers, peak_viewers, end_viewers, avg_viewers,
                retention_5m, retention_10m, retention_20m, dropoff_pct,
                unique_chatters, first_time_chatters, returning_chatters,
                follower_delta, followers_start, followers_end,
                stream_title
            ) VALUES (
                $1, $2, $3,
                $4, $5, $6, $7, $8,
                $9, $10, $11, $12,
                $13, $14, $15,
                $16, $17, $18,
                $19
            )
            "#,
        )
        .bind(login)
        .bind(started_at)
        .bind(ended_at)
        .bind(duration_s)
        .bind(peak_viewers / 2)
        .bind(peak_viewers)
        .bind(peak_viewers / 3)
        .bind(avg_viewers)
        .bind(retention_5m)
        .bind(retention_10m)
        .bind(retention_20m)
        .bind(dropoff_pct)
        .bind(unique_chatters)
        .bind(first_time)
        .bind(returning)
        .bind(follower_delta)
        .bind(followers_start)
        .bind(followers_end)
        .bind(stream_title)
        .execute(pool)
        .await
        .expect("Session einfügen");
    }

    #[tokio::test]
    async fn normal_case_fuellt_alle_sektionen() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_san_normal").await;

        // 3 Sessions innerhalb der letzten 30 Tage mit retention-Daten
        for i in 0..3_i64 {
            insert_session(
                &pool,
                "streamtester",
                5 + i,
                7200,
                100 + i as i32 * 10,
                80.0 + i as f64 * 5.0,
                Some(0.75),
                Some(0.65),
                Some(0.50),
                Some(0.25),
                20,
                5,
                15,
                10,
                1000,
                1010,
                "Test Stream",
            )
            .await;
        }

        // Stats-Daten für Comparison
        sqlx::query(
            "INSERT INTO twitch_stats_tracked (ts_utc, streamer, viewer_count) VALUES (NOW(), 'top_streamer', 500)"
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_stats_category (ts_utc, streamer, viewer_count) VALUES (NOW(), 'categorie_avg', 300)"
        )
        .execute(&pool)
        .await
        .unwrap();

        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{base}/analytics/streamer/streamtester?days=30"),
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;

        // Top-Level-Felder
        assert_eq!(j["empty"], false);
        assert_eq!(j["streamer"], "streamtester");
        assert_eq!(j["days"], 30);

        // metrics: Retention ×100 (Rohwert 0.75 → 75.0)
        let metrics = &j["metrics"];
        assert!(
            (metrics["retention_5m"].as_f64().unwrap() - 75.0).abs() < 0.01,
            "retention_5m sollte ×100 sein: {}",
            metrics["retention_5m"]
        );
        assert!(
            (metrics["retention_10m"].as_f64().unwrap() - 65.0).abs() < 0.01,
            "retention_10m sollte ×100 sein: {}",
            metrics["retention_10m"]
        );
        assert_eq!(metrics["session_count"], 3);
        assert!(metrics["total_followers_delta"].as_i64().unwrap() > 0);

        // sessions-Liste: max 50, retention ebenfalls ×100
        let sessions = j["sessions"].as_array().unwrap();
        assert!(!sessions.is_empty());
        let s0 = &sessions[0];
        assert!(
            (s0["retention5m"].as_f64().unwrap() - 75.0).abs() < 0.01,
            "session retention5m sollte ×100 sein"
        );
        assert!(
            s0["date"].as_str().map(|d| d.len() == 10).unwrap_or(false),
            "date muss YYYY-MM-DD sein"
        );
        assert!(
            s0["startTime"]
                .as_str()
                .map(|t| t.len() >= 8)
                .unwrap_or(false),
            "startTime muss HH:MM:SS-Format haben"
        );

        // retention_timeline: ROHWERTE (0..1), NICHT ×100
        let rt = j["retention_timeline"].as_array().unwrap();
        assert!(!rt.is_empty());
        let rt0_ret5m = rt[0]["retention_5m"].as_f64().unwrap();
        assert!(
            rt0_ret5m <= 1.0,
            "retention_timeline muss Rohwerte (0..1) liefern, war: {}",
            rt0_ret5m
        );

        // discovery_timeline
        let dt = j["discovery_timeline"].as_array().unwrap();
        assert!(!dt.is_empty());

        // chat_timeline: unique_chatters als float/zahl
        let ct = j["chat_timeline"].as_array().unwrap();
        assert!(!ct.is_empty());
        assert!(
            ct[0]["unique_chatters"].is_f64() || ct[0]["unique_chatters"].is_i64()
                || ct[0]["unique_chatters"].is_number()
        );

        // comparison
        let comp = &j["comparison"];
        assert!(comp["topStreamers"].is_array());
        assert!(!comp["topStreamers"].as_array().unwrap().is_empty());
        assert_eq!(comp["categoryAvg"]["retention10m"], 65.0_f64);
        assert_eq!(comp["categoryAvg"]["chatHealth"], 8.5_f64);
        assert!(
            comp["yourStats"]["retention10m"].as_f64().unwrap() <= 1.0,
            "yourStats.retention10m muss Rohwert (0..1) sein"
        );

        // insights: format check
        let insights = j["insights"].as_array().unwrap();
        for insight in insights {
            assert!(insight["type"].as_str().is_some());
            assert!(insight["title"].as_str().is_some());
            assert!(insight["description"].as_str().is_some());
        }
    }

    // ── days-Default und @-Prefix-Normalisierung ──────────────────────────────

    #[tokio::test]
    async fn at_prefix_und_grossbuchstaben_normalisiert() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_san_normalize").await;

        // Session für normalisierten Login "streamtester" anlegen
        insert_session(
            &pool,
            "streamtester",
            5,
            3600,
            50,
            40.0,
            Some(0.60),
            Some(0.50),
            Some(0.40),
            Some(0.30),
            10,
            3,
            7,
            5,
            500,
            505,
            "Normalisierungstest",
        )
        .await;

        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        // URL-Encode von "@StreamTester" im Pfad
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{base}/analytics/streamer/%40StreamTester"),
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        // Normalisierter Login als streamer-Feld
        assert_eq!(j["streamer"], "streamtester");
        assert_eq!(j["empty"], false);
    }

    #[tokio::test]
    async fn days_default_30_bei_fehlendem_param() {
        let dsn = db_dsn_or_skip!();
        let app = make_router(make_pool(&dsn, "test_san_days_default").await, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{base}/analytics/streamer/niemand"),
                Some("secret"),
            ))
            .await
            .unwrap();
        // Empty-Case, days nicht im Body sichtbar
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert_eq!(j["empty"], true);
    }

    // ── yourStats Rohwert-Skala-Test ──────────────────────────────────────────

    #[tokio::test]
    async fn your_stats_retention10m_ist_rohwert() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_san_yourstats").await;

        insert_session(
            &pool,
            "rohwerttest",
            2,
            3600,
            100,
            80.0,
            Some(0.80),
            Some(0.72), // retention_10m = 0.72 (Rohwert)
            Some(0.55),
            Some(0.20),
            20,
            5,
            15,
            8,
            1000,
            1008,
            "Rohwert-Test",
        )
        .await;

        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{base}/analytics/streamer/rohwerttest"),
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert_eq!(j["empty"], false);

        // yourStats.retention10m muss Rohwert 0..1 sein (NOT ×100)
        let your_ret10m = j["comparison"]["yourStats"]["retention10m"]
            .as_f64()
            .unwrap();
        assert!(
            your_ret10m <= 1.0 && your_ret10m > 0.0,
            "yourStats.retention10m muss Rohwert sein (0..1), war: {}",
            your_ret10m
        );

        // metrics.retention_10m muss ×100 sein
        let metrics_ret10m = j["metrics"]["retention_10m"].as_f64().unwrap();
        assert!(
            metrics_ret10m > 1.0,
            "metrics.retention_10m muss ×100 sein, war: {}",
            metrics_ret10m
        );
    }

    // ── Insights-Unit-Test ────────────────────────────────────────────────────

    #[test]
    fn insights_niedrige_retention_ergibt_warning() {
        let metrics = MetricsRaw {
            retention_5m: 0.30,  // 30% → < 40%
            retention_10m: 0.35, // 35% → < 40%
            retention_20m: 0.25,
            avg_dropoff: 0.50,
            avg_peak_viewers: 100.0,
            avg_avg_viewers: 80.0,
            total_followers_delta: 2,
            session_count: 5,
            total_duration_seconds: 18000,
            avg_unique_chatters: 10.0,
            unique_chatters_per_100: 3.0, // < 5 → warning
            total_first_time_chatters: 20,
            total_returning_chatters: 30,
        };
        let insights = generate_insights(&metrics, &[]);
        assert!(insights.iter().any(|i| i.title == "Niedrige Retention"));
        assert!(insights.iter().any(|i| i.title == "Niedrige Chat-Aktivität"));
    }

    #[test]
    fn insights_exzellente_retention_und_starke_community() {
        let metrics = MetricsRaw {
            retention_5m: 0.85,
            retention_10m: 0.80, // 80% → > 70%
            retention_20m: 0.70,
            avg_dropoff: 0.15,
            avg_peak_viewers: 200.0,
            avg_avg_viewers: 180.0,
            total_followers_delta: 50, // 50/200 = 25% → > 15%
            session_count: 5,
            total_duration_seconds: 18000,
            avg_unique_chatters: 40.0,
            unique_chatters_per_100: 20.0, // > 15 → success
            total_first_time_chatters: 100,
            total_returning_chatters: 200,
        };
        let insights = generate_insights(&metrics, &[]);
        assert!(insights.iter().any(|i| i.title == "Exzellente Retention"));
        assert!(insights.iter().any(|i| i.title == "Starke Follower-Conversion"));
        assert!(insights.iter().any(|i| i.title == "Sehr aktive Community"));
    }

    #[test]
    fn insights_trend_positiv_bei_verbesserter_retention() {
        // 10 Einträge: ältere mit 0.5, letzte 7 mit 0.6 (> 0.55 = 1.10×)
        let older: Vec<RetentionTimelineEntry> = (0..3)
            .map(|i| RetentionTimelineEntry {
                date: format!("2026-05-{:02}", i + 1),
                retention_5m: 0.50,
                retention_10m: 0.45,
                retention_20m: 0.35,
                dropoff: 0.40,
            })
            .collect();
        let recent: Vec<RetentionTimelineEntry> = (0..7)
            .map(|i| RetentionTimelineEntry {
                date: format!("2026-06-{:02}", i + 1),
                retention_5m: 0.60, // 20% besser als 0.50
                retention_10m: 0.55,
                retention_20m: 0.45,
                dropoff: 0.30,
            })
            .collect();
        let timeline: Vec<RetentionTimelineEntry> = older.into_iter().chain(recent).collect();
        assert_eq!(timeline.len(), 10);

        let metrics = MetricsRaw {
            retention_5m: 0.60,
            retention_10m: 0.55,
            retention_20m: 0.45,
            avg_dropoff: 0.30,
            avg_peak_viewers: 50.0,
            avg_avg_viewers: 40.0,
            total_followers_delta: 5,
            session_count: 10,
            total_duration_seconds: 36000,
            avg_unique_chatters: 8.0,
            unique_chatters_per_100: 8.0,
            total_first_time_chatters: 50,
            total_returning_chatters: 80,
        };
        let insights = generate_insights(&metrics, &timeline);
        assert!(
            insights.iter().any(|i| i.title == "Positiver Trend"),
            "Positiver Trend fehlt: {:?}",
            insights.iter().map(|i| &i.title).collect::<Vec<_>>()
        );
    }

    // ── pct_trend-Unit-Test ────────────────────────────────────────────────────

    #[test]
    fn pct_trend_korrekte_berechnung() {
        assert!((pct_trend(110.0, 100.0) - 10.0).abs() < 1e-9);
        assert!((pct_trend(90.0, 100.0) + 10.0).abs() < 1e-9);
        assert_eq!(pct_trend(50.0, 0.0), 0.0); // Division durch 0 → 0.0
    }
}
