//! Handler für `GET /twitch/api/admin/system/eventsub`.

use crate::auth::level::DashboardAuthLevel;
use axum::{extract::State, response::IntoResponse, Json};
use chrono::{Duration, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;
use tb_analytics::system_eventsub::eventsub_snapshot;
use tb_http_core::ApiError;

const SNAPSHOT_STALE_AFTER_MINUTES: i64 = 15;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventsubCapacity {
    pub used: i64,
    pub max: i64,
    pub remaining: i64,
    pub last_snapshot_at: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventsubResponse {
    pub websocket_status: &'static str,
    /// Transport-Modus (Python-Vertrag: identisch zu `websocketStatus`).
    pub transport_mode: &'static str,
    pub active_subscription_count: i64,
    pub capacity: EventsubCapacity,
    pub subscriptions: Vec<Value>,
    pub last_known_subscriptions: Vec<Value>,
    pub last_known_snapshot_at: Option<String>,
    pub snapshot_stale: bool,
}

/// `GET /twitch/api/admin/system/eventsub`
pub async fn eventsub_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return Err(err);
    }

    // Bug A: ApiError::internal() ohne Argument
    let snap = eventsub_snapshot(&pool).await.map_err(|e| {
        tracing::error!("eventsub_snapshot Fehler: {e}");
        ApiError::internal()
    })?;

    let response = match snap {
        None => EventsubResponse {
            websocket_status: "inactive",
            transport_mode: "inactive",
            active_subscription_count: 0,
            capacity: EventsubCapacity {
                used: 0,
                max: 0,
                remaining: 0,
                last_snapshot_at: None,
            },
            subscriptions: vec![],
            last_known_subscriptions: vec![],
            last_known_snapshot_at: None,
            snapshot_stale: false,
        },
        Some(s) => {
            // P2.78: Live-Kapazität aus dem Snapshot ableiten. Der
            // WebSocket-Listener läuft im Bot-Prozess; der Snapshot ist die
            // einzige Quelle, die der Dashboard-Prozess hat. Sobald ein
            // Snapshot mit Slots existiert, gilt der Transport als aktiv.
            let last_snapshot_at = Some(s.ts_utc.to_rfc3339());
            let parsed: Vec<Value> = serde_json::from_str(&s.listeners_json).unwrap_or_default();
            // Jeder Listener-Eintrag bekommt den Snapshot-Zeitstempel (Python:
            // `{**item, "snapshotAt": last_known_snapshot_at}`).
            let last_known: Vec<Value> = parsed
                .into_iter()
                .take(200)
                .map(|mut item| {
                    if let (Some(obj), Some(ts)) = (item.as_object_mut(), &last_snapshot_at) {
                        obj.insert("snapshotAt".to_string(), Value::String(ts.clone()));
                    }
                    item
                })
                .collect();
            let stale = Utc::now().signed_duration_since(s.ts_utc)
                > Duration::minutes(SNAPSHOT_STALE_AFTER_MINUTES);
            // Transport gilt nur mit frischem Snapshot als aktiv.
            let active = !stale && (s.total_slots > 0 || s.used_slots > 0);
            let status = if stale {
                "stale"
            } else if active {
                "connected"
            } else {
                "inactive"
            };
            let remaining = s.headroom_slots.max(0);
            EventsubResponse {
                websocket_status: status,
                transport_mode: status,
                active_subscription_count: if active { s.used_slots } else { 0 },
                capacity: EventsubCapacity {
                    used: s.used_slots,
                    max: s.total_slots,
                    remaining,
                    last_snapshot_at: last_snapshot_at.clone(),
                },
                subscriptions: if active { last_known.clone() } else { vec![] },
                last_known_subscriptions: last_known,
                last_known_snapshot_at: last_snapshot_at,
                snapshot_stale: stale,
            }
        }
    };

    Ok(Json(response))
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
    use sqlx::postgres::PgPoolOptions;
    use std::net::SocketAddr;
    use tb_http_core::ExpectedToken;
    use tower::ServiceExt;

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
                        panic!("TB_TEST_REQUIRE_DB=1 ist gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                    }
                    eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                    return;
                }
            }
        };
    }

    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect");
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
            .expect("search_path");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_eventsub_capacity_snapshot (
                id             BIGSERIAL PRIMARY KEY,
                ts_utc         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                listener_count BIGINT NOT NULL DEFAULT 0,
                used_slots     INTEGER NOT NULL DEFAULT 0,
                total_slots    INTEGER NOT NULL DEFAULT 0,
                headroom_slots INTEGER NOT NULL DEFAULT 0,
                listeners_json TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL eventsub");
        sqlx::query("TRUNCATE twitch_eventsub_capacity_snapshot")
            .execute(&pool)
            .await
            .expect("TRUNCATE");
        pool
    }

    fn make_router(pool: PgPool, token: &str) -> Router {
        Router::new()
            .route("/twitch/api/admin/system/eventsub", get(eventsub_handler))
            .with_state(pool)
            .layer(Extension(ExpectedToken(token.to_string())))
    }

    #[tokio::test]
    async fn returns_401_ohne_auth() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_eventsub_unauth").await;
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        let req = Request::builder()
            .uri("/twitch/api/admin/system/eventsub")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .body(Body::empty())
            .unwrap();
        let res = make_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn leere_tabelle_gibt_inactive_response() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_eventsub_leer").await;
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        let req = Request::builder()
            .uri("/twitch/api/admin/system/eventsub")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", "tok")
            .body(Body::empty())
            .unwrap();
        let res = make_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["websocketStatus"], "inactive");
        assert_eq!(v["activeSubscriptionCount"], 0);
        assert!(v["lastKnownSubscriptions"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn snapshot_wird_gelesen_und_geparst() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_eventsub_daten").await;
        sqlx::query(
            "INSERT INTO twitch_eventsub_capacity_snapshot \
             (listener_count, used_slots, total_slots, headroom_slots, listeners_json) \
             VALUES (2, 0, 0, 0, '[{\"id\":\"x\"},{\"id\":\"y\"}]')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        let req = Request::builder()
            .uri("/twitch/api/admin/system/eventsub")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", "tok")
            .body(Body::empty())
            .unwrap();
        let res = make_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["lastKnownSubscriptions"].as_array().unwrap().len(), 2);
        assert!(v["lastKnownSnapshotAt"].is_string());
        // Per-Item snapshotAt wird angereichert (Python-Vertrag).
        assert!(v["lastKnownSubscriptions"][0]["snapshotAt"].is_string());
    }

    // P2.78: Snapshot mit Slot-Kapazität → websocketStatus != "inactive",
    // capacity.max/remaining > 0.
    #[tokio::test]
    async fn live_kapazitaet_meldet_aktiven_transport() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_eventsub_live").await;
        sqlx::query(
            "INSERT INTO twitch_eventsub_capacity_snapshot \
             (listener_count, used_slots, total_slots, headroom_slots, listeners_json) \
             VALUES (0, 7, 30, 23, '[{\"id\":\"stream.online:42\",\"type\":\"stream.online\",\"status\":\"enabled\",\"transport\":\"webhook\"}]')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        let req = Request::builder()
            .uri("/twitch/api/admin/system/eventsub")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", "tok")
            .body(Body::empty())
            .unwrap();
        let res = make_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_ne!(v["websocketStatus"], "inactive");
        assert_eq!(v["transportMode"], "connected");
        assert_eq!(v["capacity"]["max"], 30);
        assert_eq!(v["capacity"]["used"], 7);
        assert_eq!(v["capacity"]["remaining"], 23);
        assert_eq!(v["activeSubscriptionCount"], 7);
        assert_eq!(v["subscriptions"].as_array().unwrap().len(), 1);
        assert_eq!(v["snapshotStale"], false);
    }

    #[tokio::test]
    async fn alter_snapshot_wird_nicht_als_live_gemeldet() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_eventsub_stale").await;
        sqlx::query(
            "INSERT INTO twitch_eventsub_capacity_snapshot \
             (ts_utc, listener_count, used_slots, total_slots, headroom_slots, listeners_json) \
             VALUES (NOW() - INTERVAL '1 hour', 0, 7, 10000, 9993, '[{\"id\":\"old\"}]')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        let req = Request::builder()
            .uri("/twitch/api/admin/system/eventsub")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", "tok")
            .body(Body::empty())
            .unwrap();

        let res = make_router(pool, "tok").oneshot(req).await.unwrap();
        let b = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();

        assert_eq!(v["websocketStatus"], "stale");
        assert_eq!(v["activeSubscriptionCount"], 0);
        assert_eq!(v["snapshotStale"], true);
        assert!(v["subscriptions"].as_array().unwrap().is_empty());
        assert_eq!(v["lastKnownSubscriptions"].as_array().unwrap().len(), 1);
    }
}
