use super::*;
use axum::{body::Body, extract::ConnectInfo, http::Request};
use serde_json::Value;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::net::SocketAddr;
use std::str::FromStr;
use tb_transport_twitch::HelixConfig;
use tower::ServiceExt;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = "test-token";
const DISCORD_A: &str = "388772056717590539";
const DISCORD_B: &str = "288772056717590538";

#[test]
fn body_wird_geprueft_und_normalisiert() {
    let ok = pruefe_body(SuggestionBody {
        twitch_login: " @Neuling ".into(),
        suggested_by_discord_id: DISCORD_A.into(),
        reason: Some("Spielt stark".into()),
        idempotency_key: "discord-suggestion-1".into(),
    })
    .unwrap();
    assert_eq!(ok.login, "neuling");
    assert_eq!(ok.discord_id, DISCORD_A);
    for (login, discord, key) in [
        ("na me", DISCORD_A, "k"),
        ("neuling", "123", "k"),
        ("neuling", DISCORD_A, ""),
        ("neuling", DISCORD_A, "mit leerzeichen"),
    ] {
        assert!(
            pruefe_body(SuggestionBody {
                twitch_login: login.into(),
                suggested_by_discord_id: discord.into(),
                reason: None,
                idempotency_key: key.into(),
            })
            .is_none(),
            "{login} {discord} {key}"
        );
    }
}

async fn migrated_pool(db_name: &str) -> Option<PgPool> {
    let Ok(dsn) = std::env::var("TB_TEST_DATABASE_URL") else {
        if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
            panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
        }
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
        return None;
    };
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .expect("admin connect");
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE IF EXISTS {db_name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {db_name}")))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    let opts = PgConnectOptions::from_str(&dsn).unwrap().database(db_name);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(opts)
        .await
        .expect("connect");
    sqlx::query("CREATE EXTENSION IF NOT EXISTS timescaledb")
        .execute(&pool)
        .await
        .ok();
    static MIGRATE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = MIGRATE_LOCK.lock().await;
    tb_db::migrate::MIGRATOR
        .run(&pool)
        .await
        .expect("Migrationen");
    Some(pool)
}

async fn helix_mock(users: &[(&str, &str)]) -> (MockServer, HelixClient) {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "tok", "expires_in": 3600
        })))
        .mount(&server)
        .await;
    for (id, login) in users {
        Mock::given(method("GET"))
            .and(path("/helix/users"))
            .and(query_param("login", *login))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"id": id, "login": login, "display_name": login}]
            })))
            .mount(&server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/helix/users"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .mount(&server)
        .await;
    let client = HelixClient::new(HelixConfig {
        client_id: "cid".into(),
        client_secret: "sec".into(),
        token_url: format!("{}/oauth2/token", server.uri()),
        helix_base: format!("{}/helix", server.uri()),
    })
    .unwrap();
    (server, client)
}

fn router(pool: PgPool, helix: Option<HelixClient>) -> axum::Router {
    crate::build_internal_router(
        pool,
        TOKEN.to_string(),
        Arc::new(helix),
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

async fn call(router: &axum::Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn post(body: Value, token: Option<&str>, peer: &str) -> Request<Body> {
    let addr: SocketAddr = peer.parse().unwrap();
    let mut b = Request::builder()
        .method("POST")
        .uri("/internal/twitch/v1/scout/community-suggestion")
        .header("content-type", "application/json")
        .extension(ConnectInfo(addr));
    if let Some(t) = token {
        b = b.header("X-Internal-Token", t);
    }
    b.body(Body::from(body.to_string())).unwrap()
}

fn suggest(login: &str, discord: &str, key: &str) -> Request<Body> {
    post(
        json!({"twitch_login": login, "suggested_by_discord_id": discord,
               "reason": "Spielt jeden Abend Deadlock", "idempotency_key": key}),
        Some(TOKEN),
        "127.0.0.1:5000",
    )
}

fn outcomes(query: &str) -> Request<Body> {
    let addr: SocketAddr = "127.0.0.1:5000".parse().unwrap();
    Request::builder()
        .uri(format!(
            "/internal/twitch/v1/scout/community-suggestions/outcomes{query}"
        ))
        .header("X-Internal-Token", TOKEN)
        .extension(ConnectInfo(addr))
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn auth_und_formfehler() {
    let Some(pool) = migrated_pool("tb_scout_cs_auth").await else {
        return;
    };
    let r = router(pool, None);
    let body = json!({"twitch_login": "x", "suggested_by_discord_id": DISCORD_A,
                      "idempotency_key": "k"});
    let (s, _) = call(&r, post(body.clone(), None, "127.0.0.1:5000")).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = call(&r, post(body.clone(), Some(TOKEN), "10.0.0.1:5000")).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = call(
        &r,
        post(json!({"twitch_login": "x"}), Some(TOKEN), "127.0.0.1:5000"),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = call(
        &r,
        post(
            json!({"twitch_login": "x", "suggested_by_discord_id": "kein",
                   "idempotency_key": "k"}),
            Some(TOKEN),
            "127.0.0.1:5000",
        ),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // Ohne Helix kein Raten: 503.
    let (s, v) = call(&r, post(body, Some(TOKEN), "127.0.0.1:5000")).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(v["error"], "twitch_unavailable");
    let (s, _) = call(&r, outcomes("?limit=0")).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn vorschlag_legt_kandidat_an_idempotent_und_zaehlt_vorschlagende() {
    let Some(pool) = migrated_pool("tb_scout_cs_flow").await else {
        return;
    };
    let (_server, helix) = helix_mock(&[("1001", "neuling")]).await;
    let r = router(pool.clone(), Some(helix));

    let (s, v) = call(&r, suggest("https://twitch.tv/Neuling", DISCORD_A, "k1")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v, json!({"status": "created", "twitch_user_id": "1001"}));
    let row: (
        String,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        i32,
    ) = sqlx::query_as(
        "SELECT status, twitch_user_id, source, suggested_by_discord_id, suggestion_reason,
                    suggestion_count
               FROM twitch_scout_candidates WHERE streamer_login = 'neuling'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        row,
        (
            "vorgeschlagen".into(),
            Some("1001".into()),
            "community".into(),
            Some(DISCORD_A.into()),
            Some("Spielt jeden Abend Deadlock".into()),
            1
        )
    );

    // Wiederholung mit gleichem Schlüssel: gleiche Antwort, nichts doppelt.
    let (_, v) = call(&r, suggest("neuling", DISCORD_A, "k1")).await;
    assert_eq!(v, json!({"status": "created", "twitch_user_id": "1001"}));
    // Zweite Person: schon bekannt, Zähler 2, erster Vorschlagender bleibt.
    let (_, v) = call(&r, suggest("neuling", DISCORD_B, "k2")).await;
    assert_eq!(
        v,
        json!({"status": "already_known", "twitch_user_id": "1001"})
    );
    // Dieselbe Person noch einmal mit neuem Schlüssel: Zähler bleibt.
    let (_, v) = call(&r, suggest("neuling", DISCORD_B, "k3")).await;
    assert_eq!(v["status"], "already_known");
    let (count, first): (i32, Option<String>) = sqlx::query_as(
        "SELECT suggestion_count, suggested_by_discord_id FROM twitch_scout_candidates
          WHERE streamer_login = 'neuling'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((count, first.as_deref()), (2, Some(DISCORD_A)));
    // Gleicher Schlüssel für eine andere Person: Konflikt.
    let (s, v) = call(&r, suggest("neuling", DISCORD_B, "k1")).await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(v["error"], "idempotency_conflict");
    // Kein automatischer Versand: keine Outreach-Zeile, keine Freigabe.
    let outreach: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partner_outreach")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(outreach, 0);
    assert!(tb_scout::store::approved_ohne_dispatch(&pool, 10)
        .await
        .unwrap()
        .is_empty());
    // Der Scout-Lauf der Admin-Liste läuft gegen das echte Schema
    // (cooldown_until ist dort TEXT) und lässt den Community-Kandidaten stehen.
    tb_scout::detector::laufe_scout_scan(&pool)
        .await
        .expect("Scout-Lauf gegen migriertes Schema");
    let offen = tb_scout::store::liste_offen(&pool).await.unwrap();
    assert_eq!(offen.len(), 1);
    assert_eq!(offen[0].source, "community");
}

#[tokio::test]
async fn partner_sperren_und_unbekannte_kanaele() {
    let Some(pool) = migrated_pool("tb_scout_cs_lists").await else {
        return;
    };
    sqlx::raw_sql(
        "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status)
             VALUES ('2002', 'partnerin', 'archived');
         INSERT INTO twitch_partner_signup_denylist (twitch_user_id, twitch_login, reason, added_by)
             VALUES ('3003', 'gesperrt', 'test', 'admin');
         INSERT INTO twitch_chatter_global_ban (chatter_login, chatter_id)
             VALUES ('gebannt', '3004');
         INSERT INTO twitch_scout_candidates (streamer_login, twitch_user_id, status)
             VALUES ('bekannt', '4004', 'uebersprungen');",
    )
    .execute(&pool)
    .await
    .unwrap();
    let (_server, helix) = helix_mock(&[
        ("2002", "partnerin"),
        ("3003", "gesperrt"),
        ("3004", "gebannt"),
        ("4004", "bekannt"),
    ])
    .await;
    let r = router(pool.clone(), Some(helix));
    for (login, status, id) in [
        ("partnerin", "already_partner", json!("2002")),
        ("gesperrt", "blocked", json!("3003")),
        ("gebannt", "blocked", json!("3004")),
        ("bekannt", "already_known", json!("4004")),
        ("gibtesnicht", "not_found", Value::Null),
    ] {
        let (s, v) = call(&r, suggest(login, DISCORD_A, &format!("key-{login}"))).await;
        assert_eq!(s, StatusCode::OK, "{login}");
        assert_eq!(
            v,
            json!({"status": status, "twitch_user_id": id}),
            "{login}"
        );
    }
    // Weder Partner noch Gesperrte werden Kandidaten; der bekannte Kandidat
    // behält Status und Quelle.
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT streamer_login, status, source FROM twitch_scout_candidates ORDER BY 1",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        rows,
        vec![(
            "bekannt".to_string(),
            "uebersprungen".to_string(),
            "auto".to_string()
        )]
    );
}

#[tokio::test]
async fn ergebnisse_melden_partnerschaft_mit_stabilem_cursor() {
    let Some(pool) = migrated_pool("tb_scout_cs_outcomes").await else {
        return;
    };
    let (_server, helix) = helix_mock(&[("1001", "neuling"), ("1002", "zweite")]).await;
    let r = router(pool.clone(), Some(helix));
    call(&r, suggest("neuling", DISCORD_A, "k1")).await;
    call(&r, suggest("zweite", DISCORD_B, "k2")).await;

    let (s, v) = call(&r, outcomes("?limit=1")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["has_more"], true);
    let row = &v["rows"][0];
    assert_eq!(row["twitch_user_id"], "1001");
    assert_eq!(row["twitch_login"], "neuling");
    assert_eq!(row["suggested_by_discord_id"], DISCORD_A);
    assert_eq!(row["suggestion_count"], 1);
    assert_eq!(row["candidate_status"], "vorgeschlagen");
    assert_eq!(row["is_partner_active"], false);
    assert_eq!(row["partner_since"], Value::Null);
    assert_eq!(v["next_updated_since"], row["updated_at"]);
    let cursor = v["next_updated_since"].as_str().unwrap().to_string();
    let (_, v) = call(&r, outcomes(&format!("?updated_since={cursor}"))).await;
    assert_eq!(v["rows"].as_array().unwrap().len(), 1);
    assert_eq!(v["rows"][0]["twitch_user_id"], "1002");
    let cursor = v["next_updated_since"].as_str().unwrap().to_string();

    // Der Kanal wird Partner: neue Zeile hinter dem Cursor, mit Zeitpunkt.
    sqlx::query(
        "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status, partnered_at)
         VALUES ('1001', 'neuling', 'active', '2026-10-05 18:00:00+00')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let (_, v) = call(&r, outcomes(&format!("?updated_since={cursor}"))).await;
    let rows = v["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["twitch_user_id"], "1001");
    assert_eq!(rows[0]["is_partner_active"], true);
    assert_eq!(rows[0]["partner_since"], "2026-10-05T18:00:00Z");
    assert_eq!(rows[0]["suggested_by_discord_id"], DISCORD_A);
    let cursor = v["next_updated_since"].as_str().unwrap().to_string();

    // Nichts geändert: leere Seite, Cursor bleibt.
    let (_, v) = call(&r, outcomes(&format!("?updated_since={cursor}"))).await;
    assert_eq!(
        v,
        json!({"rows": [], "next_updated_since": cursor, "has_more": false})
    );
}
