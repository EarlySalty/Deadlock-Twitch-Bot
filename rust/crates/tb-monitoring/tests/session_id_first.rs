//! Der Session-Pfad muss den Kanal über die stabile `twitch_user_id` finden,
//! nicht über den Login. Kontrollfall ist der reale Rename vom 2026-08-01:
//! `derechtecoolys` → `coolysdl`, user_id `520300019`.
//!
//! Die Zeile in `twitch_stream_sessions` trägt nach einem Rename für einen
//! Moment noch den alten Login — der Nachzug in `streamer_login.rs` läuft erst,
//! wenn der Rename überhaupt erkannt wurde. Genau in diesem Fenster legte der
//! alte Code eine zweite Session an, weil er die offene nicht fand.

use sqlx::PgPool;
use tb_monitoring::sessions::store::SessionStore;
use tb_monitoring::{LiveStateStore, NewSession, StartOutcome};

mod support;

/// Ohne Test-DB werden alle Tests dieser Datei zu stillen No-ops — ein Lauf
/// meldet dann „ok" und hat nichts geprüft. Wer einen Testnachweis über diese
/// Datei führt, setzt `TB_TEST_REQUIRE_DB=1`: `support::pool_in_schema` panict
/// dann statt `None` zu liefern.
macro_rules! pool_or_skip {
    ($schema:expr) => {
        match support::pool_in_schema($schema).await {
            Some(pool) => pool,
            None => return,
        }
    };
}

const ALT: &str = "derechtecoolys";
const NEU: &str = "coolysdl";
const UID: &str = "520300019";

async fn offene_session_mit_altem_login(pool: &PgPool) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "INSERT INTO twitch_stream_sessions (streamer_login, twitch_user_id, started_at)
         VALUES ($1, $2, '2026-08-01T10:00:00Z') RETURNING id",
    )
    .bind(ALT)
    .bind(UID)
    .fetch_one(pool)
    .await
    .expect("Session anlegen")
}

/// Kernfall: veralteter Login in der Zeile, richtige ID gebunden.
#[tokio::test]
async fn claim_open_id_findet_session_ueber_die_id_trotz_altem_login() {
    let pool = pool_or_skip!("t_sess_id_first_kern");
    let erwartet = offene_session_mit_altem_login(&pool).await;
    let store = SessionStore::new(pool);

    let gefunden = store
        .claim_open_id(NEU, Some(UID))
        .await
        .expect("Lookup darf nicht fehlschlagen");

    assert_eq!(
        gefunden,
        Some(erwartet),
        "Session mit veraltetem Login muss über die ID gefunden werden"
    );
}

/// Gegenprobe zum Kernfall: ohne ID greift nur der Login-Pfad — und der findet
/// die Zeile nicht. Wird dieser Test grün, während der Kernfall es auch ist,
/// beweist das, dass die ID die Auflösung trägt und nicht der Name.
#[tokio::test]
async fn claim_open_id_ohne_id_findet_die_umbenannte_session_nicht() {
    let pool = pool_or_skip!("t_sess_id_first_gegenprobe");
    offene_session_mit_altem_login(&pool).await;
    let store = SessionStore::new(pool);

    let gefunden = store
        .claim_open_id(NEU, None)
        .await
        .expect("Lookup darf nicht fehlschlagen");

    assert_eq!(
        gefunden, None,
        "ohne ID darf der neue Login die alte Zeile nicht treffen — sonst misst \
         der Kernfall-Test nicht die ID"
    );
}

/// Der Kernfall reicht nicht: an derselben offenen Zeile hängen Leser, die nur
/// den Login kennen — allen voran `tb-chat::chatter_tracking::resolve_session_id`
/// (`WHERE streamer_login = $1 AND ended_at IS NULL`). Findet nur das
/// Monitoring die Session über die ID und bleibt der Login stehen, verwirft der
/// Chat im Rename-Fenster jede Nachricht still: keine `twitch_chat_messages`,
/// keine `twitch_session_chatters`, `unique_chatters` im Finalize = 0. Vorher
/// entstand wenigstens eine zweite Zeile unter dem neuen Namen — also war der
/// halbe ID-Umbau schlechter als gar keiner (Merge-Kritiker 10.08.2026).
#[tokio::test]
async fn claim_open_id_zieht_den_login_nach_und_der_chat_findet_die_session_wieder() {
    let pool = pool_or_skip!("t_sess_id_first_nachzug");
    let erwartet = offene_session_mit_altem_login(&pool).await;
    let store = SessionStore::new(pool.clone());

    let gefunden = store
        .claim_open_id(NEU, Some(UID))
        .await
        .expect("Lookup darf nicht fehlschlagen");
    assert_eq!(gefunden, Some(erwartet), "Vorbedingung: über die ID gefunden");

    // Wortgleich zu chatter_tracking::resolve_session_id.
    let per_login: Option<i64> = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM twitch_stream_sessions \
         WHERE streamer_login = $1 AND ended_at IS NULL \
         ORDER BY started_at DESC LIMIT 1",
    )
    .bind(NEU)
    .fetch_optional(&pool)
    .await
    .expect("Login-Lookup");

    assert_eq!(
        per_login,
        Some(erwartet),
        "nach der ID-Adoption muss der login-geschlüsselte Zwilling dieselbe \
         Session finden — sonst schreibt der Chat ins Leere"
    );
}

/// Dasselbe für den Adoptionszweig in `start_session`.
#[tokio::test]
async fn start_session_zieht_den_login_der_adoptierten_zeile_nach() {
    let pool = pool_or_skip!("t_sess_id_first_start_nachzug");
    let bestehend = offene_session_mit_altem_login(&pool).await;
    let store = SessionStore::new(pool.clone());

    store
        .start_session(&NewSession {
            streamer_login: NEU.to_string(),
            twitch_user_id: Some(UID.to_string()),
            stream_id: Some("42".to_string()),
            started_at: chrono::Utc::now(),
            viewer_count: 7,
            followers_start: None,
            title: "Titel".to_string(),
            language: "de".to_string(),
            is_mature: false,
            tags: String::new(),
            game_name: None,
            had_deadlock: false,
        })
        .await
        .expect("start_session darf nicht fehlschlagen");

    let login: String =
        sqlx::query_scalar::<_, String>("SELECT streamer_login FROM twitch_stream_sessions WHERE id = $1")
            .bind(bestehend)
            .fetch_one(&pool)
            .await
            .expect("Zeile lesen");
    assert_eq!(login, NEU, "die adoptierte Zeile muss den aktuellen Login tragen");
}

/// Gegenprobe zum Nachzug: ohne Rename darf er nichts anfassen. Sonst prüfen
/// die beiden Tests oben nur, dass irgendein UPDATE läuft.
#[tokio::test]
async fn ohne_rename_bleibt_der_login_unveraendert() {
    let pool = pool_or_skip!("t_sess_id_first_kein_nachzug");
    let id: i64 = sqlx::query_scalar::<_, i64>(
        "INSERT INTO twitch_stream_sessions (streamer_login, twitch_user_id, started_at)
         VALUES ($1, $2, '2026-08-01T10:00:00Z') RETURNING id",
    )
    .bind(NEU)
    .bind(UID)
    .fetch_one(&pool)
    .await
    .expect("Session anlegen");
    let store = SessionStore::new(pool.clone());

    store
        .claim_open_id(NEU, Some(UID))
        .await
        .expect("Lookup darf nicht fehlschlagen");

    let login: String =
        sqlx::query_scalar::<_, String>("SELECT streamer_login FROM twitch_stream_sessions WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("Zeile lesen");
    assert_eq!(login, NEU);
}

/// Der Login-Pfad muss weiter tragen, solange die ID-Spalte leer ist
/// (Backfill-Restmenge, Prod: 8393 von 9325 Zeilen haben eine ID).
#[tokio::test]
async fn claim_open_id_faellt_auf_den_login_zurueck_wenn_die_id_fehlt() {
    let pool = pool_or_skip!("t_sess_id_first_fallback");
    let erwartet: i64 = sqlx::query_scalar::<_, i64>(
        "INSERT INTO twitch_stream_sessions (streamer_login, started_at)
         VALUES ($1, '2026-08-01T10:00:00Z') RETURNING id",
    )
    .bind(NEU)
    .fetch_one(&pool)
    .await
    .expect("Session ohne ID anlegen");
    let store = SessionStore::new(pool);

    let gefunden = store
        .claim_open_id(NEU, Some(UID))
        .await
        .expect("Lookup darf nicht fehlschlagen");

    assert_eq!(
        gefunden,
        Some(erwartet),
        "Zeile ohne ID muss über den Login gefunden werden"
    );
}

/// `start_session` darf keine zweite Session anlegen, wenn schon eine offene
/// unter dem alten Login existiert. Das ist der eigentliche Schaden des
/// Rename-Falls: doppelte Sessions, gespaltene Kennzahlen.
#[tokio::test]
async fn start_session_adoptiert_die_offene_session_des_alten_logins() {
    let pool = pool_or_skip!("t_sess_id_first_start");
    let bestehend = offene_session_mit_altem_login(&pool).await;
    let store = SessionStore::new(pool.clone());

    let outcome = store
        .start_session(&NewSession {
            streamer_login: NEU.to_string(),
            twitch_user_id: Some(UID.to_string()),
            stream_id: Some("42".to_string()),
            started_at: chrono::Utc::now(),
            viewer_count: 7,
            followers_start: None,
            title: "Titel".to_string(),
            language: "de".to_string(),
            is_mature: false,
            tags: String::new(),
            game_name: None,
            had_deadlock: false,
        })
        .await
        .expect("start_session darf nicht fehlschlagen");

    assert!(
        matches!(outcome, StartOutcome::AlreadyOpen(id) if id == bestehend),
        "erwartet AlreadyOpen({bestehend}), war {outcome:?}"
    );
    let anzahl: i64 =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM twitch_stream_sessions WHERE ended_at IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(anzahl, 1, "es darf keine zweite offene Session entstehen");
}

/// Der Schreibpfad muss die ID selbst setzen. Solange er das nicht tut, hängt
/// die Spalte am Übergangstrigger aus 20260801200000 — und der löst über den
/// Namen auf, was bei einem freigegebenen Namen die falsche Identität trifft.
#[tokio::test]
async fn start_session_schreibt_die_id_in_die_neue_zeile() {
    let pool = pool_or_skip!("t_sess_id_first_insert");
    let store = SessionStore::new(pool.clone());

    let outcome = store
        .start_session(&NewSession {
            streamer_login: NEU.to_string(),
            twitch_user_id: Some(UID.to_string()),
            stream_id: None,
            started_at: chrono::Utc::now(),
            viewer_count: 0,
            followers_start: None,
            title: String::new(),
            language: "de".to_string(),
            is_mature: false,
            tags: String::new(),
            game_name: None,
            had_deadlock: false,
        })
        .await
        .expect("start_session darf nicht fehlschlagen");

    let geschrieben: Option<String> =
        sqlx::query_scalar::<_, Option<String>>("SELECT twitch_user_id FROM twitch_stream_sessions WHERE id = $1")
            .bind(outcome.session_id())
            .fetch_one(&pool)
            .await
            .unwrap();

    assert_eq!(
        geschrieben.as_deref(),
        Some(UID),
        "die neue Session muss die ID tragen, ohne dass ein Trigger sie nachträgt"
    );
}

/// `twitch_live_state` ist über `twitch_user_id` geschlüsselt (Primärschlüssel).
/// Ein Lookup über den Login trifft nach einer Umbenennung nichts — und ohne
/// diesen Zustand schließt `finalize` die Session ohne Spielstand und ohne
/// Follower-Differenz ab.
#[tokio::test]
async fn finalize_state_findet_die_zeile_ueber_die_id_trotz_altem_login() {
    let pool = pool_or_skip!("t_live_state_id_first");
    sqlx::query(
        "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, last_game, had_deadlock_in_session)
         VALUES ($1, $2, 'Deadlock', 1)",
    )
    .bind(UID)
    .bind(ALT)
    .execute(&pool)
    .await
    .expect("Live-State anlegen");
    let store = LiveStateStore::new(pool);

    let state = store
        .finalize_state(NEU, Some(UID))
        .await
        .expect("Lookup darf nicht fehlschlagen");

    let state = state.expect("Zeile muss über die ID gefunden werden");
    assert_eq!(state.twitch_user_id.as_deref(), Some(UID));
    assert_eq!(state.last_game.as_deref(), Some("Deadlock"));
}

/// Gegenprobe: ohne ID trifft der neue Login die Zeile nicht.
#[tokio::test]
async fn finalize_state_ohne_id_findet_die_umbenannte_zeile_nicht() {
    let pool = pool_or_skip!("t_live_state_gegenprobe");
    sqlx::query(
        "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, last_game)
         VALUES ($1, $2, 'Deadlock')",
    )
    .bind(UID)
    .bind(ALT)
    .execute(&pool)
    .await
    .expect("Live-State anlegen");
    let store = LiveStateStore::new(pool);

    let state = store
        .finalize_state(NEU, None)
        .await
        .expect("Lookup darf nicht fehlschlagen");

    assert!(
        state.is_none(),
        "ohne ID darf der neue Login die alte Zeile nicht treffen"
    );
}

/// Der Orphan-Cleanup muss die ID aus der Session-Zeile mitnehmen, statt sie
/// später über den Login zurückzurechnen.
#[tokio::test]
async fn orphan_candidates_liefern_die_id_mit() {
    let pool = pool_or_skip!("t_orphan_id");
    sqlx::query(
        "INSERT INTO twitch_stream_sessions (streamer_login, twitch_user_id, started_at, samples)
         VALUES ($1, $2, (NOW() - INTERVAL '48 hours')::text, 0)",
    )
    .bind(ALT)
    .bind(UID)
    .execute(&pool)
    .await
    .expect("verwaiste Session anlegen");
    let store = SessionStore::new(pool);

    let (ohne_samples, _stale) = store
        .orphan_candidates()
        .await
        .expect("Kandidaten laden darf nicht fehlschlagen");

    let kandidat = ohne_samples
        .iter()
        .find(|c| c.streamer_login == ALT)
        .expect("die verwaiste Session muss auftauchen");
    assert_eq!(
        kandidat.twitch_user_id.as_deref(),
        Some(UID),
        "der Kandidat muss die ID aus der Zeile tragen"
    );
}

/// Die Raid-Retention löste die Ziel-Session über `to_broadcaster_login` auf,
/// obwohl `twitch_raid_history` die Ziel-ID mitliefert (Helix schickt sie an
/// jedem Raid; auf Prod ist sie sogar der Kompressions-Segmentschlüssel).
/// Nach einer Umbenennung des Ziels fand der Login-Weg keine Session — der
/// Raid fiel still als `SkippedNoSession` heraus.
#[tokio::test]
async fn raid_retention_findet_die_zielsession_ueber_die_id() {
    let Some(pool) = support::pool_with_chatters_schema("t_raid_retention_id").await else {
        return;
    };
    let executed = chrono::Utc::now() - chrono::Duration::hours(2);
    // Session des Ziels läuft noch unter dem alten Login, trägt aber die ID.
    sqlx::query(
        "INSERT INTO twitch_stream_sessions (id, streamer_login, twitch_user_id, started_at, samples)
         VALUES (9001, $1, $2, $3, 5)",
    )
    .bind(ALT)
    .bind(UID)
    .bind(executed - chrono::Duration::hours(1))
    .execute(&pool)
    .await
    .expect("Ziel-Session anlegen");
    // Der Raid kennt das Ziel bereits unter dem neuen Namen — plus ID.
    sqlx::query(
        "INSERT INTO twitch_raid_history
             (id, from_broadcaster_login, to_broadcaster_login, to_broadcaster_id,
              viewer_count, executed_at)
         VALUES (7001, 'quelle', $1, $2, 12, $3)",
    )
    .bind(NEU)
    .bind(UID)
    .bind(executed)
    .execute(&pool)
    .await
    .expect("Raid anlegen");

    let stats = tb_monitoring::raid_retention::compute_raid_retention(&pool)
        .await
        .expect("Retention-Lauf darf nicht fehlschlagen");

    assert_eq!(
        stats.raids_skipped_no_session, 0,
        "die Ziel-Session muss über die ID gefunden werden, nicht über den Namen"
    );
    assert_eq!(stats.raids_computed, 1, "der Raid muss berechnet werden");
}

/// Kehrseite des ID-Vorrangs: trägt die Session-Zeile eine *andere* ID als der
/// Raid, greift der Login-Zweig bewusst nicht mehr — der Raid fällt als
/// `SkippedNoSession` heraus, statt einer fremden Session zugerechnet zu werden.
///
/// Das ist die gewollte Richtung: bei einem Rename ist gerade der Login
/// mehrdeutig, die ID nicht. Der Preis steht hier im Test, damit ihn niemand
/// versehentlich mit einem Login-Fallback wieder einhandelt — und der Fall
/// bleibt über die `debug!`-Zeile in `compute_one` auffindbar
/// (Merge-Kritiker 10.08.2026).
#[tokio::test]
async fn raid_retention_ueberspringt_session_mit_fremder_id() {
    let Some(pool) = support::pool_with_chatters_schema("t_raid_retention_fremd").await else {
        return;
    };
    let executed = chrono::Utc::now() - chrono::Duration::hours(2);
    // Login passt, ID nicht: ein anderer Kanal, der denselben Namen führt.
    sqlx::query(
        "INSERT INTO twitch_stream_sessions (id, streamer_login, twitch_user_id, started_at, samples)
         VALUES (9002, $1, '999999999', $2, 5)",
    )
    .bind(NEU)
    .bind(executed - chrono::Duration::hours(1))
    .execute(&pool)
    .await
    .expect("Fremd-Session anlegen");
    sqlx::query(
        "INSERT INTO twitch_raid_history
             (id, from_broadcaster_login, to_broadcaster_login, to_broadcaster_id,
              viewer_count, executed_at)
         VALUES (7002, 'quelle', $1, $2, 12, $3)",
    )
    .bind(NEU)
    .bind(UID)
    .bind(executed)
    .execute(&pool)
    .await
    .expect("Raid anlegen");

    let stats = tb_monitoring::raid_retention::compute_raid_retention(&pool)
        .await
        .expect("Retention-Lauf darf nicht fehlschlagen");

    assert_eq!(
        stats.raids_computed, 0,
        "eine Session mit fremder ID darf dem Raid nicht zugerechnet werden"
    );
    assert_eq!(
        stats.raids_skipped_no_session, 1,
        "der Raid muss als SkippedNoSession gezählt werden"
    );
}
