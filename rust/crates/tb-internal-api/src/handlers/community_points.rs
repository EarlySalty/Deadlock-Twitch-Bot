//! Community-Punkte für Deadlock-Bots (Community-Streamer-Brücke, Paket B).
//!
//! - `GET /internal/twitch/v1/community-points/viewers`
//! - `GET /internal/twitch/v1/community-points/streamers`
//!
//! Query: `updated_since=<RFC3339>` (optional, exklusiv) und `limit=1..5000`
//! (Standard 1000). Antwort `{rows, next_updated_since, has_more}`; Zeilen
//! sind Tageswerte, sortiert nach `updated_at`, dann Schlüssel. `updated_at`
//! ist je Tabelle eindeutig, darum reicht `next_updated_since` als Cursor.
//! Auth wie `/streamer-invites`: `X-Internal-Token` + Loopback (Router-Layer).

use std::collections::HashMap;

use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::json;
use sqlx::PgPool;
use tb_analytics::community_points::{
    list_streamer_points, list_viewer_points, viewer_activity_since, DEFAULT_PAGE_LIMIT,
    MAX_PAGE_LIMIT,
};
use tb_http_core::ApiError;

/// Gültige Cursor-Parameter.
#[derive(Debug, PartialEq, Eq)]
pub struct CursorParams {
    pub since: Option<DateTime<Utc>>,
    pub limit: i64,
}

fn bad_request(message: &str) -> ApiError {
    ApiError::bad_request_with_body(json!({"error": "bad_request", "message": message}))
}

/// Prüft `updated_since` und `limit`.
pub fn parse_cursor_params(params: &HashMap<String, String>) -> Result<CursorParams, ApiError> {
    let since = match params.get("updated_since").map(|s| s.trim()) {
        None | Some("") => None,
        Some(raw) => Some(
            DateTime::parse_from_rfc3339(raw)
                .map_err(|_| bad_request("updated_since muss ein RFC3339-Zeitstempel sein"))?
                .with_timezone(&Utc),
        ),
    };
    let limit = match params.get("limit").map(|s| s.trim()) {
        None | Some("") => DEFAULT_PAGE_LIMIT,
        Some(raw) => raw
            .parse::<i64>()
            .ok()
            .filter(|n| (1..=MAX_PAGE_LIMIT).contains(n))
            .ok_or_else(|| bad_request("limit muss zwischen 1 und 5000 liegen"))?,
    };
    Ok(CursorParams { since, limit })
}

/// `GET /internal/twitch/v1/community-points/viewers`
pub async fn viewers_handler(
    State(pool): State<PgPool>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, ApiError> {
    let cursor = parse_cursor_params(&params)?;
    let page = list_viewer_points(&pool, cursor.since, cursor.limit)
        .await
        .map_err(|error| {
            tracing::error!(%error, "Community-Punkte (Zuschauer) konnten nicht gelesen werden");
            ApiError::internal()
        })?;
    Ok(Json(page))
}

#[derive(Debug, PartialEq, Eq)]
struct ActivityParams {
    twitch_user_id: String,
    day: NaiveDate,
    activity_since: DateTime<Utc>,
}

fn parse_activity_params(
    params: &HashMap<String, String>,
    now: DateTime<Utc>,
) -> Result<ActivityParams, ApiError> {
    let twitch_user_id = params
        .get("twitch_user_id")
        .filter(|id| tb_chat::clip_contest_submit::valid_twitch_id(id))
        .ok_or_else(|| bad_request("twitch_user_id muss eine Twitch-ID sein"))?
        .clone();
    let day = params
        .get("day")
        .filter(|day| day.len() == 10)
        .and_then(|day| NaiveDate::parse_from_str(day, "%Y-%m-%d").ok())
        .ok_or_else(|| {
            bad_request("day muss ein Berliner Kalendertag im Format YYYY-MM-DD sein")
        })?;
    let activity_since = params.get("activity_since")
        .and_then(|since| DateTime::parse_from_rfc3339(since).ok()).map(|since| since.with_timezone(&Utc))
        .filter(|since| since <= &now && since.timestamp_subsec_nanos() % 1000 == 0)
        .ok_or_else(|| bad_request("activity_since muss eine vergangene RFC3339-Consent-Grenze mit höchstens Mikrosekundenauflösung sein"))?;
    Ok(ActivityParams {
        twitch_user_id,
        day,
        activity_since,
    })
}

/// Authentifizierter Rohdatennachweis für erneute Einwilligung, ohne DB-Schreibpfad.
pub async fn viewer_activity_handler(
    State(pool): State<PgPool>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, ApiError> {
    let now = Utc::now();
    let params = parse_activity_params(&params, now)?;
    let page = viewer_activity_since(
        &pool,
        &params.twitch_user_id,
        params.day,
        params.activity_since,
        now,
    )
    .await
    .map_err(|error| {
        tracing::error!(%error, "Consent-gebundene Vieweraktivität konnte nicht gelesen werden");
        ApiError::internal()
    })?;
    Ok(Json(page))
}

/// `GET /internal/twitch/v1/community-points/streamers`
pub async fn streamers_handler(
    State(pool): State<PgPool>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, ApiError> {
    let cursor = parse_cursor_params(&params)?;
    let page = list_streamer_points(&pool, cursor.since, cursor.limit)
        .await
        .map_err(|error| {
            tracing::error!(%error, "Community-Punkte (Streamer) konnten nicht gelesen werden");
            ApiError::internal()
        })?;
    Ok(Json(page))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::{Request, StatusCode},
    };

    use std::net::SocketAddr;

    use std::sync::Arc;
    use tower::ServiceExt;

    const TOKEN: &str = "test-token";

    fn params(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn consent_parameter_erfordern_identitaet_tag_und_mikrosekundengrenze() {
        let now = DateTime::parse_from_rfc3339("2026-10-03T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let base = params(&[
            ("twitch_user_id", "123"),
            ("day", "2026-10-01"),
            ("activity_since", "2026-10-01T10:00:00.000123Z"),
        ]);
        assert_eq!(
            parse_activity_params(&base, now).unwrap().twitch_user_id,
            "123"
        );
        for (key, value) in [
            ("twitch_user_id", "anzeige"),
            ("twitch_user_id", ""),
            ("day", "2026-02-30"),
            ("activity_since", "morgen"),
            ("activity_since", "2026-10-04T00:00:00Z"),
            ("activity_since", "2026-10-01T10:00:00.000123001Z"),
        ] {
            let mut invalid = base.clone();
            invalid.insert(key.into(), value.into());
            assert!(
                parse_activity_params(&invalid, now).is_err(),
                "{key}={value}"
            );
        }
        for key in ["twitch_user_id", "day", "activity_since"] {
            let mut invalid = base.clone();
            invalid.remove(key);
            assert!(parse_activity_params(&invalid, now).is_err());
        }
    }

    #[test]
    fn cursor_parameter_werden_geprueft() {
        assert_eq!(
            parse_cursor_params(&params(&[])).unwrap(),
            CursorParams {
                since: None,
                limit: DEFAULT_PAGE_LIMIT
            }
        );
        let p = parse_cursor_params(&params(&[
            ("updated_since", "2026-10-01T20:00:00.000123Z"),
            ("limit", "5000"),
        ]))
        .unwrap();
        assert_eq!(p.limit, 5000);
        assert_eq!(
            p.since.unwrap().to_rfc3339(),
            "2026-10-01T20:00:00.000123+00:00"
        );
        for bad in ["0", "5001", "-1", "abc"] {
            assert!(
                parse_cursor_params(&params(&[("limit", bad)])).is_err(),
                "{bad}"
            );
        }
        assert!(parse_cursor_params(&params(&[("updated_since", "gestern")])).is_err());
    }

    async fn migrated_pool(_db_name: &str) -> crate::test_postgres::TestPostgres {
        let db = crate::test_postgres::TestPostgres::start_with_timescaledb().await;
        let pool = db.pool.clone();
        sqlx::query("CREATE EXTENSION IF NOT EXISTS timescaledb")
            .execute(&pool)
            .await
            .expect("TimescaleDB");
        let _guard = crate::handlers::TEST_MIGRATE_LOCK.lock().await;
        tb_db::migrate::MIGRATOR
            .run(&pool)
            .await
            .expect("Migrationen");
        db
    }

    fn router(pool: PgPool) -> axum::Router {
        crate::build_internal_router(
            pool,
            TOKEN.to_string(),
            Arc::new(None),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
    }

    fn req(path_and_query: &str, token: Option<&str>, peer: &str) -> Request<Body> {
        let addr: SocketAddr = peer.parse().unwrap();
        let mut b = Request::builder()
            .uri(format!(
                "/internal/twitch/v1/community-points/{path_and_query}"
            ))
            .extension(ConnectInfo(addr));
        if let Some(t) = token {
            b = b.header("X-Internal-Token", t);
        }
        b.body(Body::empty()).unwrap()
    }

    async fn call(pool: &PgPool, r: Request<Body>) -> (StatusCode, serde_json::Value) {
        let res = router(pool.clone()).oneshot(r).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), 4 * 1024 * 1024)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    async fn seed(pool: &PgPool) {
        sqlx::raw_sql(
            "INSERT INTO twitch_community_points_viewer_daily
                 (twitch_user_id, channel_twitch_user_id, day, twitch_login, watch_minutes,
                  chat_messages, points_watch, points_chat, points_discovery, updated_at)
             VALUES
                 ('123', '456', '2026-10-01', 'name', 95, 12, 19, 12, 10, '2026-10-01 20:00:00Z'),
                 ('124', '456', '2026-10-01', 'zwei', 10, 0, 2, 0, 0, '2026-10-01 20:00:00.000001Z'),
                 ('125', '456', '2026-10-01', 'drei', 5, 1, 1, 1, 0, '2026-10-01 20:00:00.000002Z');
             INSERT INTO twitch_community_points_streamer_daily
                 (streamer_twitch_user_id, day, streamer_login, discord_user_id, viewer_minutes,
                  unique_viewers, raids_to_partners, updated_at)
             VALUES ('456', '2026-10-01', 'name', '789', 4200, 61, 1, '2026-10-01 22:00:00Z'),
                    ('457', '2026-10-01', 'ohne', NULL, 0, 0, 1, '2026-10-01 22:00:01Z');",
        )
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn auth_und_loopback_wie_streamer_invites() {
        let _db = migrated_pool("tb_cp_handler_auth").await;
        let pool = _db.pool.clone();
        let (s, _) = call(&pool, req("viewers", None, "127.0.0.1:5000")).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        let (s, _) = call(&pool, req("streamers", Some("falsch"), "127.0.0.1:5000")).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        let (s, _) = call(&pool, req("viewers", Some(TOKEN), "10.1.2.3:5000")).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let (s, _) = call(&pool, req("viewers?limit=0", Some(TOKEN), "127.0.0.1:5000")).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        let (s, _) = call(
            &pool,
            req(
                "streamers?updated_since=morgen",
                Some(TOKEN),
                "127.0.0.1:5000",
            ),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn consent_route_erfordert_auth_und_bindet_leere_antwort() {
        let db = migrated_pool("tb_cp_handler_consent").await;
        let route="viewers/activity?twitch_user_id=123&day=2026-10-01&activity_since=2026-10-01T10:00:00Z";
        assert_eq!(
            call(&db.pool, req(route, None, "127.0.0.1:5000")).await.0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            call(&db.pool, req(route, Some(TOKEN), "10.1.2.3:5000"))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            call(
                &db.pool,
                req(
                    "viewers/activity?twitch_user_id=123",
                    Some(TOKEN),
                    "127.0.0.1:5000"
                )
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
        let (status, body) = call(&db.pool, req(route, Some(TOKEN), "127.0.0.1:5000")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["twitch_user_id"], "123");
        assert_eq!(body["day"], "2026-10-01");
        assert_eq!(body["activity_since"], "2026-10-01T10:00:00Z");
        assert!(DateTime::parse_from_rfc3339(body["computed_at"].as_str().unwrap()).is_ok());
        assert_eq!(body["rows"], json!([]));
    }

    #[tokio::test]
    async fn viewers_liefert_vertragsform_und_stabilen_cursor() {
        let _db = migrated_pool("tb_cp_handler_viewers").await;
        let pool = _db.pool.clone();
        seed(&pool).await;
        let (s, v) = call(&pool, req("viewers?limit=1", Some(TOKEN), "127.0.0.1:5000")).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(
            v,
            json!({
                "rows": [{
                    "twitch_user_id": "123", "twitch_login": "name",
                    "channel_twitch_user_id": "456", "day": "2026-10-01",
                    "watch_minutes": 95, "chat_messages": 12, "points_watch": 19,
                    "points_chat": 12, "points_discovery": 10,
                    "updated_at": "2026-10-01T20:00:00Z"
                }],
                "next_updated_since": "2026-10-01T20:00:00Z",
                "has_more": true
            })
        );
        let (_, v) = call(
            &pool,
            req(
                "viewers?updated_since=2026-10-01T20:00:00Z&limit=5000",
                Some(TOKEN),
                "127.0.0.1:5000",
            ),
        )
        .await;
        let ids: Vec<&str> = v["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["twitch_user_id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["124", "125"]);
        assert_eq!(v["next_updated_since"], "2026-10-01T20:00:00.000002Z");
        assert_eq!(v["has_more"], false);

        // Leere Folgeseite: Cursor bleibt stehen.
        let (_, v) = call(
            &pool,
            req(
                "viewers?updated_since=2026-10-01T20:00:00.000002Z",
                Some(TOKEN),
                "127.0.0.1:5000",
            ),
        )
        .await;
        assert_eq!(
            v,
            json!({"rows": [], "next_updated_since": "2026-10-01T20:00:00.000002Z", "has_more": false})
        );
    }

    #[tokio::test]
    async fn streamers_liefert_vertragsform() {
        let _db = migrated_pool("tb_cp_handler_streamers").await;
        let pool = _db.pool.clone();
        seed(&pool).await;
        let (s, v) = call(&pool, req("streamers", Some(TOKEN), "127.0.0.1:5000")).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(
            v,
            json!({
                "rows": [
                    {"streamer_twitch_user_id": "456", "streamer_login": "name",
                     "discord_user_id": "789", "day": "2026-10-01", "viewer_minutes": 4200,
                     "unique_viewers": 61, "raids_to_partners": 1,
                     "updated_at": "2026-10-01T22:00:00Z"},
                    {"streamer_twitch_user_id": "457", "streamer_login": "ohne",
                     "discord_user_id": null, "day": "2026-10-01", "viewer_minutes": 0,
                     "unique_viewers": 0, "raids_to_partners": 1,
                     "updated_at": "2026-10-01T22:00:01Z"}
                ],
                "next_updated_since": "2026-10-01T22:00:01Z",
                "has_more": false
            })
        );
    }
}
