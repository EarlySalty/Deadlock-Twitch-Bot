//! Handler für `GET /internal/twitch/v1/stats` — nativer Rust-Port.
//!
//! Vertrag: `bot/internal_api/routes/streamers.py:389–409`,
//!   Kern-Aggregation: `bot/community/leaderboard.py:490–1258`,
//!   Monetization: `bot/dashboard/dashboard_metrics_mixin.py:25–209`,
//!   EventSub-Capacity: `bot/monitoring/eventsub_mixin.py:704–875`.
//!
//! # EventSub-Port-Trait
//!
//! Die EventSub-Sektion kommt in Python aus In-Process-Bot-State
//! (`EventSub-Mixin`), der nach dem Rust-Takeover stale ist.
//! [`EventSubStatsSource`] abstrahiert diesen Snapshot:
//! - Ohne verdrahtete Quelle (Extension = `None`) fehlt `eventsub` im Response
//!   (Python-Parität: Exception catch → Feld fehlt).
//! - Die DB-Queries (Q21–Q23) werden nativ ausgeführt; `current`-Block +
//!   active_subscriptions kommen vom Trait.
//!
//! # Typ-Konventionen
//!
//! Alle SQL-Queries nutzen `sqlx::query()` (untyped) mit manuellen
//! `try_get`-Aufrufen, um Konflikte zwischen Postgres-`NUMERIC`
//! (AVG-Ergebnis) und fehlenden `bigdecimal`-Features zu vermeiden.
//! Alle `AVG()`-Spalten werden per `CAST(... AS DOUBLE PRECISION)` erzwungen.

use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Extension, Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use std::sync::Arc;
use tb_domain::normalize_twitch_login;
use tb_http_core::{ApiError, AuthLevel};

// Lokaler Fehlertyp um `anyhow` zu vermeiden.
type BoxError = Box<dyn std::error::Error + Send + Sync>;

// ── Timestamp-Helfer ──────────────────────────────────────────────────────────

/// Gibt einen TIMESTAMPTZ als Python-`datetime.isoformat()`-String aus.
///
/// Python `datetime.isoformat()`:
/// - Mikrosekunden == 0 → `"2026-06-12T14:30:00+00:00"` (kein `.000000`)
/// - Sonst → `"2026-06-12T14:30:00.123456+00:00"` (6 Dezimalstellen)
pub fn format_ts_python(dt: DateTime<Utc>) -> String {
    let us = dt.timestamp_subsec_micros();
    if us == 0 {
        dt.format("%Y-%m-%dT%H:%M:%S+00:00").to_string()
    } else {
        dt.format("%Y-%m-%dT%H:%M:%S%.6f+00:00").to_string()
    }
}

/// Wie `format_ts_python`, aber immer ohne Mikrosekunden (`timespec="seconds"`).
pub fn format_ts_seconds(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S+00:00").to_string()
}

// ── EventSub-Port-Trait ───────────────────────────────────────────────────────

/// Snapshot-Shape für einen EventSub-`current`-Block.
#[derive(Debug, Clone)]
pub struct EventSubCurrentSnapshot {
    pub ts_utc: DateTime<Utc>,
    pub listener_count: i64,
    pub ready_listeners: i64,
    pub failed_listeners: i64,
    pub used_slots: i64,
    pub total_slots: i64,
    pub headroom_slots: i64,
    pub listeners_at_limit: i64,
    pub utilization_pct: f64,
    pub subscription_count: i64,
    /// `current_snapshot["subscriptions"]` — Shape variabel (Vertrag-UNSICHER)
    pub active_subscriptions: Vec<Value>,
    /// `current_snapshot["subscription_types"]`
    pub active_subscription_types: Vec<Value>,
    /// `current_snapshot["subscription_channels"]`
    pub active_subscription_channels: Vec<Value>,
}

/// Port-Trait für den EventSub-In-Process-State (`eventsub_mixin.py:704–875`).
///
/// Ohne Verdrahtung liefert `get_snapshot` `None`; der Handler lässt die
/// `eventsub`-Sektion dann komplett weg (Python-Parität).
#[async_trait::async_trait]
pub trait EventSubStatsSource: Send + Sync {
    async fn get_snapshot(&self) -> Option<EventSubCurrentSnapshot>;
}

/// Router-Extension-Wrapper für [`EventSubStatsSource`].
#[derive(Clone)]
pub struct EventSubStatsExt(pub Option<Arc<dyn EventSubStatsSource>>);

// ── Query-Parameter ───────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct StatsQuery {
    #[serde(default)]
    pub hour_from: Option<String>,
    #[serde(default)]
    pub hour_to: Option<String>,
    #[serde(default)]
    pub streamer: Option<String>,
}

#[derive(Debug, Clone)]
enum HourFilter {
    None,
    Between(i32, i32),
    Wrap(i32, i32),
}

impl HourFilter {
    fn mode_str(&self) -> &'static str {
        match self {
            HourFilter::None => "none",
            HourFilter::Between(_, _) => "between",
            HourFilter::Wrap(_, _) => "wrap",
        }
    }
    fn start(&self) -> i32 {
        match self {
            HourFilter::None => 0,
            HourFilter::Between(s, _) | HourFilter::Wrap(s, _) => *s,
        }
    }
    fn end(&self) -> i32 {
        match self {
            HourFilter::None => 23,
            HourFilter::Between(_, e) | HourFilter::Wrap(_, e) => *e,
        }
    }
}

fn parse_optional_int(value: &Option<String>) -> Result<Option<i32>, ()> {
    match value {
        None => Ok(None),
        Some(s) => {
            let s = s.trim();
            if s.is_empty() {
                return Ok(None);
            }
            let n: i32 = s.parse().map_err(|_| ())?;
            if n < 0 {
                return Err(());
            }
            Ok(Some(n))
        }
    }
}

fn normalize_hour(h: i32) -> i32 {
    h.clamp(0, 23)
}

fn parse_hour_filter(
    hour_from: &Option<String>,
    hour_to: &Option<String>,
) -> Result<HourFilter, ()> {
    let from = parse_optional_int(hour_from)?;
    let to = parse_optional_int(hour_to)?;
    match (from, to) {
        (None, None) => Ok(HourFilter::None),
        (Some(f), None) => {
            let h = normalize_hour(f);
            Ok(HourFilter::Between(h, h))
        }
        (None, Some(t)) => {
            let h = normalize_hour(t);
            Ok(HourFilter::Between(h, h))
        }
        (Some(f), Some(t)) => {
            let start = normalize_hour(f);
            let end = normalize_hour(t);
            if start <= end {
                Ok(HourFilter::Between(start, end))
            } else {
                Ok(HourFilter::Wrap(start, end))
            }
        }
    }
}

// ── Interne Hilfstypen ────────────────────────────────────────────────────────

/// Prod-Typen der View `twitch_streamers_partner_state`: `is_on_discord` und
/// `manual_verified_permanent` sind **INTEGER** (0/1), `manual_verified_until`
/// ist **TEXT** (ISO-String) — bool-/DateTime-Dekodierung schlägt zur Laufzeit
/// fehl (Typ-Drift-Klasse; war die Ursache für is_partner=0 auf allen
/// tracked.top-Einträgen im Live-Diff 12.6.).
struct PartnerStateRow {
    twitch_login: String,
    is_on_discord: Option<i32>,
    discord_user_id: Option<String>,
    discord_display_name: Option<String>,
    manual_verified_permanent: Option<i32>,
    manual_verified_until: Option<String>,
}

struct TopRow {
    streamer: String,
    avg_viewers: f64,
    max_viewers: i64,
    samples: i64,
    is_partner: i32,
}

struct HourlyRow {
    hour: i32,
    avg_viewers: f64,
    max_viewers: f64,
    samples: i64,
}

struct WeekdayRow {
    weekday: i32,
    avg_viewers: f64,
    max_viewers: f64,
    samples: i64,
}

// ── Response-Typen ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct StreamerEntry {
    pub streamer: String,
    pub avg_viewers: f64,
    pub max_viewers: i64,
    pub samples: i64,
    pub is_partner: i32,
    pub is_on_discord: i32,
    pub discord_user_id: Option<String>,
    pub discord_display_name: Option<String>,
    pub has_discord_profile: i32,
}

#[derive(Serialize)]
pub struct HourlyEntry {
    pub hour: i32,
    pub avg_viewers: f64,
    pub max_viewers: f64,
    pub samples: i64,
}

#[derive(Serialize)]
pub struct WeekdayEntry {
    pub weekday: i32,
    pub avg_viewers: f64,
    pub max_viewers: f64,
    pub samples: i64,
}

// ── SQL-Hilfsmakro für try_get ────────────────────────────────────────────────

/// Liest einen `f64`-Wert aus einem Row-Feld, das als `DOUBLE PRECISION` im
/// SQL gecasted wurde. Gibt 0.0 zurück wenn NULL.
fn row_f64(row: &sqlx::postgres::PgRow, col: &str) -> f64 {
    row.try_get::<f64, _>(col).unwrap_or(0.0)
}

fn row_i64(row: &sqlx::postgres::PgRow, col: &str) -> i64 {
    // BIGINT → i64
    if let Ok(v) = row.try_get::<i64, _>(col) {
        return v;
    }
    // INTEGER → i32 → i64
    if let Ok(v) = row.try_get::<i32, _>(col) {
        return v as i64;
    }
    0
}

fn row_i32(row: &sqlx::postgres::PgRow, col: &str) -> i32 {
    if let Ok(v) = row.try_get::<i32, _>(col) {
        return v;
    }
    if let Ok(v) = row.try_get::<i64, _>(col) {
        return v as i32;
    }
    0
}

fn row_str_opt(row: &sqlx::postgres::PgRow, col: &str) -> Option<String> {
    row.try_get::<Option<String>, _>(col).unwrap_or(None)
}

fn row_i32_opt(row: &sqlx::postgres::PgRow, col: &str) -> Option<i32> {
    row.try_get::<Option<i32>, _>(col).unwrap_or(None)
}

/// TIMESTAMPTZ-Spalte (z. B. `twitch_eventsub_capacity_snapshot.ts_utc`).
fn row_ts_opt(row: &sqlx::postgres::PgRow, col: &str) -> Option<DateTime<Utc>> {
    row.try_get::<Option<DateTime<Utc>>, _>(col).unwrap_or(None)
}

/// Python `_parse_db_datetime` (`leaderboard.py:504-516`): ISO-String parsen,
/// naive Werte als UTC interpretieren; nicht parsebar → None.
fn parse_db_datetime(value: &str) -> Option<DateTime<Utc>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        return Some(dt.with_timezone(&Utc));
    }
    // Naive Variante ohne Offset (Python fromisoformat akzeptiert beides).
    chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S%.f"))
        .ok()
        .map(|naive| naive.and_utc())
}

// ── SQL-Templates ─────────────────────────────────────────────────────────────

fn build_top_sql(table: &str, is_tracked: bool, streamer_filter: bool) -> String {
    let partition_filter = if is_tracked {
        "           AND LOWER(streamer) IN (\n               SELECT LOWER(twitch_login)\n                 FROM twitch_streamers_partner_state\n                WHERE is_partner_active = 1\n           )\n"
    } else {
        "           AND LOWER(streamer) NOT IN (\n               SELECT LOWER(twitch_login)\n                 FROM twitch_streamers_partner_state\n                WHERE is_partner_active = 1\n           )\n"
    };
    let streamer_cond = if streamer_filter {
        "           AND LOWER(streamer) = $1\n"
    } else {
        ""
    };
    let offset = if streamer_filter { 1 } else { 0 };
    let (p1, p2, p3, p4, p5, p6, p7) = params_offset(offset);
    format!(
        r#"SELECT streamer,
               CAST(AVG(viewer_count) AS DOUBLE PRECISION) AS avg_viewers,
               CAST(MAX(viewer_count) AS BIGINT)           AS max_viewers,
               CAST(COUNT(*) AS BIGINT)                    AS samples,
               MAX(CASE WHEN is_partner THEN 1 ELSE 0 END) AS is_partner
          FROM {table}
         WHERE ts_utc >= NOW() - INTERVAL '30 days'
{streamer_cond}{partition_filter}           AND (
                {p1} = 'none'
                OR ({p2} = 'between' AND EXTRACT(HOUR FROM (ts_utc AT TIME ZONE 'UTC'))::int BETWEEN {p3} AND {p4})
                OR ({p5} = 'wrap' AND (
                        EXTRACT(HOUR FROM (ts_utc AT TIME ZONE 'UTC'))::int >= {p6}
                        OR EXTRACT(HOUR FROM (ts_utc AT TIME ZONE 'UTC'))::int <= {p7}
                    ))
           )
         GROUP BY streamer
         ORDER BY avg_viewers DESC"#,
    )
}

fn build_hourly_sql(table: &str, is_tracked: bool, streamer_filter: bool) -> String {
    let partition_filter = if is_tracked {
        "           AND LOWER(streamer) IN (\n               SELECT LOWER(twitch_login)\n                 FROM twitch_streamers_partner_state\n                WHERE is_partner_active = 1\n           )\n"
    } else {
        "           AND LOWER(streamer) NOT IN (\n               SELECT LOWER(twitch_login)\n                 FROM twitch_streamers_partner_state\n                WHERE is_partner_active = 1\n           )\n"
    };
    let streamer_cond = if streamer_filter {
        "           AND LOWER(streamer) = $1\n"
    } else {
        ""
    };
    let offset = if streamer_filter { 1 } else { 0 };
    let (p1, p2, p3, p4, p5, p6, p7) = params_offset(offset);
    format!(
        r#"SELECT EXTRACT(HOUR FROM (ts_utc AT TIME ZONE 'UTC'))::int AS hour,
               CAST(AVG(viewer_count) AS DOUBLE PRECISION) AS avg_viewers,
               CAST(MAX(viewer_count) AS DOUBLE PRECISION) AS max_viewers,
               CAST(COUNT(*) AS BIGINT)                    AS samples
          FROM {table}
         WHERE ts_utc >= NOW() - INTERVAL '30 days'
{streamer_cond}{partition_filter}           AND (
                {p1} = 'none'
                OR ({p2} = 'between' AND EXTRACT(HOUR FROM (ts_utc AT TIME ZONE 'UTC'))::int BETWEEN {p3} AND {p4})
                OR ({p5} = 'wrap' AND (
                        EXTRACT(HOUR FROM (ts_utc AT TIME ZONE 'UTC'))::int >= {p6}
                        OR EXTRACT(HOUR FROM (ts_utc AT TIME ZONE 'UTC'))::int <= {p7}
                    ))
           )
         GROUP BY hour
         ORDER BY hour"#,
    )
}

fn build_weekday_sql(table: &str, is_tracked: bool, streamer_filter: bool) -> String {
    let partition_filter = if is_tracked {
        "           AND LOWER(streamer) IN (\n               SELECT LOWER(twitch_login)\n                 FROM twitch_streamers_partner_state\n                WHERE is_partner_active = 1\n           )\n"
    } else {
        "           AND LOWER(streamer) NOT IN (\n               SELECT LOWER(twitch_login)\n                 FROM twitch_streamers_partner_state\n                WHERE is_partner_active = 1\n           )\n"
    };
    let streamer_cond = if streamer_filter {
        "           AND LOWER(streamer) = $1\n"
    } else {
        ""
    };
    let offset = if streamer_filter { 1 } else { 0 };
    let (p1, p2, p3, p4, p5, p6, p7) = params_offset(offset);
    format!(
        r#"SELECT EXTRACT(DOW FROM (ts_utc AT TIME ZONE 'UTC'))::int AS weekday,
               CAST(AVG(viewer_count) AS DOUBLE PRECISION) AS avg_viewers,
               CAST(MAX(viewer_count) AS DOUBLE PRECISION) AS max_viewers,
               CAST(COUNT(*) AS BIGINT)                    AS samples
          FROM {table}
         WHERE ts_utc >= NOW() - INTERVAL '30 days'
{streamer_cond}{partition_filter}           AND (
                {p1} = 'none'
                OR ({p2} = 'between' AND EXTRACT(HOUR FROM (ts_utc AT TIME ZONE 'UTC'))::int BETWEEN {p3} AND {p4})
                OR ({p5} = 'wrap' AND (
                        EXTRACT(HOUR FROM (ts_utc AT TIME ZONE 'UTC'))::int >= {p6}
                        OR EXTRACT(HOUR FROM (ts_utc AT TIME ZONE 'UTC'))::int <= {p7}
                    ))
           )
         GROUP BY weekday
         ORDER BY weekday"#,
    )
}

/// Gibt die Parameter-Platzhalter $1…$7 zurück, verschoben um `offset`
/// (0 = kein Streamer-Filter, 1 = +1 wegen $1=login).
fn params_offset(offset: usize) -> (&'static str, &'static str, &'static str, &'static str, &'static str, &'static str, &'static str) {
    match offset {
        0 => ("$1", "$2", "$3", "$4", "$5", "$6", "$7"),
        1 => ("$2", "$3", "$4", "$5", "$6", "$7", "$8"),
        _ => ("$1", "$2", "$3", "$4", "$5", "$6", "$7"),
    }
}

// ── DB-Queries ────────────────────────────────────────────────────────────────

/// Q1 — Partner-State-Basis (`leaderboard.py:520–531`).
async fn fetch_partner_state(pool: &PgPool) -> Result<Vec<PartnerStateRow>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT twitch_login,
               is_on_discord,
               discord_user_id,
               discord_display_name,
               manual_verified_permanent,
               manual_verified_until
          FROM twitch_streamers_partner_state
         WHERE is_partner_active = 1
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| PartnerStateRow {
            twitch_login: r.try_get("twitch_login").unwrap_or_default(),
            is_on_discord: row_i32_opt(r, "is_on_discord"),
            discord_user_id: row_str_opt(r, "discord_user_id"),
            discord_display_name: row_str_opt(r, "discord_display_name"),
            manual_verified_permanent: row_i32_opt(r, "manual_verified_permanent"),
            manual_verified_until: row_str_opt(r, "manual_verified_until"),
        })
        .collect())
}

async fn fetch_top(
    pool: &PgPool,
    table: &str,
    is_tracked: bool,
    hf: &HourFilter,
) -> Result<Vec<TopRow>, sqlx::Error> {
    let mode = hf.mode_str();
    let start = hf.start();
    let end = hf.end();
    let sql = build_top_sql(table, is_tracked, false);
    let rows = sqlx::query(&sql)
        .bind(mode)
        .bind(mode)
        .bind(start)
        .bind(end)
        .bind(mode)
        .bind(start)
        .bind(end)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .iter()
        .map(|r| TopRow {
            streamer: r.try_get("streamer").unwrap_or_default(),
            avg_viewers: row_f64(r, "avg_viewers"),
            max_viewers: row_i64(r, "max_viewers"),
            samples: row_i64(r, "samples"),
            is_partner: row_i32(r, "is_partner"),
        })
        .collect())
}

async fn fetch_hourly(
    pool: &PgPool,
    table: &str,
    is_tracked: bool,
    hf: &HourFilter,
) -> Result<Vec<HourlyRow>, sqlx::Error> {
    let mode = hf.mode_str();
    let start = hf.start();
    let end = hf.end();
    let sql = build_hourly_sql(table, is_tracked, false);
    let rows = sqlx::query(&sql)
        .bind(mode)
        .bind(mode)
        .bind(start)
        .bind(end)
        .bind(mode)
        .bind(start)
        .bind(end)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .iter()
        .map(|r| HourlyRow {
            hour: row_i32(r, "hour"),
            avg_viewers: row_f64(r, "avg_viewers"),
            max_viewers: row_f64(r, "max_viewers"),
            samples: row_i64(r, "samples"),
        })
        .collect())
}

async fn fetch_weekday(
    pool: &PgPool,
    table: &str,
    is_tracked: bool,
    hf: &HourFilter,
) -> Result<Vec<WeekdayRow>, sqlx::Error> {
    let mode = hf.mode_str();
    let start = hf.start();
    let end = hf.end();
    let sql = build_weekday_sql(table, is_tracked, false);
    let rows = sqlx::query(&sql)
        .bind(mode)
        .bind(mode)
        .bind(start)
        .bind(end)
        .bind(mode)
        .bind(start)
        .bind(end)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .iter()
        .map(|r| WeekdayRow {
            weekday: row_i32(r, "weekday"),
            avg_viewers: row_f64(r, "avg_viewers"),
            max_viewers: row_f64(r, "max_viewers"),
            samples: row_i64(r, "samples"),
        })
        .collect())
}

async fn fetch_user_top(
    pool: &PgPool,
    table: &str,
    is_tracked: bool,
    login: &str,
    hf: &HourFilter,
) -> Result<Vec<TopRow>, sqlx::Error> {
    let mode = hf.mode_str();
    let start = hf.start();
    let end = hf.end();
    let sql = build_top_sql(table, is_tracked, true);
    let rows = sqlx::query(&sql)
        .bind(login)
        .bind(mode)
        .bind(mode)
        .bind(start)
        .bind(end)
        .bind(mode)
        .bind(start)
        .bind(end)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .iter()
        .map(|r| TopRow {
            streamer: r.try_get("streamer").unwrap_or_default(),
            avg_viewers: row_f64(r, "avg_viewers"),
            max_viewers: row_i64(r, "max_viewers"),
            samples: row_i64(r, "samples"),
            is_partner: row_i32(r, "is_partner"),
        })
        .collect())
}

async fn fetch_user_hourly(
    pool: &PgPool,
    table: &str,
    is_tracked: bool,
    login: &str,
    hf: &HourFilter,
) -> Result<Vec<HourlyRow>, sqlx::Error> {
    let mode = hf.mode_str();
    let start = hf.start();
    let end = hf.end();
    let sql = build_hourly_sql(table, is_tracked, true);
    let rows = sqlx::query(&sql)
        .bind(login)
        .bind(mode)
        .bind(mode)
        .bind(start)
        .bind(end)
        .bind(mode)
        .bind(start)
        .bind(end)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .iter()
        .map(|r| HourlyRow {
            hour: row_i32(r, "hour"),
            avg_viewers: row_f64(r, "avg_viewers"),
            max_viewers: row_f64(r, "max_viewers"),
            samples: row_i64(r, "samples"),
        })
        .collect())
}

async fn fetch_user_weekday(
    pool: &PgPool,
    table: &str,
    is_tracked: bool,
    login: &str,
    hf: &HourFilter,
) -> Result<Vec<WeekdayRow>, sqlx::Error> {
    let mode = hf.mode_str();
    let start = hf.start();
    let end = hf.end();
    let sql = build_weekday_sql(table, is_tracked, true);
    let rows = sqlx::query(&sql)
        .bind(login)
        .bind(mode)
        .bind(mode)
        .bind(start)
        .bind(end)
        .bind(mode)
        .bind(start)
        .bind(end)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .iter()
        .map(|r| WeekdayRow {
            weekday: row_i32(r, "weekday"),
            avg_viewers: row_f64(r, "avg_viewers"),
            max_viewers: row_f64(r, "max_viewers"),
            samples: row_i64(r, "samples"),
        })
        .collect())
}

// ── Partner-Maps + Anreicherung ───────────────────────────────────────────────

struct PartnerMaps {
    tracked_logins: std::collections::HashSet<String>,
    verified_logins: std::collections::HashSet<String>,
    discord_info: std::collections::HashMap<String, (Option<String>, Option<String>, bool)>,
}

fn build_partner_maps(rows: Vec<PartnerStateRow>, now: DateTime<Utc>) -> PartnerMaps {
    let mut tracked = std::collections::HashSet::new();
    let mut verified = std::collections::HashSet::new();
    let mut discord_info = std::collections::HashMap::new();

    for r in rows {
        let login = r.twitch_login.to_lowercase();
        tracked.insert(login.clone());

        // Python: manual_verified_permanent truthy ODER verified_until >= now.
        let is_verified = r.manual_verified_permanent.unwrap_or(0) != 0
            || r.manual_verified_until
                .as_deref()
                .and_then(parse_db_datetime)
                .map(|dt| dt >= now)
                .unwrap_or(false);
        if is_verified {
            verified.insert(login.clone());
        }

        let has_profile = r.discord_user_id.as_deref().map(|s| !s.is_empty()).unwrap_or(false)
            || r.discord_display_name.as_deref().map(|s| !s.is_empty()).unwrap_or(false);
        let is_on_disc = r.is_on_discord.unwrap_or(0) != 0 || has_profile;
        discord_info.insert(login, (r.discord_user_id, r.discord_display_name, is_on_disc));
    }

    PartnerMaps { tracked_logins: tracked, verified_logins: verified, discord_info }
}

fn enrich_top_row(row: TopRow, maps: &PartnerMaps) -> StreamerEntry {
    let login_lower = row.streamer.to_lowercase();

    let is_partner = if maps.tracked_logins.contains(&login_lower) {
        if maps.verified_logins.contains(&login_lower) { 1 } else { 0 }
    } else {
        row.is_partner
    };

    let (discord_user_id, discord_display_name, raw_is_on_discord) =
        maps.discord_info.get(&login_lower).cloned().unwrap_or((None, None, false));

    let has_discord_profile = discord_user_id.as_deref().map(|s| !s.is_empty()).unwrap_or(false)
        || discord_display_name.as_deref().map(|s| !s.is_empty()).unwrap_or(false);

    let is_on_discord = if raw_is_on_discord || has_discord_profile
        || maps.verified_logins.contains(&login_lower)
    {
        1
    } else {
        0
    };

    StreamerEntry {
        streamer: row.streamer,
        avg_viewers: row.avg_viewers,
        max_viewers: row.max_viewers,
        samples: row.samples,
        is_partner,
        is_on_discord,
        discord_user_id,
        discord_display_name,
        has_discord_profile: if has_discord_profile { 1 } else { 0 },
    }
}

fn map_hourly(rows: Vec<HourlyRow>) -> Vec<HourlyEntry> {
    rows.into_iter()
        .map(|r| HourlyEntry { hour: r.hour, avg_viewers: r.avg_viewers, max_viewers: r.max_viewers, samples: r.samples })
        .collect()
}

fn map_weekday(rows: Vec<WeekdayRow>) -> Vec<WeekdayEntry> {
    rows.into_iter()
        .map(|r| WeekdayEntry { weekday: r.weekday, avg_viewers: r.avg_viewers, max_viewers: r.max_viewers, samples: r.samples })
        .collect()
}


// ── Monetization (Q16–Q20) ───────────────────────────────────────────────────

async fn fetch_monetization(pool: &PgPool) -> Result<Value, BoxError> {
    // ACHTUNG: started_at/received_at sind TIMESTAMPTZ — der Cutoff muss als
    // DateTime gebunden werden. Ein String-Bind sendet konkretes TEXT und
    // Postgres lehnt `timestamptz >= text` für Parameter ab (psycopg sendet
    // „unknown" und lässt den Server inferieren — deshalb läuft Python).
    let cutoff = Utc::now() - chrono::Duration::days(30);

    // Q16 — Ad-Break-Aggregation (`dashboard_metrics_mixin.py:48–58`)
    let ad_row = sqlx::query(
        r#"
        SELECT CAST(COUNT(*) AS BIGINT) AS total_ads,
               CAST(SUM(CASE WHEN is_automatic IS TRUE THEN 1 ELSE 0 END) AS BIGINT) AS auto_ads,
               CAST(AVG(duration_seconds) AS DOUBLE PRECISION) AS avg_duration,
               CAST(COUNT(DISTINCT session_id) AS BIGINT) AS sessions_with_ads
          FROM twitch_ad_break_events
         WHERE started_at >= $1
        "#,
    )
    .bind(cutoff)
    .fetch_one(pool)
    .await?;

    let total_ads = row_i64(&ad_row, "total_ads");
    let auto_ads = row_i64(&ad_row, "auto_ads");
    let manual_ads = total_ads - auto_ads;
    let sessions_with_ads = row_i64(&ad_row, "sessions_with_ads");
    let avg_duration = row_f64(&ad_row, "avg_duration");

    // Q17 — Ad-Einzel-Rows
    let ad_rows = sqlx::query(
        r#"
        SELECT a.id, a.session_id, a.started_at, a.duration_seconds, a.is_automatic,
               s.started_at AS session_start
          FROM twitch_ad_break_events a
          JOIN twitch_stream_sessions s ON s.id = a.session_id
         WHERE a.started_at >= $1
           AND a.session_id IS NOT NULL
         ORDER BY a.started_at DESC
         LIMIT 200
        "#,
    )
    .bind(cutoff)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let session_ids: Vec<i64> = ad_rows
        .iter()
        .filter_map(|r| {
            r.try_get::<Option<i64>, _>("session_id").ok().flatten()
                .or_else(|| r.try_get::<Option<i32>, _>("session_id").ok().flatten().map(|v| v as i64))
        })
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    // Q17b — Viewer-Timeline (nur wenn Session-IDs vorhanden)
    let timeline_map: std::collections::HashMap<i64, Vec<(i32, i64)>> = if session_ids.is_empty() {
        std::collections::HashMap::new()
    } else {
        let session_ids_json = serde_json::to_string(&session_ids).unwrap_or_default();
        // json_each ist SQLite-spezifisch; in PostgreSQL nutzen wir json_array_elements_text
        let tl_rows = sqlx::query(
            r#"
            SELECT session_id, minutes_from_start, viewer_count
              FROM twitch_session_viewers
             WHERE session_id = ANY(SELECT value::int FROM json_array_elements_text($1::json))
             ORDER BY session_id, minutes_from_start
            "#,
        )
        .bind(&session_ids_json)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        let mut map: std::collections::HashMap<i64, Vec<(i32, i64)>> = std::collections::HashMap::new();
        for r in &tl_rows {
            let sid = row_i64(r, "session_id");
            let min = row_i32(r, "minutes_from_start");
            let vc = row_i64(r, "viewer_count");
            map.entry(sid).or_default().push((min, vc));
        }
        map
    };

    // Viewer-Drop-Berechnung
    let mut drop_pcts: Vec<f64> = Vec::new();
    let mut worst_ads_data: Vec<(String, i64, f64, bool)> = Vec::new();

    for r in &ad_rows {
        let sid = r.try_get::<Option<i64>, _>("session_id").ok().flatten()
            .or_else(|| r.try_get::<Option<i32>, _>("session_id").ok().flatten().map(|v| v as i64));
        let sid = match sid { Some(s) => s, None => continue };

        // started_at und session_start können TEXT (ISO) oder TIMESTAMPTZ sein
        let ad_started_ts = r.try_get::<Option<DateTime<Utc>>, _>("started_at").ok().flatten();
        let session_start_ts = r.try_get::<Option<DateTime<Utc>>, _>("session_start").ok().flatten();

        let (ad_minute_opt, started_at_str) = if let (Some(ad_dt), Some(ss_dt)) = (ad_started_ts, session_start_ts) {
            let ad_minute = ((ad_dt - ss_dt).num_seconds() / 60) as i32;
            let s = ad_dt.format("%Y-%m-%dT%H:%M").to_string();
            (Some(ad_minute), s)
        } else {
            // TEXT-Felder → kein Timeline-Lookup möglich, aber started_at für worst_ads
            let s = r.try_get::<Option<String>, _>("started_at").ok().flatten().unwrap_or_default();
            let s16 = if s.len() >= 16 { s[..16].to_string() } else { s };
            (None, s16)
        };

        let dur_secs = row_i64(r, "duration_seconds");
        let is_auto = r.try_get::<Option<bool>, _>("is_automatic").ok().flatten().unwrap_or(false);

        if let Some(ad_minute) = ad_minute_opt {
            if let Some(tl) = timeline_map.get(&sid) {
                // Python dashboard_metrics_mixin.py:124-147: 5-Min-Mittel VOR der Ad
                // und 5-Min-Mittel NACH Ad-Ende, signierter Drop (negativ bei Verlust).
                let duration_min = dur_secs as f64 / 60.0;
                let ad_min_f = ad_minute as f64;
                let post_start = ad_min_f + duration_min;
                let pre_vals: Vec<f64> = tl
                    .iter()
                    .filter(|(m, _)| (ad_min_f - 5.0) <= *m as f64 && (*m as f64) < ad_min_f)
                    .map(|(_, v)| *v as f64)
                    .collect();
                let post_vals: Vec<f64> = tl
                    .iter()
                    .filter(|(m, _)| post_start <= *m as f64 && (*m as f64) < post_start + 5.0)
                    .map(|(_, v)| *v as f64)
                    .collect();
                if !pre_vals.is_empty() && !post_vals.is_empty() {
                    let pre_avg = pre_vals.iter().sum::<f64>() / pre_vals.len() as f64;
                    if pre_avg > 0.0 {
                        let post_avg = post_vals.iter().sum::<f64>() / post_vals.len() as f64;
                        let drop = (post_avg - pre_avg) / pre_avg * 100.0;
                        let drop_rounded = (drop * 10.0).round() / 10.0;
                        drop_pcts.push(drop); // ungerundet mitteln (wie Python)
                        worst_ads_data.push((started_at_str, dur_secs, drop_rounded, is_auto));
                    }
                }
            }
        }
    }

    let avg_viewer_drop_pct = if drop_pcts.is_empty() {
        Value::Null
    } else {
        let avg = drop_pcts.iter().sum::<f64>() / drop_pcts.len() as f64;
        json!((avg * 10.0).round() / 10.0)
    };

    // Python sortiert aufsteigend nach drop_pct (negativster Drop = schlimmste Ad zuerst).
    worst_ads_data.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
    let worst_ads: Vec<Value> = worst_ads_data.into_iter().take(5).map(|(started_at, duration_s, drop_pct, is_automatic)| {
        json!({ "started_at": started_at, "duration_s": duration_s, "drop_pct": drop_pct, "is_automatic": is_automatic })
    }).collect();

    // Q18 — Hype Train (`dashboard_metrics_mixin.py:150–170`). Python hat
    // pro Query ein eigenes try/except → Fehler lassen die Defaults stehen,
    // die Sektion bleibt erhalten.
    let (hype_total, hype_avg_level, hype_max_level, hype_avg_dur) = match sqlx::query(
        r#"
        SELECT CAST(COUNT(*) AS BIGINT) AS total_trains,
               CAST(AVG(level) AS DOUBLE PRECISION) AS avg_level,
               MAX(level) AS max_level,
               CAST(AVG(duration_seconds) AS DOUBLE PRECISION) AS avg_duration
          FROM twitch_hype_train_events
         WHERE started_at >= $1
           AND ended_at IS NOT NULL
        "#,
    )
    .bind(cutoff)
    .fetch_one(pool)
    .await
    {
        Ok(hype) => (
            row_i64(&hype, "total_trains"),
            (row_f64(&hype, "avg_level") * 10.0).round() / 10.0,
            row_i32(&hype, "max_level"),
            row_f64(&hype, "avg_duration").round(),
        ),
        Err(e) => {
            tracing::debug!("Hype Train query fehlgeschlagen: {e}");
            (0, 0.0, 0, 0.0)
        }
    };

    // Q19 — Bits (`dashboard_metrics_mixin.py:172–185`)
    let (bits_total, cheer_events) = match sqlx::query(
        r#"
        SELECT CAST(SUM(amount) AS BIGINT) AS total_bits, CAST(COUNT(*) AS BIGINT) AS cheer_events
        FROM twitch_bits_events
        WHERE received_at >= $1
        "#,
    )
    .bind(cutoff)
    .fetch_one(pool)
    .await
    {
        Ok(bits) => (row_i64(&bits, "total_bits"), row_i64(&bits, "cheer_events")),
        Err(e) => {
            tracing::debug!("Bits query fehlgeschlagen: {e}");
            (0, 0)
        }
    };

    // Q20 — Subscriptions-Events (`dashboard_metrics_mixin.py:187–202`).
    // Bewusste Abweichung (Bugfix): Python vergleicht `is_gift=1` gegen die
    // BOOLEAN-Spalte — das schlägt auf Postgres fehl und die subs-Zahlen sind
    // in Python deshalb immer 0. Rust nutzt `is_gift IS TRUE` und liefert
    // echte Werte.
    let (subs_total, subs_gifted) = match sqlx::query(
        r#"
        SELECT CAST(COUNT(*) AS BIGINT) AS total_events,
               CAST(SUM(CASE WHEN is_gift IS TRUE THEN 1 ELSE 0 END) AS BIGINT) AS gifted
          FROM twitch_subscription_events
         WHERE received_at >= $1
        "#,
    )
    .bind(cutoff)
    .fetch_one(pool)
    .await
    {
        Ok(subs_ev) => (row_i64(&subs_ev, "total_events"), row_i64(&subs_ev, "gifted")),
        Err(e) => {
            tracing::debug!("Subs query fehlgeschlagen: {e}");
            (0, 0)
        }
    };

    Ok(json!({
        "window_days": 30,
        "ads": {
            "total": total_ads,
            "auto": auto_ads,
            "manual": manual_ads,
            "sessions_with_ads": sessions_with_ads,
            "avg_duration_s": avg_duration,
            "avg_viewer_drop_pct": avg_viewer_drop_pct,
            "worst_ads": worst_ads,
        },
        "hype_train": {
            "total": hype_total,
            "avg_level": hype_avg_level,
            "max_level": hype_max_level,
            "avg_duration_s": hype_avg_dur,
        },
        "bits": { "total": bits_total, "cheer_events": cheer_events },
        "subs": { "total_events": subs_total, "gifted": subs_gifted },
    }))
}

// ── EventSub-Capacity-DB (Q21–Q23) ───────────────────────────────────────────

async fn fetch_eventsub_capacity_db(pool: &PgPool) -> Result<Value, sqlx::Error> {
    let window = "24 hours";

    // Q21 (`eventsub_mixin.py:710–725`)
    let snap_rows = sqlx::query(
        r#"
        SELECT ts_utc, trigger_reason, listener_count, ready_listeners, failed_listeners,
               used_slots, total_slots, headroom_slots, listeners_at_limit, utilization_pct
          FROM twitch_eventsub_capacity_snapshot
         WHERE ts_utc >= NOW() - ($1::interval)
         ORDER BY ts_utc ASC
        "#,
    )
    .bind(window)
    .fetch_all(pool)
    .await?;

    if snap_rows.is_empty() {
        return Ok(json!({
            "window_hours": 24, "samples": 0, "last_snapshot_at": Value::Null,
            "avg_utilization_pct": 0.0, "p95_utilization_pct": 0.0, "max_utilization_pct": 0.0,
            "avg_used_slots": 0.0, "max_used_slots": 0, "avg_listener_count": 0.0,
            "max_listener_count": 0, "avg_ready_listeners": 0.0, "max_failed_listeners": 0,
            "hourly": [], "reasons": [],
            "active_subscriptions": [], "active_subscription_types": [], "active_subscription_channels": [],
            "current": { "ts_utc": Value::Null, "listener_count": 0, "ready_listeners": 0,
                "failed_listeners": 0, "used_slots": 0, "total_slots": 0, "headroom_slots": 0,
                "listeners_at_limit": 0, "utilization_pct": 0.0, "subscription_count": 0 },
        }));
    }

    let last_ts = snap_rows.last()
        .and_then(|r| row_ts_opt(r, "ts_utc"))
        .map(format_ts_python);

    let util_vals: Vec<f64> = snap_rows.iter().map(|r| row_f64(r, "utilization_pct")).collect();
    let used_vals: Vec<i64> = snap_rows.iter().map(|r| row_i64(r, "used_slots")).collect();
    let listener_vals: Vec<i64> = snap_rows.iter().map(|r| row_i64(r, "listener_count")).collect();
    let ready_vals: Vec<i64> = snap_rows.iter().map(|r| row_i64(r, "ready_listeners")).collect();
    let failed_vals: Vec<i64> = snap_rows.iter().map(|r| row_i64(r, "failed_listeners")).collect();

    let avg_f = |vals: &[f64]| if vals.is_empty() { 0.0 } else { vals.iter().sum::<f64>() / vals.len() as f64 };
    let avg_i = |vals: &[i64]| if vals.is_empty() { 0.0 } else { vals.iter().sum::<i64>() as f64 / vals.len() as f64 };
    let max_i = |vals: &[i64]| vals.iter().copied().max().unwrap_or(0);

    let mut sorted_util = util_vals.clone();
    sorted_util.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p95_idx = ((sorted_util.len() as f64 * 0.95) as usize).min(sorted_util.len().saturating_sub(1));
    let p95 = sorted_util.get(p95_idx).copied().unwrap_or(0.0);
    let max_util = util_vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let max_util = if max_util == f64::NEG_INFINITY { 0.0 } else { max_util };

    // Q22 (`eventsub_mixin.py:726–748`)
    let hourly_rows = sqlx::query(
        r#"
        SELECT EXTRACT(HOUR FROM ts_utc AT TIME ZONE 'UTC')::int AS hour,
               CAST(COUNT(*) AS BIGINT) AS samples,
               CAST(AVG(utilization_pct) AS DOUBLE PRECISION) AS avg_utilization_pct,
               CAST(MAX(utilization_pct) AS DOUBLE PRECISION) AS max_utilization_pct,
               CAST(AVG(used_slots) AS DOUBLE PRECISION) AS avg_used_slots,
               MAX(used_slots) AS max_used_slots,
               CAST(AVG(listener_count) AS DOUBLE PRECISION) AS avg_listener_count,
               MAX(listener_count) AS max_listener_count
          FROM twitch_eventsub_capacity_snapshot
         WHERE ts_utc >= NOW() - ($1::interval)
         GROUP BY hour
         ORDER BY hour ASC
        "#,
    )
    .bind(window)
    .fetch_all(pool)
    .await?;

    let hourly: Vec<Value> = hourly_rows.iter().map(|r| {
        json!({
            "hour": row_i32(r, "hour"),
            "samples": row_i64(r, "samples"),
            "avg_utilization_pct": row_f64(r, "avg_utilization_pct"),
            "max_utilization_pct": row_f64(r, "max_utilization_pct"),
            "avg_used_slots": row_f64(r, "avg_used_slots"),
            "max_used_slots": row_i64(r, "max_used_slots"),
            "avg_listener_count": row_f64(r, "avg_listener_count"),
            "max_listener_count": row_i64(r, "max_listener_count"),
        })
    }).collect();

    // Q23 (`eventsub_mixin.py:749–762`)
    let reason_rows = sqlx::query(
        r#"
        SELECT trigger_reason,
               CAST(COUNT(*) AS BIGINT) AS samples,
               CAST(MAX(utilization_pct) AS DOUBLE PRECISION) AS peak_utilization_pct
          FROM twitch_eventsub_capacity_snapshot
         WHERE ts_utc >= NOW() - ($1::interval)
         GROUP BY trigger_reason
         ORDER BY samples DESC, trigger_reason ASC
        "#,
    )
    .bind(window)
    .fetch_all(pool)
    .await?;

    let reasons: Vec<Value> = reason_rows.iter().map(|r| {
        json!({
            "reason": r.try_get::<Option<String>, _>("trigger_reason").ok().flatten().unwrap_or_default(),
            "samples": row_i64(r, "samples"),
            "peak_utilization_pct": row_f64(r, "peak_utilization_pct"),
        })
    }).collect();

    let last_row = snap_rows.last().unwrap();
    Ok(json!({
        "window_hours": 24,
        "samples": snap_rows.len(),
        "last_snapshot_at": last_ts,
        "avg_utilization_pct": (avg_f(&util_vals) * 100.0).round() / 100.0,
        "p95_utilization_pct": (p95 * 100.0).round() / 100.0,
        "max_utilization_pct": (max_util * 100.0).round() / 100.0,
        "avg_used_slots": avg_i(&used_vals),
        "max_used_slots": max_i(&used_vals),
        "avg_listener_count": avg_i(&listener_vals),
        "max_listener_count": max_i(&listener_vals),
        "avg_ready_listeners": avg_i(&ready_vals),
        "max_failed_listeners": max_i(&failed_vals),
        "hourly": hourly,
        "reasons": reasons,
        "active_subscriptions": [],
        "active_subscription_types": [],
        "active_subscription_channels": [],
        "current": {
            "ts_utc": last_ts,
            "listener_count": row_i64(last_row, "listener_count"),
            "ready_listeners": row_i64(last_row, "ready_listeners"),
            "failed_listeners": row_i64(last_row, "failed_listeners"),
            "used_slots": row_i64(last_row, "used_slots"),
            "total_slots": row_i64(last_row, "total_slots"),
            "headroom_slots": row_i64(last_row, "headroom_slots"),
            "listeners_at_limit": row_i64(last_row, "listeners_at_limit"),
            "utilization_pct": row_f64(last_row, "utilization_pct"),
            "subscription_count": 0_i64,
        },
    }))
}

fn merge_eventsub_current(mut db_block: Value, snapshot: Option<EventSubCurrentSnapshot>) -> Value {
    let Some(snap) = snapshot else { return db_block; };
    let ts = format_ts_seconds(snap.ts_utc);
    if let Some(obj) = db_block.as_object_mut() {
        obj["current"] = json!({
            "ts_utc": ts,
            "listener_count": snap.listener_count,
            "ready_listeners": snap.ready_listeners,
            "failed_listeners": snap.failed_listeners,
            "used_slots": snap.used_slots,
            "total_slots": snap.total_slots,
            "headroom_slots": snap.headroom_slots,
            "listeners_at_limit": snap.listeners_at_limit,
            "utilization_pct": snap.utilization_pct,
            "subscription_count": snap.subscription_count,
        });
        obj["active_subscriptions"] = json!(snap.active_subscriptions);
        obj["active_subscription_types"] = json!(snap.active_subscription_types);
        obj["active_subscription_channels"] = json!(snap.active_subscription_channels);
    }
    db_block
}

// ── Haupt-Handler ─────────────────────────────────────────────────────────────

/// `GET /internal/twitch/v1/stats`
///
/// Vertrag: `bot/internal_api/routes/streamers.py:389–409`.
pub async fn stats_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Extension(eventsub_ext): Extension<EventSubStatsExt>,
    Query(params): Query<StatsQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let hf = parse_hour_filter(&params.hour_from, &params.hour_to).map_err(|_| {
        tracing::warn!("internal api stats query bad request (invalid hour params)");
        ApiError::bad_request("invalid query parameters")
    })?;

    let streamer_login = match &params.streamer {
        None => None,
        Some(raw) => {
            let s = raw.trim();
            if s.is_empty() {
                None
            } else {
                match normalize_twitch_login(s) {
                    Some(login) => Some(login),
                    None => {
                        tracing::warn!("internal api stats query bad request (invalid streamer)");
                        return Err(ApiError::bad_request("invalid streamer login"));
                    }
                }
            }
        }
    };

    let result = compute_stats(&pool, &eventsub_ext, &hf, streamer_login.as_deref())
        .await
        .map_err(|e| {
            tracing::error!("internal api stats failed: {e}");
            ApiError::bad_request_with_body(json!({
                "error": "internal_error",
                "message": "failed to fetch stats"
            }))
        })?;

    Ok(Json(result))
}

async fn compute_stats(
    pool: &PgPool,
    eventsub_ext: &EventSubStatsExt,
    hf: &HourFilter,
    streamer_login: Option<&str>,
) -> Result<Value, BoxError> {
    let now = Utc::now();

    let partner_rows = fetch_partner_state(pool).await.map_err(|e| {
        tracing::error!("Konnte gespeicherte Twitch-Logins nicht laden: {e}");
        Box::new(e) as BoxError
    })?;
    let maps = build_partner_maps(partner_rows, now);

    let tracked_top_raw = fetch_top(pool, "twitch_stats_tracked", true, hf).await?;
    let tracked_hourly_raw = fetch_hourly(pool, "twitch_stats_tracked", true, hf).await?;
    let tracked_weekday_raw = fetch_weekday(pool, "twitch_stats_tracked", true, hf).await?;
    let category_top_raw = fetch_top(pool, "twitch_stats_category", false, hf).await?;
    let category_hourly_raw = fetch_hourly(pool, "twitch_stats_category", false, hf).await?;
    let category_weekday_raw = fetch_weekday(pool, "twitch_stats_category", false, hf).await?;

    let tracked_top: Vec<StreamerEntry> = tracked_top_raw.into_iter().map(|r| enrich_top_row(r, &maps)).collect();
    let category_top: Vec<StreamerEntry> = category_top_raw.into_iter().map(|r| enrich_top_row(r, &maps)).collect();

    let avg_viewers_tracked = if tracked_top.is_empty() { 0.0_f64 } else {
        tracked_top.iter().map(|s| s.avg_viewers).sum::<f64>() / tracked_top.len() as f64
    };
    let avg_viewers_all = if category_top.is_empty() { 0.0_f64 } else {
        category_top.iter().map(|s| s.avg_viewers).sum::<f64>() / category_top.len() as f64
    };

    let tracked_samples: i64 = tracked_top.iter().map(|s| s.samples).sum();
    let tracked_unique = tracked_top.len() as i64;
    let category_samples: i64 = category_top.iter().map(|s| s.samples).sum();
    let category_unique = category_top.len() as i64;

    let tracked_hourly = map_hourly(tracked_hourly_raw);
    let tracked_weekday = map_weekday(tracked_weekday_raw);
    let category_hourly = map_hourly(category_hourly_raw);
    let category_weekday = map_weekday(category_weekday_raw);

    let mut out = json!({
        "tracked": {
            "top": tracked_top,
            "hourly": tracked_hourly,
            "weekday": tracked_weekday,
            "samples": tracked_samples,
            "unique_streamers": tracked_unique,
        },
        "category": {
            "top": category_top,
            "hourly": category_hourly,
            "weekday": category_weekday,
            "samples": category_samples,
            "unique_streamers": category_unique,
        },
        "avg_viewers_all": avg_viewers_all,
        "avg_viewers_tracked": avg_viewers_tracked,
    });

    if let Some(login) = streamer_login {
        let streamer_block = compute_streamer_block(pool, login, hf, &maps).await;
        if let Some(obj) = out.as_object_mut() {
            obj.insert("streamer".to_string(), streamer_block);
        }
    }

    // Monetization (exception-safe)
    match fetch_monetization(pool).await {
        Err(e) => { tracing::debug!("Konnte Monetization-Stats nicht laden: {e}"); }
        Ok(mono) => { if let Some(obj) = out.as_object_mut() { obj.insert("monetization".to_string(), mono); } }
    }

    // EventSub-Capacity (exception-safe)
    match fetch_eventsub_capacity_db(pool).await {
        Err(e) => { tracing::debug!("Konnte EventSub-Capacity-Overview nicht laden: {e}"); }
        Ok(db_block) => {
            let snapshot = match &eventsub_ext.0 {
                Some(src) => src.get_snapshot().await,
                None => None,
            };
            let merged = merge_eventsub_current(db_block, snapshot);
            if let Some(obj) = out.as_object_mut() { obj.insert("eventsub".to_string(), merged); }
        }
    }

    Ok(out)
}

async fn compute_streamer_block(
    pool: &PgPool,
    login: &str,
    hf: &HourFilter,
    maps: &PartnerMaps,
) -> Value {
    let is_tracked = maps.tracked_logins.contains(login);

    let (discord_user_id, discord_display_name, raw_is_on_discord) =
        maps.discord_info.get(login).cloned().unwrap_or((None, None, false));
    let has_profile = discord_user_id.as_deref().map(|s| !s.is_empty()).unwrap_or(false)
        || discord_display_name.as_deref().map(|s| !s.is_empty()).unwrap_or(false);
    let is_on_discord: i32 = if raw_is_on_discord || has_profile || maps.verified_logins.contains(login) { 1 } else { 0 };

    let mut summary = json!({});
    let mut hourly = Vec::new();
    let mut weekday = Vec::new();
    let mut source: Option<String> = None;
    let mut had_results = false;

    for src_key in &["tracked", "category"] {
        let (table, is_tracked) = if *src_key == "tracked" {
            ("twitch_stats_tracked", true)
        } else {
            ("twitch_stats_category", false)
        };
        match fetch_user_top(pool, table, is_tracked, login, hf).await {
            Ok(rows) if !rows.is_empty() && rows[0].samples > 0 => {
                let entry = enrich_top_row(rows.into_iter().next().unwrap(), maps);
                summary = json!({
                    "streamer": entry.streamer,
                    "avg_viewers": entry.avg_viewers,
                    "max_viewers": entry.max_viewers,
                    "samples": entry.samples,
                    "is_partner": entry.is_partner,
                    "is_on_discord": entry.is_on_discord,
                    "discord_user_id": entry.discord_user_id,
                    "discord_display_name": entry.discord_display_name,
                    "has_discord_profile": entry.has_discord_profile,
                });
                source = Some(src_key.to_string());
                had_results = true;
                if let Ok(hr) = fetch_user_hourly(pool, table, is_tracked, login, hf).await {
                    hourly = map_hourly(hr);
                }
                if let Ok(wd) = fetch_user_weekday(pool, table, is_tracked, login, hf).await {
                    weekday = map_weekday(wd);
                }
                break;
            }
            _ => continue,
        }
    }

    let display_login = summary.get("streamer").and_then(|v| v.as_str()).unwrap_or(login).to_string();

    let mut block = json!({
        "login": login,
        "display_login": display_login,
        "summary": summary,
        "hourly": hourly,
        "weekday": weekday,
        "source": source,
        "had_results": had_results,
        "is_tracked": is_tracked,
        "discord_user_id": discord_user_id,
        "discord_display_name": discord_display_name,
        "is_on_discord": is_on_discord,
    });

    match fetch_streamer_subs_and_audience(pool, login).await {
        Err(e) => { tracing::error!("Failed to fetch extended user stats (subs/shared): {e}"); }
        Ok((subs, shared)) => {
            if let Some(obj) = block.as_object_mut() {
                if let Some(s) = subs { obj.insert("subs".to_string(), s); }
                obj.insert("shared_audience".to_string(), json!(shared));
            }
        }
    }

    block
}

async fn fetch_streamer_subs_and_audience(
    pool: &PgPool,
    login: &str,
) -> Result<(Option<Value>, Vec<Value>), BoxError> {
    // Q8 (`leaderboard.py:963–967`)
    let sub_row = sqlx::query(
        r#"
        SELECT total, points, snapshot_at
          FROM twitch_subscriptions_snapshot
         WHERE twitch_login = $1
         ORDER BY snapshot_at DESC
         LIMIT 1
        "#,
    )
    .bind(login)
    .fetch_optional(pool)
    .await?;

    let subs = sub_row.as_ref().map(|r| {
        // total/points/updated_at direkt aus DB, kein Cast (Vertrag-UNSICHER)
        let total: Value = r.try_get::<i64, _>("total").map(|v| json!(v))
            .or_else(|_| r.try_get::<i32, _>("total").map(|v| json!(v)))
            .unwrap_or(Value::Null);
        let points: Value = r.try_get::<i64, _>("points").map(|v| json!(v))
            .or_else(|_| r.try_get::<i32, _>("points").map(|v| json!(v)))
            .unwrap_or(Value::Null);
        // snapshot_at ist TIMESTAMPTZ in Prod (Python serialisiert via
        // isoformat); String-Fallback für ältere Schemata.
        let updated_at: Value = r
            .try_get::<Option<DateTime<Utc>>, _>("snapshot_at")
            .ok()
            .flatten()
            .map(|dt| json!(format_ts_python(dt)))
            .or_else(|| {
                r.try_get::<Option<String>, _>("snapshot_at")
                    .ok()
                    .flatten()
                    .map(|s| json!(s))
            })
            .unwrap_or(Value::Null);
        json!({ "total": total, "points": points, "updated_at": updated_at })
    });

    // Q9 (`leaderboard.py:975–987`)
    let audience_rows = sqlx::query(
        r#"
        SELECT other.streamer_login, CAST(COUNT(DISTINCT t1.chatter_login) AS BIGINT) as overlap
        FROM twitch_chatter_rollup t1
        JOIN twitch_chatter_rollup other ON t1.chatter_login = other.chatter_login
        WHERE t1.streamer_login = $1
          AND other.streamer_login != $2
          AND t1.last_seen_at >= NOW() - INTERVAL '30 days'
        GROUP BY other.streamer_login
        ORDER BY overlap DESC
        LIMIT 10
        "#,
    )
    .bind(login)
    .bind(login)
    .fetch_all(pool)
    .await?;

    let shared: Vec<Value> = audience_rows.iter().map(|r| {
        json!({
            "streamer": r.try_get::<String, _>("streamer_login").unwrap_or_default(),
            "overlap": row_i64(r, "overlap"),
        })
    }).collect();

    Ok((subs, shared))
}

// ── B13-1: Extended-Stats (Dashboard-Leaderboard) ──────────────────────────────
//
// Vier Zusatz-Sektionen aus `leaderboard.py:_compute_stats` (Zeilen 1024–1256):
// `retention`, `chat`, `discovery`, `content_performance`. Sie leben bewusst
// NICHT im `/stats`-Vertrag (der den Python-Internal-API-Vertrag 1:1 spiegelt),
// sondern unter `GET /stats/extended` als Datenquelle für das neue Web-Dashboard-
// Leaderboard (Grillme-Block-13: „Leaderboard sauber neu im Dashboard").
//
// Python-Parität: Der gesamte Block steht in einem try/except — schlägt eine
// Query fehl, fehlen ALLE vier Sektionen (graceful-degrade). Hier abgebildet,
// indem ein Query-Fehler propagiert wird; der Handler fängt ihn ab.

/// Durchschnitt einer Werteliste, `None` bei leerer Liste (Python `_avg`).
fn avg_or_none(values: &[f64]) -> Value {
    if values.is_empty() {
        Value::Null
    } else {
        json!(values.iter().sum::<f64>() / values.len() as f64)
    }
}

/// `started_at` (TIMESTAMPTZ) als Python-isoformat-String; `null` wenn leer.
fn session_started_at(row: &sqlx::postgres::PgRow) -> Value {
    row_ts_opt(row, "started_at")
        .map(|dt| json!(format_ts_python(dt)))
        .unwrap_or(Value::Null)
}

/// `GET /internal/twitch/v1/stats/extended` — die vier Dashboard-Leaderboard-
/// Sektionen (`retention`/`chat`/`discovery`/`content_performance`).
pub async fn extended_stats_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    // Python-Parität: bei DB-Fehler bleiben die Sektionen schlicht leer.
    // Wir liefern dann ein leeres Objekt statt 500 (graceful-degrade).
    let result = compute_extended_stats(&pool).await.unwrap_or_else(|e| {
        tracing::debug!("Konnte erweiterte Twitch-Metriken nicht berechnen: {e}");
        json!({})
    });

    Ok(Json(result))
}

/// Berechnet die vier Extended-Stats-Sektionen aus `twitch_stream_sessions`,
/// `twitch_chat_messages` und `twitch_chatter_rollup`.
async fn compute_extended_stats(pool: &PgPool) -> Result<Value, BoxError> {
    // 30-Tage-Fenster, max. 400 abgeschlossene Sessions (Python LIMIT 400).
    let session_rows = sqlx::query(
        r#"
        SELECT id, streamer_login, started_at, duration_seconds, start_viewers, peak_viewers,
               end_viewers, avg_viewers, samples, retention_5m, retention_10m, retention_20m,
               dropoff_pct, dropoff_label, unique_chatters, first_time_chatters,
               returning_chatters, follower_delta, followers_start, followers_end,
               stream_title, notification_text
          FROM twitch_stream_sessions
         WHERE started_at >= NOW() - INTERVAL '30 days'
           AND ended_at IS NOT NULL
         ORDER BY started_at DESC
         LIMIT 400
        "#,
    )
    .fetch_all(pool)
    .await?;

    let chat_peak_rows = sqlx::query(
        r#"
        SELECT cm.session_id,
               s.streamer_login,
               s.started_at,
               TO_CHAR(
                   date_trunc('minute', cm.message_ts AT TIME ZONE 'UTC'),
                   'YYYY-MM-DD"T"HH24:MI:00"Z"'
               ) AS minute_bucket,
               COUNT(*) AS messages
          FROM twitch_chat_messages cm
          JOIN twitch_stream_sessions s ON s.id = cm.session_id
         WHERE cm.message_ts >= NOW() - INTERVAL '30 days'
         GROUP BY cm.session_id, s.streamer_login, s.started_at, minute_bucket
         ORDER BY messages DESC
         LIMIT 5
        "#,
    )
    .fetch_all(pool)
    .await?;

    let count_since = |interval: &str| -> String {
        format!(
            "SELECT CAST(COUNT(*) AS BIGINT) FROM twitch_chatter_rollup \
             WHERE last_seen_at >= NOW() - INTERVAL '{interval}'"
        )
    };
    let returning_since = |interval: &str| -> String {
        format!(
            "SELECT CAST(COUNT(*) AS BIGINT) FROM twitch_chatter_rollup \
             WHERE first_seen_at < NOW() - INTERVAL '{interval}' \
               AND last_seen_at >= NOW() - INTERVAL '{interval}'"
        )
    };
    let active_7: i64 = sqlx::query_scalar(&count_since("7 days")).fetch_one(pool).await?;
    let returning_7: i64 = sqlx::query_scalar(&returning_since("7 days")).fetch_one(pool).await?;
    let active_30: i64 = sqlx::query_scalar(&count_since("30 days")).fetch_one(pool).await?;
    let returning_30: i64 = sqlx::query_scalar(&returning_since("30 days")).fetch_one(pool).await?;

    let sessions_count = session_rows.len() as i64;

    // ── Retention ──────────────────────────────────────────────────────────────
    let collect_opt = |col: &str| -> Vec<f64> {
        session_rows
            .iter()
            .filter_map(|r| r.try_get::<Option<f64>, _>(col).ok().flatten())
            .collect()
    };
    let ret5 = collect_opt("retention_5m");
    let ret10 = collect_opt("retention_10m");
    let ret20 = collect_opt("retention_20m");
    let drops = collect_opt("dropoff_pct");

    let mut dropoff_examples: Vec<(f64, Value)> = session_rows
        .iter()
        .filter_map(|r| {
            r.try_get::<Option<f64>, _>("dropoff_pct").ok().flatten().map(|pct| {
                (
                    pct,
                    json!({
                        "streamer": row_str_opt(r, "streamer_login").unwrap_or_default(),
                        "started_at": session_started_at(r),
                        "dropoff_pct": pct,
                        "label": row_str_opt(r, "dropoff_label").unwrap_or_default(),
                        "start_viewers": row_i64(r, "start_viewers"),
                        "peak_viewers": row_i64(r, "peak_viewers"),
                    }),
                )
            })
        })
        .collect();
    dropoff_examples.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let examples: Vec<Value> = dropoff_examples.into_iter().take(5).map(|(_, v)| v).collect();

    let retention = json!({
        "sessions": sessions_count,
        "ret5": avg_or_none(&ret5),
        "ret10": avg_or_none(&ret10),
        "ret20": avg_or_none(&ret20),
        "avg_drop": avg_or_none(&drops),
        "examples": examples,
    });

    // ── Discovery ──────────────────────────────────────────────────────────────
    let peak_vals: Vec<f64> = session_rows.iter().map(|r| row_i64(r, "peak_viewers") as f64).collect();
    let follower_deltas: Vec<f64> = session_rows
        .iter()
        .filter_map(|r| r.try_get::<Option<i32>, _>("follower_delta").ok().flatten().map(|v| v as f64))
        .collect();
    let follower_per_hour: Vec<f64> = session_rows
        .iter()
        .filter_map(|r| {
            let delta = r.try_get::<Option<i32>, _>("follower_delta").ok().flatten()?;
            let duration = row_i64(r, "duration_seconds");
            if duration == 0 {
                return None;
            }
            let hours = (duration as f64 / 3600.0).max(0.1);
            Some(delta as f64 / hours)
        })
        .collect();
    let followers_total_delta: i64 = follower_deltas.iter().map(|v| *v as i64).sum();

    let discovery = json!({
        "sessions": sessions_count,
        "unique_viewers_estimate": avg_or_none(&peak_vals),
        "followers_total_delta": followers_total_delta,
        "followers_per_session": avg_or_none(&follower_deltas),
        "followers_per_hour": avg_or_none(&follower_per_hour),
        "returning_7d": { "total": active_7, "returning": returning_7 },
        "returning_30d": { "total": active_30, "returning": returning_30 },
    });

    // ── Chat ───────────────────────────────────────────────────────────────────
    let unique_sessions: Vec<&sqlx::postgres::PgRow> =
        session_rows.iter().filter(|r| row_i64(r, "unique_chatters") > 0).collect();
    let total_unique: i64 = unique_sessions.iter().map(|r| row_i64(r, "unique_chatters")).sum();
    let total_first: i64 = unique_sessions.iter().map(|r| row_i64(r, "first_time_chatters")).sum();
    let total_returning: i64 = unique_sessions.iter().map(|r| row_i64(r, "returning_chatters")).sum();

    let unique_per_100 = if unique_sessions.is_empty() {
        Value::Null
    } else {
        let ratios: Vec<f64> = unique_sessions
            .iter()
            .map(|r| {
                let avg = row_f64(r, "avg_viewers");
                let base = if avg > 0.0 { avg } else { row_i64(r, "start_viewers") as f64 };
                (row_i64(r, "unique_chatters") as f64 / base.max(1.0)) * 100.0
            })
            .collect();
        avg_or_none(&ratios)
    };

    let chat_peaks: Vec<Value> = chat_peak_rows
        .iter()
        .map(|r| {
            json!({
                "session_id": row_i64(r, "session_id"),
                "streamer": row_str_opt(r, "streamer_login").unwrap_or_default(),
                "minute": row_str_opt(r, "minute_bucket").unwrap_or_default(),
                "messages": row_i64(r, "messages"),
                "started_at": session_started_at(r),
            })
        })
        .collect();

    let share = |part: i64| -> Value {
        if total_unique > 0 {
            json!(part as f64 / total_unique as f64)
        } else {
            Value::Null
        }
    };
    let chat = json!({
        "unique_per_100": unique_per_100,
        "first_share": share(total_first),
        "returning_share": share(total_returning),
        "peaks": chat_peaks,
        "total_unique": total_unique,
    });

    // ── Content-Performance (Top 20 nach peak_viewers) ──────────────────────────
    let mut content: Vec<(i64, Value)> = session_rows
        .iter()
        .filter_map(|r| {
            let title = row_str_opt(r, "stream_title").unwrap_or_default();
            let notify = row_str_opt(r, "notification_text").unwrap_or_default();
            if title.is_empty() && notify.is_empty() {
                return None;
            }
            let followers_start = row_i64(r, "followers_start");
            let peak = row_i64(r, "peak_viewers");
            let engagement_ratio = if followers_start > 0 {
                json!((peak as f64 / followers_start as f64) * 100.0)
            } else {
                Value::Null
            };
            Some((
                peak,
                json!({
                    "streamer": row_str_opt(r, "streamer_login").unwrap_or_default(),
                    "started_at": session_started_at(r),
                    "title": title,
                    "notification": notify,
                    "peak_viewers": peak,
                    "avg_viewers": row_f64(r, "avg_viewers"),
                    "followers_start": followers_start,
                    "engagement_ratio": engagement_ratio,
                }),
            ))
        })
        .collect();
    content.sort_by_key(|(peak, _)| std::cmp::Reverse(*peak));
    let content_performance: Vec<Value> = content.into_iter().take(20).map(|(_, v)| v).collect();

    Ok(json!({
        "retention": retention,
        "chat": chat,
        "discovery": discovery,
        "content_performance": content_performance,
    }))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Unit-Tests: Timestamp-Helfer ──────────────────────────────────────────

    #[test]
    fn format_ts_python_ohne_mikrosekunden() {
        use chrono::TimeZone;
        let dt = Utc.with_ymd_and_hms(2026, 6, 12, 14, 30, 0).unwrap();
        assert_eq!(format_ts_python(dt), "2026-06-12T14:30:00+00:00");
    }

    #[test]
    fn format_ts_python_mit_mikrosekunden() {
        use chrono::TimeZone;
        let dt = Utc.with_ymd_and_hms(2026, 6, 12, 14, 30, 0).unwrap()
            + chrono::Duration::microseconds(123456);
        let s = format_ts_python(dt);
        assert!(s.ends_with("+00:00"), "Muss +00:00-Suffix haben: {s}");
        assert!(s.contains(".123456"), "Muss Mikrosekunden enthalten: {s}");
    }

    #[test]
    fn format_ts_seconds_immer_ohne_mikrosekunden() {
        use chrono::TimeZone;
        let dt = Utc.with_ymd_and_hms(2026, 6, 12, 14, 30, 0).unwrap()
            + chrono::Duration::microseconds(999999);
        assert_eq!(format_ts_seconds(dt), "2026-06-12T14:30:00+00:00");
    }

    // ── Unit-Tests: Stunden-Filter ────────────────────────────────────────────

    #[test]
    fn hour_filter_beide_none_ergibt_none() {
        let hf = parse_hour_filter(&None, &None).unwrap();
        assert_eq!(hf.mode_str(), "none");
    }

    #[test]
    fn hour_filter_between_wenn_start_kleiner_end() {
        let hf = parse_hour_filter(&Some("8".into()), &Some("20".into())).unwrap();
        assert_eq!(hf.mode_str(), "between");
        assert_eq!(hf.start(), 8);
        assert_eq!(hf.end(), 20);
    }

    #[test]
    fn hour_filter_wrap_wenn_start_groesser_end() {
        let hf = parse_hour_filter(&Some("22".into()), &Some("4".into())).unwrap();
        assert_eq!(hf.mode_str(), "wrap");
        assert_eq!(hf.start(), 22);
        assert_eq!(hf.end(), 4);
    }

    #[test]
    fn hour_filter_nur_from_setzt_beide_gleich() {
        let hf = parse_hour_filter(&Some("10".into()), &None).unwrap();
        assert_eq!(hf.mode_str(), "between");
        assert_eq!(hf.start(), 10);
        assert_eq!(hf.end(), 10);
    }

    #[test]
    fn hour_filter_clampt_auf_0_bis_23() {
        let hf = parse_hour_filter(&Some("30".into()), &Some("99".into())).unwrap();
        assert_eq!(hf.start(), 23);
        assert_eq!(hf.end(), 23);
    }

    #[test]
    fn hour_filter_negativ_gibt_err() {
        assert!(parse_hour_filter(&Some("-1".into()), &None).is_err());
    }

    #[test]
    fn hour_filter_nicht_zahl_gibt_err() {
        assert!(parse_hour_filter(&Some("abc".into()), &None).is_err());
    }

    // ── Unit-Tests: Partner-Maps ──────────────────────────────────────────────

    #[test]
    fn partner_maps_tracked_und_verified() {
        use chrono::TimeZone;
        let now = Utc.with_ymd_and_hms(2026, 6, 12, 12, 0, 0).unwrap();
        let rows = vec![
            PartnerStateRow {
                twitch_login: "Helmi".into(),
                is_on_discord: Some(1),
                discord_user_id: Some("123".into()),
                discord_display_name: Some("HelmiDC".into()),
                manual_verified_permanent: Some(1),
                manual_verified_until: None,
            },
            PartnerStateRow {
                twitch_login: "DragScope".into(),
                is_on_discord: Some(0),
                discord_user_id: None,
                discord_display_name: None,
                manual_verified_permanent: Some(0),
                manual_verified_until: Some((now - chrono::Duration::hours(1)).format("%Y-%m-%dT%H:%M:%S%.6f+00:00").to_string()),
            },
        ];
        let maps = build_partner_maps(rows, now);
        assert!(maps.tracked_logins.contains("helmi"));
        assert!(maps.tracked_logins.contains("dragscope"));
        assert!(maps.verified_logins.contains("helmi"));
        assert!(!maps.verified_logins.contains("dragscope"));
    }

    #[test]
    fn enrich_top_row_ueberschreibt_is_partner_wenn_tracked_nicht_verified() {
        use chrono::TimeZone;
        let now = Utc.with_ymd_and_hms(2026, 6, 12, 12, 0, 0).unwrap();
        let rows = vec![PartnerStateRow {
            twitch_login: "testuser".into(), is_on_discord: None,
            discord_user_id: None, discord_display_name: None,
            manual_verified_permanent: Some(0), manual_verified_until: None,
        }];
        let maps = build_partner_maps(rows, now);
        let top = TopRow { streamer: "testuser".into(), avg_viewers: 100.0, max_viewers: 200, samples: 50, is_partner: 1 };
        let entry = enrich_top_row(top, &maps);
        assert_eq!(entry.is_partner, 0, "Nicht-verified → is_partner=0");
    }

    #[test]
    fn enrich_top_row_is_partner_1_wenn_verified() {
        use chrono::TimeZone;
        let now = Utc.with_ymd_and_hms(2026, 6, 12, 12, 0, 0).unwrap();
        let rows = vec![PartnerStateRow {
            twitch_login: "verifieduser".into(), is_on_discord: None,
            discord_user_id: None, discord_display_name: None,
            manual_verified_permanent: Some(1), manual_verified_until: None,
        }];
        let maps = build_partner_maps(rows, now);
        let top = TopRow { streamer: "verifieduser".into(), avg_viewers: 100.0, max_viewers: 200, samples: 50, is_partner: 0 };
        let entry = enrich_top_row(top, &maps);
        assert_eq!(entry.is_partner, 1, "Verified → is_partner=1");
    }

    #[test]
    fn enrich_top_row_is_on_discord_durch_verified() {
        use chrono::TimeZone;
        let now = Utc.with_ymd_and_hms(2026, 6, 12, 12, 0, 0).unwrap();
        let rows = vec![PartnerStateRow {
            twitch_login: "streamer1".into(), is_on_discord: Some(0),
            discord_user_id: None, discord_display_name: None,
            manual_verified_permanent: Some(1), manual_verified_until: None,
        }];
        let maps = build_partner_maps(rows, now);
        let top = TopRow { streamer: "streamer1".into(), avg_viewers: 50.0, max_viewers: 100, samples: 10, is_partner: 0 };
        let entry = enrich_top_row(top, &maps);
        assert_eq!(entry.is_on_discord, 1, "Verified → is_on_discord=1");
    }

    // ── Unit-Tests: EventSub-Shape ────────────────────────────────────────────

    #[test]
    fn eventsub_stats_ext_none_ist_klonbar() {
        let ext = EventSubStatsExt(None);
        let _ = ext.clone();
    }

    #[test]
    fn merge_eventsub_current_ohne_snapshot_unveraendert() {
        let block = json!({ "active_subscriptions": [], "current": {"ts_utc": Value::Null} });
        let merged = merge_eventsub_current(block.clone(), None);
        assert_eq!(merged, block);
    }

    #[test]
    fn merge_eventsub_current_mit_snapshot_ueberschreibt_current() {
        use chrono::TimeZone;
        let block = json!({
            "active_subscriptions": [], "active_subscription_types": [], "active_subscription_channels": [],
            "current": {"ts_utc": Value::Null, "subscription_count": 0},
        });
        let dt = Utc.with_ymd_and_hms(2026, 6, 12, 10, 0, 0).unwrap();
        let snap = EventSubCurrentSnapshot {
            ts_utc: dt, listener_count: 5, ready_listeners: 4, failed_listeners: 1,
            used_slots: 10, total_slots: 100, headroom_slots: 90, listeners_at_limit: 0,
            utilization_pct: 0.1, subscription_count: 42,
            active_subscriptions: vec![json!({"id": "sub1"})],
            active_subscription_types: vec![json!("channel.follow")],
            active_subscription_channels: vec![json!("helmi")],
        };
        let merged = merge_eventsub_current(block, Some(snap));
        assert_eq!(merged["current"]["subscription_count"], 42);
        assert_eq!(merged["active_subscriptions"][0]["id"], "sub1");
        assert_eq!(merged["current"]["ts_utc"], "2026-06-12T10:00:00+00:00");
    }

    // ── DB-Tests ──────────────────────────────────────────────────────────────

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    macro_rules! db_dsn_or_skip {
        () => {
            match test_dsn() {
                Some(d) => d,
                None => {
                    if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                        panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                    }
                    eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                    return;
                }
            }
        };
    }

    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
        use std::str::FromStr;

        // Erst schema anlegen (braucht public-scope-Verbindung ohne after_connect)
        {
            let setup = PgPoolOptions::new().max_connections(1).connect(dsn).await.expect("connect setup");
            sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&setup).await.expect("drop schema");
            sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&setup).await.expect("create schema");
        }

        // Pool mit after_connect → search_path auf isoliertes Schema für ALLE Verbindungen
        let schema_owned = schema.to_string();
        let opts = PgConnectOptions::from_str(dsn).expect("parse dsn");
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .after_connect(move |conn, _meta| {
                let s = schema_owned.clone();
                Box::pin(async move {
                    sqlx::query(&format!("SET search_path TO {s}"))
                        .execute(&mut *conn)
                        .await
                        .map(|_| ())
                })
            })
            .connect_with(opts)
            .await
            .expect("connect pool");

        sqlx::query(r#"CREATE TABLE twitch_streamers_partner_state (
            twitch_login TEXT NOT NULL PRIMARY KEY,
            is_partner_active INTEGER NOT NULL DEFAULT 0,
            is_on_discord INTEGER,
            discord_user_id TEXT, discord_display_name TEXT,
            manual_verified_permanent INTEGER DEFAULT 0,
            manual_verified_until TEXT
        )"#).execute(&pool).await.expect("DDL partner_state");

        sqlx::query(r#"CREATE TABLE twitch_stats_tracked (
            id BIGSERIAL PRIMARY KEY, streamer TEXT NOT NULL,
            viewer_count INTEGER NOT NULL DEFAULT 0,
            is_partner BOOLEAN NOT NULL DEFAULT FALSE, ts_utc TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )"#).execute(&pool).await.expect("DDL stats_tracked");

        sqlx::query(r#"CREATE TABLE twitch_stats_category (
            id BIGSERIAL PRIMARY KEY, streamer TEXT NOT NULL,
            viewer_count INTEGER NOT NULL DEFAULT 0,
            is_partner BOOLEAN NOT NULL DEFAULT FALSE, ts_utc TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )"#).execute(&pool).await.expect("DDL stats_category");

        sqlx::query(r#"CREATE TABLE twitch_stream_sessions (
            id BIGSERIAL PRIMARY KEY, streamer_login TEXT NOT NULL,
            started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ,
            duration_seconds INTEGER, start_viewers INTEGER, peak_viewers INTEGER,
            end_viewers INTEGER, avg_viewers DOUBLE PRECISION, samples INTEGER,
            retention_5m DOUBLE PRECISION, retention_10m DOUBLE PRECISION, retention_20m DOUBLE PRECISION,
            dropoff_pct DOUBLE PRECISION, dropoff_label TEXT,
            unique_chatters INTEGER, first_time_chatters INTEGER, returning_chatters INTEGER,
            follower_delta INTEGER, followers_start INTEGER, followers_end INTEGER,
            stream_title TEXT, notification_text TEXT
        )"#).execute(&pool).await.expect("DDL stream_sessions");

        sqlx::query(r#"CREATE TABLE twitch_chatter_rollup (
            id BIGSERIAL PRIMARY KEY, chatter_login TEXT NOT NULL,
            streamer_login TEXT NOT NULL, first_seen_at TIMESTAMPTZ, last_seen_at TIMESTAMPTZ
        )"#).execute(&pool).await.expect("DDL chatter_rollup");

        sqlx::query(r#"CREATE TABLE twitch_chat_messages (
            id BIGSERIAL PRIMARY KEY, session_id BIGINT, message_ts TIMESTAMPTZ
        )"#).execute(&pool).await.expect("DDL chat_messages");

        sqlx::query(r#"CREATE TABLE twitch_eventsub_capacity_snapshot (
            id BIGSERIAL PRIMARY KEY, ts_utc TIMESTAMPTZ, trigger_reason TEXT,
            listener_count INTEGER, ready_listeners INTEGER, failed_listeners INTEGER,
            used_slots INTEGER, total_slots INTEGER, headroom_slots INTEGER,
            listeners_at_limit INTEGER, utilization_pct DOUBLE PRECISION
        )"#).execute(&pool).await.expect("DDL eventsub_snapshot");

        sqlx::query(r#"CREATE TABLE twitch_ad_break_events (
            id BIGSERIAL PRIMARY KEY, session_id BIGINT, started_at TIMESTAMPTZ,
            duration_seconds INTEGER, is_automatic BOOLEAN
        )"#).execute(&pool).await.expect("DDL ad_break");

        sqlx::query(r#"CREATE TABLE twitch_session_viewers (
            session_id BIGINT, minutes_from_start INTEGER, viewer_count INTEGER
        )"#).execute(&pool).await.expect("DDL session_viewers");

        sqlx::query(r#"CREATE TABLE twitch_hype_train_events (
            id BIGSERIAL PRIMARY KEY, started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ, level INTEGER,
            duration_seconds INTEGER
        )"#).execute(&pool).await.expect("DDL hype_train");

        sqlx::query(r#"CREATE TABLE twitch_bits_events (
            id BIGSERIAL PRIMARY KEY, amount INTEGER, received_at TIMESTAMPTZ
        )"#).execute(&pool).await.expect("DDL bits");

        sqlx::query(r#"CREATE TABLE twitch_subscription_events (
            id BIGSERIAL PRIMARY KEY, is_gift BOOLEAN, received_at TIMESTAMPTZ
        )"#).execute(&pool).await.expect("DDL subscription_events");

        sqlx::query(r#"CREATE TABLE twitch_subscriptions_snapshot (
            id BIGSERIAL PRIMARY KEY, twitch_login TEXT, total INTEGER, points INTEGER, snapshot_at TIMESTAMPTZ
        )"#).execute(&pool).await.expect("DDL subscriptions_snapshot");

        pool
    }

    #[tokio::test]
    async fn stats_leer_liefert_tracked_und_category_mit_leeren_listen() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_stats_leer").await;
        let hf = HourFilter::None;
        let ext = EventSubStatsExt(None);
        let result = compute_stats(&pool, &ext, &hf, None).await.expect("compute_stats darf nicht fehlschlagen");

        assert!(result.get("tracked").is_some());
        assert!(result.get("category").is_some());
        assert_eq!(result["tracked"]["top"], json!([]));
        assert_eq!(result["tracked"]["samples"], 0);
        assert_eq!(result["category"]["top"], json!([]));
    }

    #[tokio::test]
    async fn stats_mit_daten_liefert_korrekte_aggregation() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_stats_daten").await;

        sqlx::query("INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active, manual_verified_permanent) VALUES ('helmi', 1, 1), ('dragscope', 1, 0)")
            .execute(&pool).await.unwrap();

        sqlx::query("INSERT INTO twitch_stats_tracked (streamer, viewer_count, is_partner, ts_utc) VALUES ('helmi', 100, TRUE, NOW()), ('helmi', 200, TRUE, NOW()), ('dragscope', 50, FALSE, NOW())")
            .execute(&pool).await.unwrap();

        let hf = HourFilter::None;
        let ext = EventSubStatsExt(None);
        let result = compute_stats(&pool, &ext, &hf, None).await.expect("compute_stats");

        let top = result["tracked"]["top"].as_array().unwrap();
        assert!(!top.is_empty(), "Top muss Einträge haben");

        let helmi = top.iter().find(|e| e["streamer"] == "helmi");
        assert!(helmi.is_some(), "helmi muss in top sein");
        assert_eq!(helmi.unwrap()["is_partner"], 1);

        let drag = top.iter().find(|e| e["streamer"] == "dragscope");
        assert!(drag.is_some());
        assert_eq!(drag.unwrap()["is_partner"], 0);
    }

    #[tokio::test]
    async fn stats_streamer_param_liefert_streamer_block() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_stats_streamer").await;

        sqlx::query("INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active) VALUES ('helmi', 1)")
            .execute(&pool).await.unwrap();

        sqlx::query("INSERT INTO twitch_stats_tracked (streamer, viewer_count, is_partner, ts_utc) VALUES ('helmi', 120, TRUE, NOW())")
            .execute(&pool).await.unwrap();

        let hf = HourFilter::None;
        let ext = EventSubStatsExt(None);
        let result = compute_stats(&pool, &ext, &hf, Some("helmi")).await.unwrap();

        let streamer = result.get("streamer").expect("streamer-Block muss vorhanden sein");
        assert_eq!(streamer["login"], "helmi");
        assert!(streamer.get("had_results").is_some());
    }

    #[tokio::test]
    async fn eventsub_db_leer_liefert_korrekte_shape() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_stats_eventsub_leer").await;
        let result = fetch_eventsub_capacity_db(&pool).await.expect("fetch_eventsub_capacity_db");

        assert_eq!(result["window_hours"], 24);
        assert_eq!(result["samples"], 0);
        assert_eq!(result["active_subscriptions"], json!([]));
        assert_eq!(result["current"]["subscription_count"], 0);
    }

    #[tokio::test]
    async fn eventsub_db_mit_daten_liefert_aggregation() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_stats_eventsub_data").await;

        sqlx::query("INSERT INTO twitch_eventsub_capacity_snapshot (ts_utc, trigger_reason, listener_count, ready_listeners, failed_listeners, used_slots, total_slots, headroom_slots, listeners_at_limit, utilization_pct) VALUES (NOW(), 'test', 10, 8, 2, 50, 500, 450, 0, 0.1), (NOW() - INTERVAL '1 hour', 'periodic', 12, 10, 2, 60, 500, 440, 0, 0.12)")
            .execute(&pool).await.unwrap();

        let result = fetch_eventsub_capacity_db(&pool).await.unwrap();
        assert!(result["samples"].as_i64().unwrap_or(0) > 0);
        assert!(result["last_snapshot_at"].as_str().is_some());
        assert!(!result["reasons"].as_array().unwrap().is_empty());
    }

    /// Exakte Top-Level-Key-Menge gemäß Python-Vertrag:
    /// {avg_viewers_all, avg_viewers_tracked, category, eventsub*, monetization*, tracked}
    /// (* = fehlt nur bei DB-Fehler, nicht bei leerem Ergebnis)
    #[tokio::test]
    async fn stats_top_level_keys_exakt_wie_python() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_stats_top_level_keys").await;
        let hf = HourFilter::None;
        let ext = EventSubStatsExt(None);
        let result = compute_stats(&pool, &ext, &hf, None).await.expect("compute_stats");

        let obj = result.as_object().expect("result ist kein Object");
        let keys: std::collections::HashSet<&str> = obj.keys().map(|s| s.as_str()).collect();

        // Pflicht-Keys (immer vorhanden)
        for k in &["avg_viewers_all", "avg_viewers_tracked", "category", "tracked"] {
            assert!(keys.contains(k), "Pflicht-Key fehlt: {k}");
        }
        // monetization muss bei funktionierender DB vorhanden sein
        assert!(keys.contains("monetization"), "monetization fehlt — DB-Verbindung OK aber Sektion fehlt");

        // Verbotene Extra-Keys (Python liefert diese nicht)
        for k in &["chat", "content_performance", "discovery", "retention"] {
            assert!(!keys.contains(k), "Extra-Key darf nicht vorhanden sein: {k}");
        }
    }

    /// max_viewers muss als Integer (i64) serialisiert werden, nicht als Float
    #[tokio::test]
    async fn stats_max_viewers_ist_integer() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_stats_max_viewers_int").await;

        sqlx::query("INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active, manual_verified_permanent) VALUES ('helmi', 1, 1)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_stats_tracked (streamer, viewer_count, is_partner, ts_utc) VALUES ('helmi', 19, TRUE, NOW()), ('helmi', 15, TRUE, NOW())")
            .execute(&pool).await.unwrap();

        let hf = HourFilter::None;
        let ext = EventSubStatsExt(None);
        let result = compute_stats(&pool, &ext, &hf, None).await.expect("compute_stats");

        let top = result["tracked"]["top"].as_array().expect("top ist array");
        let helmi = top.iter().find(|e| e["streamer"] == "helmi").expect("helmi fehlt");
        let max_v = &helmi["max_viewers"];
        // Muss Integer sein (JSON-Zahl ohne Dezimalstelle), nicht Float
        assert!(max_v.is_i64() || max_v.is_u64(),
            "max_viewers muss Integer sein, ist: {max_v:?}");
        assert_eq!(max_v.as_i64().unwrap(), 19);
    }

    // ── B13-1: Extended-Stats (retention/chat/discovery/content_performance) ──

    /// Leere DB → alle vier Sektionen vorhanden, mit Null/0-Defaults.
    /// Spiegelt `leaderboard.py:_compute_stats` (Sektionen werden im else-Zweig
    /// nach erfolgreichen Queries immer gesetzt — auch ohne Sessions).
    #[tokio::test]
    async fn extended_stats_leer_liefert_alle_vier_sektionen() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_ext_stats_leer").await;
        let result = compute_extended_stats(&pool).await.expect("compute_extended_stats");

        for k in &["retention", "chat", "discovery", "content_performance"] {
            assert!(result.get(*k).is_some(), "Sektion fehlt: {k}");
        }
        assert_eq!(result["retention"]["sessions"], 0);
        assert!(result["retention"]["ret5"].is_null());
        assert_eq!(result["discovery"]["followers_total_delta"], 0);
        assert!(result["content_performance"].as_array().unwrap().is_empty());
        assert_eq!(result["chat"]["total_unique"], 0);
    }

    /// Sessions mit Retention/Follower/Title → Aggregate werden korrekt berechnet.
    #[tokio::test]
    async fn extended_stats_aggregiert_sessions() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_ext_stats_agg").await;

        // Zwei abgeschlossene Sessions innerhalb 30 Tagen.
        sqlx::query(
            "INSERT INTO twitch_stream_sessions \
             (streamer_login, started_at, ended_at, duration_seconds, start_viewers, \
              peak_viewers, end_viewers, avg_viewers, samples, retention_5m, retention_10m, \
              retention_20m, dropoff_pct, dropoff_label, unique_chatters, first_time_chatters, \
              returning_chatters, follower_delta, followers_start, followers_end, stream_title, \
              notification_text) VALUES \
             ('alpha', NOW() - INTERVAL '2 days', NOW() - INTERVAL '2 days' + INTERVAL '2 hours', \
              7200, 10, 50, 40, 30.0, 100, 0.8, 0.6, 0.4, 0.2, 'mild', 30, 10, 20, 12, 100, 112, \
              'Deadlock Ranked', 'Wir gehen live'), \
             ('beta', NOW() - INTERVAL '1 day', NOW() - INTERVAL '1 day' + INTERVAL '1 hour', \
              3600, 20, 80, 60, 50.0, 60, 0.6, 0.4, 0.2, 0.4, 'steep', 40, 25, 15, 8, 200, 208, \
              'Chill Stream', '')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let result = compute_extended_stats(&pool).await.expect("compute_extended_stats");

        // Retention: 2 Sessions, ret5 = avg(0.8, 0.6) = 0.7
        assert_eq!(result["retention"]["sessions"], 2);
        let ret5 = result["retention"]["ret5"].as_f64().expect("ret5 f64");
        assert!((ret5 - 0.7).abs() < 1e-9, "ret5 falsch: {ret5}");
        // Dropoff-Examples nach dropoff_pct absteigend → beta (0.4) zuerst.
        let examples = result["retention"]["examples"].as_array().unwrap();
        assert_eq!(examples.len(), 2);
        assert_eq!(examples[0]["streamer"], "beta");

        // Discovery: follower_delta-Summe = 12 + 8 = 20
        assert_eq!(result["discovery"]["followers_total_delta"], 20);
        assert_eq!(result["discovery"]["sessions"], 2);

        // Content-Performance: beide Sessions (haben title) → nach peak absteigend (beta 80 > alpha 50)
        let cp = result["content_performance"].as_array().unwrap();
        assert_eq!(cp.len(), 2);
        assert_eq!(cp[0]["streamer"], "beta");
        assert_eq!(cp[0]["peak_viewers"], 80);
        // engagement_ratio = peak/followers_start*100 = 80/200*100 = 40.0
        let er = cp[0]["engagement_ratio"].as_f64().expect("engagement_ratio");
        assert!((er - 40.0).abs() < 1e-9, "engagement_ratio falsch: {er}");

        // Chat: total_unique = 30 + 40 = 70
        assert_eq!(result["chat"]["total_unique"], 70);
    }

    /// Der bestehende `/stats`-Vertrag darf die vier Sektionen NICHT enthalten
    /// (sie leben separat unter `/stats/extended`). Gegenprobe zum Top-Level-Key-Test.
    #[tokio::test]
    async fn extended_sektionen_nicht_im_stats_vertrag() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_ext_not_in_stats").await;
        let hf = HourFilter::None;
        let ext = EventSubStatsExt(None);
        let stats = compute_stats(&pool, &ext, &hf, None).await.expect("compute_stats");
        let obj = stats.as_object().unwrap();
        for k in &["retention", "chat", "discovery", "content_performance"] {
            assert!(!obj.contains_key(*k), "/stats darf {k} nicht enthalten");
        }
    }
}
