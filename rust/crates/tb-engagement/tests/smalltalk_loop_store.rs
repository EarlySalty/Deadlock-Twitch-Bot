use std::str::FromStr;

// Testuhr ist bewusst `Utc::now()` und kein fixes Datum: die Tabelle setzt
// `discord_next_attempt_at`/`expires_at` per DB-`NOW()`, und `claim_reports`
// vergleicht beide gegen die übergebene Zeit. Eine erfundene Uhr lief dieser
// Vergleichsbasis davon — der Report-Test wurde am 2026-07-27 um 20:08 UTC
// von selbst rot, ohne dass sich Code geändert hatte.
use chrono::{Duration, Utc};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{Executor, PgPool};
use tb_engagement::minimax_chat::TestModeRejectReason;
use tb_engagement::smalltalk_loop_store::{GeneratedOutcome, SmalltalkLoopStore};
use tb_engagement::stream_transcripts::StreamTranscriptSegment;

const MIGRATION: &str =
    include_str!("../../../migrations/20260727150000_twitch_smalltalk_loop.sql");
const TRANSCRIPT_MIGRATION: &str =
    include_str!("../../../migrations/20260813220000_twitch_smalltalk_transcripts.sql");

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
    let Some(pool) = test_pool("smalltalk_loop_case").await else {
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

/// Ein Kanal, in dem der Bot gebannt ist, landet als `bot_banned` in
/// `twitch_raid_blacklist` (`token_lifecycle::mark_bot_banned_inner`). Dort
/// zu messen, ob der Bot mitreden koennte, ist sinnlos: senden koennte er
/// ohnehin nie. Die Blacklist gilt komplett, auch fuer von Hand gesetzte
/// Eintraege, denn beides heisst "in diesem Kanal nicht auftreten".
#[tokio::test]
async fn gebannte_und_geblacklistete_kanaele_werden_nie_ausgewaehlt() {
    let Some(pool) = test_pool("smalltalk_loop_blacklist").await else {
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
    let Some(pool) = test_pool("smalltalk_loop_blacklist_id").await else {
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
    let Some(pool) = test_pool("smalltalk_loop_transcripts").await else {
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
    let Some(pool) = test_pool("smalltalk_loop_reports").await else {
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
    let Some(pool) = test_pool("smalltalk_loop_end_conditions").await else {
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
        CREATE TABLE twitch_raid_blacklist (
            target_id TEXT,
            target_login TEXT PRIMARY KEY,
            reason TEXT
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
    pool.execute(TRANSCRIPT_MIGRATION)
        .await
        .expect("Transkript-Migration ausführen");
    Some(pool)
}

fn parse_timestamp(raw: &str) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .expect("RFC3339-Cooldown")
        .with_timezone(&Utc)
}
