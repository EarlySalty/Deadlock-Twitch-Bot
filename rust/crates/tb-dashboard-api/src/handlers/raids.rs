//! Handler für `GET /twitch/api/v2/public/recent-raids`.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;
use sqlx::PgPool;
use tb_analytics::raids::{recent_raids, RaidRow};

/// JSON-Repräsentation einer einzelnen Raid-Zeile.
///
/// Feldnamen entsprechen dem Python-API-Output:
/// `from_channel`, `to_channel`, `viewers`, `executed_at`.
#[derive(Serialize)]
pub struct RaidRowJson {
    pub from_channel: String,
    pub to_channel: String,
    pub viewers: Option<i32>,
    pub executed_at: Option<String>,
}

impl From<RaidRow> for RaidRowJson {
    fn from(r: RaidRow) -> Self {
        Self {
            from_channel: r.from_channel,
            to_channel: r.to_channel,
            viewers: r.viewers,
            executed_at: r.executed_at,
        }
    }
}

/// Top-Level-Response.
#[derive(Serialize)]
pub struct RaidsResponse {
    pub raids: Vec<RaidRowJson>,
}

/// `GET /twitch/api/v2/public/recent-raids`
pub async fn recent_raids_handler(State(pool): State<PgPool>) -> impl IntoResponse {
    match recent_raids(&pool).await {
        Ok(rows) => {
            let resp = RaidsResponse {
                raids: rows.into_iter().map(RaidRowJson::from).collect(),
            };
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(e) => {
            tracing::error!("recent_raids Query-Fehler: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;
    use axum::http::{Request, StatusCode};
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use crate::build_public_router;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    /// Gibt die DSN zurück oder bricht den Test ab.
    /// Mit `TB_TEST_REQUIRE_DB=1` wird statt des stillen Skips ein panic ausgelöst.
    macro_rules! db_dsn_or_skip {
        () => {
            match test_dsn() {
                Some(d) => d,
                None => {
                    if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                        panic!(
                            "TB_TEST_REQUIRE_DB=1 ist gesetzt, aber TB_TEST_DATABASE_URL fehlt"
                        );
                    }
                    eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                    return;
                }
            }
        };
    }

    async fn make_pool(dsn: &str, schema: &str) -> sqlx::PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&pool)
            .await
            .expect("Schema droppen");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .expect("Schema anlegen");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path setzen");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_raid_history (
                id                     BIGSERIAL PRIMARY KEY,
                from_broadcaster_login TEXT NOT NULL,
                to_broadcaster_login   TEXT NOT NULL,
                viewer_count           INTEGER DEFAULT 0,
                executed_at            TIMESTAMPTZ
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL fehlgeschlagen");
        sqlx::query("TRUNCATE twitch_raid_history")
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    #[tokio::test]
    async fn raids_endpoint_leere_tabelle_json_form() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "api_raids_leer").await;

        let app = build_public_router(pool);
        let req = Request::builder()
            .uri("/twitch/api/v2/public/recent-raids")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert!(json.get("raids").is_some(), "Feld 'raids' fehlt");
        assert!(json["raids"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn raids_endpoint_fixture_feldnamen() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "api_raids_fixture").await;

        sqlx::query(
            "INSERT INTO twitch_raid_history \
             (from_broadcaster_login, to_broadcaster_login, viewer_count, executed_at) \
             VALUES ('von', 'nach', 200, NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();

        let app = build_public_router(pool);
        let req = Request::builder()
            .uri("/twitch/api/v2/public/recent-raids")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let raid = &json["raids"][0];
        assert_eq!(raid["from_channel"], "von");
        assert_eq!(raid["to_channel"], "nach");
        assert_eq!(raid["viewers"], 200);
        assert!(raid["executed_at"].is_string());
    }
}
