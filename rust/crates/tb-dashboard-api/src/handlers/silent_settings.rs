//! GET/POST `/twitch/api/v2/streamer/silent-settings`.
//!
//! Streamer-Selbstbedienung für die Silent-Notification-Flags auf
//! `twitch_partners` (`silent_ban`, `silent_raid`). Es sind **dieselben**
//! Spalten, die die Chat-Commands `!silentban`/`!silentraid` toggeln
//! (`chat_wiring.rs::toggle_partner_flag`) — daher ist die Dashboard-Steuerung
//! automatisch synchron zum Chat.
//!
//! Auth: Partner setzt die Flags des EIGENEN Kanals (Login aus der Session);
//! Admin/Localhost dürfen via `?streamer=` einen Kanal adressieren.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::{PgPool, Row};

use crate::auth::level::DashboardAuthLevel;

#[derive(Deserialize, Default)]
pub struct SilentQuery {
    /// Nur für Admin/Localhost relevant; Partner nutzen ihren Session-Login.
    #[serde(default)]
    pub streamer: Option<String>,
}

#[derive(Deserialize)]
pub struct SilentUpdate {
    pub silent_ban: bool,
    pub silent_raid: bool,
}

/// Ziel-Login auflösen: Partner → eigener Session-Login; Admin/Localhost →
/// `?streamer=` (sonst 400); None → 401.
#[allow(clippy::result_large_err)] // axum-Response als Err — lokal, selten aufgerufen.
fn resolve_login(auth: &DashboardAuthLevel, streamer: &Option<String>) -> Result<String, Response> {
    match auth {
        DashboardAuthLevel::Partner { twitch_login, .. } => Ok(twitch_login.to_lowercase()),
        DashboardAuthLevel::Admin { .. } => {
            match streamer.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                Some(s) => Ok(s.to_lowercase()),
                None => Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "streamer required" })),
                )
                    .into_response()),
            }
        }
        DashboardAuthLevel::None => Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "unauthorized" })),
        )
            .into_response()),
    }
}

/// Aktiver Partner (status='active', jüngste Zeile) — wie `toggle_partner_flag`.
const SELECT_SQL: &str = "SELECT COALESCE(silent_ban, 0) AS sb, COALESCE(silent_raid, 0) AS sr
       FROM twitch_partners
      WHERE LOWER(twitch_login) = $1 AND status = 'active'
      ORDER BY id DESC
      LIMIT 1";

/// `GET …/silent-settings` — aktuelle Flags lesen.
pub async fn get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<SilentQuery>,
) -> Response {
    let login = match resolve_login(&auth, &query.streamer) {
        Ok(l) => l,
        Err(resp) => return resp,
    };
    match sqlx::query(SELECT_SQL).bind(&login).fetch_optional(&pool).await {
        Ok(Some(row)) => {
            let sb: i32 = row.try_get("sb").unwrap_or(0);
            let sr: i32 = row.try_get("sr").unwrap_or(0);
            Json(json!({ "silent_ban": sb != 0, "silent_raid": sr != 0 })).into_response()
        }
        // Kein aktiver Partner → Default-Aus (kein Fehler, Dashboard zeigt Toggles aus).
        Ok(None) => Json(json!({ "silent_ban": false, "silent_raid": false })).into_response(),
        Err(error) => {
            tracing::error!(%error, "silent-settings GET DB-Fehler");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "db" }))).into_response()
        }
    }
}

/// `POST …/silent-settings` — beide Flags explizit setzen.
pub async fn post_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<SilentQuery>,
    Json(body): Json<SilentUpdate>,
) -> Response {
    let login = match resolve_login(&auth, &query.streamer) {
        Ok(l) => l,
        Err(resp) => return resp,
    };
    let result = sqlx::query(
        "UPDATE twitch_partners
            SET silent_ban = $2, silent_raid = $3
          WHERE id = (
              SELECT id FROM twitch_partners
               WHERE LOWER(twitch_login) = $1 AND status = 'active'
               ORDER BY id DESC LIMIT 1
          )",
    )
    .bind(&login)
    .bind(i32::from(body.silent_ban))
    .bind(i32::from(body.silent_raid))
    .execute(&pool)
    .await;
    match result {
        Ok(res) if res.rows_affected() > 0 => Json(json!({
            "ok": true,
            "silent_ban": body.silent_ban,
            "silent_raid": body.silent_raid
        }))
        .into_response(),
        Ok(_) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "no active partner" })),
        )
            .into_response(),
        Err(error) => {
            tracing::error!(%error, "silent-settings POST DB-Fehler");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "db" }))).into_response()
        }
    }
}
