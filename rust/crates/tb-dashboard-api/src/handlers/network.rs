//! Handler für `GET /twitch/api/v2/public/network`.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;
use serde_json::json;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tb_analytics::network::{network_streamers, NetworkStreamerRow};
use tb_transport_twitch::{HelixClient, HelixConfig};
use tokio::sync::Mutex;

/// JSON-Repräsentation eines Streamers im Netzwerk.
///
/// `is_partner` ist immer `true`: der Endpoint filtert bereits auf aktive Partner.
/// `is_live` kommt als `i32` aus der DB und wird hier zu `bool` (Python-Verhalten: truthy).
#[derive(Serialize)]
pub struct NetworkStreamerJson {
    pub login: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
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
            display_name: None,
            avatar_url: None,
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

struct ProfileCache {
    at: Instant,
    by_login: HashMap<String, (Option<String>, Option<String>)>,
}

struct CacheState {
    good: Option<ProfileCache>,
    last_failure: Option<Instant>,
    in_flight: bool,
}

impl CacheState {
    const fn empty() -> Self {
        Self {
            good: None,
            last_failure: None,
            in_flight: false,
        }
    }
}

struct EnrichConfig {
    profile_ttl: Duration,
    negative_ttl: Duration,
    budget: Duration,
}

const ENRICH_CONFIG: EnrichConfig = EnrichConfig {
    profile_ttl: Duration::from_secs(3600),
    negative_ttl: Duration::from_secs(60),
    budget: Duration::from_secs(5),
};

fn apply_cached(
    streamers: &mut [NetworkStreamerJson],
    by_login: &HashMap<String, (Option<String>, Option<String>)>,
) {
    for streamer in streamers.iter_mut() {
        if let Some((display, avatar)) = by_login.get(&streamer.login) {
            streamer.display_name = display.clone();
            streamer.avatar_url = avatar.clone();
        }
    }
}

fn helix_client() -> Option<&'static HelixClient> {
    static HELIX: OnceLock<Option<HelixClient>> = OnceLock::new();
    HELIX
        .get_or_init(|| {
            let client_id = std::env::var("TWITCH_CLIENT_ID")
                .ok()
                .filter(|value| !value.is_empty())?;
            let client_secret = std::env::var("TWITCH_CLIENT_SECRET")
                .ok()
                .filter(|value| !value.is_empty())?;
            HelixClient::new(HelixConfig::new(client_id, client_secret)).ok()
        })
        .as_ref()
}

async fn enrich_profiles(streamers: &mut [NetworkStreamerJson]) {
    if streamers.is_empty() {
        return;
    }
    let Some(helix) = helix_client() else {
        return;
    };
    static CACHE: Mutex<CacheState> = Mutex::const_new(CacheState::empty());
    enrich_with(streamers, helix, &CACHE, &ENRICH_CONFIG).await;
}

async fn enrich_with(
    streamers: &mut [NetworkStreamerJson],
    helix: &HelixClient,
    cache: &Mutex<CacheState>,
    cfg: &EnrichConfig,
) {
    let logins: Vec<String> = streamers.iter().map(|s| s.login.clone()).collect();

    let stale_snapshot = {
        let mut guard = cache.lock().await;
        if let Some(good) = guard.good.as_ref() {
            if good.at.elapsed() < cfg.profile_ttl {
                apply_cached(streamers, &good.by_login);
                return;
            }
        }
        let negativ_gesperrt = guard
            .last_failure
            .map(|t| t.elapsed() < cfg.negative_ttl)
            .unwrap_or(false);
        if negativ_gesperrt || guard.in_flight {
            if let Some(good) = guard.good.as_ref() {
                apply_cached(streamers, &good.by_login);
            }
            return;
        }
        guard.in_flight = true;
        guard.good.as_ref().map(|g| g.by_login.clone())
    };

    let refs: Vec<&str> = logins.iter().map(String::as_str).collect();
    let outcome = tokio::time::timeout(cfg.budget, helix.get_users(&refs)).await;

    match outcome {
        Ok(Ok(users)) => {
            let mut by_login = HashMap::new();
            for streamer in streamers.iter_mut() {
                if let Some(user) = users.get(&streamer.login) {
                    let display =
                        Some(user.display_name.clone()).filter(|name| !name.is_empty());
                    let avatar = user
                        .profile_image_url
                        .clone()
                        .filter(|url| !url.trim().is_empty());
                    streamer.display_name = display.clone();
                    streamer.avatar_url = avatar.clone();
                    by_login.insert(streamer.login.clone(), (display, avatar));
                }
            }
            let mut guard = cache.lock().await;
            guard.good = Some(ProfileCache {
                at: Instant::now(),
                by_login,
            });
            guard.last_failure = None;
            guard.in_flight = false;
        }
        _ => {
            if let Some(stale) = stale_snapshot.as_ref() {
                apply_cached(streamers, stale);
            }
            let mut guard = cache.lock().await;
            guard.last_failure = Some(Instant::now());
            guard.in_flight = false;
        }
    }
}

/// `GET /twitch/api/v2/public/network`
pub async fn network_handler(State(pool): State<PgPool>) -> impl IntoResponse {
    match network_streamers(&pool).await {
        Ok(rows) => {
            let mut streamers: Vec<NetworkStreamerJson> = rows
                .into_iter()
                .filter_map(NetworkStreamerJson::from_row)
                .collect();
            enrich_profiles(&mut streamers).await;
            let resp = NetworkResponse { streamers };
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

#[cfg(test)]
mod enrich_tests {
    use super::*;
    use std::time::Duration;
    use tb_transport_twitch::HelixConfig;
    use tokio::sync::Mutex;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn streamer(login: &str) -> NetworkStreamerJson {
        NetworkStreamerJson {
            login: login.to_string(),
            display_name: None,
            avatar_url: None,
            is_partner: true,
            is_live: false,
            viewer_count: 0,
            game: None,
            deadlock_streams_30d: 0,
            avg_viewers_30d: 0.0,
        }
    }

    async fn mock_helix(server: &MockServer) -> HelixClient {
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "tok", "expires_in": 3600
            })))
            .mount(server)
            .await;
        let config = HelixConfig {
            client_id: "cid".to_string(),
            client_secret: "sec".to_string(),
            token_url: format!("{}/oauth2/token", server.uri()),
            helix_base: format!("{}/helix", server.uri()),
        };
        HelixClient::new(config).unwrap()
    }

    #[tokio::test]
    async fn enrich_serviert_frisch_ohne_zweiten_call() {
        let server = MockServer::start().await;
        let helix = mock_helix(&server).await;
        Mock::given(method("GET"))
            .and(path("/helix/users"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"id":"1","login":"nani","display_name":"Nani","profile_image_url":"http://x/a.png"}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let cache = Mutex::new(CacheState::empty());
        let cfg = EnrichConfig {
            profile_ttl: Duration::from_secs(60),
            negative_ttl: Duration::from_secs(60),
            budget: Duration::from_secs(5),
        };

        let mut rows = vec![streamer("nani")];
        enrich_with(&mut rows, &helix, &cache, &cfg).await;
        assert_eq!(rows[0].display_name.as_deref(), Some("Nani"));
        assert_eq!(rows[0].avatar_url.as_deref(), Some("http://x/a.png"));

        let mut rows2 = vec![streamer("nani")];
        enrich_with(&mut rows2, &helix, &cache, &cfg).await;
        assert_eq!(rows2[0].display_name.as_deref(), Some("Nani"));
        server.verify().await;
    }

    #[tokio::test]
    async fn enrich_reicht_stale_bei_timeout_weiter() {
        let server = MockServer::start().await;
        let helix = mock_helix(&server).await;

        Mock::given(method("GET"))
            .and(path("/helix/users"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"id":"1","login":"nani","display_name":"Nani","profile_image_url":"http://x/a.png"}]
            })))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/helix/users"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(3))
                    .set_body_json(json!({ "data": [] })),
            )
            .mount(&server)
            .await;

        let cache = Mutex::new(CacheState::empty());
        let cfg = EnrichConfig {
            profile_ttl: Duration::from_millis(0),
            negative_ttl: Duration::from_millis(0),
            budget: Duration::from_millis(150),
        };

        let mut rows = vec![streamer("nani")];
        enrich_with(&mut rows, &helix, &cache, &cfg).await;
        assert_eq!(rows[0].display_name.as_deref(), Some("Nani"));

        let start = std::time::Instant::now();
        let mut rows2 = vec![streamer("nani")];
        enrich_with(&mut rows2, &helix, &cache, &cfg).await;
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "das eigene Timeout-Budget muss den haengenden Helix-Call kappen"
        );
        assert_eq!(
            rows2[0].display_name.as_deref(),
            Some("Nani"),
            "abgelaufene, aber brauchbare Cache-Werte muessen weitergereicht werden"
        );
    }

    #[tokio::test]
    async fn enrich_negativ_cache_unterdrueckt_zweiten_call() {
        let server = MockServer::start().await;
        let helix = mock_helix(&server).await;

        Mock::given(method("GET"))
            .and(path("/helix/users"))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&server)
            .await;

        let cache = Mutex::new(CacheState::empty());
        let cfg = EnrichConfig {
            profile_ttl: Duration::from_millis(0),
            negative_ttl: Duration::from_secs(60),
            budget: Duration::from_secs(5),
        };

        let mut rows = vec![streamer("nani")];
        enrich_with(&mut rows, &helix, &cache, &cfg).await;

        let mut rows2 = vec![streamer("nani")];
        enrich_with(&mut rows2, &helix, &cache, &cfg).await;

        server.verify().await;
    }
}
