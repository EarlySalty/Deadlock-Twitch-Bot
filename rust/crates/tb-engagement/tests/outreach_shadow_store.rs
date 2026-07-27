use std::str::FromStr;

use chrono::{Duration, TimeZone, Utc};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{Executor, PgPool};
use tb_engagement::outreach_shadow::{CycleResult, NewOutreachEvent, OutreachStage};
use tb_engagement::outreach_shadow_store::OutreachShadowStore;
use uuid::Uuid;

const MIGRATION: &str =
    include_str!("../../../migrations/20260727120000_twitch_outreach_shadow.sql");

#[tokio::test]
async fn global_startet_auch_bei_konkurrenz_nur_eine_sitzung() {
    let Some(pool) = test_pool("outreach_shadow_global_singleton").await else {
        return;
    };
    seed_candidate(&pool, "eins", "1", None).await;
    seed_candidate(&pool, "zwei", "2", None).await;
    let store = OutreachShadowStore::new(pool.clone());
    let now = Utc.with_ymd_and_hms(2026, 7, 27, 20, 0, 0).unwrap();

    let (left, right) = tokio::join!(store.start_next_session(now), store.start_next_session(now));
    let started = [left.expect("linker Start"), right.expect("rechter Start")]
        .into_iter()
        .flatten()
        .count();

    assert_eq!(started, 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM twitch_outreach_shadow_sessions WHERE ended_at IS NULL"
        )
        .fetch_one(&pool)
        .await
        .expect("offene Sitzungen zählen"),
        1
    );

    let (left_claim, right_claim) = tokio::join!(
        store.claim_active_session(now),
        store.claim_active_session(now)
    );
    let claimed = [
        left_claim.expect("linker Claim"),
        right_claim.expect("rechter Claim"),
    ]
    .into_iter()
    .flatten()
    .count();
    assert_eq!(claimed, 1);
}

#[tokio::test]
async fn partner_und_cooldown_kandidaten_werden_nie_ausgewaehlt() {
    let Some(pool) = test_pool("outreach_shadow_candidate_guards").await else {
        return;
    };
    let now = Utc.with_ymd_and_hms(2026, 7, 27, 20, 0, 0).unwrap();
    seed_candidate(&pool, "partner", "1", None).await;
    seed_candidate(&pool, "view_partner", "4", None).await;
    seed_candidate(
        &pool,
        "cooldown",
        "2",
        Some((now + Duration::hours(1)).to_rfc3339()),
    )
    .await;
    seed_candidate(&pool, "frei", "3", None).await;
    sqlx::query(
        "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status)
         VALUES ('1', 'partner', 'active')",
    )
    .execute(&pool)
    .await
    .expect("Partner setzen");
    sqlx::query(
        "INSERT INTO twitch_streamers_partner_state
            (twitch_user_id, twitch_login, is_partner_active)
         VALUES ('4', 'view_partner', 1)",
    )
    .execute(&pool)
    .await
    .expect("Partner-View setzen");

    let store = OutreachShadowStore::new(pool.clone());
    let selected = store
        .start_next_session(now)
        .await
        .expect("Kandidat wählen")
        .expect("freier Kandidat");

    assert_eq!(selected.channel_login, "frei");

    sqlx::query(
        "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status)
         VALUES ('3', 'frei', 'active')",
    )
    .execute(&pool)
    .await
    .expect("Partnerwechsel setzen");
    assert_eq!(
        store
            .close_ineligible_session(now + Duration::minutes(1))
            .await
            .expect("Partner-Neuprüfung"),
        Some("became_partner")
    );
}

#[tokio::test]
async fn jeder_ausgang_wird_je_zyklus_genau_einmal_persistiert() {
    let Some(pool) = test_pool("outreach_shadow_cycle_once").await else {
        return;
    };
    seed_candidate(&pool, "eins", "1", None).await;
    let store = OutreachShadowStore::new(pool.clone());
    let now = Utc.with_ymd_and_hms(2026, 7, 27, 20, 0, 0).unwrap();
    let session = store
        .start_next_session(now)
        .await
        .expect("Sitzung starten")
        .expect("Sitzung");
    let silent = tb_engagement::outreach_shadow::OutreachDecision {
        hooks: vec![],
        stage: OutreachStage::Watch,
        silent_reason: Some("kein anlass".to_owned()),
    };
    let hook = tb_engagement::outreach_shadow::OutreachDecision {
        hooks: vec![tb_engagement::outreach_shadow::OutreachHook {
            kind: tb_engagement::outreach_shadow::HookKind::Smalltalk,
            occasion: None,
            evidence: "transkript".to_owned(),
            evidence_source: tb_engagement::outreach_shadow::EvidenceSource::Transcript,
            evidence_at: now,
            opener: "wie laufen die runden".to_owned(),
            why: "test".to_owned(),
            confidence: 0.8,
        }],
        stage: OutreachStage::Smalltalk,
        silent_reason: None,
    };
    for result in [
        CycleResult::Decision(silent),
        CycleResult::ParserError,
        CycleResult::Timeout,
        CycleResult::ProviderError("http_status".to_owned()),
        CycleResult::Decision(hook),
    ] {
        let event = NewOutreachEvent::from_cycle_result(
            &session,
            Uuid::new_v4(),
            now,
            Some("transkript".to_owned()),
            result,
        );
        assert!(store.record_cycle(&event).await.expect("erster Insert"));
        assert!(!store.record_cycle(&event).await.expect("zweiter Insert"));
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM twitch_outreach_shadow_events")
            .fetch_one(&pool)
            .await
            .expect("Zyklen zählen"),
        5
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT stage FROM twitch_outreach_shadow_sessions WHERE id = $1"
        )
        .bind(session.id)
        .fetch_one(&pool)
        .await
        .expect("Session-Stufe lesen"),
        "smalltalk"
    );

    sqlx::query("UPDATE twitch_outreach_shadow_events SET expires_at = $1")
        .bind(now - Duration::seconds(1))
        .execute(&pool)
        .await
        .expect("Events ablaufen lassen");
    let expired_id = sqlx::query_scalar::<_, i64>(
        "UPDATE twitch_outreach_shadow_events
         SET discord_message_id = 'review-message'
         WHERE outcome = 'hook'
         RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("abgelaufenes Discord-Ereignis markieren");
    assert!(store
        .claim_discord_events(20, now)
        .await
        .expect("Discord-Events claimen")
        .is_empty());
    for _ in 0..3 {
        store
            .mark_discord_delete_failed(expired_id, "transport", now)
            .await
            .expect("Löschfehler markieren");
    }
    let (transcript, decision, tombstoned_at) = sqlx::query_as::<
        _,
        (
            Option<String>,
            Option<serde_json::Value>,
            Option<chrono::DateTime<Utc>>,
        ),
    >(
        "SELECT transcript, decision, content_tombstoned_at
             FROM twitch_outreach_shadow_events
             WHERE id = $1",
    )
    .bind(expired_id)
    .fetch_one(&pool)
    .await
    .expect("Tombstone prüfen");
    assert!(transcript.is_none());
    assert!(decision.is_none());
    assert!(tombstoned_at.is_some());

    // Der lokale Tombstone beendet die Löschpflicht nicht: solange der Post in
    // Discord steht, muss er weiter zum Löschen anstehen. Mit einer
    // Versuchsobergrenze bliebe er dort dauerhaft sichtbar und die
    // Aufbewahrungsfrist wäre für den externen Post wirkungslos.
    let faellig = store
        .expired_discord_events(20, now + Duration::hours(2))
        .await
        .expect("abgelaufene Discord-Posts lesen");
    assert_eq!(
        faellig.len(),
        1,
        "Post muss nach drei Fehlversuchen weiter zum Loeschen anstehen"
    );
    assert_eq!(faellig[0].id, expired_id);
    assert_eq!(faellig[0].message_id, "review-message");
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
        std::env::var("TEST_DATABASE_URL").or_else(|_| std::env::var("TWITCH_ANALYTICS_DSN"))
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
        CREATE TABLE twitch_chat_messages (
            id BIGSERIAL PRIMARY KEY,
            streamer_login TEXT NOT NULL,
            chatter_login TEXT,
            message_ts TIMESTAMPTZ NOT NULL,
            content TEXT
        );",
    )
    .await
    .expect("Testtabellen anlegen");
    pool.execute(MIGRATION)
        .await
        .expect("Outreach-Migration ausführen");
    Some(pool)
}
