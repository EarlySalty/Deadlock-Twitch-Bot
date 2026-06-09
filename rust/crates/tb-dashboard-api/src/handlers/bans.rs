//! Handler für `GET /twitch/api/v2/public/recent-bans`.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;
use sqlx::PgPool;
use tb_analytics::bans::{recent_bans, BanRow};

/// JSON-Repräsentation einer einzelnen Ban-Zeile.
///
/// Feldnamen 1:1 wie Python-Response (`target_login`, `moderator_login`, `reason`, `received_at`).
#[derive(Serialize)]
pub struct BanRowJson {
    pub target_login: String,
    pub moderator_login: Option<String>,
    pub reason: Option<String>,
    pub received_at: Option<String>,
}

impl From<BanRow> for BanRowJson {
    fn from(r: BanRow) -> Self {
        Self {
            target_login: r.target_login,
            moderator_login: r.moderator_login,
            reason: r.reason,
            received_at: r.received_at,
        }
    }
}

/// JSON-Stats-Block.
#[derive(Serialize)]
pub struct BanStatsJson {
    pub today: i64,
    pub total_30d: i64,
    pub channels_protected: i64,
}

/// Top-Level-Response.
#[derive(Serialize)]
pub struct BansResponse {
    pub bans: Vec<BanRowJson>,
    pub stats: BanStatsJson,
}

/// `GET /twitch/api/v2/public/recent-bans`
pub async fn recent_bans_handler(State(pool): State<PgPool>) -> impl IntoResponse {
    match recent_bans(&pool).await {
        Ok(result) => {
            let resp = BansResponse {
                bans: result.bans.into_iter().map(BanRowJson::from).collect(),
                stats: BanStatsJson {
                    today: result.stats.today,
                    total_30d: result.stats.total_30d,
                    channels_protected: result.stats.channels_protected,
                },
            };
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(e) => {
            tracing::error!("recent_bans Query-Fehler: {e}");
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

    /// Baut einen Pool mit max 1 Connection und setzt `search_path` auf das isolierte Schema.
    async fn make_pool(dsn: &str, schema: &str) -> sqlx::PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
            .execute(&pool)
            .await
            .expect("Schema anlegen");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path setzen");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_ban_events (
                id              BIGSERIAL PRIMARY KEY,
                twitch_user_id  TEXT NOT NULL DEFAULT 'default_uid',
                target_login    TEXT NOT NULL,
                moderator_login TEXT,
                reason          TEXT,
                received_at     TIMESTAMPTZ
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL fehlgeschlagen");
        sqlx::query("TRUNCATE twitch_ban_events")
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    #[tokio::test]
    async fn bans_endpoint_leere_tabelle_json_form() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP");
                return;
            }
        };
        let pool = make_pool(&dsn, "api_bans_leer").await;

        let app = build_public_router(pool);
        let req = Request::builder()
            .uri("/twitch/api/v2/public/recent-bans")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Exakte Schlüssel-Struktur
        assert!(json.get("bans").is_some(), "Feld 'bans' fehlt");
        assert!(json.get("stats").is_some(), "Feld 'stats' fehlt");
        assert!(
            json["bans"].as_array().unwrap().is_empty(),
            "bans muss [] sein"
        );
        assert_eq!(json["stats"]["today"], 0);
        assert_eq!(json["stats"]["total_30d"], 0);
        assert_eq!(json["stats"]["channels_protected"], 0);
    }

    #[tokio::test]
    async fn bans_endpoint_null_felder_korrekt() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP");
                return;
            }
        };
        let pool = make_pool(&dsn, "api_bans_null").await;

        sqlx::query(
            "INSERT INTO twitch_ban_events \
             (twitch_user_id, target_login, moderator_login, reason, received_at) \
             VALUES ('uid_x', 'null_user', NULL, NULL, NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let app = build_public_router(pool);
        let req = Request::builder()
            .uri("/twitch/api/v2/public/recent-bans")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let ban = &json["bans"][0];
        assert_eq!(ban["target_login"], "null_user");
        assert!(
            ban["moderator_login"].is_null(),
            "moderator_login muss null sein"
        );
        assert!(ban["reason"].is_null(), "reason muss null sein");
        assert!(ban["received_at"].is_null(), "received_at muss null sein");
    }
}
