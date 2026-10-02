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
    let _guard = crate::handlers::TEST_MIGRATE_LOCK.lock().await;
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

#[tokio::test]
async fn replay_bleibt_nach_umbenennung_neuvergabe_und_helix_ausfall_gebunden() {
    let Some(pool) = migrated_pool("tb_scout_cs_replay").await else {
        return;
    };
    let (_server, helix) = helix_mock(&[("1001", "neuling")]).await;
    let r = router(pool.clone(), Some(helix));
    let original = call(&r, suggest("neuling", DISCORD_A, "replay-k1")).await;
    assert_eq!(original.1["status"], "created");
    sqlx::query("UPDATE twitch_scout_candidates SET streamer_login = 'umbenannt' WHERE twitch_user_id = '1001'")
        .execute(&pool).await.unwrap();
    // Gleiche ursprüngliche Anfrage: selbst ohne Helix die ursprüngliche ID.
    let offline = router(pool.clone(), None);
    assert_eq!(
        call(&offline, suggest("neuling", DISCORD_A, "replay-k1")).await,
        original
    );
    // Der alte Login gehört jetzt einem anderen Konto. Beim Replay kein Lookup.
    let (server, helix) = helix_mock(&[("9999", "neuling")]).await;
    let reassigned = router(pool.clone(), Some(helix));
    assert_eq!(
        call(&reassigned, suggest("neuling", DISCORD_A, "replay-k1")).await,
        original
    );
    assert!(server.received_requests().await.unwrap().is_empty());
    // Fremde Person und veränderte Anfrage erhalten keine fremde Antwort.
    for (login, discord) in [("neuling", DISCORD_B), ("umbenannt", DISCORD_A)] {
        let (status, value) = call(&offline, suggest(login, discord, "replay-k1")).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(value["error"], "idempotency_conflict");
        assert!(value.get("twitch_user_id").is_none());
    }
    let (_, value) = call(
        &offline,
        post(
            json!({"twitch_login":"neuling", "suggested_by_discord_id":DISCORD_A,
        "reason":"Anderer Grund", "idempotency_key":"replay-k1"}),
            Some(TOKEN),
            "127.0.0.1:5000",
        ),
    )
    .await;
    assert_eq!(value["error"], "idempotency_conflict");
    let (status, _) = call(
        &offline,
        post(
            json!({"twitch_login":"neuling", "suggested_by_discord_id":DISCORD_A,
        "idempotency_key":"replay-k1"}),
            None,
            "127.0.0.1:5000",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn alle_guardpfade_unterscheiden_konten_mit_gleichem_login() {
    let Some(pool) = migrated_pool("tb_scout_cs_ids").await else {
        return;
    };
    sqlx::raw_sql("INSERT INTO twitch_partners (twitch_user_id, twitch_login, status) VALUES ('9101','partner_alt','archived');
        INSERT INTO twitch_raid_blacklist (target_id,target_login) VALUES ('9102','raid_alt');
        INSERT INTO twitch_partner_signup_denylist (twitch_user_id,twitch_login,reason,added_by) VALUES ('9103','signup_alt','test','admin');
        INSERT INTO twitch_scout_pitch_blacklist (streamer_login,twitch_user_id) VALUES ('pitch_alt','9104');
        INSERT INTO twitch_chatter_global_ban (chatter_login,chatter_id) VALUES ('ban_alt','9105');
        INSERT INTO twitch_outbound_chat_suppressions (target_login,target_id,source,reason_code,suppressed_until) VALUES ('suppression_alt','9106','recruitment','test',NOW()+INTERVAL '1 day');
        INSERT INTO twitch_scout_candidates (streamer_login,twitch_user_id,status) VALUES ('kandidat_alt','9107','approved');
        INSERT INTO twitch_partner_outreach (streamer_login,streamer_user_id,detected_at,cooldown_until) VALUES ('outreach_alt','9108',NOW()::text,(NOW()+INTERVAL '1 day')::text);")
        .execute(&pool).await.unwrap();
    let users = [
        ("8101", "partner_alt"),
        ("8102", "raid_alt"),
        ("8103", "signup_alt"),
        ("8104", "pitch_alt"),
        ("8105", "ban_alt"),
        ("8106", "suppression_alt"),
        ("8107", "kandidat_alt"),
        ("8108", "outreach_alt"),
    ];
    let (_server, helix) = helix_mock(&users).await;
    let r = router(pool.clone(), Some(helix));
    for (_, login) in users {
        let (status, value) = call(&r, suggest(login, DISCORD_A, &format!("new-{login}"))).await;
        if login == "kandidat_alt" {
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(value["error"], "identity_unresolved");
        } else {
            assert_eq!(status, StatusCode::OK, "{login}: {value}");
            assert_eq!(value["status"], "created", "{login}");
        }
    }
    for (i, erwarteter_status) in [
        "already_partner",
        "blocked",
        "blocked",
        "blocked",
        "blocked",
        "blocked",
        "already_known",
        "already_known",
    ]
    .into_iter()
    .enumerate()
    {
        let result = tb_scout::community::vorschlag_einreichen(
            &pool,
            &VorschlagEingabe {
                twitch_user_id: format!("{}", 9101 + i),
                twitch_login: format!("altkonto_neu{i}"),
                discord_id: DISCORD_B.into(),
                grund: None,
                idempotency_key: format!("old-id-{i}"),
            },
        )
        .await
        .unwrap();
        assert_eq!(result.as_str(), erwarteter_status, "Guard {i}");
    }
    // Gleiche ID unter neuem Login übernimmt die ID-gebundene Entscheidung.
    let (_server, helix) = helix_mock(&[("9102", "raid_neu"), ("9107", "kandidat_neu")]).await;
    let r = router(pool.clone(), Some(helix));
    assert_eq!(
        call(&r, suggest("raid_neu", DISCORD_A, "rename-raid"))
            .await
            .1["status"],
        "blocked"
    );
    assert_eq!(
        call(&r, suggest("kandidat_neu", DISCORD_A, "rename-candidate"))
            .await
            .1["status"],
        "already_known"
    );
    let candidate:(String,String,i32) = sqlx::query_as("SELECT twitch_user_id,status,suggestion_count FROM twitch_scout_candidates WHERE streamer_login='kandidat_alt'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(candidate, ("9107".into(), "approved".into(), 2));
    // Ohne gespeicherte Konto-ID muss zuerst die zuständige Auflösung erfolgen.
    sqlx::query(
        "INSERT INTO twitch_scout_pitch_blacklist (streamer_login) VALUES ('unaufgeloest')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let (_server, helix) = helix_mock(&[("8110", "unaufgeloest")]).await;
    let r = router(pool, Some(helix));
    assert_eq!(
        call(&r, suggest("unaufgeloest", DISCORD_A, "missing-id"))
            .await
            .1["error"],
        "identity_unresolved"
    );
}

#[tokio::test]
async fn outcome_cursor_hat_eindeutige_mikrosekunden_und_verliert_keine_zeile() {
    let Some(pool) = migrated_pool("tb_scout_cs_cursor").await else {
        return;
    };
    for i in 0..25 {
        tb_scout::community::vorschlag_einreichen(
            &pool,
            &VorschlagEingabe {
                twitch_user_id: format!("{}", 6000 + i),
                twitch_login: format!("kanal{i}"),
                discord_id: DISCORD_A.into(),
                grund: None,
                idempotency_key: format!("cursor-{i}"),
            },
        )
        .await
        .unwrap();
    }
    let stamps:(i64,i64)=sqlx::query_as("SELECT COUNT(*),COUNT(DISTINCT community_updated_at) FROM twitch_scout_candidates WHERE source='community'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(stamps, (25, 25));
    let mut since = None;
    let mut seen = std::collections::HashSet::new();
    loop {
        let page = liste_ergebnisse(&pool, since, 7).await.unwrap();
        for row in page.rows {
            assert!(seen.insert(row.twitch_user_id));
        }
        since = page.next_updated_since.as_deref().map(|s| {
            chrono::DateTime::parse_from_rfc3339(s)
                .unwrap()
                .with_timezone(&chrono::Utc)
        });
        if !page.has_more {
            break;
        }
    }
    assert_eq!(seen.len(), 25);
}
