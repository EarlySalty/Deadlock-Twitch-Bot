//! Hermetische DB-Tests für OutboundSuppressionStore.
//!
//! Schema-isoliert in eigenen Postgres-Schemata (prod-treue DDL).
//! Voraussetzung: `TB_TEST_DATABASE_URL=postgres://postgres:tbtest@127.0.0.1:5434/postgres`
//! Prod-Schema: `twitch_outbound_chat_suppressions.suppressed_until = timestamp with time zone`.

use std::str::FromStr;

use chrono::{Duration, Utc};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_chat::moderation::{
    OutboundSuppressionCheck, OutboundSuppressionStore, SUPPRESSION_DDL,
    SUPPRESSION_PARTNER_RAID_SECS, SUPPRESSION_PROMO_SECS,
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
    // Prod-treue DDL: suppressed_until = TIMESTAMPTZ.
    // Aus SUPPRESSION_DDL-Konstante (tb_chat::moderation::SUPPRESSION_DDL).
    for stmt in SUPPRESSION_DDL
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        sqlx::query(stmt).execute(pool).await.unwrap();
    }
}

// ---------------------------------------------------------------------------
// Upsert + Lesen
// ---------------------------------------------------------------------------

#[tokio::test]
async fn upsert_und_check_suppression() {
    let pool = pool_or_skip!("sup_upsert");
    let store = OutboundSuppressionStore::new(pool.clone());

    let ttl = Duration::seconds(SUPPRESSION_PROMO_SECS);
    store
        .upsert_suppression(
            "testkanal",
            Some("999"),
            "promo",
            "channel_settings",
            None,
            ttl,
        )
        .await
        .unwrap();

    let entry = store.check_suppression("testkanal", "promo").await;
    assert!(entry.is_some(), "Eintrag soll gefunden werden");
    let e = entry.unwrap();
    assert_eq!(e.target_login, "testkanal");
    assert_eq!(e.source, "promo");
    assert_eq!(e.reason_code, "channel_settings");
    assert!(
        e.suppressed_until > Utc::now(),
        "suppressed_until in der Zukunft"
    );
}

#[tokio::test]
async fn abgelaufene_suppression_wird_nicht_gefunden() {
    let pool = pool_or_skip!("sup_expired");
    let store = OutboundSuppressionStore::new(pool.clone());

    // Direkt in DB mit vergangenem suppressed_until schreiben
    let past = Utc::now() - Duration::seconds(1);
    sqlx::query(
        r#"INSERT INTO twitch_outbound_chat_suppressions
           (target_login, source, reason_code, suppressed_until, created_at, updated_at)
           VALUES ($1, $2, $3, $4, $5, $6)"#,
    )
    .bind("altkanal")
    .bind("promo")
    .bind("channel_settings")
    .bind(past)
    .bind(Utc::now())
    .bind(Utc::now())
    .execute(&pool)
    .await
    .unwrap();

    let entry = store.check_suppression("altkanal", "promo").await;
    assert!(entry.is_none(), "abgelaufene Suppression: None erwartet");
}

#[tokio::test]
async fn upsert_aktualisiert_bestehenden_eintrag() {
    let pool = pool_or_skip!("sup_upsert_update");
    let store = OutboundSuppressionStore::new(pool.clone());

    let ttl_kurz = Duration::seconds(10);
    store
        .upsert_suppression(
            "kanal2",
            None,
            "partner_raid",
            "channel_settings",
            None,
            ttl_kurz,
        )
        .await
        .unwrap();

    // Länger überschreiben
    let ttl_lang = Duration::seconds(SUPPRESSION_PARTNER_RAID_SECS);
    store
        .upsert_suppression(
            "kanal2",
            Some("777"),
            "partner_raid",
            "channel_settings",
            Some("Detail"),
            ttl_lang,
        )
        .await
        .unwrap();

    let entry = store.check_suppression("kanal2", "partner_raid").await;
    assert!(entry.is_some());
    let e = entry.unwrap();
    // target_id: COALESCE auf existierenden Wert (hier neu gesetzt)
    assert_eq!(e.target_id.as_deref(), Some("777"));
    assert_eq!(e.reason_detail.as_deref(), Some("Detail"));
    // suppressed_until muss weit in der Zukunft liegen (3 Tage)
    assert!(
        e.suppressed_until > Utc::now() + Duration::days(2),
        "3-Tage-TTL erwartet"
    );
}

#[tokio::test]
async fn check_suppression_gibt_none_fuer_unbekannte_source() {
    let pool = pool_or_skip!("sup_unknown_src");
    let store = OutboundSuppressionStore::new(pool);
    // Kein DB-Call bei unbekannter source
    let entry = store.check_suppression("kanal", "unknown_source").await;
    assert!(entry.is_none());
}

/// P1.1: Ein `channel_settings`-Drop eines Promo-Sends muss über die
/// Schreibseite (`OutboundSuppressionWriter::suppress_for_drop`) eine Zeile
/// mit suppressed_until ≈ now+7d anlegen, sodass `is_muted()` danach true ist.
#[tokio::test]
async fn suppress_for_drop_promo_channel_settings_schreibt_7d_und_mutet() {
    use tb_chat::promos::{OutboundSuppressionCheck as PromoCheck, OutboundSuppressionWriter};

    let pool = pool_or_skip!("sup_drop_promo");
    let store = OutboundSuppressionStore::new(pool.clone());

    // Vorher: nicht stumm.
    assert!(
        !PromoCheck::is_muted(&store, "dropkanal").await,
        "vor dem Drop darf der Kanal nicht stumm sein"
    );

    store
        .suppress_for_drop(
            "dropkanal",
            Some("4242"),
            "promo",
            "channel_settings",
            Some("Blocked by channel settings"),
        )
        .await;

    // is_muted (Promo-Pfad) muss jetzt true sein.
    assert!(
        PromoCheck::is_muted(&store, "dropkanal").await,
        "nach channel_settings-Drop muss der Promo-Pfad den Kanal als stumm sehen"
    );

    // Zeile existiert mit suppressed_until ≈ now+7d.
    let entry = OutboundSuppressionCheck::check_suppression(&store, "dropkanal", "promo")
        .await
        .expect("Suppression-Zeile muss existieren");
    assert_eq!(entry.reason_code, "channel_settings");
    assert!(
        entry.suppressed_until > Utc::now() + Duration::days(6),
        "7-Tage-TTL erwartet, war {}",
        entry.suppressed_until
    );
    assert!(
        entry.suppressed_until < Utc::now() + Duration::days(8),
        "TTL darf nicht über 7d hinausschießen"
    );
}

/// Gegenprobe: ein Nicht-channel_settings-Drop (z. B. sender_timedout) darf
/// KEINE channel_settings-Suppression schreiben (No-op auf der Schreibseite).
#[tokio::test]
async fn suppress_for_drop_ignoriert_fremde_reason_codes() {
    use tb_chat::promos::{OutboundSuppressionCheck as PromoCheck, OutboundSuppressionWriter};

    let pool = pool_or_skip!("sup_drop_ignore");
    let store = OutboundSuppressionStore::new(pool.clone());

    store
        .suppress_for_drop("kein_mute", Some("1"), "promo", "sender_timedout", None)
        .await;

    assert!(
        !PromoCheck::is_muted(&store, "kein_mute").await,
        "sender_timedout darf keine Promo-Suppression schreiben"
    );
}

#[tokio::test]
async fn check_suppression_recruitment_7_tage() {
    let pool = pool_or_skip!("sup_recruitment");
    let store = OutboundSuppressionStore::new(pool);

    let ttl = Duration::seconds(tb_chat::moderation::SUPPRESSION_RECRUITMENT_SECS);
    store
        .upsert_suppression(
            "recr_kanal",
            None,
            "recruitment",
            "channel_settings",
            None,
            ttl,
        )
        .await
        .unwrap();

    let entry = store.check_suppression("recr_kanal", "recruitment").await;
    assert!(entry.is_some());
    let e = entry.unwrap();
    assert!(
        e.suppressed_until > Utc::now() + Duration::days(6),
        "7-Tage-TTL erwartet für recruitment"
    );
}
