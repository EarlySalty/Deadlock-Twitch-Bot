//! KI-Schalter pro Server (Paket K) gegen eine echte Postgres: DB-Funktion
//! `twitch_streamer_ai_allowed`, Sperre im Eingang und Beleg im Ledger.
//!
//! Das Guild-Schema ist auf die Spalten reduziert, die die Funktion liest
//! (`test-support/multiserver_ai_schema.sql`); die Migrationen von Paket K
//! selbst laufen unverändert. Ein
//! Testbinary, ein Test: Ledger-Pool und Guard-Cache sind prozessweit.

use std::time::Duration;

use sqlx::postgres::PgPoolOptions;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[path = "../../../test-support/database.rs"]
mod test_database;

const LLM_USAGE_SCHEMA: &str = include_str!("../sqlx/schema.sql");
const PAKET_K_REGEL: &str =
    include_str!("../../../migrations/20261012140000_ai_features_rule.sql");
const PAKET_K_LEDGER: &str =
    include_str!("../../../migrations/20261012140100_llm_usage_streamer.sql");
const DB_NAME: &str = "tb_llm_ai_guard_test";

const DE: i64 = 1_289_721_245_281_292_288;
const EN: i64 = 900_000_000_000_000_001;
const EN_ENTFERNT: i64 = 900_000_000_000_000_002;

const GUILD_SCHEMA: &str = include_str!("../../../test-support/multiserver_ai_schema.sql");

fn endpoint(server: &MockServer) -> tb_llm::LlmEndpoint {
    tb_llm::LlmEndpoint {
        provider: "fireworks",
        base_url: server.uri(),
        model: tb_llm::selection::configured_fireworks_model().into(),
        api_key: Some("synthetic-key".into()),
    }
}

#[tokio::test]
async fn ki_schalter_sperrt_fremde_server_und_belegt_im_ledger() {
    let dsn = test_database::database_url();
    assert!(
        dsn.is_some() || !test_database::required(),
        "Test-Datenbank verlangt, aber kein DSN gesetzt"
    );
    let Some(dsn) = dsn else {
        eprintln!("KI-Schalter-DB-Test übersprungen: kein Test-DSN");
        return;
    };
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .expect("admin connect");
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE IF EXISTS {DB_NAME} WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .expect("drop test db");
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {DB_NAME}")))
        .execute(&admin)
        .await
        .expect("create test db");
    let mut options: sqlx::postgres::PgConnectOptions = dsn.parse().expect("DSN");
    options = options.database(DB_NAME);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .expect("test db connect");

    sqlx::raw_sql(LLM_USAGE_SCHEMA).execute(&pool).await.expect("llm_usage");
    sqlx::raw_sql(GUILD_SCHEMA).execute(&pool).await.expect("guild schema");
    // Idempotent: ein zweiter Lauf darf nicht scheitern.
    for _ in 0..2 {
        sqlx::raw_sql(PAKET_K_REGEL).execute(&pool).await.expect("Migration Regel");
        sqlx::raw_sql(PAKET_K_LEDGER).execute(&pool).await.expect("Migration Ledger");
    }

    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO discord_guilds (guild_id, ai_features) VALUES ({DE}, true), ({EN}, false);
         INSERT INTO discord_guilds (guild_id, ai_features, status, removed_at)
              VALUES ({EN_ENTFERNT}, true, 'removed', now());
         INSERT INTO streamer_guilds (twitch_user_id, guild_id, active) VALUES
             ('100', {DE}, true),
             ('200', {EN}, true),
             ('300', {EN}, true), ('300', {DE}, true),
             ('500', {EN}, false),
             ('600', {DE}, false),
             ('700', {EN_ENTFERNT}, true), ('700', {EN}, true);
         INSERT INTO twitch_streamer_identities VALUES ('200', 'fremd_kanal'), ('100', 'de_kanal');"
    )))
    .execute(&pool)
    .await
    .expect("seed");

    // Die Regel als DB-Funktion.
    for (id, login, erwartet) in [
        (Some("100"), None, true),            // deutscher Server
        (Some("200"), None, false),           // nur fremder Server ohne KI
        (Some("300"), None, true),            // beide Server
        (Some("400"), None, true),            // ohne Zeile: deutscher Server
        (Some("500"), None, false),           // nur inaktiv, fremd
        (Some("600"), None, true),            // nur inaktiv, deutsch
        (Some("700"), None, false),           // entfernter Server zählt nicht als aktiv
        (None, Some("FREMD_KANAL"), false),   // per Login
        (None, Some("de_kanal"), true),
        (None, Some("unbekannt"), true),      // unbekannter Login: deutscher Server
    ] {
        let erlaubt: bool = sqlx::query_scalar("SELECT twitch_streamer_ai_allowed($1, $2)")
            .bind(id)
            .bind(login)
            .fetch_one(&pool)
            .await
            .expect("Funktion");
        assert_eq!(erlaubt, erwartet, "id={id:?} login={login:?}");
    }

    tb_llm::ledger::initialize(pool.clone(), "twitch-test", "ai-guard").unwrap();

    assert_eq!(tb_llm::streamer_ai_allowed(Some("200"), None).await, Ok(false));
    assert_eq!(tb_llm::streamer_ai_allowed(None, Some("de_kanal")).await, Ok(true));

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"content": "ok"}}],
            "usage": {"prompt_tokens": 3, "completion_tokens": 1}
        })))
        .expect(2)
        .mount(&server)
        .await;
    let request = || {
        tb_llm::Request::prompt("Fixture")
            .timeout(Duration::from_secs(5))
            .endpoint(endpoint(&server))
    };

    // Gesperrt: kein HTTP-Aufruf, keine Ledger-Zeile.
    let gesperrt = tb_llm::complete("promo_pitch", request().twitch_broadcaster("200")).await;
    assert!(matches!(gesperrt, Err(tb_llm::LlmError::Disabled(_))), "{gesperrt:?}");
    let gesperrt = tb_llm::complete("lfg_pitch", request().twitch_channel("Fremd_Kanal")).await;
    assert!(matches!(gesperrt, Err(tb_llm::LlmError::Disabled(_))), "{gesperrt:?}");
    // Streamer-Zweck ohne Streamerbezug, während ein Server die KI aus hat.
    let gesperrt = tb_llm::complete("promo_pitch", request()).await;
    assert!(matches!(gesperrt, Err(tb_llm::LlmError::Disabled(_))), "{gesperrt:?}");

    // Erlaubt: Streamer per Login aufgelöst, ID im Ledger.
    tb_llm::complete("engagement", request().twitch_channel("de_kanal"))
        .await
        .expect("deutscher Kanal");
    // Globaler Zweck ohne Streamerbezug läuft weiter, ohne ID.
    tb_llm::complete("community_sentiment", request())
        .await
        .expect("globaler Job");
    server.verify().await;

    let zeilen: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT purpose, twitch_user_id FROM llm_usage ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .expect("Ledger lesen");
    assert_eq!(
        zeilen,
        vec![
            ("engagement".to_string(), Some("100".to_string())),
            ("community_sentiment".to_string(), None),
        ]
    );

    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE IF EXISTS {DB_NAME} WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .ok();
}
