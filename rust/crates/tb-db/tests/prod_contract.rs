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
    for removed in [
        "discord_user_id",
        "discord_display_name",
        "is_on_discord",
        "is_monitored_only",
        "archived_at",
    ] {
        assert!(
            !streamers.contains_key(removed),
            "{removed} darf nicht mehr auf twitch_streamers liegen"
        );
    }

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

#[tokio::test]
async fn prod_monitoring_tables_match_contract() {
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

    // Verifiziert 2026-06-09: Das pg.py-DDL ist veraltet — Prod hat für die
    // Session-Tabellen längst timestamptz/boolean/bigint. Der Rust-Port bindet
    // diese Typen direkt; dieser Test schlägt an, falls Prod davon abweicht.
    let sessions = column_types(&pool, "twitch_stream_sessions").await;
    assert_eq!(sessions.get("id").map(String::as_str), Some("bigint"));
    assert_eq!(
        sessions.get("started_at").map(String::as_str),
        Some("timestamp with time zone")
    );
    assert_eq!(
        sessions.get("is_mature").map(String::as_str),
        Some("boolean")
    );
    assert_eq!(
        sessions.get("had_deadlock_in_session").map(String::as_str),
        Some("boolean")
    );
    assert_eq!(
        sessions.get("avg_viewers").map(String::as_str),
        Some("double precision")
    );

    let viewers = column_types(&pool, "twitch_session_viewers").await;
    assert_eq!(
        viewers.get("session_id").map(String::as_str),
        Some("bigint")
    );
    assert_eq!(
        viewers.get("ts_utc").map(String::as_str),
        Some("timestamp with time zone")
    );

    let chat_messages = column_types(&pool, "twitch_chat_messages").await;
    assert_eq!(
        chat_messages.get("session_id").map(String::as_str),
        Some("bigint")
    );
    assert_eq!(
        chat_messages.get("message_ts").map(String::as_str),
        Some("timestamp with time zone")
    );

    let raid_retention = column_types(&pool, "twitch_raid_retention").await;
    assert_eq!(
        raid_retention.get("target_session_id").map(String::as_str),
        Some("bigint")
    );

    // Live-State dagegen führt TEXT-Timestamps und INTEGER-Flags.
    let live_state = column_types(&pool, "twitch_live_state").await;
    assert_eq!(
        live_state.get("last_seen_at").map(String::as_str),
        Some("text")
    );
    assert_eq!(
        live_state.get("is_live").map(String::as_str),
        Some("integer")
    );
    assert_eq!(
        live_state.get("active_session_id").map(String::as_str),
        Some("bigint")
    );

    let stats = column_types(&pool, "twitch_stats_tracked").await;
    assert_eq!(
        stats.get("ts_utc").map(String::as_str),
        Some("timestamp with time zone")
    );
    assert_eq!(stats.get("is_partner").map(String::as_str), Some("boolean"));

    let guard = column_types(&pool, "eventsub_guard_state").await;
    assert_eq!(
        guard.get("expires_at").map(String::as_str),
        Some("double precision")
    );

    let exp = column_types(&pool, "exp_sessions").await;
    assert_eq!(exp.get("started_at").map(String::as_str), Some("text"));
    assert_eq!(exp.get("avg_viewers").map(String::as_str), Some("real"));
}
