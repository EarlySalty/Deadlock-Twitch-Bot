//! Handler für `GET /twitch/raid/history`.
//!
//! Admin-only: gibt 401 zurück wenn [`DashboardAuthLevel`] nicht privileged
//! (Localhost/Admin) ist — gleiches Gate wie `streamers_handler`. Liest die
//! Raid-Historie über [`tb_analytics::raid_history::load_raid_history`]; die
//! Query-Parameter `from`/`from_broadcaster` (Login-Filter) und `limit`
//! (Default 50, im Loader auf 1..=500 geklemmt) werden durchgereicht.

use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use sqlx::PgPool;
use tb_http_core::ApiError;

use crate::auth::level::DashboardAuthLevel;

#[derive(Debug, Deserialize)]
pub struct RaidHistoryQuery {
    /// Login des Quell-Broadcasters (`from` ist die Kurzform aus dem Frontend).
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub from_broadcaster: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

/// `GET /twitch/raid/history`
pub async fn raid_history_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<RaidHistoryQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    // `from` und `from_broadcaster` sind Aliasse; der erste nicht-leere gewinnt.
    let from = params
        .from
        .as_deref()
        .or(params.from_broadcaster.as_deref());
    let rows = tb_analytics::raid_history::load_raid_history(&pool, from, params.limit)
        .await
        .map_err(|_| ApiError::internal())?;
    Ok(Json(rows))
}
