//! Admin-Auswertung der globalen Deadlock-Erfassung. Keine Rohchat-Ausgabe.
//!
//! Die Freigabe wird vor jedem Cache-Zugriff geprüft. Nur erfolgreiche,
//! aggregierte Antworten werden kurz zwischengespeichert; Datenbankfehler
//! sind niemals eine erfolgreiche Statistik mit lauter Nullen.

use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    extract::{Extension, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{Timelike, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use tokio::sync::Mutex;

use crate::auth::level::DashboardAuthLevel;

#[derive(Debug, Default, Deserialize)]
pub struct CollectorQuery {
    pub days: Option<String>,
}

type CachedPeriods = BTreeMap<i32, (Instant, Value)>;

/// Ein Cache je Router, nicht global zwischen Datenbanken geteilt.
/// Der gemeinsame Lock verhindert parallele identische 90-Tage-Vollabfragen.
#[derive(Clone, Default)]
pub struct CategoryCollectorCache(Arc<Mutex<CachedPeriods>>);

const CACHE_TTL: Duration = Duration::from_secs(60);
const QUERY_TIMEOUT: Duration = Duration::from_secs(25);

fn parse_days(query: &CollectorQuery) -> Option<i32> {
    match query.days.as_deref() {
        None | Some("7") => Some(7),
        Some("30") => Some(30),
        Some("90") => Some(90),
        _ => None,
    }
}

fn reply(status: StatusCode, body: Value) -> Response {
    (
        status,
        [(header::CACHE_CONTROL, "private, no-store")],
        Json(body),
    )
        .into_response()
}

/// GET /twitch/api/v2/admin/category-collector?days=7
pub async fn get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<CollectorQuery>,
    cache: Option<Extension<CategoryCollectorCache>>,
) -> Response {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return error.into_response();
    }
    let Some(days) = parse_days(&query) else {
        return reply(
            StatusCode::BAD_REQUEST,
            json!({
                "error": "invalid_period", "allowed_days": [7, 30, 90]
            }),
        );
    };

    // Direkte Handler-Tests benötigen keine globale Cache-Infrastruktur.
    let cache = cache.map(|Extension(cache)| cache).unwrap_or_default();
    let mut periods = cache.0.lock().await;
    if let Some((saved_at, body)) = periods.get(&days) {
        if saved_at.elapsed() < CACHE_TTL {
            return reply(StatusCode::OK, body.clone());
        }
    }

    let result = tokio::time::timeout(QUERY_TIMEOUT, load(&pool, days)).await;
    match result {
        Ok(Ok(body)) => {
            periods.insert(days, (Instant::now(), body.clone()));
            reply(StatusCode::OK, body)
        }
        Ok(Err(error)) => {
            let schema_missing = error
                .as_database_error()
                .and_then(|error| error.code())
                .is_some_and(|code| code == "42P01" || code == "42703");
            tracing::error!(%error, "Kategoriesammler-Auswertung nicht verfügbar");
            reply(
                StatusCode::SERVICE_UNAVAILABLE,
                json!({
                    "error": if schema_missing { "category_collector_schema_missing" }
                        else { "category_collector_unavailable" }
                }),
            )
        }
        Err(_) => {
            tracing::warn!("Kategoriesammler-Auswertung überschreitet Abfragefrist");
            reply(
                StatusCode::SERVICE_UNAVAILABLE,
                json!({
                    "error": "category_collector_timeout"
                }),
            )
        }
    }
}

async fn load(pool: &PgPool, days: i32) -> Result<Value, sqlx::Error> {
    let now = Utc::now();
    // Historische Chat-Aggregate haben Stundenauflösung. Die Antwort nennt
    // deshalb ausdrücklich den tatsächlich verwendeten UTC-Stundenbeginn.
    let start = now
        .with_minute(0)
        .and_then(|t| t.with_second(0))
        .and_then(|t| t.with_nanosecond(0))
        .expect("gültige UTC-Stunde")
        - chrono::Duration::days(i64::from(days));
    let mut transaction = pool.begin().await?;
    for setting in [
        "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY",
        "SET LOCAL TIME ZONE 'UTC'",
        "SET LOCAL statement_timeout = '20s'",
        "SET LOCAL lock_timeout = '2s'",
    ] {
        sqlx::query(setting).execute(&mut *transaction).await?;
    }
    let body = sqlx::query_scalar::<_, Value>(include_str!("category_collector.sql"))
        .bind(start)
        .bind(now)
        .bind(days)
        .fetch_one(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(body)
}

#[cfg(test)]
#[path = "category_collector_tests.rs"]
mod tests;
