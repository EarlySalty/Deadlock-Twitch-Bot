//! GET/POST `/twitch/api/v2/streamer/lurker-tax-settings`.
//!
//! Streamer-Selbstbedienung für das Lurker-Steuer-Flag auf `streamer_plans`
//! (`lurker_tax_enabled`, INTEGER, Default 0). Es ist **dieselbe** Spalte, die
//! der Chat-Command `!lurkersteuer_off` schreibt (Block 9) — daher ist die
//! Dashboard-Steuerung automatisch synchron zum Chat.
//!
//! **B9-BUILD-lurkertax-toggle-dashboard:** Toggle für ALLE Partner, **default
//! deaktiviert** (opt-in, bewusste Zustands-Entscheidung der Grillme). Python
//! gated den Endpoint zusätzlich hinter dem `chat.lurker_tax`-Entitlement; die
//! Grillme weitet ihn bewusst auf alle Partner aus (kein Paid-Gate).
//!
//! Auth: Partner setzt das Flag des EIGENEN Kanals (Login + User-ID aus der
//! Session); Admin/Localhost dürfen via `?streamer=` einen Kanal adressieren.

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
pub struct LurkerTaxQuery {
    /// Nur für Admin/Localhost relevant; Partner nutzen ihren Session-Login.
    #[serde(default)]
    pub streamer: Option<String>,
}

#[derive(Deserialize)]
pub struct LurkerTaxUpdate {
    pub lurker_tax_enabled: bool,
}

/// Aufgelöstes Ziel: `(login, user_id)`. Partner → Session-Werte; Admin/Localhost
/// → `?streamer=` (user_id leer, Match nur über Login); None → 401.
#[allow(clippy::result_large_err)]
fn resolve_target(
    auth: &DashboardAuthLevel,
    streamer: &Option<String>,
) -> Result<(String, String), Response> {
    match auth {
        DashboardAuthLevel::Partner {
            twitch_login,
            twitch_user_id,
            ..
        } => Ok((
            twitch_login.to_lowercase(),
            twitch_user_id.trim().to_string(),
        )),
        DashboardAuthLevel::Admin { .. } => {
            match streamer.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                Some(s) => Ok((s.to_lowercase(), String::new())),
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

/// Aktuellen Flag-Wert lesen (Login- ODER User-ID-Match).
const SELECT_SQL: &str = "SELECT COALESCE(lurker_tax_enabled, 0) AS lt \
       FROM streamer_plans \
      WHERE LOWER(COALESCE(twitch_login, '')) = $1 \
         OR ($2 <> '' AND twitch_user_id = $2) \
      LIMIT 1";

/// Pflicht-Scope, damit die Lurker-Steuer im Chat-Runtime feuert.
const CHATTERS_SCOPE: &str = "moderator:read:chatters";

/// Prüft, ob der Streamer-eigene `twitch_raid_auth`-Eintrag den
/// `moderator:read:chatters`-Scope trägt (P2.109).
///
/// Spiegelt die Runtime-Bedingung aus `tb-chat::promos` (Python `promos.py:1410`):
/// ohne diesen Scope läuft die Lurker-Steuer ins Leere — der Toggle wäre ein
/// stilles Dead-Toggle. Anders als der Chat-Runtime hat dieses Crate keinen
/// Bot-Token-Manager-Fallback; geprüft wird ausschließlich der Streamer-Scope.
async fn has_moderator_read_chatters(pool: &PgPool, login: &str) -> bool {
    let scopes: Option<String> = sqlx::query_scalar!(
        "SELECT scopes FROM twitch_raid_auth \
          WHERE LOWER(COALESCE(twitch_login, '')) = $1 \
          LIMIT 1",
        login
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    scopes
        .unwrap_or_default()
        .split_whitespace()
        .any(|s| s.eq_ignore_ascii_case(CHATTERS_SCOPE))
}

/// `GET …/lurker-tax-settings` — aktuellen Flag-Wert lesen.
pub async fn get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<LurkerTaxQuery>,
) -> Response {
    let (login, user_id) = match resolve_target(&auth, &query.streamer) {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    // P2.109: Readiness-Signal — feuert die Lurker-Steuer überhaupt? Ohne den
    // Scope ist der Toggle ein Dead-Toggle; das Dashboard kann so warnen.
    let scope_ready = has_moderator_read_chatters(&pool, &login).await;

    match sqlx::query(SELECT_SQL)
        .bind(&login)
        .bind(&user_id)
        .fetch_optional(&pool)
        .await
    {
        Ok(Some(row)) => {
            let lt: i32 = row.try_get("lt").unwrap_or(0);
            Json(json!({
                "lurker_tax_enabled": lt != 0,
                "has_moderator_read_chatters": scope_ready,
            }))
            .into_response()
        }
        // Kein Plan-Eintrag → Default-Aus (kein Fehler; Dashboard zeigt Toggle aus).
        Ok(None) => Json(json!({
            "lurker_tax_enabled": false,
            "has_moderator_read_chatters": scope_ready,
        }))
        .into_response(),
        Err(error) => {
            tracing::error!(%error, "lurker-tax-settings GET DB-Fehler");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "db" })),
            )
                .into_response()
        }
    }
}

/// `POST …/lurker-tax-settings` — Flag explizit setzen.
///
/// Upsert über `twitch_user_id` (PK von `streamer_plans`), wenn vorhanden; sonst
/// Login-`UPDATE`. So funktioniert der Toggle für jeden Partner, auch wenn noch
/// kein Paid-Plan-Eintrag existiert (default-Zeile mit plan_name='free').
pub async fn post_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<LurkerTaxQuery>,
    Json(body): Json<LurkerTaxUpdate>,
) -> Response {
    let (login, user_id) = match resolve_target(&auth, &query.streamer) {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    let flag = i32::from(body.lurker_tax_enabled);

    let result = if !user_id.is_empty() {
        // Partner (kennt User-ID) → Upsert auf den PK.
        sqlx::query!(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, lurker_tax_enabled) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (twitch_user_id) \
             DO UPDATE SET lurker_tax_enabled = EXCLUDED.lurker_tax_enabled, \
                           twitch_login = COALESCE(streamer_plans.twitch_login, EXCLUDED.twitch_login)",
            user_id,
            login,
            flag
        )
        .execute(&pool)
        .await
    } else {
        // Admin/Localhost über Login (keine User-ID) → reines UPDATE.
        sqlx::query!(
            "UPDATE streamer_plans SET lurker_tax_enabled = $2 \
              WHERE LOWER(COALESCE(twitch_login, '')) = $1",
            login,
            flag
        )
        .execute(&pool)
        .await
    };

    match result {
        Ok(res) if res.rows_affected() > 0 => {
            Json(json!({ "ok": true, "lurker_tax_enabled": body.lurker_tax_enabled }))
                .into_response()
        }
        // Login-UPDATE ohne Treffer (Admin adressiert unbekannten Plan) → 404.
        Ok(_) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "no plan row" })),
        )
            .into_response(),
        Err(error) => {
            tracing::error!(%error, "lurker-tax-settings POST DB-Fehler");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "db" })),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE streamer_plans (twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT, \
             plan_name TEXT DEFAULT 'free' NOT NULL, lurker_tax_enabled INTEGER DEFAULT 0 NOT NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("CREATE TABLE twitch_raid_auth (twitch_login TEXT, scopes TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        Some(pool)
    }

    fn partner(login: &str, uid: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: login.into(),
            twitch_user_id: uid.into(),
            display_name: String::new(),
        }
    }

    async fn body_of(resp: Response) -> (StatusCode, serde_json::Value) {
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    #[tokio::test]
    async fn default_aus_und_partner_toggle_roundtrip() {
        let Some(pool) = make_pool("t_lurkertax").await else {
            return;
        };
        // Fresh: kein Plan-Eintrag → default false.
        let (s, j) = body_of(
            get_handler(
                partner("nani", "42"),
                State(pool.clone()),
                Query(LurkerTaxQuery::default()),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["lurker_tax_enabled"], false);

        // Partner schaltet AN (Upsert legt die Zeile an).
        let (s, j) = body_of(
            post_handler(
                partner("nani", "42"),
                State(pool.clone()),
                Query(LurkerTaxQuery::default()),
                Json(LurkerTaxUpdate {
                    lurker_tax_enabled: true,
                }),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["lurker_tax_enabled"], true);

        // GET liest AN.
        let (_s, j) = body_of(
            get_handler(
                partner("nani", "42"),
                State(pool.clone()),
                Query(LurkerTaxQuery::default()),
            )
            .await,
        )
        .await;
        assert_eq!(j["lurker_tax_enabled"], true);

        // Partner schaltet wieder AUS (Update auf bestehende Zeile).
        let (_s, j) = body_of(
            post_handler(
                partner("nani", "42"),
                State(pool.clone()),
                Query(LurkerTaxQuery::default()),
                Json(LurkerTaxUpdate {
                    lurker_tax_enabled: false,
                }),
            )
            .await,
        )
        .await;
        assert_eq!(j["lurker_tax_enabled"], false);
    }

    /// P2.109: Readiness-Feld spiegelt den `moderator:read:chatters`-Scope wider.
    #[tokio::test]
    async fn readiness_feld_spiegelt_scope() {
        let Some(pool) = make_pool("t_lurkertax_scope").await else {
            return;
        };

        // Ohne raid_auth-Eintrag → Scope fehlt → false.
        let (s, j) = body_of(
            get_handler(
                partner("nani", "42"),
                State(pool.clone()),
                Query(LurkerTaxQuery::default()),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["has_moderator_read_chatters"], false);

        // Mit Scope im raid_auth-Eintrag → true.
        sqlx::query(
            "INSERT INTO twitch_raid_auth (twitch_login, scopes) \
             VALUES ('nani', 'channel:read:subscriptions moderator:read:chatters')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let (_s, j) = body_of(
            get_handler(
                partner("nani", "42"),
                State(pool.clone()),
                Query(LurkerTaxQuery::default()),
            )
            .await,
        )
        .await;
        assert_eq!(j["has_moderator_read_chatters"], true);

        // Anderer Scope-Satz ohne den Chatters-Scope → false.
        sqlx::query("UPDATE twitch_raid_auth SET scopes = 'bits:read' WHERE twitch_login = 'nani'")
            .execute(&pool)
            .await
            .unwrap();
        let (_s, j) = body_of(
            get_handler(
                partner("nani", "42"),
                State(pool),
                Query(LurkerTaxQuery::default()),
            )
            .await,
        )
        .await;
        assert_eq!(j["has_moderator_read_chatters"], false);
    }

    #[tokio::test]
    async fn unauth_401_und_admin_braucht_streamer() {
        let Some(pool) = make_pool("t_lurkertax_auth").await else {
            return;
        };
        let (s, _) = body_of(
            get_handler(
                DashboardAuthLevel::None,
                State(pool.clone()),
                Query(LurkerTaxQuery::default()),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        // Admin ohne ?streamer= → 400.
        let (s, _) = body_of(
            get_handler(
                DashboardAuthLevel::admin(),
                State(pool),
                Query(LurkerTaxQuery::default()),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }
}
