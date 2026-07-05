//! Handler fuer `GET /twitch/api/v2/public/network-stats`.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;
use serde_json::json;
use sqlx::PgPool;
use tb_analytics::network_stats::{network_stats, LivePartnerRow};

#[derive(Serialize)]
pub struct LivePartnerJson {
    pub login: String,
    pub display_name: String,
    pub started_at: Option<String>,
}

#[derive(Serialize)]
pub struct NetworkStatsResponse {
    pub active_partners: u64,
    pub raids_total: u64,
    pub raids_7d: u64,
    pub viewers_forwarded_total: Option<u64>,
    pub live: Vec<LivePartnerJson>,
}

impl From<LivePartnerRow> for LivePartnerJson {
    fn from(row: LivePartnerRow) -> Self {
        Self {
            login: row.login,
            display_name: row.display_name,
            started_at: row.started_at,
        }
    }
}

/// `GET /twitch/api/v2/public/network-stats`
pub async fn network_stats_handler(State(pool): State<PgPool>) -> impl IntoResponse {
    match network_stats(&pool).await {
        Ok(stats) => {
            let resp = NetworkStatsResponse {
                active_partners: stats.active_partners,
                raids_total: stats.raids_total,
                raids_7d: stats.raids_7d,
                viewers_forwarded_total: stats.viewers_forwarded_total,
                live: stats.live.into_iter().map(LivePartnerJson::from).collect(),
            };
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(e) => {
            tracing::error!("network_stats Query-Fehler: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "internal_error" })),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;
    use axum::http::{Request, StatusCode};
    use serde_json::Value;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use crate::build_public_router;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    macro_rules! db_dsn_or_skip {
        () => {
            match test_dsn() {
                Some(d) => d,
                None => {
                    if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                        panic!("TB_TEST_REQUIRE_DB=1 ist gesetzt, aber TB_TEST_DATABASE_URL fehlt");
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
            .expect("connect test-db");
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
            CREATE TABLE twitch_streamers_partner_state (
                twitch_login      TEXT NOT NULL,
                twitch_user_id    TEXT,
                is_partner_active INTEGER NOT NULL DEFAULT 0
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_streamers_partner_state");

        sqlx::query(
            r#"
            CREATE TABLE twitch_live_state (
                twitch_user_id    TEXT,
                streamer_login    TEXT NOT NULL,
                is_live           INTEGER NOT NULL DEFAULT 0,
                last_started_at   TEXT,
                active_session_id BIGINT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_live_state");

        sqlx::query(
            r#"
            CREATE TABLE twitch_raid_history (
                id                     BIGSERIAL PRIMARY KEY,
                from_broadcaster_id    TEXT NOT NULL DEFAULT 'from-id',
                from_broadcaster_login TEXT NOT NULL DEFAULT 'from-login',
                to_broadcaster_id      TEXT NOT NULL DEFAULT 'to-id',
                to_broadcaster_login   TEXT NOT NULL DEFAULT 'to-login',
                viewer_count           INTEGER DEFAULT 0,
                executed_at            TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                success                BOOLEAN DEFAULT TRUE
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_raid_history");

        pool
    }

    fn assert_object_keys(value: &Value, keys: &[&str]) {
        let object = value.as_object().expect("JSON object");
        assert_eq!(
            object.len(),
            keys.len(),
            "unerwartete Top-Level-Keys: {object:?}"
        );
        for key in keys {
            assert!(object.contains_key(*key), "Feld '{key}' fehlt");
        }
    }

    async fn get_json(pool: sqlx::PgPool) -> (StatusCode, Value) {
        let app = build_public_router(pool);
        let req = Request::builder()
            .uri("/twitch/api/v2/public/network-stats")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let status = resp.status();
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    #[tokio::test]
    async fn network_stats_endpoint_leere_tabelle_json_vertrag() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "api_network_stats_empty").await;

        let (status, json) = get_json(pool).await;

        assert_eq!(status, StatusCode::OK);
        assert_object_keys(
            &json,
            &[
                "active_partners",
                "raids_total",
                "raids_7d",
                "viewers_forwarded_total",
                "live",
            ],
        );
        assert_eq!(json["active_partners"], 0);
        assert_eq!(json["raids_total"], 0);
        assert_eq!(json["raids_7d"], 0);
        assert!(json["viewers_forwarded_total"].is_null());
        assert!(json["live"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn network_stats_endpoint_aggregiert_raids_partner_und_live_liste() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "api_network_stats_fixture").await;

        sqlx::query(
            r#"
            INSERT INTO twitch_streamers_partner_state
                (twitch_login, twitch_user_id, is_partner_active)
            VALUES
                ('PartnerOne', 'uid-1', 1),
                ('offline_partner', 'uid-2', 1),
                ('paused_partner', 'uid-3', 0),
                ('nosession_partner', 'uid-4', 1)
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            INSERT INTO twitch_live_state
                (twitch_user_id, streamer_login, is_live, last_started_at, active_session_id)
            VALUES
                ('uid-1', 'partnerone', 1, '2026-07-06T10:00:00Z', 42),
                ('uid-2', 'offline_partner', 0, '2026-07-05T10:00:00Z', NULL),
                ('uid-3', 'paused_partner', 1, '2026-07-06T11:00:00Z', 43),
                ('uid-4', 'nosession_partner', 1, '2026-07-06T12:00:00Z', NULL)
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            INSERT INTO twitch_raid_history (viewer_count, executed_at, success)
            VALUES
                (10, NOW() - INTERVAL '1 day', TRUE),
                (20, NOW() - INTERVAL '8 days', TRUE),
                (NULL, NOW() - INTERVAL '2 days', TRUE),
                (99, NOW(), FALSE)
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let (status, json) = get_json(pool).await;

        assert_eq!(status, StatusCode::OK);
        assert_object_keys(
            &json,
            &[
                "active_partners",
                "raids_total",
                "raids_7d",
                "viewers_forwarded_total",
                "live",
            ],
        );
        assert_eq!(json["active_partners"], 3);
        assert_eq!(json["raids_total"], 3);
        assert_eq!(json["raids_7d"], 2);
        assert_eq!(json["viewers_forwarded_total"], 30);

        let live = json["live"].as_array().unwrap();
        assert_eq!(live.len(), 1);
        assert_object_keys(&live[0], &["login", "display_name", "started_at"]);
        assert_eq!(live[0]["login"], "partnerone");
        assert_eq!(live[0]["display_name"], "PartnerOne");
        assert_eq!(live[0]["started_at"], "2026-07-06T10:00:00Z");
    }

    #[tokio::test]
    async fn network_stats_endpoint_public_ohne_auth_und_ohne_rate_limit_layer() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "api_network_stats_public").await;
        let app = build_public_router(pool);

        for _ in 0..4 {
            let req = Request::builder()
                .uri("/twitch/api/v2/public/network-stats")
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }
    }
}
