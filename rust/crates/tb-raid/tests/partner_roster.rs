//! Tests des Partner-Roster-Readers + Online-Kandidaten-Aufbau.

use std::collections::HashMap;
use std::str::FromStr;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_raid::{build_online_candidates, PartnerRosterEntry, PartnerRosterStore, StreamData};

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
    // is_partner_active ist INTEGER (Prod).
    sqlx::query(
        "CREATE TABLE twitch_streamers_partner_state (
            twitch_login TEXT, twitch_user_id TEXT, is_partner_active INTEGER )",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TABLE twitch_raid_auth (
            twitch_user_id TEXT PRIMARY KEY, raid_enabled BOOLEAN, authorized_at TIMESTAMPTZ )",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

#[tokio::test]
async fn roster_filtert_aktiv_quelle_und_nicht_autorisierte() {
    let pool = pool_or_skip!("t6f_roster");
    sqlx::query(
        "INSERT INTO twitch_streamers_partner_state (twitch_login, twitch_user_id, is_partner_active) VALUES
            ('Quelle', '100', 1),       -- Quelle selbst → raus
            ('Enabled', '200', 1),      -- raid_enabled → rein
            ('OnlyAuth', '300', 1),     -- nur authorized_at → rein
            ('Neither', '400', 1),      -- weder noch → raus
            ('Inaktiv', '500', 0)       -- nicht aktiv → raus",
    ).execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO twitch_raid_auth (twitch_user_id, raid_enabled, authorized_at) VALUES
            ('200', TRUE, NULL),
            ('300', FALSE, NOW()),
            ('400', FALSE, NULL)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let mut roster = PartnerRosterStore::new(pool)
        .load_roster("100")
        .await
        .unwrap();
    roster.sort_by(|a, b| a.twitch_user_id.cmp(&b.twitch_user_id));
    assert_eq!(
        roster,
        vec![
            PartnerRosterEntry {
                twitch_login: "enabled".into(),
                twitch_user_id: "200".into(),
                raid_enabled: true
            },
            PartnerRosterEntry {
                twitch_login: "onlyauth".into(),
                twitch_user_id: "300".into(),
                raid_enabled: true
            },
        ]
    );
}

#[test]
fn online_kandidaten_nur_fuer_live_partner() {
    let roster = vec![
        PartnerRosterEntry {
            twitch_login: "live".into(),
            twitch_user_id: "1".into(),
            raid_enabled: true,
        },
        PartnerRosterEntry {
            twitch_login: "offline".into(),
            twitch_user_id: "2".into(),
            raid_enabled: true,
        },
    ];
    let mut streams = HashMap::new();
    streams.insert(
        "live".to_string(),
        StreamData {
            viewer_count: 50,
            ..Default::default()
        },
    );

    let candidates = build_online_candidates(&roster, &streams);
    assert_eq!(candidates.len(), 1, "nur der live Partner");
    assert_eq!(candidates[0].twitch_user_id, "1");
    assert_eq!(candidates[0].stream.viewer_count, 50);
}
