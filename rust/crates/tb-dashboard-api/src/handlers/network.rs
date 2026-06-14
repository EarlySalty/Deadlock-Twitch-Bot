//! Handler für `GET /twitch/api/v2/public/network`.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;
use sqlx::PgPool;
use tb_analytics::network::{network_streamers, NetworkStreamerRow};

/// JSON-Repräsentation eines Streamers im Netzwerk.
///
/// `is_partner` ist immer `true`: der Endpoint filtert bereits auf aktive Partner.
/// `is_live` kommt als `i32` aus der DB und wird hier zu `bool` (Python-Verhalten: truthy).
#[derive(Serialize)]
pub struct NetworkStreamerJson {
    pub login: String,
    pub is_partner: bool,
    pub is_live: bool,
    pub viewer_count: i32,
}

impl From<NetworkStreamerRow> for NetworkStreamerJson {
    fn from(r: NetworkStreamerRow) -> Self {
        Self {
            login: r.twitch_login,
            is_partner: true,
            is_live: r.is_live != 0,
            viewer_count: r.viewer_count,
        }
    }
}

/// Top-Level-Response.
#[derive(Serialize)]
pub struct NetworkResponse {
    pub streamers: Vec<NetworkStreamerJson>,
}

/// Prüft, ob ein `sqlx::Error` auf eine fehlende Relation (View/Tabelle) zurückgeht.
///
/// Postgres meldet das mit SQLSTATE `42P01` (`undefined_table`). Genau dieser Fall
/// wird vom Python-Vorbild abgefangen: dort probt `_load_network_sync` zuerst, ob die
/// View `twitch_streamers_partner_state` existiert, und liefert bei fehlender View
/// graceful `{"streamers": []}` statt eines 500ers.
fn ist_fehlende_relation(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .and_then(|db| db.code())
        .map(|code| code == "42P01")
        .unwrap_or(false)
}

/// `GET /twitch/api/v2/public/network`
pub async fn network_handler(State(pool): State<PgPool>) -> impl IntoResponse {
    match network_streamers(&pool).await {
        Ok(rows) => {
            let resp = NetworkResponse {
                streamers: rows.into_iter().map(NetworkStreamerJson::from).collect(),
            };
            (StatusCode::OK, Json(resp)).into_response()
        }
        // Fehlende View/Tabelle → graceful leeres Ergebnis (Python-Parität):
        // 200 mit `{"streamers": []}` statt 500.
        Err(e) if ist_fehlende_relation(&e) => {
            tracing::warn!("network: Relation fehlt, liefere leeres Ergebnis: {e}");
            let resp = NetworkResponse { streamers: vec![] };
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(e) => {
            tracing::error!("network Query-Fehler: {e}");
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
            r#"CREATE TABLE IF NOT EXISTS twitch_live_state (
                streamer_login    TEXT PRIMARY KEY,
                is_live           INTEGER NOT NULL DEFAULT 0,
                last_viewer_count INTEGER NOT NULL DEFAULT 0
            )"#,
        )
        .execute(&pool)
        .await
        .expect("DDL live_state");

        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS _partner_state_base (
                twitch_login      TEXT PRIMARY KEY,
                is_partner_active INTEGER NOT NULL DEFAULT 1
            )"#,
        )
        .execute(&pool)
        .await
        .expect("DDL partner_state_base");

        sqlx::query(
            r#"CREATE OR REPLACE VIEW twitch_streamers_partner_state AS
               SELECT twitch_login, is_partner_active FROM _partner_state_base"#,
        )
        .execute(&pool)
        .await
        .expect("DDL view");

        sqlx::query("TRUNCATE _partner_state_base CASCADE")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("TRUNCATE twitch_live_state")
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    #[tokio::test]
    async fn network_endpoint_leere_tabelle_json_form() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "api_network_leer").await;

        let app = build_public_router(pool);
        let req = Request::builder()
            .uri("/twitch/api/v2/public/network")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert!(json.get("streamers").is_some(), "Feld 'streamers' fehlt");
        assert!(json["streamers"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn network_is_live_bool_und_is_partner_true() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "api_network_bool").await;

        sqlx::query("INSERT INTO _partner_state_base VALUES ('liveuser', 1), ('offuser', 1)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO twitch_live_state VALUES ('liveuser', 1, 300)")
            .execute(&pool)
            .await
            .unwrap();

        let app = build_public_router(pool);
        let req = Request::builder()
            .uri("/twitch/api/v2/public/network")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let live = json["streamers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["login"] == "liveuser")
            .unwrap();
        assert_eq!(live["is_live"], true, "is_live muss bool true sein");
        assert_eq!(live["is_partner"], true, "is_partner muss immer true sein");
        assert_eq!(live["viewer_count"], 300);

        let offline = json["streamers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["login"] == "offuser")
            .unwrap();
        assert_eq!(offline["is_live"], false, "is_live muss bool false sein");
    }

    /// Fehlende View → graceful: 200 mit `{"streamers": []}` statt 500 (Python-Parität).
    /// Wir löschen die View nach dem Setup, sodass der Query gegen eine fehlende Relation läuft.
    #[tokio::test]
    async fn network_fehlende_view_liefert_leer_200() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "api_network_missing").await;

        // View entfernen → der Endpoint-Query findet `twitch_streamers_partner_state` nicht.
        sqlx::query("DROP VIEW IF EXISTS twitch_streamers_partner_state")
            .execute(&pool)
            .await
            .expect("View droppen");

        let app = build_public_router(pool);
        let req = Request::builder()
            .uri("/twitch/api/v2/public/network")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "fehlende View muss graceful 200 liefern, nicht 500"
        );
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json.get("streamers").is_some(), "Feld 'streamers' fehlt");
        assert!(json["streamers"].as_array().unwrap().is_empty());
    }

    /// Reiner Logik-Test ohne DB: ein Nicht-Datenbank-Fehler ist keine fehlende Relation.
    #[test]
    fn ist_fehlende_relation_false_fuer_nicht_db_fehler() {
        assert!(!super::ist_fehlende_relation(&sqlx::Error::RowNotFound));
        assert!(!super::ist_fehlende_relation(&sqlx::Error::PoolClosed));
    }
}
