use std::str::FromStr;

use chrono::{Duration, TimeZone, Utc};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{Executor, PgPool};
use tb_engagement::minimax_chat::TestModeRejectReason;
use tb_engagement::smalltalk_loop_store::{GeneratedOutcome, SmalltalkLoopStore};

const MIGRATION: &str =
    include_str!("../../../migrations/20260727150000_twitch_smalltalk_loop.sql");

#[tokio::test]
async fn globale_sitzung_setzt_testmodus_und_stellt_settings_wieder_her() {
    let Some(pool) = test_pool("smalltalk_loop_singleton").await else {
        return;
    };
    seed_candidate(&pool, "eins", "1", None).await;
    seed_candidate(&pool, "zwei", "2", None).await;
    sqlx::query(
        "INSERT INTO twitch_engagement_settings
            (channel_login, enabled, irc_read, output_mode)
         VALUES ('eins', FALSE, FALSE, 'shadow')",
    )
    .execute(&pool)
    .await
    .expect("alte Settings setzen");
    let store = SmalltalkLoopStore::new(pool.clone());
    let now = Utc.with_ymd_and_hms(2026, 7, 27, 20, 0, 0).unwrap();

    let (left, right) = tokio::join!(store.start_next_session(now), store.start_next_session(now));
    let sessions = [left.expect("linker Start"), right.expect("rechter Start")]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    assert_eq!(sessions.len(), 1);
    let session = &sessions[0];
    assert_eq!(session.channel_login, "eins");
    assert_eq!(
        sqlx::query_as::<_, (bool, bool, String)>(
            "SELECT enabled, irc_read, output_mode
             FROM twitch_engagement_settings
             WHERE channel_login = 'eins'",
        )
        .fetch_one(&pool)
        .await
        .expect("aktive Settings lesen"),
        (true, true, "test".to_string())
    );

    store
        .close_all_open_sessions("process_start", now + Duration::minutes(1))
        .await
        .expect("offene Sitzung schließen");
    assert_eq!(
        sqlx::query_as::<_, (bool, bool, String)>(
            "SELECT enabled, irc_read, output_mode
             FROM twitch_engagement_settings
             WHERE channel_login = 'eins'",
        )
        .fetch_one(&pool)
        .await
        .expect("zurückgesetzte Settings lesen"),
        // Der Kanal stand vorher auf "shadow" — genau dahin muss er zurück,
        // sonst schaltet eine Testsession ihn dauerhaft ab.
        (false, false, "shadow".to_string())
    );
    let cooldown: String = sqlx::query_scalar(
        "SELECT cooldown_until FROM twitch_partner_outreach WHERE streamer_login = 'eins'",
    )
    .fetch_one(&pool)
    .await
    .expect("Cooldown lesen");
    assert!(
        parse_timestamp(&cooldown) > now,
        "Sitzungsende setzt einen zukünftigen Cooldown"
    );
}

#[tokio::test]
async fn laufender_cooldown_in_postgres_textform_sperrt_den_kandidaten() {
    let Some(pool) = test_pool("smalltalk_loop_cooldown").await else {
        return;
    };
    seed_candidate(&pool, "gesperrt", "9", None).await;
    sqlx::query(
        "UPDATE twitch_partner_outreach
         SET cooldown_until = (NOW() + interval '6 hours')::text
         WHERE streamer_login = 'gesperrt'",
    )
    .execute(&pool)
    .await
    .expect("laufenden Cooldown setzen");

    assert!(
        SmalltalkLoopStore::new(pool)
            .start_next_session(Utc::now())
            .await
            .expect("Kandidatensuche")
            .is_none(),
        "ein unlesbarer Cooldown würde hier fälschlich eine Sitzung starten"
    );
}

#[tokio::test]
async fn erzeugte_nachricht_wird_unabhaengig_vom_filter_genau_einmal_gespeichert() {
    let Some(pool) = test_pool("smalltalk_loop_messages").await else {
        return;
    };
    seed_candidate(&pool, "eins", "1", None).await;
    let store = SmalltalkLoopStore::new(pool.clone());
    let now = Utc.with_ymd_and_hms(2026, 7, 27, 20, 0, 0).unwrap();
    let session = store
        .start_next_session(now)
        .await
        .expect("Sitzung starten")
        .expect("Sitzung");

    assert!(store
        .record_generated(
            &session.channel_login,
            Some("msg-1"),
            "haze ist stark",
            "was haltet ihr von haze",
            GeneratedOutcome::WouldSend,
            now,
        )
        .await
        .expect("durchgelassene Nachricht"));
    assert!(!store
        .record_generated(
            &session.channel_login,
            Some("msg-1"),
            "haze ist stark",
            "was haltet ihr von haze",
            GeneratedOutcome::WouldSend,
            now,
        )
        .await
        .expect("doppelte Nachricht"));
    assert!(store
        .record_generated(
            &session.channel_login,
            Some("msg-2"),
            "komm auf discord",
            "wo spielt ihr",
            GeneratedOutcome::Rejected(TestModeRejectReason::OfferOrLink),
            now + Duration::seconds(1),
        )
        .await
        .expect("verworfene Nachricht"));

    let rows = sqlx::query_as::<_, (String, Option<String>, String)>(
        "SELECT outcome, reject_reason, generated_text
         FROM twitch_smalltalk_messages
         ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .expect("Nachrichten lesen");
    assert_eq!(
        rows,
        vec![
            ("would_send".to_string(), None, "haze ist stark".to_string()),
            (
                "rejected".to_string(),
                Some("offer_or_link".to_string()),
                "komm auf discord".to_string(),
            ),
        ]
    );
}

#[tokio::test]
async fn jede_beendete_sitzung_wird_mit_nachrichten_oder_providerfehler_geclaimt() {
    let Some(pool) = test_pool("smalltalk_loop_reports").await else {
        return;
    };
    seed_candidate(&pool, "leer", "1", None).await;
    let store = SmalltalkLoopStore::new(pool.clone());
    let now = Utc.with_ymd_and_hms(2026, 7, 27, 20, 0, 0).unwrap();
    let empty = store
        .start_next_session(now)
        .await
        .expect("leere Sitzung starten")
        .expect("leere Sitzung");
    store
        .close_active_session("stream_ended", now + Duration::minutes(5))
        .await
        .expect("leere Sitzung schließen");

    seed_candidate(&pool, "fehler", "2", None).await;
    let failed = store
        .start_next_session(now + Duration::minutes(6))
        .await
        .expect("Fehlersitzung starten")
        .expect("Fehlersitzung");
    store
        .record_provider_error(&failed.channel_login, "http_status")
        .await
        .expect("Provider-Fehler erfassen");
    store
        .close_active_session("provider_error", now + Duration::minutes(7))
        .await
        .expect("Fehlersitzung schließen");

    let claimed = store
        .claim_reports(10, now + Duration::minutes(8))
        .await
        .expect("Auswertungen claimen");
    assert_eq!(claimed.len(), 2);
    let empty_report = claimed
        .iter()
        .find(|claim| claim.report.session.id == empty.id)
        .expect("leere Sitzung im Report");
    assert!(empty_report.report.messages.is_empty());
    assert_eq!(empty_report.report.session.end_reason, "stream_ended");
    let failed_report = claimed
        .iter()
        .find(|claim| claim.report.session.id == failed.id)
        .expect("Fehlersitzung im Report");
    assert_eq!(failed_report.report.session.provider_error_count, 1);
    assert_eq!(
        failed_report.report.session.last_provider_error.as_deref(),
        Some("http_status")
    );
}

#[tokio::test]
async fn streamende_und_zeitlimit_beenden_die_aktive_sitzung() {
    let Some(pool) = test_pool("smalltalk_loop_end_conditions").await else {
        return;
    };
    seed_candidate(&pool, "eins", "1", None).await;
    let store = SmalltalkLoopStore::new(pool.clone());
    let now = Utc.with_ymd_and_hms(2026, 7, 27, 20, 0, 0).unwrap();
    store
        .start_next_session(now)
        .await
        .expect("Sitzung starten")
        .expect("Sitzung");
    sqlx::query("UPDATE twitch_live_state SET is_live = 0 WHERE twitch_user_id = '1'")
        .execute(&pool)
        .await
        .expect("Stream beenden");
    assert_eq!(
        store
            .close_ineligible_session(now + Duration::minutes(1))
            .await
            .expect("Streamende prüfen"),
        Some("stream_ended")
    );

    seed_candidate(&pool, "zwei", "2", None).await;
    store
        .start_next_session(now + Duration::minutes(2))
        .await
        .expect("zweite Sitzung starten")
        .expect("zweite Sitzung");
    assert_eq!(
        store
            .close_ineligible_session(now + Duration::minutes(63))
            .await
            .expect("Zeitlimit prüfen"),
        Some("session_timeout")
    );
}

async fn seed_candidate(pool: &PgPool, login: &str, user_id: &str, cooldown: Option<String>) {
    sqlx::query(
        "INSERT INTO twitch_partner_outreach
            (streamer_login, streamer_user_id, detected_at, status, cooldown_until)
         VALUES ($1, $2, '2026-07-27T18:00:00Z', 'pending', $3)",
    )
    .bind(login)
    .bind(user_id)
    .bind(cooldown)
    .execute(pool)
    .await
    .expect("Kandidat setzen");
    sqlx::query(
        "INSERT INTO twitch_live_state
            (twitch_user_id, streamer_login, is_live, last_game, last_viewer_count)
         VALUES ($1, $2, 1, 'Deadlock', 10)",
    )
    .bind(user_id)
    .bind(login)
    .execute(pool)
    .await
    .expect("Live-State setzen");
}

async fn test_pool(schema: &str) -> Option<PgPool> {
    let Ok(url) =
        std::env::var("TB_TEST_DATABASE_URL")
            .or_else(|_| std::env::var("TEST_DATABASE_URL"))
            .or_else(|_| std::env::var("TWITCH_ANALYTICS_DSN"))
    else {
        return None;
    };
    let options = PgConnectOptions::from_str(&url)
        .expect("Test-DSN parsen")
        .options([("search_path", schema)]);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .expect("Testdatenbank verbinden");
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .expect("Testdatenbank administrativ verbinden");
    admin
        .execute(format!("DROP SCHEMA IF EXISTS {schema} CASCADE").as_str())
        .await
        .expect("altes Testschema löschen");
    admin
        .execute(format!("CREATE SCHEMA {schema}").as_str())
        .await
        .expect("Testschema anlegen");
    pool.execute(
        "CREATE TABLE twitch_partner_outreach (
            streamer_login TEXT PRIMARY KEY,
            streamer_user_id TEXT,
            detected_at TEXT NOT NULL,
            status TEXT,
            cooldown_until TEXT
        );
        CREATE TABLE twitch_live_state (
            twitch_user_id TEXT PRIMARY KEY,
            streamer_login TEXT NOT NULL,
            is_live INTEGER,
            last_game TEXT,
            last_viewer_count INTEGER
        );
        CREATE TABLE twitch_partners (
            twitch_user_id TEXT NOT NULL,
            twitch_login TEXT NOT NULL,
            status TEXT NOT NULL
        );
        CREATE TABLE twitch_streamers_partner_state (
            twitch_user_id TEXT,
            twitch_login TEXT NOT NULL,
            is_partner_active INTEGER NOT NULL
        );
        CREATE TABLE twitch_engagement_settings (
            channel_login TEXT PRIMARY KEY,
            enabled BOOLEAN NOT NULL DEFAULT FALSE,
            irc_read BOOLEAN NOT NULL DEFAULT FALSE,
            output_mode TEXT NOT NULL DEFAULT 'off'
                CHECK (output_mode IN ('off', 'shadow', 'live', 'test'))
        );",
    )
    .await
    .expect("Testtabellen anlegen");
    pool.execute(MIGRATION)
        .await
        .expect("Smalltalk-Migration ausführen");
    Some(pool)
}

fn parse_timestamp(raw: &str) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .expect("RFC3339-Cooldown")
        .with_timezone(&Utc)
}
