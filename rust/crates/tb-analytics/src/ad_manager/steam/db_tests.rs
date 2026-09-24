//! Integration tests intentionally provide a Twitch-only schema. No Steam tables.
use super::*;
use serde_json::json;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::str::FromStr;
use wiremock::{
    matchers::{method, path, query_param},
    Mock, MockServer, ResponseTemplate,
};

struct Fixture {
    pool: PgPool,
    admin: PgPool,
    schema: String,
    server: MockServer,
    client: Client,
}
impl Fixture {
    async fn new() -> Self {
        let dsn =
            std::env::var("TB_TEST_DATABASE_URL").expect("isolated TB_TEST_DATABASE_URL required");
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .expect("test database connection");
        let schema = format!("t_adm_http_{}", Utc::now().timestamp_nanos_opt().unwrap());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        let options = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::raw_sql("CREATE TABLE twitch_player_steam_links (twitch_user_id text primary key,steam_id64 bigint,lookup_enabled bool NOT NULL,revision bigint NOT NULL);
            CREATE TABLE twitch_ad_manager_settings (twitch_user_id text primary key,twitch_login text NOT NULL);
            CREATE TABLE twitch_engagement_settings (channel_login text primary key,steam_id text);
            CREATE TABLE twitch_streamer_identities (twitch_user_id text primary key,discord_user_id text);
            INSERT INTO twitch_ad_manager_settings VALUES ('42','steamtest');").execute(&pool).await.unwrap();
        let server = MockServer::start().await;
        let client = Client::with_base(&server.uri(), Some("test-token".into()));
        Self {
            pool,
            admin,
            schema,
            server,
            client,
        }
    }
    async fn legacy(&self, id: &str) {
        sqlx::query("INSERT INTO twitch_engagement_settings VALUES ('steamtest',$1) ON CONFLICT(channel_login) DO UPDATE SET steam_id=EXCLUDED.steam_id")
            .bind(id).execute(&self.pool).await.unwrap();
    }
    async fn direct(&self, enabled: bool) {
        sqlx::query("INSERT INTO twitch_player_steam_links VALUES ('42',76561198000000021,$1,1)")
            .bind(enabled)
            .execute(&self.pool)
            .await
            .unwrap();
    }
    async fn presence(&self, stamp: i64, in_match: bool) {
        Mock::given(method("GET"))
            .and(path("/internal/player-live"))
            .and(query_param("steam_id", "76561198000000021"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "found":true,"in_match":in_match,"in_deadlock":true,"hero":"Haze",
                "stage":if in_match { "match" } else { "lobby" },"last_update":stamp
            })))
            .mount(&self.server)
            .await;
    }
    async fn read(&self) -> SteamMatchSummary {
        summary(&self.pool, &self.client, "42", Utc::now())
            .await
            .unwrap()
    }
    async fn cleanup(self) {
        self.pool.close().await;
        sqlx::query(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .execute(&self.admin)
            .await
            .unwrap();
        self.admin.close().await;
    }
}

#[tokio::test]
async fn ohne_engagement_profil_ist_nichts_verknuepft() {
    let f = Fixture::new().await;
    let result = f.read().await;
    assert!(!result.steam_linked);
    assert!(result.state.is_none());
    assert!(result.observed_at.is_none());
    assert!(f.server.received_requests().await.unwrap().is_empty());
    f.cleanup().await;
}
#[tokio::test]
async fn frische_presence_liefert_match_status() {
    let f = Fixture::new().await;
    f.legacy(" 76561198000000021 ").await;
    f.presence(Utc::now().timestamp() - 30, true).await;
    let result = f.read().await;
    assert!(result.steam_linked);
    let state = result.state.unwrap();
    assert!(state.in_match);
    assert!(state.in_deadlock);
    assert_eq!(state.hero.as_deref(), Some("Haze"));
    assert_eq!(state.stage.as_deref(), Some("match"));
    f.cleanup().await;
}
#[tokio::test]
async fn veraltete_presence_faellt_nicht_auf_chatruhe_zurueck_und_zeigt_das_alter() {
    let f = Fixture::new().await;
    f.legacy("76561198000000021").await;
    let stamp = Utc::now().timestamp() - 240;
    f.presence(stamp, false).await;
    let result = f.read().await;
    assert!(result.steam_linked);
    assert!(result.state.is_none());
    assert_eq!(result.observed_at.unwrap().timestamp(), stamp);
    f.cleanup().await;
}
#[tokio::test]
async fn steam_id_ohne_presence_zusaetze_ist_verknuepft_ohne_status() {
    let f = Fixture::new().await;
    f.legacy("76561198000000021").await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"found":false})))
        .mount(&f.server)
        .await;
    let result = f.read().await;
    assert!(result.steam_linked);
    assert!(result.state.is_none());
    assert!(result.observed_at.is_none());
    f.cleanup().await;
}
#[tokio::test]
async fn leerer_steam_id_eintrag_ist_nicht_verknuepft() {
    let f = Fixture::new().await;
    f.legacy("   ").await;
    assert!(!f.read().await.steam_linked);
    assert!(f.server.received_requests().await.unwrap().is_empty());
    f.cleanup().await;
}
#[tokio::test]
async fn direkter_link_braucht_kein_engagement_und_hat_vorrang() {
    let f = Fixture::new().await;
    f.direct(true).await;
    f.legacy("76561198000000022").await;
    f.presence(Utc::now().timestamp(), false).await;
    let result = f.read().await;
    assert!(result.steam_linked);
    assert!(!result.state.unwrap().in_match);
    f.cleanup().await;
}
#[tokio::test]
async fn optout_verhindert_jeden_legacy_abruf() {
    let f = Fixture::new().await;
    f.direct(false).await;
    f.legacy("76561198000000021").await;
    assert!(!f.read().await.steam_linked);
    assert!(f.server.received_requests().await.unwrap().is_empty());
    f.cleanup().await;
}
#[tokio::test]
async fn unlink_waehrend_http_verwirft_alten_status() {
    let f = Fixture::new().await;
    f.direct(true).await;
    Mock::given(method("GET")).respond_with(ResponseTemplate::new(200).set_delay(StdDuration::from_millis(300))
        .set_body_json(json!({"found":true,"in_match":false,"in_deadlock":true,"last_update":Utc::now().timestamp()})))
        .mount(&f.server).await;
    let pool = f.pool.clone();
    let client = f.client.clone();
    let task =
        tokio::spawn(async move { summary(&pool, &client, "42", Utc::now()).await.unwrap() });
    tokio::time::timeout(StdDuration::from_secs(2), async {
        while f.server.received_requests().await.unwrap().is_empty() {
            tokio::time::sleep(StdDuration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    sqlx::query("UPDATE twitch_player_steam_links SET lookup_enabled=false,revision=2 WHERE twitch_user_id='42'").execute(&f.pool).await.unwrap();
    let result = task.await.unwrap();
    assert!(!result.steam_linked);
    assert!(result.state.is_none());
    f.cleanup().await;
}
#[tokio::test]
async fn discord_verknuepfung_nutzt_bestehenden_http_vertrag() {
    let f = Fixture::new().await;
    sqlx::query("INSERT INTO twitch_streamer_identities VALUES ('42','123')")
        .execute(&f.pool)
        .await
        .unwrap();
    Mock::given(method("GET")).and(path("/player-live")).and(query_param("discord_id","123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"linked":true,"live":true,"in_deadlock":true,"last_update":Utc::now().timestamp()})))
        .mount(&f.server).await;
    assert!(f.read().await.state.unwrap().in_match);
    f.cleanup().await;
}
