//! Tests der Scout-Erkennung und des Stores gegen echte PG
//! (TB_TEST_DATABASE_URL; ohne DSN wird übersprungen, mit
//! TB_TEST_REQUIRE_DB=1 wird stattdessen panikt, damit CI nicht still grün).

use chrono::Utc;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use std::str::FromStr;
use tb_scout::detector::{finde_kandidaten, laufe_scout_scan, KandidatFund};
use tb_scout::store::KandidatZeile;
use tb_scout::store::{
    approved_ohne_dispatch, liste_offen, setze_entscheidung, vermerke_dispatch, vermerke_kandidat,
};
use tb_scout::{STATUS_APPROVED, STATUS_UEBERSPRUNGEN};

fn dsn_or_skip() -> Option<String> {
    match std::env::var("TB_TEST_DATABASE_URL") {
        Ok(dsn) => Some(dsn),
        Err(_) => {
            if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
            }
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            None
        }
    }
}

/// Frisches Test-Schema mit dem Prod-Schema der beteiligten Tabellen
/// (Spaltenumfang wie in den Migrationen, für die Filter relevant).
async fn pool_or_skip(schema: &str) -> Option<PgPool> {
    let dsn = dsn_or_skip()?;
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .expect("connect test database");
    sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
        .execute(&admin)
        .await
        .expect("drop schema");
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .expect("create schema");
    admin.close().await;
    let opts = PgConnectOptions::from_str(&dsn)
        .expect("parse dsn")
        .options([("search_path", schema)]);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(opts)
        .await
        .expect("connect schema pool");
    for ddl in [
        // twitch_stats_category: ts_utc ist seit 20260621060000 TIMESTAMPTZ,
        // language seit 20260629130000.
        "CREATE TABLE twitch_stats_category (ts_utc TIMESTAMPTZ, streamer TEXT, \
             viewer_count INTEGER, is_partner BOOLEAN DEFAULT FALSE, game_name TEXT, \
             stream_title TEXT, tags TEXT, language TEXT)",
        "CREATE TABLE twitch_stream_sessions (id SERIAL PRIMARY KEY, streamer_login TEXT, \
             started_at TIMESTAMPTZ, avg_viewers REAL, language TEXT, \
             had_deadlock_in_session INTEGER DEFAULT 0, twitch_user_id TEXT)",
        "CREATE TABLE twitch_partners (id BIGSERIAL PRIMARY KEY, twitch_login TEXT, \
             twitch_user_id TEXT, status TEXT)",
        "CREATE TABLE twitch_raid_blacklist (target_id TEXT, target_login TEXT, \
             reason TEXT, added_at TEXT)",
        "CREATE TABLE twitch_partner_signup_denylist (twitch_user_id TEXT PRIMARY KEY, \
             twitch_login TEXT NOT NULL, reason TEXT NOT NULL, public_message TEXT, \
             added_by TEXT NOT NULL, added_at TIMESTAMPTZ NOT NULL DEFAULT now(), \
             partner_paused_by_block BOOLEAN NOT NULL DEFAULT false)",
        "CREATE TABLE twitch_scout_pitch_blacklist (streamer_login TEXT PRIMARY KEY, \
             reason TEXT, created_at TIMESTAMPTZ DEFAULT NOW(), twitch_user_id TEXT)",
        "CREATE TABLE twitch_outbound_chat_suppressions (target_login TEXT NOT NULL, \
             source TEXT NOT NULL, target_id TEXT, reason_code TEXT, reason_detail TEXT, \
             suppressed_until TIMESTAMPTZ NOT NULL, \
             CONSTRAINT twitch_outbound_chat_suppressions_pkey PRIMARY KEY (target_login, source))",
        "CREATE TABLE twitch_partner_outreach (streamer_login TEXT PRIMARY KEY, \
             streamer_user_id TEXT, detected_at TIMESTAMPTZ, contacted_at TIMESTAMPTZ, \
             status TEXT, cooldown_until TIMESTAMPTZ, raid_used_at TIMESTAMPTZ)",
        "CREATE TABLE twitch_chatter_global_ban (chatter_login TEXT, chatter_id TEXT)",
        // Ziel-Tabelle wortgleich aus der Migration 20260829090000.
        "CREATE TABLE twitch_scout_candidates (streamer_login TEXT PRIMARY KEY, \
             twitch_user_id TEXT, sessions_count INTEGER NOT NULL DEFAULT 0, \
             avg_viewers REAL NOT NULL DEFAULT 0, first_seen TIMESTAMPTZ, last_seen TIMESTAMPTZ, \
             language TEXT, deadlock_share REAL NOT NULL DEFAULT 0, \
             status TEXT NOT NULL DEFAULT 'vorgeschlagen', entscheid_grund TEXT, approver TEXT, \
             decided_at TIMESTAMPTZ, dispatched_at TIMESTAMPTZ, visited_at TIMESTAMPTZ)",
    ] {
        sqlx::query(ddl)
            .execute(&pool)
            .await
            .expect("create test table");
    }
    Some(pool)
}

/// Fügt `count` getrennte Sessions ein: je 3 Ticks à 10 Minuten, Blöcke
/// 3 Stunden auseinander (Lücke > 30-Minuten-Gap → je eine Session).
async fn seed_sessions(pool: &PgPool, streamer: &str, tage: i64, sessions: i64, viewers: i32) {
    for session in 0..sessions {
        sqlx::query(
            "INSERT INTO twitch_stats_category (streamer, ts_utc, viewer_count, language, game_name) \
             SELECT $1, NOW() - ($2 || ' days')::interval - ($3 || ' minutes')::interval \
                      - (s * 10 || ' minutes')::interval, $4, 'de', 'Deadlock' \
             FROM generate_series(0, 2) AS s",
        )
        .bind(streamer)
        .bind(tage)
        .bind(session * 180)
        .bind(viewers)
        .execute(pool)
        .await
        .expect("seed sessions");
    }
}

async fn seed_user_id(pool: &PgPool, login: &str, user_id: &str) {
    sqlx::query(
        "INSERT INTO twitch_stream_sessions (streamer_login, started_at, twitch_user_id) \
         VALUES ($1, NOW() - INTERVAL '1 day', $2)",
    )
    .bind(login)
    .bind(user_id)
    .execute(pool)
    .await
    .expect("seed user id");
}

#[tokio::test]
async fn findet_kleinen_neuen_kanal_mit_kennzahlen() {
    let Some(pool) = pool_or_skip("t_scout_findet").await else {
        return;
    };
    seed_sessions(&pool, "KleinAnfang", 1, 2, 5).await;
    seed_user_id(&pool, "kleinanfang", "4711").await;

    let funds = finde_kandidaten(&pool).await.unwrap();

    assert_eq!(funds.len(), 1, "genau der kleine neue Kanal");
    let fund = &funds[0];
    assert_eq!(fund.login, "kleinanfang");
    assert_eq!(fund.twitch_user_id.as_deref(), Some("4711"));
    assert_eq!(fund.sessions_count, 2, "zwei Blöcke mit 60-Minuten-Abstand");
    assert_eq!(fund.avg_viewers, 5.0);
    assert_eq!(fund.language.as_deref(), Some("de"));
    assert!((fund.deadlock_share - 1.0).abs() < 0.0001);
    assert!(fund.first_seen <= fund.last_seen);
}

#[tokio::test]
async fn filter_schlagen_an() {
    let Some(pool) = pool_or_skip("t_scout_filter").await else {
        return;
    };
    seed_sessions(&pool, "sauber", 1, 1, 5).await;
    // Partner-Tabelle (egal welcher Status) → raus.
    seed_sessions(&pool, "schonpartner", 1, 1, 5).await;
    sqlx::query(
        "INSERT INTO twitch_partners (twitch_login, status) VALUES ('schonpartner', 'active')",
    )
    .execute(&pool)
    .await
    .unwrap();
    // is_partner-Flag am Tick → raus.
    seed_sessions(&pool, "flagpartner", 1, 1, 5).await;
    sqlx::query(
        "UPDATE twitch_stats_category SET is_partner = TRUE WHERE streamer = 'flagpartner'",
    )
    .execute(&pool)
    .await
    .unwrap();
    // Raid-Blacklist → raus.
    seed_sessions(&pool, "geblockt", 1, 1, 5).await;
    sqlx::query("INSERT INTO twitch_raid_blacklist (target_login) VALUES ('geblockt')")
        .execute(&pool)
        .await
        .unwrap();
    // Partneraufnahme-Denylist → raus.
    seed_sessions(&pool, "denylisted", 1, 1, 5).await;
    sqlx::query(
        "INSERT INTO twitch_partner_signup_denylist (twitch_user_id, twitch_login, reason, added_by) \
         VALUES ('d1', 'denylisted', 'owner_decision', 'test')",
    )
    .execute(&pool)
    .await
    .unwrap();
    // Scout-Pitch-Blacklist → raus.
    seed_sessions(&pool, "pitchblock", 1, 1, 5).await;
    sqlx::query("INSERT INTO twitch_scout_pitch_blacklist (streamer_login) VALUES ('pitchblock')")
        .execute(&pool)
        .await
        .unwrap();
    // Aktive Recruitment-Suppression → raus.
    seed_sessions(&pool, "suppressed", 1, 1, 5).await;
    sqlx::query(
        "INSERT INTO twitch_outbound_chat_suppressions (target_login, source, suppressed_until) \
         VALUES ('suppressed', 'recruitment', NOW() + INTERVAL '3 days')",
    )
    .execute(&pool)
    .await
    .unwrap();
    // Abgelaufene andere-Sourcen-Suppression hält NICHT auf.
    seed_sessions(&pool, "alt_suppressed", 2, 1, 5).await;
    sqlx::query(
        "INSERT INTO twitch_outbound_chat_suppressions (target_login, source, suppressed_until) \
         VALUES ('alt_suppressed', 'recruitment', NOW() - INTERVAL '3 days')",
    )
    .execute(&pool)
    .await
    .unwrap();
    // Aktiver Outreach-Cooldown → raus.
    seed_sessions(&pool, "cooldown", 1, 1, 5).await;
    sqlx::query(
        "INSERT INTO twitch_partner_outreach (streamer_login, status, cooldown_until) \
         VALUES ('cooldown', 'sent', NOW() + INTERVAL '10 days')",
    )
    .execute(&pool)
    .await
    .unwrap();
    // Abgelaufener Cooldown hält nicht auf.
    seed_sessions(&pool, "alt_cooldown", 3, 1, 5).await;
    sqlx::query(
        "INSERT INTO twitch_partner_outreach (streamer_login, status, cooldown_until) \
         VALUES ('alt_cooldown', 'sent', NOW() - INTERVAL '1 day')",
    )
    .execute(&pool)
    .await
    .unwrap();
    // Global gebannt (per Login) → raus.
    seed_sessions(&pool, "globalban", 1, 1, 5).await;
    sqlx::query(
        "INSERT INTO twitch_chatter_global_ban (chatter_login, chatter_id) VALUES ('globalban', 'gb1')",
    )
    .execute(&pool)
    .await
    .unwrap();
    // Global gebannt (nur per ID) → raus.
    seed_sessions(&pool, "banperid", 1, 1, 5).await;
    seed_user_id(&pool, "banperid", "gb-id-2").await;
    sqlx::query(
        "INSERT INTO twitch_chatter_global_ban (chatter_login, chatter_id) VALUES ('anderer', 'gb-id-2')",
    )
    .execute(&pool)
    .await
    .unwrap();
    // Zu groß (Ø > 10) → raus.
    seed_sessions(&pool, "zugross", 1, 1, 40).await;
    // Zu viele Sessions (> 5) → raus.
    seed_sessions(&pool, "zuviele", 1, 6, 5).await;
    // Erste Sichtung älter als 60 Tage → raus.
    seed_sessions(&pool, "altkanal", 90, 1, 5).await;
    // Schon entschieden (übersprungen) → raus.
    seed_sessions(&pool, "schonwegs", 1, 1, 5).await;
    sqlx::query(
        "INSERT INTO twitch_scout_candidates (streamer_login, status) VALUES ('schonwegs', 'uebersprungen')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let logins: Vec<String> = finde_kandidaten(&pool)
        .await
        .unwrap()
        .into_iter()
        .map(|f| f.login)
        .collect();

    assert_eq!(
        logins,
        vec![
            "alt_cooldown".to_string(),
            "alt_suppressed".to_string(),
            "sauber".to_string()
        ],
        "nur ungefilterte Kandidaten, sortiert nach first_seen"
    );
}

#[tokio::test]
async fn vermerke_aktualisiert_nur_vorgeschlagene_zeilen() {
    let Some(pool) = pool_or_skip("t_scout_vermerke").await else {
        return;
    };
    let fund_a = KandidatFund {
        login: "kandidat".into(),
        twitch_user_id: Some("1".into()),
        sessions_count: 2,
        avg_viewers: 4.0,
        first_seen: Utc::now() - chrono::Duration::days(3),
        last_seen: Utc::now(),
        language: Some("de".into()),
        deadlock_share: 1.0,
    };
    assert!(
        vermerke_kandidat(&pool, &fund_a).await.unwrap(),
        "erstmalig anlegt"
    );

    // Zweiter Scan mit neuen Kennzahlen → vorgeschlagene Zeile wird aktualisiert.
    let fund_b = KandidatFund {
        sessions_count: 4,
        avg_viewers: 8.0,
        ..fund_a.clone()
    };
    assert!(vermerke_kandidat(&pool, &fund_b).await.unwrap());
    let zeile = sqlx::query_as::<_, (i32, f32)>(
        "SELECT sessions_count, avg_viewers FROM twitch_scout_candidates WHERE streamer_login = 'kandidat'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(zeile, (4, 8.0));

    // Entscheidung fällt, danach schützt der Upsert Status UND Kennzahlen.
    assert!(
        setze_entscheidung(&pool, "Kandidat", "approve", Some("passt"), "discord:1")
            .await
            .unwrap()
    );
    let fund_c = KandidatFund {
        sessions_count: 9,
        avg_viewers: 9.9,
        ..fund_a
    };
    assert!(
        !vermerke_kandidat(&pool, &fund_c).await.unwrap(),
        "entschiedene Zeile bleibt unangetastet"
    );
    let (status, sessions): (String, i32) = sqlx::query_as(
        "SELECT status, sessions_count FROM twitch_scout_candidates WHERE streamer_login = 'kandidat'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, STATUS_APPROVED);
    assert_eq!(sessions, 4, "Kennzahlen der entschiedenen Zeile bleiben");
}

#[tokio::test]
async fn entscheidung_validiert_status_und_login() {
    let Some(pool) = pool_or_skip("t_scout_entscheidung").await else {
        return;
    };
    sqlx::query("INSERT INTO twitch_scout_candidates (streamer_login) VALUES ('kandidat')")
        .execute(&pool)
        .await
        .unwrap();

    // Ungültiger Status → false, Zeile bleibt vorgeschlagen.
    assert!(
        !setze_entscheidung(&pool, "kandidat", "vorgeschlagen", None, "discord:1")
            .await
            .unwrap()
    );
    // Unbekannter Login → false, kein Fehler.
    assert!(
        !setze_entscheidung(&pool, "gibtsnicht", "approve", None, "discord:1")
            .await
            .unwrap()
    );
    // Gültige Entscheidungen landen mit Grund und Entscheider.
    assert!(setze_entscheidung(
        &pool,
        " kandidat ",
        "uebersprungen",
        Some("  "),
        "discord:1"
    )
    .await
    .unwrap());
    let zeile = sqlx::query_as::<_, (String, Option<String>, String)>(
        "SELECT status, entscheid_grund, approver FROM twitch_scout_candidates WHERE streamer_login = 'kandidat'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(zeile.0, STATUS_UEBERSPRUNGEN);
    assert_eq!(zeile.1, None, "leerer Grund wird nicht gespeichert");
    assert_eq!(zeile.2, "discord:1");
}

#[tokio::test]
async fn liste_offen_zeigt_nur_vorgeschlagen_und_pausiert() {
    let Some(pool) = pool_or_skip("t_scout_liste").await else {
        return;
    };
    for (login, status) in [
        ("a_offen", "vorgeschlagen"),
        ("b_pause", "pausiert"),
        ("c_weg", "uebersprungen"),
        ("d_freig", "approved"),
    ] {
        sqlx::query("INSERT INTO twitch_scout_candidates (streamer_login, status) VALUES ($1, $2)")
            .bind(login)
            .bind(status)
            .execute(&pool)
            .await
            .unwrap();
    }
    let zeilen: Vec<KandidatZeile> = liste_offen(&pool).await.unwrap();
    let logins: Vec<&str> = zeilen.iter().map(|z| z.login.as_str()).collect();
    assert_eq!(logins, vec!["a_offen", "b_pause"]);
}

#[tokio::test]
async fn approved_ohne_dispatch_und_stempel_idempotent() {
    let Some(pool) = pool_or_skip("t_scout_dispatch_read").await else {
        return;
    };
    sqlx::query(
        "INSERT INTO twitch_scout_candidates \
             (streamer_login, twitch_user_id, status, decided_at) VALUES \
             ('erster', '1', 'approved', NOW() - INTERVAL '2 hours'), \
             ('zweiter', '2', 'approved', NOW() - INTERVAL '1 hour'), \
             ('ohne_id', NULL, 'approved', NOW()), \
             ('leere_id', '', 'approved', NOW()), \
             ('pausiert', '3', 'pausiert', NOW())",
    )
    .execute(&pool)
    .await
    .unwrap();

    let kandidaten = approved_ohne_dispatch(&pool, 10).await.unwrap();
    let logins: Vec<&str> = kandidaten.iter().map(|k| k.login.as_str()).collect();
    assert_eq!(
        logins,
        vec!["erster", "zweiter"],
        "nur approved mit ID, älteste Entscheidung zuerst"
    );

    assert!(vermerke_dispatch(&pool, "ERSTER").await.unwrap());
    assert!(
        !vermerke_dispatch(&pool, "erster").await.unwrap(),
        "zweiter Stempel greift nicht (INV-06)"
    );
    let kandidaten = approved_ohne_dispatch(&pool, 10).await.unwrap();
    assert_eq!(kandidaten.len(), 1);
    assert_eq!(kandidaten[0].login, "zweiter");
}

#[tokio::test]
async fn scan_vormerkt_und_ban_probe_faellt_zu() {
    let Some(pool) = pool_or_skip("t_scout_scan").await else {
        return;
    };
    seed_sessions(&pool, "kandidat", 1, 1, 5).await;
    seed_user_id(&pool, "kandidat", "1").await;
    seed_sessions(&pool, "gebannt", 1, 1, 5).await;
    sqlx::query(
        "INSERT INTO twitch_chatter_global_ban (chatter_login, chatter_id) VALUES ('gebannt', '2')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let angefasst = laufe_scout_scan(&pool).await.unwrap();
    assert_eq!(
        angefasst, 1,
        "nur der ungebannte Kandidat landet in der Tabelle"
    );
    let status: String = sqlx::query_scalar(
        "SELECT status FROM twitch_scout_candidates WHERE streamer_login = 'kandidat'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "vorgeschlagen");

    // Ohne Global-Ban-Tabelle wird der Scan sichtbar abgebrochen, statt eine
    // leere Kandidatenliste als Erfolg auszugeben.
    sqlx::query("DROP TABLE twitch_chatter_global_ban")
        .execute(&pool)
        .await
        .unwrap();
    seed_sessions(&pool, "zweiter", 1, 1, 5).await;
    assert!(finde_kandidaten(&pool).await.is_err());
}
