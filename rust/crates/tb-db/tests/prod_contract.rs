//! Read-only Schema-Vertrag gegen die echte DB (`TWITCH_ANALYTICS_DSN`).
//! Prüft, dass die erwarteten Owner-Spalten mit den erwarteten Typen existieren.
//! Liest nur `information_schema` — keine Zeilendaten, keine Secrets in Ausgaben.

use std::collections::HashMap;
use std::time::Duration;

use sqlx::Row;
use tb_config::DbConfig;

fn prod_dsn() -> Option<String> {
    std::env::var("TWITCH_ANALYTICS_DSN").ok()
}

async fn column_types(pool: &sqlx::PgPool, table: &str) -> HashMap<String, String> {
    let rows = sqlx::query(
        "SELECT column_name, data_type FROM information_schema.columns WHERE table_name = $1",
    )
    .bind(table)
    .fetch_all(pool)
    .await
    .expect("information_schema query");
    rows.into_iter()
        .map(|r| {
            (
                r.get::<String, _>("column_name"),
                r.get::<String, _>("data_type"),
            )
        })
        .collect()
}

#[tokio::test]
async fn prod_owner_tables_match_contract() {
    let dsn = match prod_dsn() {
        Some(d) => d,
        None => {
            eprintln!(
                "SKIP: TWITCH_ANALYTICS_DSN nicht gesetzt — Vertrags-Test wird übersprungen."
            );
            return;
        }
    };
    let cfg = DbConfig {
        dsn,
        pool_max: 2,
        acquire_timeout: Duration::from_secs(5),
        connect_timeout: Duration::from_secs(5),
    };
    let pool = tb_db::connect(&cfg)
        .await
        .expect("connect prod (read-only)");

    let streamers = column_types(&pool, "twitch_streamers").await;
    assert_eq!(
        streamers.get("twitch_login").map(String::as_str),
        Some("text")
    );
    assert_eq!(
        streamers.get("twitch_user_id").map(String::as_str),
        Some("text")
    );
    assert_eq!(
        streamers.get("is_on_discord").map(String::as_str),
        Some("integer")
    );

    let partners = column_types(&pool, "twitch_partners").await;
    assert_eq!(partners.get("id").map(String::as_str), Some("bigint"));
    assert_eq!(partners.get("status").map(String::as_str), Some("text"));
    assert_eq!(
        partners.get("live_ping_role_id").map(String::as_str),
        Some("bigint")
    );

    let plans = column_types(&pool, "streamer_plans").await;
    assert_eq!(plans.get("plan_name").map(String::as_str), Some("text"));
    assert_eq!(
        plans.get("promo_disabled").map(String::as_str),
        Some("integer")
    );
}
