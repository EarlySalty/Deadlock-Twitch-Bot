//! Handler für `GET /twitch/api/admin/system/oauth-scopes` (P1.29 / P2.74).
//!
//! Aggregiert pro autorisiertem Streamer den OAuth-Scope-Status für das
//! Admin-Scope-Diff-Panel. Bei fehlendem Schema (z. B. frische DB) wird eine
//! leere `items`-Liste statt eines 500ers geliefert (Python-Parität).

use axum::{extract::State, response::IntoResponse, Json};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use tb_analytics::system_oauth_scopes::{
    load_oauth_scope_rows, partner_status, scope_snapshot, CRITICAL_SCOPES, REQUIRED_SCOPES,
    SCOPE_COLUMN_LABELS,
};
use tb_http_core::{ApiError, AuthLevel};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScopeSummary {
    total_authorized: i64,
    full_scope_count: i64,
    missing_scope_count: i64,
}

fn labels_value() -> Value {
    let mut map = serde_json::Map::new();
    for (scope, label) in SCOPE_COLUMN_LABELS {
        map.insert((*scope).to_string(), Value::String((*label).to_string()));
    }
    Value::Object(map)
}

fn empty_payload() -> Value {
    json!({
        "requiredScopes": REQUIRED_SCOPES,
        "criticalScopes": sorted_critical(),
        "labels": labels_value(),
        "summary": {
            "totalAuthorized": 0,
            "fullScopeCount": 0,
            "missingScopeCount": 0,
        },
        "items": [],
    })
}

fn sorted_critical() -> Vec<String> {
    let mut v: Vec<String> = CRITICAL_SCOPES.iter().map(|s| s.to_string()).collect();
    v.sort();
    v
}

/// `GET /twitch/api/admin/system/oauth-scopes`
pub async fn oauth_scopes_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let rows = match load_oauth_scope_rows(&pool).await {
        Ok(rows) => rows,
        Err(e) => {
            // Fehlendes Schema (frische DB) → leere items statt 500.
            if is_missing_schema_error(&e) {
                return Ok(Json(empty_payload()));
            }
            tracing::error!("oauth-scopes Loader-Fehler: {e}");
            return Err(ApiError::internal());
        }
    };

    let mut items: Vec<Value> = Vec::with_capacity(rows.len());
    let mut total_authorized: i64 = 0;
    let mut full_scope_count: i64 = 0;

    for row in rows {
        total_authorized += 1;
        let snap = scope_snapshot(row.scopes_raw.as_deref(), row.needs_reauth);
        if snap.connected && snap.missing_scopes.is_empty() && !snap.needs_reauth {
            full_scope_count += 1;
        }
        let status = partner_status(
            row.status.as_deref(),
            row.archived_at.as_deref(),
            row.manual_partner_opt_out,
            row.technical_pause_reason.as_deref(),
        );
        let display = row
            .display_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&row.effective_login)
            .to_string();
        items.push(json!({
            "login": row.effective_login,
            "displayName": display,
            "partnerStatus": status,
            "archivedAt": row.archived_at,
            "oauthStatus": snap.status,
            "oauthNeedsReauth": snap.needs_reauth,
            "grantedScopes": snap.granted_scopes,
            "missingScopes": snap.missing_scopes,
        }));
    }

    let summary = ScopeSummary {
        total_authorized,
        full_scope_count,
        missing_scope_count: (total_authorized - full_scope_count).max(0),
    };

    Ok(Json(json!({
        "requiredScopes": REQUIRED_SCOPES,
        "criticalScopes": sorted_critical(),
        "labels": labels_value(),
        "summary": summary,
        "items": items,
    })))
}

/// `true` wenn der Fehler auf eine fehlende Tabelle/Spalte zurückgeht
/// (Python: `_admin_is_missing_schema_error`).
fn is_missing_schema_error(e: &sqlx::Error) -> bool {
    if let sqlx::Error::Database(db) = e {
        // Postgres: 42P01 undefined_table, 42703 undefined_column,
        // 3F000 invalid_schema_name.
        matches!(
            db.code().as_deref(),
            Some("42P01") | Some("42703") | Some("3F000")
        )
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::{Request, StatusCode},
        routing::get,
        Extension, Router,
    };
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::net::SocketAddr;
    use std::str::FromStr;
    use tb_http_core::ExpectedToken;
    use tower::ServiceExt;

    async fn pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.ok()?;
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.ok()?;
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.ok()?;
        admin.close().await;
        let options = PgConnectOptions::from_str(&dsn).ok()?.options([("search_path", schema)]);
        PgPoolOptions::new().max_connections(2).connect_with(options).await.ok()
    }

    fn router(pool: PgPool) -> Router {
        Router::new()
            .route("/twitch/api/admin/system/oauth-scopes", get(oauth_scopes_handler))
            .with_state(pool)
            .layer(Extension(ExpectedToken("tok".to_string())))
    }

    fn admin_req() -> Request<Body> {
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        Request::builder()
            .uri("/twitch/api/admin/system/oauth-scopes")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", "tok")
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test]
    async fn ohne_auth_401() {
        let Some(pool) = pool("t_oauth_h_unauth").await else { return };
        for ddl in [
            "CREATE TABLE twitch_raid_auth (twitch_login TEXT, twitch_user_id TEXT, scopes TEXT, needs_reauth INTEGER DEFAULT 0, authorized_at TIMESTAMPTZ)",
            "CREATE TABLE twitch_partners_all_state (twitch_login TEXT, twitch_user_id TEXT, discord_display_name TEXT, manual_partner_opt_out INTEGER DEFAULT 0, archived_at TEXT, status TEXT, technical_pause_reason TEXT)",
        ] { sqlx::query(ddl).execute(&pool).await.unwrap(); }
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        let req = Request::builder()
            .uri("/twitch/api/admin/system/oauth-scopes")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .body(Body::empty())
            .unwrap();
        let res = router(pool).oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn summary_und_items() {
        let Some(pool) = pool("t_oauth_h_data").await else { return };
        for ddl in [
            "CREATE TABLE twitch_raid_auth (twitch_login TEXT, twitch_user_id TEXT, scopes TEXT, needs_reauth INTEGER DEFAULT 0, authorized_at TIMESTAMPTZ)",
            "CREATE TABLE twitch_partners_all_state (twitch_login TEXT, twitch_user_id TEXT, discord_display_name TEXT, manual_partner_opt_out INTEGER DEFAULT 0, archived_at TEXT, status TEXT, technical_pause_reason TEXT)",
        ] { sqlx::query(ddl).execute(&pool).await.unwrap(); }
        sqlx::query("INSERT INTO twitch_raid_auth (twitch_login, twitch_user_id, scopes, needs_reauth, authorized_at) VALUES ('a','1','bits:read',0, NOW())")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_partners_all_state (twitch_login, twitch_user_id, status) VALUES ('a','1','active')")
            .execute(&pool).await.unwrap();

        let res = router(pool).oneshot(admin_req()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 65536).await.unwrap();
        let v: Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["summary"]["totalAuthorized"], 1);
        assert_eq!(v["summary"]["fullScopeCount"], 0);
        assert_eq!(v["summary"]["missingScopeCount"], 1);
        assert_eq!(v["items"][0]["login"], "a");
        assert_eq!(v["items"][0]["oauthStatus"], "partial");
        assert!(v["requiredScopes"].as_array().unwrap().contains(&Value::String("channel:bot".into())));
    }

    #[tokio::test]
    async fn fehlendes_schema_liefert_leere_items() {
        // Schema ohne die Tabellen → Loader wirft undefined_table → leere items.
        let Some(pool) = pool("t_oauth_h_noschema").await else { return };
        let res = router(pool).oneshot(admin_req()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 65536).await.unwrap();
        let v: Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["summary"]["totalAuthorized"], 0);
        assert!(v["items"].as_array().unwrap().is_empty());
    }
}
