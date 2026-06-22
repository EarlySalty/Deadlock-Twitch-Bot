//! Hermetische Tests des Token-Lockout-Stores (`twitch_token_blacklist`,
//! Alt-Stil: TEXT-Timestamps, INTEGER-Flags, error_count DEFAULT 1).

use std::str::FromStr;

use chrono::{Duration, SecondsFormat, Utc};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_raid::token_refresher::TokenBlacklist;
use tb_raid::TokenBlacklistStore;

macro_rules! pool_or_skip {
    ($schema:expr) => {{
        let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
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
    // Prod-Typen: TEXT-Timestamps, INTEGER-Flags, error_count DEFAULT 1.
    sqlx::query(
        "CREATE TABLE twitch_token_blacklist (
            twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT, error_message TEXT,
            error_count INTEGER DEFAULT 1, first_error_at TEXT, last_error_at TEXT,
            notified INTEGER DEFAULT 0, grace_expires_at TEXT,
            user_dm_sent INTEGER DEFAULT 0, reminder_sent INTEGER DEFAULT 0,
            role_removed INTEGER DEFAULT 0
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    // add_to_blacklist setzt im selben Tx den Raid-Lockout (raid_enabled=FALSE,
    // needs_reauth=TRUE). Prod-Typen: raid_enabled/needs_reauth sind BOOLEAN.
    sqlx::query(
        "CREATE TABLE twitch_raid_auth (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT,
            raid_enabled BOOLEAN DEFAULT TRUE,
            needs_reauth BOOLEAN DEFAULT FALSE
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    // add_to_blacklist spiegelt den Pause-Grund in twitch_partners.
    sqlx::query(
        "CREATE TABLE twitch_partners (
            twitch_user_id TEXT,
            twitch_login TEXT,
            technical_pause_reason TEXT,
            raid_bot_enabled INTEGER DEFAULT 1,
            manual_partner_opt_out INTEGER DEFAULT 0
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

fn iso_ago(hours: i64) -> String {
    (Utc::now() - Duration::hours(hours)).to_rfc3339_opts(SecondsFormat::Secs, false)
}

#[tokio::test]
async fn add_legt_an_inkrementiert_und_resettet_nach_fenster() {
    let pool = pool_or_skip!("t6b_bl_add");
    let store = TokenBlacklistStore::new(pool.clone());

    // Neuer Eintrag → error_count DEFAULT 1, Grace gesetzt.
    store.add_to_blacklist("42", "drag", "boom").await;
    let (count, grace): (i32, Option<String>) = sqlx::query_as(
        "SELECT error_count, grace_expires_at FROM twitch_token_blacklist WHERE twitch_user_id='42'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
    assert!(grace.is_some());

    // Zweiter Fehler innerhalb des Fensters → Counter +1.
    store.add_to_blacklist("42", "drag", "boom2").await;
    let count: i32 = sqlx::query_scalar(
        "SELECT error_count FROM twitch_token_blacklist WHERE twitch_user_id='42'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 2);

    // Letzter Fehler > 12 h her → Reset auf 1.
    sqlx::query("UPDATE twitch_token_blacklist SET error_count=2, last_error_at=$1 WHERE twitch_user_id='42'")
        .bind(iso_ago(13))
        .execute(&pool)
        .await
        .unwrap();
    store.add_to_blacklist("42", "drag", "spaeter").await;
    let count: i32 = sqlx::query_scalar(
        "SELECT error_count FROM twitch_token_blacklist WHERE twitch_user_id='42'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1, "ausserhalb 12h-Fenster → Reset");
}

#[tokio::test]
async fn is_blacklisted_ab_schwelle_und_recent_failure_im_cooldown() {
    let pool = pool_or_skip!("t6b_bl_checks");
    let store = TokenBlacklistStore::new(pool.clone());

    // error_count 2, letzter Fehler 1 h her → recent failure, NICHT blacklisted.
    sqlx::query(
        "INSERT INTO twitch_token_blacklist (twitch_user_id, twitch_login, error_count, last_error_at)
         VALUES ('42', 'drag', 2, $1)",
    )
    .bind(iso_ago(1))
    .execute(&pool)
    .await
    .unwrap();
    assert!(!store.is_blacklisted("42").await);
    assert!(store.has_recent_failure("42").await);

    // Letzter Fehler 3 h her (> 2h Cooldown) → kein recent failure.
    sqlx::query("UPDATE twitch_token_blacklist SET last_error_at=$1 WHERE twitch_user_id='42'")
        .bind(iso_ago(3))
        .execute(&pool)
        .await
        .unwrap();
    assert!(!store.has_recent_failure("42").await);

    // error_count 3 → blacklisted; recent_failure dann false (separat behandelt).
    sqlx::query("UPDATE twitch_token_blacklist SET error_count=3, last_error_at=$1 WHERE twitch_user_id='42'")
        .bind(iso_ago(1))
        .execute(&pool)
        .await
        .unwrap();
    assert!(store.is_blacklisted("42").await);
    assert!(
        !store.has_recent_failure("42").await,
        "voll blacklisted → kein recent-failure-Cooldown"
    );

    // Unbekannt → beides false.
    assert!(!store.is_blacklisted("x").await);
    assert!(!store.has_recent_failure("x").await);
}

#[tokio::test]
async fn clear_loescht_den_eintrag() {
    let pool = pool_or_skip!("t6b_bl_clear");
    let store = TokenBlacklistStore::new(pool.clone());
    store.add_to_blacklist("42", "drag", "boom").await;
    store.clear_failure_count("42").await;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_token_blacklist")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

/// P2.28: Ein Partner, dessen Token via reinem Refresh (ohne Re-Auth) wieder
/// funktioniert, muss `technical_pause_reason='token_error'` verlieren — sonst
/// bleibt er in Dashboard/Analytics-Gates als pausiert hängen. clear_failure_count
/// räumt den Pause-Grund wie Python (token_error_handler.py:852-867) mit auf.
#[tokio::test]
async fn clear_raeumt_token_error_pause_reason() {
    let pool = pool_or_skip!("t6b_bl_clear_pause");
    let store = TokenBlacklistStore::new(pool.clone());

    // Drei Partner: token_error (zu räumen), bot_banned (Guard), NULL (No-Op).
    sqlx::query(
        "INSERT INTO twitch_partners (twitch_user_id, twitch_login, technical_pause_reason, raid_bot_enabled, manual_partner_opt_out)
         VALUES ('42','drag','token_error',0,0), ('88','banned','bot_banned',1,0), ('99','clean',NULL,1,0)",
    )
    .execute(&pool)
    .await
    .unwrap();
    // Blacklist-Eintrag für den token_error-Partner.
    sqlx::query(
        "INSERT INTO twitch_token_blacklist (twitch_user_id, twitch_login, error_count, last_error_at)
         VALUES ('42','drag',2,$1)",
    )
    .bind(iso_ago(1))
    .execute(&pool)
    .await
    .unwrap();

    store.clear_failure_count("42").await;

    let reason = |uid: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, Option<String>>(
                "SELECT technical_pause_reason FROM twitch_partners WHERE twitch_user_id=$1",
            )
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };

    // token_error → NULL geräumt.
    assert_eq!(reason("42").await, None, "token_error-Pause aufgehoben");
    // Blacklist-Eintrag gelöscht.
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_token_blacklist")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);

    // Fremde Pause-Gründe bleiben unangetastet.
    store.clear_failure_count("88").await;
    assert_eq!(
        reason("88").await.as_deref(),
        Some("bot_banned"),
        "bot_banned bleibt erhalten"
    );
    assert_eq!(reason("99").await, None);
}

#[tokio::test]
async fn add_spiegelt_token_error_in_partner_mit_guards() {
    let pool = pool_or_skip!("t6b_bl_mirror");
    let store = TokenBlacklistStore::new(pool.clone());

    // Drei Partner: normal, manueller Opt-out, bot_banned.
    sqlx::query(
        "INSERT INTO twitch_partners (twitch_user_id, twitch_login, technical_pause_reason, raid_bot_enabled, manual_partner_opt_out)
         VALUES ('42','drag',NULL,1,0), ('77','optout',NULL,1,1), ('88','banned','bot_banned',1,0)",
    )
    .execute(&pool)
    .await
    .unwrap();

    store.add_to_blacklist("42", "drag", "boom").await;
    store.add_to_blacklist("77", "optout", "boom").await;
    store.add_to_blacklist("88", "banned", "boom").await;

    let row = |uid: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query_as::<_, (Option<String>, i32)>(
                "SELECT technical_pause_reason, raid_bot_enabled FROM twitch_partners WHERE twitch_user_id=$1",
            )
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };

    // Normaler Partner: token_error gesetzt, raid_bot_enabled=0.
    let (reason, enabled) = row("42").await;
    assert_eq!(reason.as_deref(), Some("token_error"));
    assert_eq!(enabled, 0);

    // Manueller Opt-out: Pause-Grund unangetastet (Guard), aber raid_bot_enabled=0.
    let (reason, enabled) = row("77").await;
    assert_eq!(reason, None, "manueller Opt-out überschreibt nicht auf token_error");
    assert_eq!(enabled, 0);

    // bot_banned: Pause-Grund bleibt bot_banned (Guard).
    let (reason, _) = row("88").await;
    assert_eq!(reason.as_deref(), Some("bot_banned"), "bot_banned bleibt erhalten");
}
