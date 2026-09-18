// Testuhr ist bewusst `Utc::now()` und kein fixes Datum: die Tabelle setzt
// `discord_next_attempt_at`/`expires_at` per DB-`NOW()`, und `claim_reports`
// vergleicht beide gegen die übergebene Zeit. Eine erfundene Uhr lief dieser
// Vergleichsbasis davon — der Report-Test wurde am 2026-07-27 um 20:08 UTC
// von selbst rot, ohne dass sich Code geändert hatte.
use chrono::{Duration, Utc};
use sqlx::{Executor, PgPool};
use tb_engagement::llm_chat::TestModeRejectReason;
use tb_engagement::smalltalk_loop_store::{GeneratedOutcome, SmalltalkLoopStore};
use tb_engagement::stream_transcripts::StreamTranscriptSegment;

#[path = "../../../test-support/postgres.rs"]
mod test_postgres;

const MIGRATION: &str =
    include_str!("../../../migrations/20260727150000_twitch_smalltalk_loop.sql");
const TRANSCRIPT_MIGRATION: &str =
    include_str!("../../../migrations/20260813220000_twitch_smalltalk_transcripts.sql");
const LIVE_MODE_MIGRATION: &str =
    include_str!("../../../migrations/20260916013000_smalltalk_live_mode.sql");

/// `twitch_engagement_settings` wird laut Vertrag kleingeschrieben befuellt
/// und exakt gelesen (`auto_off.rs`, `gate::load_settings`). Eine abweichend
/// geschriebene Zeile ist ein Datenfehler, den der Loop nicht sauber
/// behandeln kann: kleingeschrieben schreiben erzeugt eine zweite Zeile, die
/// nach Sitzungsende aktiv zurueckbleibt; in der vorhandenen Schreibweise
/// schreiben laesst die Pipeline den Kanal zur Laufzeit nicht finden, die
/// Sitzung liefe leer und meldete "keine Nachrichten". Also wird der Kandidat
/// uebersprungen, bekommt Cooldown und das steht im Log.
#[tokio::test]
async fn kandidat_mit_abweichender_settings_schreibweise_wird_uebersprungen() {
    let Some((_db, pool)) = test_pool("smalltalk_loop_case").await else {
        return;
    };
    seed_candidate(&pool, "MixedCase", "7", None).await;
    sqlx::query(
        "INSERT INTO twitch_engagement_settings
            (channel_login, enabled, irc_read, output_mode)
         VALUES ('MixedCase', TRUE, TRUE, 'live')",
    )
    .execute(&pool)
    .await
    .expect("alte Settings setzen");
    let store = SmalltalkLoopStore::new(pool.clone());
    let now = Utc::now();

    let gestartet = store.start_next_session(now).await.expect("Start");

    assert!(
        gestartet.is_none(),
        "ein Kanal, dessen Settings der Loop nicht sauber wiederherstellen kann, wird nicht belegt"
    );
    let zeilen: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_engagement_settings WHERE LOWER(channel_login) = 'mixedcase'",
    )
    .fetch_one(&pool)
    .await
    .expect("zaehlen");
    assert_eq!(zeilen, 1, "es darf keine zweite Settings-Zeile entstehen");

    let (login, enabled, irc_read, modus) = sqlx::query_as::<_, (String, bool, bool, String)>(
        "SELECT channel_login, enabled, irc_read, output_mode
         FROM twitch_engagement_settings
         WHERE LOWER(channel_login) = 'mixedcase'",
    )
    .fetch_one(&pool)
    .await
    .expect("Settings lesen");
    assert_eq!(login, "MixedCase");
    assert!(enabled && irc_read, "der Vorzustand bleibt unangetastet");
    assert_eq!(modus, "live", "der Ausgabemodus wird nicht ueberschrieben");

    let cooldown: Option<String> = sqlx::query_scalar(
        "SELECT cooldown_until FROM twitch_partner_outreach
         WHERE LOWER(streamer_login) = 'mixedcase'",
    )
    .fetch_one(&pool)
    .await
    .expect("Cooldown lesen");
    assert!(
        cooldown.is_some(),
        "der uebersprungene Kandidat bekommt Cooldown, sonst faellt er bei jedem Tick erneut an"
    );
}

#[tokio::test]
async fn live_test_nimmt_nur_kandidaten_unter_50_und_setzt_eigenen_modus() {
    let Some((_db, pool)) = test_pool("smalltalk_loop_live_followers").await else {
        return;
    };
    seed_candidate(&pool, "zu_gross", "50", None).await;
    seed_candidate(&pool, "klein", "49", None).await;
    sqlx::query(
        "INSERT INTO twitch_stream_sessions
            (streamer_login, followers_start, followers_end, started_at)
         VALUES ('zu_gross', 50, 50, NOW()), ('klein', 49, 49, NOW())",
    )
    .execute(&pool)
    .await
    .unwrap();

    let store = SmalltalkLoopStore::new(pool.clone()).with_live_send(true);
    for (id, login, count) in [("50", "zu_gross", 50), ("49", "klein", 49)] {
        store
            .record_live_preflight(
                &tb_engagement::smalltalk_loop_store::LivePreflightTarget {
                    twitch_user_id: id.into(),
                    channel_login: login.into(),
                },
                Some(count),
                true,
                Utc::now(),
                None,
            )
            .await
            .unwrap();
    }
    let session = store
        .start_next_session(Utc::now())
        .await
        .unwrap()
        .expect("49-Follower-Kandidat muss starten");
    assert_eq!(session.channel_login, "klein");
    let mode: String = sqlx::query_scalar(
        "SELECT output_mode FROM twitch_engagement_settings WHERE channel_login = 'klein'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(mode, "smalltalk_live");
}

/// Ein Kanal, in dem der Bot gebannt ist, landet als `bot_banned` in
/// `twitch_raid_blacklist` (`token_lifecycle::mark_bot_banned_inner`). Dort
/// zu messen, ob der Bot mitreden koennte, ist sinnlos: senden koennte er
/// ohnehin nie. Die Blacklist gilt komplett, auch fuer von Hand gesetzte
/// Eintraege, denn beides heisst "in diesem Kanal nicht auftreten".
#[tokio::test]
async fn gebannte_und_geblacklistete_kanaele_werden_nie_ausgewaehlt() {
    let Some((_db, pool)) = test_pool("smalltalk_loop_blacklist").await else {
        return;
    };
    seed_candidate(&pool, "gebannt", "1", None).await;
    seed_candidate(&pool, "sauber", "2", None).await;
    sqlx::query(
        "INSERT INTO twitch_raid_blacklist (target_id, target_login, reason)
         VALUES ('1', 'Gebannt', 'bot_banned: channel_settings')",
    )
    .execute(&pool)
    .await
    .expect("Blacklist setzen");
    let store = SmalltalkLoopStore::new(pool.clone());
    let now = Utc::now();

    let session = store
        .start_next_session(now)
        .await
        .expect("Start")
        .expect("Sitzung");

    assert_eq!(
        session.channel_login, "sauber",
        "der geblacklistete Kanal darf nie gewaehlt werden, auch nicht bei abweichender Schreibweise"
    );
}

/// Der Bot-Ban schreibt `target_id` in die Blacklist, und der Repo-Vertrag
/// (`RaidBlacklistStore::is_blacklisted`) matcht per ID ODER Login. Benennt
/// sich ein gebannter Kanal um, traegt nur noch die ID. Ein reiner
/// Login-Abgleich haette ihn danach wieder als Kandidaten zugelassen.
#[tokio::test]
async fn gebannter_kanal_bleibt_nach_umbenennung_gesperrt() {
    let Some((_db, pool)) = test_pool("smalltalk_loop_blacklist_id").await else {
        return;
    };
    seed_candidate(&pool, "neuer_name", "42", None).await;
    seed_candidate(&pool, "sauber", "43", None).await;
    sqlx::query(
        "INSERT INTO twitch_raid_blacklist (target_id, target_login, reason)
         VALUES ('42', 'alter_name', 'bot_banned: channel_settings')",
    )
    .execute(&pool)
    .await
    .expect("Blacklist setzen");
    let store = SmalltalkLoopStore::new(pool.clone());
    let now = Utc::now();

    let session = store
        .start_next_session(now)
        .await
        .expect("Start")
        .expect("Sitzung");

    assert_eq!(
        session.channel_login, "sauber",
        "der Ban haengt an der twitch_user_id, nicht am Login"
    );
}

#[tokio::test]
async fn globale_sitzung_setzt_testmodus_und_stellt_settings_wieder_her() {
    let Some((_db, pool)) = test_pool("smalltalk_loop_singleton").await else {
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
    let now = Utc::now();

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
    let Some((_db, pool)) = test_pool("smalltalk_loop_cooldown").await else {
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
    let Some((_db, pool)) = test_pool("smalltalk_loop_messages").await else {
        return;
    };
    seed_candidate(&pool, "eins", "1", None).await;
    let store = SmalltalkLoopStore::new(pool.clone());
    let now = Utc::now();
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

/// Der Stream-Ton gehoert zur Sitzung, nicht zum Ringpuffer: der wird nach
/// einer Stunde getrimmt, eine Sitzung dauert genau so lange, und ausgewertet
/// wird erst danach. Aufbewahrt wird nur, was waehrend einer offenen Sitzung
/// aufgenommen wurde.
#[tokio::test]
async fn stream_ton_haengt_an_der_offenen_sitzung_und_liegt_dem_report_bei() {
    let Some((_db, pool)) = test_pool("smalltalk_loop_transcripts").await else {
        return;
    };
    seed_candidate(&pool, "tonkanal", "1", None).await;
    let store = SmalltalkLoopStore::new(pool.clone());
    let now = Utc::now();
    let session = store
        .start_next_session(now)
        .await
        .expect("Sitzung starten")
        .expect("Sitzung");

    let segment = |text: &str, versatz: i64| StreamTranscriptSegment {
        channel_login: session.channel_login.clone(),
        started_at: now + Duration::seconds(versatz),
        ended_at: now + Duration::seconds(versatz + 45),
        text: text.to_string(),
        engine: "openai_api".to_string(),
        model: Some("whisper-1".to_string()),
    };

    assert!(store
        .record_transcript(&session.channel_login, &segment("der ult war zu spaet", 0))
        .await
        .expect("Ton speichern"));
    assert!(
        !store
            .record_transcript(&session.channel_login, &segment("   ", 60))
            .await
            .expect("leerer Ton"),
        "eine stille Passage ist kein Abschnitt und wird nicht abgelegt"
    );
    assert!(
        !store
            .record_transcript("fremdkanal", &segment("gehoert nicht dazu", 60))
            .await
            .expect("fremder Kanal"),
        "Ton aus einem Kanal ohne offene Sitzung gehoert zu keinem Test"
    );

    store
        .close_active_session("session_timeout", now + Duration::minutes(60))
        .await
        .expect("Sitzung beenden");

    assert!(
        !store
            .record_transcript(&session.channel_login, &segment("nach dem ende", 3600))
            .await
            .expect("Ton nach Sitzungsende"),
        "nach dem Ende laeuft kein Test mehr, dessen Ton aufzubewahren waere"
    );

    let claimed = store
        .claim_reports(5, Utc::now())
        .await
        .expect("Report claimen");
    let report = claimed
        .into_iter()
        .find(|claimed| claimed.report.session.id == session.id)
        .expect("Report der Sitzung");
    let texte: Vec<String> = report
        .report
        .transcripts
        .iter()
        .map(|segment| segment.text.clone())
        .collect();
    assert_eq!(texte, vec!["der ult war zu spaet".to_string()]);
}

#[tokio::test]
async fn jede_beendete_sitzung_wird_mit_nachrichten_oder_providerfehler_geclaimt() {
    let Some((_db, pool)) = test_pool("smalltalk_loop_reports").await else {
        return;
    };
    seed_candidate(&pool, "leer", "1", None).await;
    let store = SmalltalkLoopStore::new(pool.clone());
    let now = Utc::now();
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
    let Some((_db, pool)) = test_pool("smalltalk_loop_end_conditions").await else {
        return;
    };
    seed_candidate(&pool, "eins", "1", None).await;
    let store = SmalltalkLoopStore::new(pool.clone());
    let now = Utc::now();
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

#[tokio::test]
async fn live_state_ohne_outreach_startet_genau_eine_session_und_behaelt_id_cooldown() {
    let (_db, pool) = test_pool("independent_live").await.unwrap();
    sqlx::query("INSERT INTO twitch_live_state VALUES ('101', 'eins', 1, 'Deadlock', 2), ('102', 'zwei', 1, 'Deadlock', 1)")
        .execute(&pool).await.unwrap();
    let store = SmalltalkLoopStore::new(pool.clone()).with_live_send(true);
    let now = Utc::now();
    for (id, login) in [("101", "eins"), ("102", "zwei")] {
        store
            .record_live_preflight(
                &tb_engagement::smalltalk_loop_store::LivePreflightTarget {
                    twitch_user_id: id.into(),
                    channel_login: login.into(),
                },
                Some(49),
                true,
                now,
                None,
            )
            .await
            .unwrap();
    }
    let first = store.start_next_session(now).await.unwrap().unwrap();
    assert_eq!(first.streamer_user_id, "101");
    assert!(store
        .clone()
        .start_next_session(now)
        .await
        .unwrap()
        .is_none());
    store
        .close_active_session("test_done", now + Duration::seconds(1))
        .await
        .unwrap();
    // The other candidate is still eligible, but this process consumed its slot.
    assert!(store
        .clone()
        .start_next_session(now + Duration::seconds(2))
        .await
        .unwrap()
        .is_none());
    assert!(store
        .live_preflight_target(now + Duration::minutes(1))
        .await
        .unwrap()
        .is_none());
    let outreach: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partner_outreach")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(outreach, 0, "Smalltalk must never fabricate outreach rows");
    let cooldown: chrono::DateTime<Utc> = sqlx::query_scalar(
        "SELECT cooldown_until FROM twitch_smalltalk_candidate_state WHERE twitch_user_id='101'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        (cooldown - (now + Duration::seconds(1) + Duration::hours(24)))
            .num_microseconds()
            .unwrap()
            .abs()
            <= 1,
        "PostgreSQL stores microsecond precision"
    );
    sqlx::query(
        "UPDATE twitch_live_state SET streamer_login='umbenannt' WHERE twitch_user_id='101'",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE twitch_live_state SET is_live=0 WHERE twitch_user_id='102'")
        .execute(&pool)
        .await
        .unwrap();
    let restarted = SmalltalkLoopStore::new(pool.clone()).with_live_send(true);
    assert!(restarted
        .live_preflight_target(now + Duration::minutes(1))
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn live_auswahl_bleibt_bei_fehlenden_alten_oder_falschen_nachweisen_zu() {
    let (_db, pool) = test_pool("live_evidence_matrix").await.unwrap();
    let mutations = [
        "UPDATE twitch_smalltalk_candidate_state SET follower_count=NULL",
        "UPDATE twitch_smalltalk_candidate_state SET follower_count=50",
        "UPDATE twitch_smalltalk_candidate_state SET checked_at=NOW()-INTERVAL '12 hours 1 second'",
        "UPDATE twitch_smalltalk_candidate_state SET checked_at=NOW()+INTERVAL '1 minute'",
        "UPDATE twitch_smalltalk_candidate_state SET live_checked_at=NOW()-INTERVAL '91 seconds'",
        "UPDATE twitch_smalltalk_candidate_state SET live_deadlock=FALSE",
        "UPDATE twitch_smalltalk_candidate_state SET channel_login='anderer'",
        "UPDATE twitch_live_state SET twitch_user_id='999'",
        "UPDATE twitch_live_state SET last_game='Just Chatting'",
        "INSERT INTO twitch_raid_blacklist VALUES ('101', 'anderer_name', 'blocked')",
        "INSERT INTO twitch_partners VALUES ('101', 'eins', 'active')",
        "INSERT INTO twitch_streamers_partner_state VALUES ('101', 'eins', 1)",
        "INSERT INTO twitch_partner_outreach VALUES ('eins', '101', 'now', 'pending', 'not-a-date')",
    ];
    for mutation in mutations {
        sqlx::raw_sql("TRUNCATE twitch_smalltalk_sessions, twitch_engagement_settings, twitch_smalltalk_candidate_state, twitch_live_state, twitch_raid_blacklist, twitch_partners, twitch_streamers_partner_state, twitch_partner_outreach CASCADE")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_live_state VALUES ('101', 'eins', 1, 'Deadlock', 1)")
            .execute(&pool)
            .await
            .unwrap();
        let store = SmalltalkLoopStore::new(pool.clone()).with_live_send(true);
        store
            .record_live_preflight(
                &tb_engagement::smalltalk_loop_store::LivePreflightTarget {
                    twitch_user_id: "101".into(),
                    channel_login: "eins".into(),
                },
                Some(49),
                true,
                Utc::now(),
                None,
            )
            .await
            .unwrap();
        sqlx::raw_sql(mutation).execute(&pool).await.unwrap();
        let selected = store.start_next_session(Utc::now()).await;
        if mutation.contains("not-a-date") {
            assert!(selected.is_err(), "malformed cooldown must fail closed");
        } else {
            assert!(
                selected.expect(mutation).is_none(),
                "must fail closed: {mutation}"
            );
        }
    }
}

#[tokio::test]
async fn fehlgeschlagener_refresh_loescht_alten_erfolgsnachweis() {
    let (_db, pool) = test_pool("live_failed_refresh").await.unwrap();
    sqlx::query("INSERT INTO twitch_live_state VALUES ('101', 'eins', 1, 'Deadlock', 1)")
        .execute(&pool)
        .await
        .unwrap();
    let store = SmalltalkLoopStore::new(pool.clone()).with_live_send(true);
    let target = store
        .live_preflight_target(Utc::now())
        .await
        .unwrap()
        .unwrap();
    store
        .record_live_preflight(&target, Some(21), true, Utc::now(), None)
        .await
        .unwrap();
    store
        .record_live_preflight(&target, None, false, Utc::now(), Some("preflight_timeout"))
        .await
        .unwrap();
    assert!(store
        .start_next_session(Utc::now())
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn kandidaten_schema_und_runtime_rechte_passen_zum_release() {
    let (_db, pool) = test_pool("live_schema_grants").await.unwrap();
    let actual: std::collections::BTreeSet<String> =
        sqlx::query_as::<_, (String, String, String, String, String)>(
            "SELECT table_name, column_name, data_type, is_nullable, COALESCE(column_default, '')
         FROM information_schema.columns WHERE table_schema='public'
           AND (table_name='twitch_smalltalk_candidate_state'
                OR (table_name='twitch_smalltalk_sessions' AND column_name='live_test'))",
        )
        .fetch_all(&pool)
        .await
        .unwrap()
        .into_iter()
        .map(|(t, c, d, n, v)| format!("{t}|{c}|{d}|{n}|{v}"))
        .collect();
    let expected: std::collections::BTreeSet<String> =
        include_str!("../../tb-db/tests/fresh_schema_snapshot.txt")
            .lines()
            .filter(|line| {
                line.starts_with("twitch_smalltalk_candidate_state|")
                    || line.starts_with("twitch_smalltalk_sessions|live_test|")
            })
            .map(str::to_owned)
            .collect();
    assert_eq!(actual.len(), 11);
    assert_eq!(actual, expected);

    // This is an isolated temporary PostgreSQL cluster, never the live DB.
    sqlx::query("CREATE DATABASE twitch_analytics")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("CREATE ROLE postgres NOLOGIN")
        .execute(&pool)
        .await
        .unwrap();
    let roles_sql = include_str!("../../../../ops/systemd/twitch-runtime-roles.sql")
        .lines()
        .filter(|line| !line.trim_start().starts_with('\\'))
        .collect::<Vec<_>>()
        .join("\n");
    sqlx::raw_sql(&roles_sql).execute(&pool).await.unwrap();
    for role in ["twitchbot", "twitchdash", "twitchlegacy"] {
        for privilege in ["SELECT", "INSERT", "UPDATE", "DELETE", "TRUNCATE"] {
            let allowed: bool = sqlx::query_scalar(
                "SELECT has_table_privilege($1, 'public.twitch_smalltalk_candidate_state', $2)",
            )
            .bind(role)
            .bind(privilege)
            .fetch_one(&pool)
            .await
            .unwrap();
            let expected = privilege == "SELECT"
                || (role == "twitchbot" && matches!(privilege, "INSERT" | "UPDATE"));
            assert_eq!(allowed, expected, "{role} {privilege}");
        }
    }
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

async fn test_pool(_schema: &str) -> Option<(test_postgres::TestPostgres, PgPool)> {
    // These safety tests must run, not silently skip without an environment
    // variable. Never fall back to a production analytics DSN.
    let db = test_postgres::TestPostgres::start().await;
    let pool = db.pool.clone();
    create_test_tables(&pool).await;
    Some((db, pool))
}

async fn create_test_tables(pool: &PgPool) {
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
        CREATE TABLE twitch_raid_blacklist (
            target_id TEXT,
            target_login TEXT PRIMARY KEY,
            reason TEXT
        );
        CREATE TABLE twitch_stream_sessions (
            streamer_login TEXT NOT NULL,
            followers_start INTEGER,
            followers_end INTEGER,
            started_at TIMESTAMPTZ
        );
        CREATE TABLE twitch_engagement_settings (
            channel_login TEXT PRIMARY KEY,
            enabled BOOLEAN NOT NULL DEFAULT FALSE,
            irc_read BOOLEAN NOT NULL DEFAULT FALSE,
            output_mode TEXT NOT NULL DEFAULT 'off'
                CHECK (output_mode IN ('off', 'shadow', 'live', 'test', 'smalltalk_live'))
        );",
    )
    .await
    .expect("Testtabellen anlegen");
    pool.execute(MIGRATION)
        .await
        .expect("Smalltalk-Migration ausführen");
    pool.execute(TRANSCRIPT_MIGRATION)
        .await
        .expect("Transkript-Migration ausführen");
    pool.execute(LIVE_MODE_MIGRATION)
        .await
        .expect("Smalltalk-Live-Modus-Migration ausführen");
    pool.execute(include_str!(
        "../../../migrations/20260918024500_smalltalk_candidate_state.sql"
    ))
    .await
    .expect("Smalltalk-Kandidaten-Migration ausführen");
}

#[tokio::test]
async fn adminwahl_bleibt_nach_smalltalk_testende_auch_bei_gleichzeitigem_schreiben() {
    let db = test_postgres::TestPostgres::start().await;
    create_test_tables(&db.pool).await;
    // Sowohl vorher vorhandene Settings als auch vom Test erzeugte Zeilen:
    // explizites An/Aus muss das Ende und die Start-Aufräumung überleben.
    for existed in [false, true] {
        for enabled in [false, true] {
            for concurrent in [false, true] {
                sqlx::raw_sql("TRUNCATE twitch_smalltalk_sessions, twitch_partner_outreach, twitch_live_state, twitch_engagement_settings CASCADE")
                    .execute(&db.pool).await.unwrap();
                seed_candidate(&db.pool, "eins", "1", None).await;
                if existed {
                    sqlx::query(
                        "INSERT INTO twitch_engagement_settings (channel_login) VALUES ('eins')",
                    )
                    .execute(&db.pool)
                    .await
                    .unwrap();
                }
                let store = SmalltalkLoopStore::new(db.pool.clone());
                let now = Utc::now();
                store.start_next_session(now).await.unwrap().unwrap();
                let admin_write = async {
                    // Exakter Zustandswechsel des Dashboard-Toggles. UPSERT
                    // bildet auch den Fall ab, dass das Testende zuerst die
                    // provisorische Zeile entfernt hat.
                    sqlx::query("INSERT INTO twitch_engagement_settings (channel_login, enabled, irc_read, output_mode) VALUES ('eins', $1, $1, $2) ON CONFLICT (channel_login) DO UPDATE SET enabled=EXCLUDED.enabled, irc_read=EXCLUDED.irc_read, output_mode=EXCLUDED.output_mode")
                        .bind(enabled).bind(if enabled { "live" } else { "off" })
                        .execute(&db.pool).await.unwrap();
                };
                if concurrent {
                    let (_, closed) = tokio::join!(
                        admin_write,
                        store.close_all_open_sessions("process_start", now + Duration::minutes(1))
                    );
                    closed.unwrap();
                } else {
                    admin_write.await;
                    store
                        .close_active_session("session_timeout", now + Duration::minutes(60))
                        .await
                        .unwrap();
                }
                let actual: (bool, bool, String) = sqlx::query_as("SELECT enabled, irc_read, output_mode FROM twitch_engagement_settings WHERE channel_login='eins'")
                    .fetch_one(&db.pool).await.unwrap();
                assert_eq!(
                    actual,
                    (
                        enabled,
                        enabled,
                        if enabled { "live" } else { "off" }.into()
                    ),
                    "existed={existed}, concurrent={concurrent}"
                );
            }
        }
    }
}

fn parse_timestamp(raw: &str) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .expect("RFC3339-Cooldown")
        .with_timezone(&Utc)
}
