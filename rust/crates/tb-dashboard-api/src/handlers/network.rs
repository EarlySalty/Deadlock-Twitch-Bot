//! Handler für `GET /twitch/api/v2/public/network`.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;
use serde_json::json;
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
    /// Zuletzt gemeldete Twitch-Kategorie, `null` wenn unbekannt. Die Landing
    /// darf einen Live-Kanal nur dann als Deadlock-Stream ausgeben, wenn hier
    /// wirklich "Deadlock" steht.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game: Option<String>,
    /// Deadlock-Streams der letzten 30 Tage.
    pub deadlock_streams_30d: i64,
    /// Ungewichteter Mittelwert der `avg_viewers` ueber die Deadlock-Sessions
    /// der letzten 30 Tage. Sessions ohne gemessenen Schnitt zaehlen NICHT
    /// mit, der Wert kann also aus weniger Sessions stammen, als
    /// `deadlock_streams_30d` angibt. 0, wenn es keine Messung gab.
    pub avg_viewers_30d: f64,
}

impl NetworkStreamerJson {
    /// Wandelt eine DB-Zeile in die JSON-Form um und normalisiert dabei den Login
    /// (trim + lowercase, Python-Parität: `api_public.py:219-221`). Leere Logins
    /// werden mit `None` übersprungen.
    fn from_row(r: NetworkStreamerRow) -> Option<Self> {
        let login = r.twitch_login.trim().to_lowercase();
        if login.is_empty() {
            return None;
        }
        Some(Self {
            login,
            is_partner: true,
            is_live: r.is_live != 0,
            viewer_count: r.viewer_count,
            game: r
                .last_game
                .map(|g| g.trim().to_string())
                .filter(|g| !g.is_empty()),
            deadlock_streams_30d: r.dl_streams_30d,
            avg_viewers_30d: r.dl_avg_viewers_30d.unwrap_or(0.0),
        })
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
                streamers: rows
                    .into_iter()
                    .filter_map(NetworkStreamerJson::from_row)
                    .collect(),
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
                last_viewer_count INTEGER NOT NULL DEFAULT 0,
                last_game         TEXT
            )"#,
        )
        .execute(&pool)
        .await
        .expect("DDL live_state");

        // Sessions-Tabelle fuer die 30-Tage-Aggregate. Ohne sie laeuft der
        // Query in "relation does not exist" statt in ein leeres Aggregat.
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS twitch_stream_sessions (
                id                      BIGSERIAL PRIMARY KEY,
                streamer_login          TEXT NOT NULL,
                started_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
                had_deadlock_in_session BOOLEAN NOT NULL DEFAULT false,
                avg_viewers             DOUBLE PRECISION
            )"#,
        )
        .execute(&pool)
        .await
        .expect("DDL stream_sessions");

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
        sqlx::query("TRUNCATE twitch_stream_sessions")
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
        sqlx::query("INSERT INTO twitch_live_state VALUES ('liveuser', 1, 300, 'Deadlock')")
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
        assert_eq!(
            live["game"], "Deadlock",
            "Kategorie muss durchgereicht werden, sonst kann die Landing \
             live nicht von live-in-Deadlock unterscheiden"
        );

        let offline = json["streamers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["login"] == "offuser")
            .unwrap();
        assert!(
            offline.get("game").is_none(),
            "ohne Kategorie darf kein game-Feld erscheinen, war: {offline}"
        );

        assert_eq!(offline["is_live"], false, "is_live muss bool false sein");
    }

    /// P3.15: Login wird lowercased + getrimmt; leerer Login fällt raus (Python-Parität).
    #[tokio::test]
    async fn network_login_lowercase_und_leer_uebersprungen() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "api_network_norm").await;

        sqlx::query(
            "INSERT INTO _partner_state_base VALUES ('MixedCaseUser', 1), ('   ', 1), ('cleanuser', 1)",
        )
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

        let logins: Vec<&str> = json["streamers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["login"].as_str().unwrap())
            .collect();

        assert!(
            logins.contains(&"mixedcaseuser"),
            "Login muss lowercased sein, war: {logins:?}"
        );
        assert!(logins.contains(&"cleanuser"));
        assert!(
            !logins.iter().any(|l| l.trim().is_empty()),
            "Leerer Login darf nicht erscheinen, war: {logins:?}"
        );
        assert_eq!(logins.len(), 2, "Leer-Zeile muss übersprungen sein");
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

    /// Die 30-Tage-Aggregate waren bislang nur gegen die leere Tabelle
    /// geprueft. Dieser Test deckt ab, was dabei stumm falsch sein koennte:
    /// der `FILTER (WHERE had_deadlock_in_session)`, das 30-Tage-Fenster und
    /// der `LOWER()`-Join auf den Login.
    #[tokio::test]
    async fn network_dreissig_tage_aggregate() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "api_network_agg").await;

        sqlx::query("INSERT INTO _partner_state_base VALUES ('AggUser', 1)")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(
            r#"INSERT INTO twitch_stream_sessions
                   (streamer_login, started_at, had_deadlock_in_session, avg_viewers)
               VALUES
                   -- zaehlt: Deadlock, innerhalb des Fensters, Login in anderer Schreibweise
                   ('agguser', now() - interval '2 days',  true,  100),
                   ('AGGUSER', now() - interval '5 days',  true,  200),
                   -- zaehlt fuer die Anzahl, nicht fuer den Schnitt (kein Messwert)
                   ('agguser', now() - interval '7 days',  true,  NULL),
                   -- kein Deadlock
                   ('agguser', now() - interval '3 days',  false, 999),
                   -- ausserhalb des Fensters
                   ('agguser', now() - interval '40 days', true,  999)"#,
        )
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

        let row = json["streamers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["login"] == "agguser")
            .expect("Partner fehlt, der LOWER()-Join greift nicht");

        assert_eq!(
            row["deadlock_streams_30d"], 3,
            "nur Deadlock-Sessions der letzten 30 Tage zaehlen, war: {row}"
        );
        assert_eq!(
            row["avg_viewers_30d"], 150.0,
            "Schnitt aus 100 und 200; die NULL-Session und die 999er duerfen \
             nicht eingehen, war: {row}"
        );
    }

    /// Ohne Sessions bleiben beide Werte bei 0 statt null.
    #[tokio::test]
    async fn network_aggregate_ohne_sessions_sind_null_werte() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "api_network_agg_leer").await;

        sqlx::query("INSERT INTO _partner_state_base VALUES ('lonely', 1)")
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

        let row = &json["streamers"][0];
        assert_eq!(row["deadlock_streams_30d"], 0);
        assert_eq!(row["avg_viewers_30d"], 0.0);
    }

    /// Reiner Logik-Test ohne DB: ein Nicht-Datenbank-Fehler ist keine fehlende Relation.
    #[test]
    fn ist_fehlende_relation_false_fuer_nicht_db_fehler() {
        assert!(!super::ist_fehlende_relation(&sqlx::Error::RowNotFound));
        assert!(!super::ist_fehlende_relation(&sqlx::Error::PoolClosed));
    }
}
