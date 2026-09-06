use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{Duration, Utc};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_chat::types::{ChatBadge, ChatMessageEvent};
use tb_chat::zuschauer_register::{
    GateOutcome, MemberIndexSource, MemberLite, ZuschauerRegister,
};

macro_rules! pool_or_skip {
    ($schema:expr) => {{
        let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
            if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
            }
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        pool_in_schema(&dsn, $schema).await
    }};
}

async fn pool_in_schema(dsn: &str, schema: &str) -> PgPool {
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(dsn)
        .await
        .unwrap();
    sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;

    let opts = PgConnectOptions::from_str(dsn)
        .unwrap()
        .options([("search_path", schema)]);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(opts)
        .await
        .unwrap();
    apply_ddl(&pool).await;
    pool
}

async fn apply_ddl(pool: &PgPool) {
    for ddl in [
        r#"CREATE TABLE twitch_zuschauer_register (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT,
            discord_user_id TEXT,
            community_probability DOUBLE PRECISION NOT NULL,
            signals JSONB NOT NULL DEFAULT '{}'::jsonb,
            first_partner_channel TEXT,
            first_seen_at TIMESTAMPTZ,
            computed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )"#,
        r#"CREATE TABLE twitch_streamer_identities (
            twitch_user_id TEXT NOT NULL,
            twitch_login TEXT NOT NULL,
            discord_user_id TEXT
        )"#,
        r#"CREATE TABLE twitch_stream_sessions (
            id BIGINT PRIMARY KEY,
            streamer_login TEXT NOT NULL,
            started_at TIMESTAMPTZ NOT NULL
        )"#,
        r#"CREATE TABLE twitch_live_state (
            streamer_login TEXT NOT NULL,
            twitch_user_id TEXT,
            active_session_id BIGINT,
            is_live INTEGER DEFAULT 0
        )"#,
        r#"CREATE TABLE twitch_partners (twitch_user_id TEXT NOT NULL, status TEXT)"#,
        r#"CREATE TABLE twitch_streamers (twitch_user_id TEXT)"#,
        r#"CREATE TABLE twitch_raid_auth (twitch_user_id TEXT NOT NULL)"#,
        r#"CREATE TABLE twitch_partner_signup_denylist (twitch_user_id TEXT NOT NULL)"#,
        r#"CREATE TABLE twitch_scout_pitch_blacklist (twitch_user_id TEXT)"#,
        r#"CREATE TABLE twitch_partner_outreach (
            twitch_user_id TEXT,
            streamer_login TEXT,
            contacted_at TEXT
        )"#,
    ] {
        sqlx::query(ddl).execute(pool).await.unwrap();
    }
}

struct TestMembers(Vec<MemberLite>);

#[async_trait]
impl MemberIndexSource for TestMembers {
    async fn fetch_members(&self) -> Option<Vec<MemberLite>> {
        Some(self.0.clone())
    }
}

struct NoMembers;

#[async_trait]
impl MemberIndexSource for NoMembers {
    async fn fetch_members(&self) -> Option<Vec<MemberLite>> {
        None
    }
}

fn event(channel: &str, chatter_id: &str, chatter_login: &str, mod_badge: bool) -> ChatMessageEvent {
    let badges = if mod_badge {
        vec![ChatBadge {
            set_id: "moderator".to_string(),
            id: String::new(),
            info: String::new(),
        }]
    } else {
        Vec::new()
    };
    ChatMessageEvent {
        broadcaster_user_login: channel.to_string(),
        chatter_user_id: chatter_id.to_string(),
        chatter_user_login: chatter_login.to_string(),
        badges,
        ..Default::default()
    }
}

async fn insert_register(
    pool: &PgPool,
    uid: &str,
    p: f64,
    first_seen: chrono::DateTime<Utc>,
    computed: chrono::DateTime<Utc>,
) {
    sqlx::query(
        "INSERT INTO twitch_zuschauer_register
            (twitch_user_id, twitch_login, discord_user_id, community_probability,
             signals, first_partner_channel, first_seen_at, computed_at)
         VALUES ($1, $2, NULL, $3, '{}'::jsonb, $4, $5, $6)",
    )
    .bind(uid)
    .bind(uid)
    .bind(p)
    .bind("somechannel")
    .bind(first_seen)
    .bind(computed)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn gate_lehnt_ohne_register_eintrag_ab() {
    let pool = pool_or_skip!("tb_zr_kein_eintrag");
    let register = ZuschauerRegister::new(pool.clone(), Arc::new(NoMembers));
    let ev = event("somechannel", "u1", "neuling", false);
    assert_eq!(register.gate(&ev).await, GateOutcome::Reject("register_fehlt"));
}

#[tokio::test]
async fn gate_lehnt_bei_hoher_wahrscheinlichkeit_ab() {
    let pool = pool_or_skip!("tb_zr_hohe_p");
    let now = Utc::now();
    insert_register(&pool, "u2", 0.9, now, now).await;
    let register = ZuschauerRegister::new(pool.clone(), Arc::new(TestMembers(Vec::new())));
    let ev = event("somechannel", "u2", "bekannt", false);
    assert_eq!(
        register.gate(&ev).await,
        GateOutcome::Reject("register_community")
    );
}

#[tokio::test]
async fn gate_lehnt_alt_bekannten_ab() {
    let pool = pool_or_skip!("tb_zr_alt_bekannt");
    let now = Utc::now();
    let first_seen = now - Duration::days(30);
    insert_register(&pool, "u3", 0.2, first_seen, now).await;
    sqlx::query("INSERT INTO twitch_stream_sessions (id, streamer_login, started_at) VALUES (77, 'somechannel', $1)")
        .bind(now - Duration::hours(2))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO twitch_live_state (streamer_login, active_session_id, is_live) VALUES ('somechannel', 77, 1)")
        .execute(&pool)
        .await
        .unwrap();
    let register = ZuschauerRegister::new(pool.clone(), Arc::new(TestMembers(Vec::new())));
    let ev = event("somechannel", "u3", "altgast", false);
    assert_eq!(register.gate(&ev).await, GateOutcome::Reject("kein_neuling"));
}

#[tokio::test]
async fn gate_lehnt_partner_streamer_raid_denylist_blacklist_outreach_ab() {
    let faelle: [(&str, &str); 6] = [
        ("twitch_partners", "INSERT INTO twitch_partners (twitch_user_id, status) VALUES ($1, 'active')"),
        ("twitch_streamers", "INSERT INTO twitch_streamers (twitch_user_id) VALUES ($1)"),
        ("twitch_raid_auth", "INSERT INTO twitch_raid_auth (twitch_user_id) VALUES ($1)"),
        ("twitch_partner_signup_denylist", "INSERT INTO twitch_partner_signup_denylist (twitch_user_id) VALUES ($1)"),
        ("twitch_scout_pitch_blacklist", "INSERT INTO twitch_scout_pitch_blacklist (twitch_user_id) VALUES ($1)"),
        ("twitch_partner_outreach", "INSERT INTO twitch_partner_outreach (twitch_user_id, contacted_at) VALUES ($1, 'irgendwann')"),
    ];
    for (idx, (tabelle, insert_sql)) in faelle.iter().enumerate() {
        let schema = format!("tb_zr_ausschluss_{idx}");
        let pool = pool_or_skip!(&schema);
        let uid = "u4";
        let now = Utc::now();
        insert_register(&pool, uid, 0.2, now, now).await;
        sqlx::query(insert_sql).bind(uid).execute(&pool).await.unwrap();
        let register = ZuschauerRegister::new(pool.clone(), Arc::new(TestMembers(Vec::new())));
        let ev = event("somechannel", uid, "kandidat", false);
        assert_eq!(
            register.gate(&ev).await,
            GateOutcome::Reject("partner_oder_streamer"),
            "Ausschluss ueber {tabelle} muss greifen"
        );
    }
}

#[tokio::test]
async fn gate_lehnt_mod_und_bot_ab() {
    let pool = pool_or_skip!("tb_zr_mod_bot");
    let register = ZuschauerRegister::new(pool.clone(), Arc::new(TestMembers(Vec::new())));
    let mod_ev = event("somechannel", "u5", "einmod", true);
    assert_eq!(
        register.gate(&mod_ev).await,
        GateOutcome::Reject("broadcaster_mod_bot")
    );
    let bot_ev = event("somechannel", "u6", "nightbot", false);
    assert_eq!(
        register.gate(&bot_ev).await,
        GateOutcome::Reject("broadcaster_mod_bot")
    );
}

#[tokio::test]
async fn gate_laesst_frischen_neuling_durch() {
    let pool = pool_or_skip!("tb_zr_neuling_durch");
    let register = ZuschauerRegister::new(pool.clone(), Arc::new(TestMembers(Vec::new())));
    let ev = event("somechannel", "u7", "frischerneuling", false);
    assert_eq!(register.gate(&ev).await, GateOutcome::Pass);
}

#[tokio::test]
async fn ensure_current_erneuert_nach_sieben_tagen_ohne_first_seen_zu_aendern() {
    let pool = pool_or_skip!("tb_zr_erneuert");
    let first_seen = Utc::now() - Duration::days(40);
    let alt_computed = Utc::now() - Duration::days(10);
    insert_register(&pool, "u8", 0.2, first_seen, alt_computed).await;
    let register = ZuschauerRegister::new(pool.clone(), Arc::new(TestMembers(Vec::new())));
    let entry = register
        .ensure_current("u8", "u8", "somechannel")
        .await
        .expect("entry");
    assert!((entry.first_seen_at - first_seen).num_seconds().abs() <= 1);
    assert!((Utc::now() - entry.computed_at).num_seconds() < 60);
}
