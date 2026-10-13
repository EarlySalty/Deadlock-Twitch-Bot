#[path = "../../../test-support/postgres.rs"]
mod test_postgres;

use sqlx::PgPool;
use tb_raid::OfflineEligibilityStore;

async fn fixture() -> test_postgres::TestPostgres {
    let db = test_postgres::TestPostgres::start().await;
    sqlx::raw_sql(
        "CREATE TABLE twitch_partners (
            id BIGSERIAL PRIMARY KEY, twitch_user_id TEXT NOT NULL,
            twitch_login TEXT NOT NULL, status TEXT NOT NULL,
            raid_bot_enabled INTEGER DEFAULT 0, manual_partner_opt_out INTEGER DEFAULT 0,
            technical_pause_reason TEXT, admin_archived_at TEXT, departnered_at TEXT,
            require_discord_link INTEGER, next_link_check_at TEXT, partnered_at TEXT,
            silent_ban INTEGER, silent_raid INTEGER, verified BOOLEAN DEFAULT FALSE,
            live_ping_role_id BIGINT, live_ping_enabled INTEGER, inactivity_flagged_at TEXT
        );
        CREATE TABLE twitch_streamer_identities (
            twitch_user_id TEXT PRIMARY KEY, discord_user_id TEXT,
            discord_display_name TEXT, is_on_discord INTEGER
        );
        CREATE TABLE twitch_raid_auth (
            twitch_user_id TEXT PRIMARY KEY, raid_enabled BOOLEAN, authorized_at TIMESTAMPTZ
        );
        CREATE TABLE discord_guilds (
            guild_id BIGINT PRIMARY KEY, status TEXT NOT NULL DEFAULT 'active', removed_at TIMESTAMPTZ
        );
        CREATE TABLE streamer_guilds (
            twitch_user_id TEXT, guild_id BIGINT REFERENCES discord_guilds(guild_id),
            active BOOLEAN NOT NULL DEFAULT TRUE, PRIMARY KEY (twitch_user_id, guild_id)
        );
        CREATE TABLE twitch_exclusions (
            twitch_user_id TEXT PRIMARY KEY, kind TEXT, reactivated_at TIMESTAMPTZ
        );
        CREATE TABLE twitch_partner_signup_denylist (twitch_user_id TEXT, twitch_login TEXT);
        INSERT INTO discord_guilds (guild_id) VALUES (1), (2);",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../migrations/20261013150100_multiserver_partner_contract.sql"
    ))
    .execute(&db.pool)
    .await
    .unwrap();
    let mut tx = db.pool.begin().await.unwrap();
    sqlx::query(
        "CREATE VIEW twitch_partners_all_state AS SELECT * FROM twitch_partners_legacy_state",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../migrations/20261013150300_multiserver_partner_cutover.sql"
    ))
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    db
}

async fn insert_partner(pool: &PgPool, user_id: &str, status: &str, raid_bot_enabled: i32) {
    sqlx::query(
        "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status, raid_bot_enabled)
         VALUES ($1, $1, $2, $3)",
    )
    .bind(user_id)
    .bind(status)
    .bind(raid_bot_enabled)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO streamer_guilds (twitch_user_id, guild_id) VALUES ($1, 1) ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_auth(pool: &PgPool, user_id: &str, raid_enabled: bool) {
    sqlx::query("INSERT INTO twitch_raid_auth (twitch_user_id, raid_enabled) VALUES ($1, $2)")
        .bind(user_id)
        .bind(raid_enabled)
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn voll_freigeschalteter_partner_darf_auto_raiden() {
    let db = fixture().await;
    let pool = db.pool.clone();
    insert_partner(&pool, "42", "active", 1).await;
    insert_auth(&pool, "42", true).await;

    let elig = OfflineEligibilityStore::new(pool).load("42").await.unwrap();
    assert!(elig.active_partner);
    assert!(elig.auth_row_found);
    assert!(elig.raid_bot_enabled);
    assert!(elig.raid_auth_enabled);
    assert!(elig.can_auto_raid());
    assert_eq!(elig.skip_reason(), None);
}

#[tokio::test]
async fn unbekannter_streamer_wird_uebersprungen() {
    let db = fixture().await;
    let elig = OfflineEligibilityStore::new(db.pool.clone())
        .load("99")
        .await
        .unwrap();
    assert!(!elig.active_partner);
    assert!(!elig.auth_row_found);
    assert_eq!(elig.skip_reason(), Some("not_found"));
}

#[tokio::test]
async fn archivierter_partner_mit_auth_zaehlt_nicht_als_aktiv() {
    let db = fixture().await;
    let pool = db.pool.clone();
    insert_partner(&pool, "42", "archived", 1).await;
    insert_auth(&pool, "42", true).await;

    let elig = OfflineEligibilityStore::new(pool).load("42").await.unwrap();
    assert!(!elig.active_partner);
    assert!(elig.auth_row_found);
    assert!(!elig.can_auto_raid());
    assert_eq!(elig.skip_reason(), Some("not_active_partner"));
}

#[tokio::test]
async fn raid_bot_setting_aus_blockiert() {
    let db = fixture().await;
    let pool = db.pool.clone();
    insert_partner(&pool, "42", "active", 0).await;
    insert_auth(&pool, "42", true).await;

    let elig = OfflineEligibilityStore::new(pool).load("42").await.unwrap();
    assert!(elig.active_partner);
    assert!(!elig.raid_bot_enabled);
    assert_eq!(elig.skip_reason(), Some("setting_disabled"));
}

#[tokio::test]
async fn fehlende_oder_deaktivierte_auth_blockiert() {
    let db = fixture().await;
    let pool = db.pool.clone();
    insert_partner(&pool, "42", "active", 1).await;
    insert_auth(&pool, "42", false).await;
    insert_partner(&pool, "77", "active", 1).await;

    let store = OfflineEligibilityStore::new(pool);
    let elig = store.load("42").await.unwrap();
    assert!(elig.auth_row_found);
    assert!(!elig.raid_auth_enabled);
    assert_eq!(elig.skip_reason(), Some("no_auth"));

    let elig77 = store.load("77").await.unwrap();
    assert!(!elig77.auth_row_found);
    assert_eq!(elig77.skip_reason(), Some("no_auth"));
}

#[tokio::test]
async fn neueste_aktive_zeile_gewinnt_und_leere_id_ist_not_found() {
    let db = fixture().await;
    let pool = db.pool.clone();
    insert_partner(&pool, "42", "active", 0).await;
    insert_partner(&pool, "42", "active", 1).await;
    insert_auth(&pool, "42", true).await;

    let store = OfflineEligibilityStore::new(pool);
    let elig = store.load("42").await.unwrap();
    assert!(elig.raid_bot_enabled);
    assert!(elig.can_auto_raid());

    let leer = store.load("  ").await.unwrap();
    assert_eq!(leer.skip_reason(), Some("not_found"));
}

#[tokio::test]
async fn last_guild_deactivation_and_removed_guild_block_offline_raids() {
    let db = fixture().await;
    let pool = &db.pool;
    insert_partner(pool, "42", "active", 1).await;
    insert_auth(pool, "42", true).await;
    let store = OfflineEligibilityStore::new(pool.clone());
    assert!(store.load("42").await.unwrap().can_auto_raid());
    sqlx::query("UPDATE streamer_guilds SET active = FALSE")
        .execute(pool)
        .await
        .unwrap();
    assert_eq!(
        store.load("42").await.unwrap().skip_reason(),
        Some("not_active_partner")
    );
    sqlx::raw_sql("UPDATE streamer_guilds SET active = TRUE; UPDATE discord_guilds SET removed_at = NOW() WHERE guild_id = 1;")
        .execute(pool).await.unwrap();
    assert!(!store.load("42").await.unwrap().can_auto_raid());
}

#[tokio::test]
async fn another_installed_guild_retains_offline_eligibility() {
    let db = fixture().await;
    let pool = &db.pool;
    insert_partner(pool, "42", "active", 1).await;
    insert_auth(pool, "42", true).await;
    sqlx::raw_sql("INSERT INTO streamer_guilds VALUES ('42', 2, TRUE); UPDATE streamer_guilds SET active = FALSE WHERE guild_id = 1;")
        .execute(pool).await.unwrap();
    let store = OfflineEligibilityStore::new(pool.clone());
    assert!(store.load("42").await.unwrap().can_auto_raid());
    sqlx::query("UPDATE discord_guilds SET status = 'removed' WHERE guild_id = 2")
        .execute(pool)
        .await
        .unwrap();
    assert!(!store.load("42").await.unwrap().can_auto_raid());
}

#[tokio::test]
async fn global_settings_and_bans_override_active_membership() {
    let db = fixture().await;
    let pool = &db.pool;
    insert_partner(pool, "42", "active", 1).await;
    insert_auth(pool, "42", true).await;
    let store = OfflineEligibilityStore::new(pool.clone());
    for change in [
        "UPDATE twitch_partners SET manual_partner_opt_out = 1",
        "UPDATE twitch_partners SET technical_pause_reason = 'token_error'",
        "UPDATE twitch_partners SET inactivity_flagged_at = '2026-01-01T00:00:00Z'",
        "UPDATE twitch_partners SET admin_archived_at = '2026-01-01T00:00:00Z'",
        "UPDATE twitch_partners SET technical_pause_reason = ' Bot_Banned '",
        "INSERT INTO twitch_exclusions VALUES ('42', 'banned', NULL)",
        "INSERT INTO twitch_partner_signup_denylist VALUES ('999', '42')",
    ] {
        let mut tx = pool.begin().await.unwrap();
        sqlx::query(sqlx::AssertSqlSafe(change))
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert!(!store.load("42").await.unwrap().can_auto_raid());
        sqlx::raw_sql("UPDATE twitch_partners SET manual_partner_opt_out = 0, technical_pause_reason = NULL, admin_archived_at = NULL, inactivity_flagged_at = NULL; DELETE FROM twitch_exclusions; DELETE FROM twitch_partner_signup_denylist;")
            .execute(pool).await.unwrap();
        assert!(store.load("42").await.unwrap().can_auto_raid());
    }
}

#[tokio::test]
async fn unavailable_membership_projection_fails_closed() {
    let db = fixture().await;
    insert_partner(&db.pool, "42", "active", 1).await;
    insert_auth(&db.pool, "42", true).await;
    sqlx::query("ALTER VIEW partner_guild_active RENAME TO unavailable_membership")
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("DROP VIEW twitch_streamers_partner_state, partner_status_shadow_mismatch, twitch_partners_all_state").execute(&db.pool).await.unwrap();
    assert!(OfflineEligibilityStore::new(db.pool.clone())
        .load("42")
        .await
        .is_err());
}
