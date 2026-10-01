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
use crate::auth::streamer_scope::resolve_settings_target;

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

/// Aktiver Partner (status='active', jüngste Zeile) — wie `toggle_partner_flag`.
const SELECT_SQL: &str = "SELECT COALESCE(silent_ban, 0) AS sb, COALESCE(silent_raid, 0) AS sr
       FROM twitch_partners
      WHERE (($2 <> '' AND twitch_user_id = $2)
         OR ($2 = '' AND LOWER(twitch_login) = $1)) AND status = 'active'
      ORDER BY id DESC
      LIMIT 1";

/// `GET …/silent-settings` — aktuelle Flags lesen.
pub async fn get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<SilentQuery>,
) -> Response {
    let (login, user_id) = match resolve_settings_target(&auth, &query.streamer) {
        Ok(l) => l,
        Err(resp) => return resp,
    };
    match sqlx::query(SELECT_SQL)
        .bind(&login)
        .bind(&user_id)
        .fetch_optional(&pool)
        .await
    {
        Ok(Some(row)) => {
            let sb: i32 = row.try_get("sb").unwrap_or(0);
            let sr: i32 = row.try_get("sr").unwrap_or(0);
            Json(json!({ "silent_ban": sb != 0, "silent_raid": sr != 0 })).into_response()
        }
        // Kein aktiver Partner → Default-Aus (kein Fehler, Dashboard zeigt Toggles aus).
        Ok(None) => Json(json!({ "silent_ban": false, "silent_raid": false })).into_response(),
        Err(error) => {
            tracing::error!(%error, "silent-settings GET DB-Fehler");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "db" })),
            )
                .into_response()
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
    let (login, user_id) = match resolve_settings_target(&auth, &query.streamer) {
        Ok(l) => l,
        Err(resp) => return resp,
    };
    let result = sqlx::query(
        "UPDATE twitch_partners
            SET silent_ban = $2, silent_raid = $3
          WHERE id = (
              SELECT id FROM twitch_partners
               WHERE (($4 <> '' AND twitch_user_id = $4)
                  OR ($4 = '' AND LOWER(twitch_login) = $1)) AND status = 'active'
               ORDER BY id DESC LIMIT 1
          )",
    )
    .bind(&login)
    .bind(i32::from(body.silent_ban))
    .bind(i32::from(body.silent_raid))
    .bind(&user_id)
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
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "db" })),
            )
                .into_response()
        }
    }
}
