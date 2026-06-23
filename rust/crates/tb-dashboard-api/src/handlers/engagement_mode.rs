//! GET/POST `/twitch/api/v2/engagement/mode`.
//!
//! **B19-dash-mode-toggle:** Streamer-Selbstbedienung für den Output-Modus der
//! Engagement-KI auf `twitch_engagement_settings.output_mode`. Drei Zustände,
//! exakt der CHECK-Constraint der Spalte (Migration
//! `20260616000000_add_engagement_output_mode.sql`):
//!
//! - `off`    — kein KI-Output (Default; die KI bleibt ohne expliziten Toggle stumm)
//! - `shadow` — Antwort wird erzeugt + gestaged (Decision-Log), aber NICHT gesendet
//! - `live`   — Antwort wird normal in den Twitch-Chat gesendet
//!
//! **Orthogonal zu `enabled`:** `shadow`/`live` greifen erst, wenn der Kanal
//! zusätzlich `enabled = TRUE` hat — die Pipeline prüft `enabled` zuerst und bricht
//! sonst ab (`tb_engagement::pipeline`, `settings.enabled`-Gate vor dem
//! output_mode-Gate). Dieses Endpoint setzt ausschließlich `output_mode`; der
//! enabled-Toggle läuft über `…/engagement/toggle`.
//!
//! Muster gespiegelt von [`super::lurker_tax_settings`]: GET liest, POST setzt
//! explizit; CSRF greift über den Router-Layer (Write-Methoden), Default `off`.
//!
//! Auth (wie der übrige Engagement-Dashboard-Teil): Partner setzt den Modus des
//! EIGENEN Kanals (Login aus der Session) — die Zeile wird bei Bedarf angelegt;
//! Admin/Localhost adressieren via `?channel=` einen Kanal (reines UPDATE, 404 wenn
//! der Kanal keine Settings-Zeile hat). None → 401.

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

/// Gültige Output-Modi (identisch zum DB-CHECK-Constraint).
const VALID_MODES: [&str; 3] = ["off", "shadow", "live"];

#[derive(Deserialize, Default)]
pub struct ChannelQuery {
    /// Nur für Admin/Localhost relevant; Partner nutzen ihren Session-Login.
    #[serde(default)]
    pub channel: Option<String>,
}

#[derive(Deserialize)]
pub struct ModeUpdate {
    pub output_mode: String,
}

/// Aufgelöstes Ziel: `(channel_login, partner_self)`. Partner → eigener
/// Session-Login (`partner_self = true`, darf die Zeile anlegen); Admin/Localhost →
/// `?channel=` (`partner_self = false`, reines UPDATE); None → 401.
#[allow(clippy::result_large_err)] // axum-Response als Err — lokal, selten aufgerufen.
fn resolve_channel(
    auth: &DashboardAuthLevel,
    channel: &Option<String>,
) -> Result<(String, bool), Response> {
    match auth {
        DashboardAuthLevel::Partner { twitch_login, .. } => {
            Ok((twitch_login.trim().to_lowercase(), true))
        }
        DashboardAuthLevel::Admin { .. } => {
            match channel.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                Some(s) => Ok((s.to_lowercase(), false)),
                None => Err(err(StatusCode::BAD_REQUEST, "channel required")),
            }
        }
        DashboardAuthLevel::None => Err(err(StatusCode::UNAUTHORIZED, "unauthorized")),
    }
}

fn err(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

fn db_error(error: sqlx::Error, ctx: &str) -> Response {
    tracing::error!(%error, "engagement-mode {ctx} DB-Fehler");
    err(StatusCode::INTERNAL_SERVER_ERROR, "db")
}

/// `GET …/engagement/mode` — aktuellen Output-Modus lesen.
pub async fn get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<ChannelQuery>,
) -> Response {
    let (channel, _) = match resolve_channel(&auth, &query.channel) {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    let row = sqlx::query(
        "SELECT output_mode FROM twitch_engagement_settings WHERE channel_login = $1",
    )
    .bind(&channel)
    .fetch_optional(&pool)
    .await;

    match row {
        // Kein Eintrag → Default `off` (kein Fehler; Dashboard zeigt den Toggle aus).
        Ok(None) => Json(json!({ "output_mode": "off" })).into_response(),
        Ok(Some(row)) => {
            let mode: String = row.try_get("output_mode").unwrap_or_else(|_| "off".into());
            Json(json!({ "output_mode": mode })).into_response()
        }
        Err(error) => db_error(error, "get"),
    }
}

/// `POST …/engagement/mode` — Output-Modus explizit setzen (`off|shadow|live`).
///
/// Partner: Upsert auf den PK `channel_login` (legt die Zeile bei Bedarf an).
/// Admin/Localhost: reines UPDATE auf den per `?channel=` adressierten Kanal
/// (404, wenn keine Settings-Zeile existiert — Admins legen keine Kanäle an).
pub async fn post_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<ChannelQuery>,
    Json(body): Json<ModeUpdate>,
) -> Response {
    let (channel, partner_self) = match resolve_channel(&auth, &query.channel) {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    let mode = body.output_mode.trim().to_lowercase();
    if !VALID_MODES.contains(&mode.as_str()) {
        return err(
            StatusCode::BAD_REQUEST,
            "output_mode muss off, shadow oder live sein.",
        );
    }

    let result = if partner_self {
        // Partner → Upsert auf den PK; neue Zeile startet enabled=FALSE (Default),
        // shadow/live greift erst nach separatem enabled-Toggle.
        sqlx::query(
            "INSERT INTO twitch_engagement_settings (channel_login, output_mode, updated_at) \
             VALUES ($1, $2, NOW()) \
             ON CONFLICT (channel_login) \
             DO UPDATE SET output_mode = EXCLUDED.output_mode, updated_at = NOW()",
        )
        .bind(&channel)
        .bind(&mode)
        .execute(&pool)
        .await
    } else {
        // Admin/Localhost → reines UPDATE (kein Kanal-Anlegen).
        sqlx::query(
            "UPDATE twitch_engagement_settings SET output_mode = $2, updated_at = NOW() \
             WHERE channel_login = $1",
        )
        .bind(&channel)
        .bind(&mode)
        .execute(&pool)
        .await
    };

    match result {
        Ok(res) if res.rows_affected() > 0 => {
            Json(json!({ "ok": true, "output_mode": mode })).into_response()
        }
        // Admin-UPDATE ohne Treffer → der Kanal hat (noch) keine Settings-Zeile.
        Ok(_) => err(StatusCode::NOT_FOUND, "no engagement settings row"),
        Err(error) => db_error(error, "post"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        sqlx::query(
            "CREATE TABLE twitch_engagement_settings (channel_login TEXT PRIMARY KEY, \
             enabled BOOLEAN NOT NULL DEFAULT FALSE, steam_id TEXT, persona_override TEXT, \
             tabu_topics TEXT[], enabled_at TIMESTAMPTZ, enabled_by TEXT, \
             updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), \
             output_mode TEXT NOT NULL DEFAULT 'off' \
                 CHECK (output_mode IN ('off', 'shadow', 'live')))",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    fn partner(login: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: login.into(),
            twitch_user_id: "42".into(),
            display_name: String::new(),
        }
    }

    async fn body_of(resp: Response) -> (StatusCode, serde_json::Value) {
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
    }

    #[tokio::test]
    async fn default_off_und_partner_roundtrip() {
        let Some(pool) = make_pool("t_eng_mode_roundtrip").await else { return };
        // Fresh: keine Zeile → default off.
        let (s, j) = body_of(
            get_handler(partner("nani"), State(pool.clone()), Query(ChannelQuery::default())).await,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["output_mode"], "off");

        // Partner setzt shadow (Upsert legt die Zeile an).
        let (s, j) = body_of(
            post_handler(
                partner("nani"),
                State(pool.clone()),
                Query(ChannelQuery::default()),
                Json(ModeUpdate { output_mode: "shadow".into() }),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["output_mode"], "shadow");

        // GET liest shadow.
        let (_s, j) = body_of(
            get_handler(partner("nani"), State(pool.clone()), Query(ChannelQuery::default())).await,
        )
        .await;
        assert_eq!(j["output_mode"], "shadow");

        // Auf live wechseln (Update auf bestehende Zeile).
        let (_s, j) = body_of(
            post_handler(
                partner("nani"),
                State(pool.clone()),
                Query(ChannelQuery::default()),
                Json(ModeUpdate { output_mode: "LIVE".into() }), // case-insensitiv
            )
            .await,
        )
        .await;
        assert_eq!(j["output_mode"], "live");
    }

    #[tokio::test]
    async fn ungueltiger_modus_400() {
        let Some(pool) = make_pool("t_eng_mode_invalid").await else { return };
        let (s, j) = body_of(
            post_handler(
                partner("nani"),
                State(pool),
                Query(ChannelQuery::default()),
                Json(ModeUpdate { output_mode: "loud".into() }),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert!(j["error"].is_string());
    }

    #[tokio::test]
    async fn unauth_401_und_admin_branch() {
        let Some(pool) = make_pool("t_eng_mode_auth").await else { return };
        // None → 401.
        let (s, _) = body_of(
            get_handler(DashboardAuthLevel::None, State(pool.clone()), Query(ChannelQuery::default())).await,
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);

        // Admin ohne ?channel= → 400.
        let (s, _) = body_of(
            get_handler(DashboardAuthLevel::admin(), State(pool.clone()), Query(ChannelQuery::default())).await,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);

        // Admin-POST auf unbekannten Kanal (reines UPDATE) → 404.
        let (s, _) = body_of(
            post_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Query(ChannelQuery { channel: Some("ghost".into()) }),
                Json(ModeUpdate { output_mode: "live".into() }),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::NOT_FOUND);

        // Partner legt Zeile an, Admin adressiert sie per ?channel= und setzt off.
        let _ = post_handler(
            partner("nani"),
            State(pool.clone()),
            Query(ChannelQuery::default()),
            Json(ModeUpdate { output_mode: "live".into() }),
        )
        .await;
        let (s, j) = body_of(
            post_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Query(ChannelQuery { channel: Some("nani".into()) }),
                Json(ModeUpdate { output_mode: "off".into() }),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["output_mode"], "off");
    }
}
