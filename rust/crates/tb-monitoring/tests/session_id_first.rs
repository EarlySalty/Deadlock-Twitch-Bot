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
use tb_monitoring::{NewSession, StartOutcome};

mod support;

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
async fn find_open_id_findet_session_ueber_die_id_trotz_altem_login() {
    let pool = pool_or_skip!("t_sess_id_first_kern");
    let erwartet = offene_session_mit_altem_login(&pool).await;
    let store = SessionStore::new(pool);

    let gefunden = store
        .find_open_id(NEU, Some(UID))
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
async fn find_open_id_ohne_id_findet_die_umbenannte_session_nicht() {
    let pool = pool_or_skip!("t_sess_id_first_gegenprobe");
    offene_session_mit_altem_login(&pool).await;
    let store = SessionStore::new(pool);

    let gefunden = store
        .find_open_id(NEU, None)
        .await
        .expect("Lookup darf nicht fehlschlagen");

    assert_eq!(
        gefunden, None,
        "ohne ID darf der neue Login die alte Zeile nicht treffen — sonst misst \
         der Kernfall-Test nicht die ID"
    );
}

/// Der Login-Pfad muss weiter tragen, solange die ID-Spalte leer ist
/// (Backfill-Restmenge, Prod: 8393 von 9325 Zeilen haben eine ID).
#[tokio::test]
async fn find_open_id_faellt_auf_den_login_zurueck_wenn_die_id_fehlt() {
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
        .find_open_id(NEU, Some(UID))
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
