use super::*;
use tb_scout::community_privacy::{apply_operation, export};

#[tokio::test]
async fn privacy_httpwege_pruefen_auth_auftrag_und_fehlerstatus() {
    use axum::{body::Body, extract::ConnectInfo, http::Request};
    let db = super::tests::migrated_pool("tb_scout_privacy_http").await;
    let router = super::tests::router(db.pool.clone(), None);
    let request = |route: &str, body: Option<serde_json::Value>, token: Option<&str>| {
        let mut request = Request::builder()
            .method(if body.is_some() { "POST" } else { "GET" })
            .uri(format!(
                "/internal/twitch/v1/scout/community-privacy/{route}"
            ))
            .extension(ConnectInfo(
                "127.0.0.1:5000".parse::<std::net::SocketAddr>().unwrap(),
            ));
        if let Some(token) = token {
            request = request.header("X-Internal-Token", token);
        }
        if body.is_some() {
            request = request.header("Content-Type", "application/json");
        }
        request
            .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
            .unwrap()
    };
    let erase = json!({"discord_user_id":A,"operation_id":uuid::Uuid::new_v4(),"epoch":1});
    for (route, body) in [
        (format!("export?discord_user_id={A}"), None),
        ("erase".into(), Some(erase.clone())),
        ("consent".into(), Some(erase.clone())),
    ] {
        assert_eq!(
            super::tests::call(&router, request(&route, body, None))
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        super::tests::call(
            &router,
            request("consent", Some(erase.clone()), Some("test-token"))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (status, value) = super::tests::call(
        &router,
        request("erase", Some(erase.clone()), Some("test-token")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["status"], "applied");
    assert_eq!(value["discord_user_id"], A);
    let (_, value) =
        super::tests::call(&router, request("erase", Some(erase), Some("test-token"))).await;
    assert_eq!(value["status"], "replayed");
    let conflict = json!({"discord_user_id":A,"operation_id":uuid::Uuid::new_v4(),"epoch":1});
    assert_eq!(
        super::tests::call(
            &router,
            request("erase", Some(conflict), Some("test-token"))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        super::tests::call(
            &router,
            request("export?discord_user_id=kein", None, Some("test-token"))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
}
const A: &str = "388772056717590539";
const B: &str = "288772056717590538";
fn at() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339("2026-10-01T10:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc)
}
fn entry(actor: &str, key: &str, id: &str, login: &str) -> VorschlagEingabe {
    VorschlagEingabe {
        discord_id: actor.into(),
        idempotency_key: key.into(),
        twitch_user_id: id.into(),
        twitch_login: login.into(),
        grund: Some("Synthetischer eigener Grund".into()),
        submitted_at: Some(at()),
        privacy_epoch: Some(0),
    }
}

#[tokio::test]
async fn export_erasure_verlorene_antwort_replay_und_monotone_consent_epoche() {
    let db = super::tests::migrated_pool("tb_scout_privacy_flow").await;
    let pool = &db.pool;
    let original = entry(A, "erase-original", "1001", "neuling");
    vorschlag_einreichen(pool, &original).await.unwrap();
    vorschlag_einreichen(pool, &entry(B, "fremde-zeile", "1002", "anderer"))
        .await
        .unwrap();
    let before = export(pool, A).await.unwrap();
    assert_eq!(before.suggestions.len(), 1);
    assert_eq!(before.candidates.len(), 1);
    assert_eq!(
        before.suggestions[0].reason.as_deref(),
        Some("Synthetischer eigener Grund")
    );
    let erase_id = uuid::Uuid::new_v4();
    // Die erste erfolgreiche Antwort geht verloren. Der zentrale Caller muss
    // seinen gespeicherten Auftrag nach Rollback mit derselben UUID wiederholen.
    let _ = apply_operation(pool, A, erase_id, 1, None).await.unwrap();
    assert_eq!(
        apply_operation(pool, A, erase_id, 1, None)
            .await
            .unwrap()
            .status,
        "replayed"
    );
    assert!(matches!(
        apply_operation(pool, A, uuid::Uuid::new_v4(), 1, None).await,
        Err(VorschlagFehler::Konflikt)
    ));
    let erased = export(pool, A).await.unwrap();
    assert!(erased.suggestions.is_empty());
    assert!(erased.candidates.is_empty());
    assert_eq!(export(pool, B).await.unwrap().suggestions.len(), 1);
    let candidate:(String,Option<String>,Option<String>,Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as("SELECT status,suggested_by_discord_id,suggestion_reason,suggested_at FROM twitch_scout_candidates WHERE twitch_user_id='1001'")
        .fetch_one(pool).await.unwrap();
    assert_eq!(candidate, ("vorgeschlagen".into(), None, None, None));
    assert!(matches!(
        vorschlag_einreichen(pool, &original).await,
        Err(VorschlagFehler::PrivacyGesperrt)
    ));
    let consent_id = uuid::Uuid::new_v4();
    apply_operation(pool, A, consent_id, 2, Some(at()))
        .await
        .unwrap();
    assert_eq!(
        apply_operation(pool, A, consent_id, 2, Some(at()))
            .await
            .unwrap()
            .status,
        "replayed"
    );
    let late = apply_operation(pool, A, erase_id, 1, None).await.unwrap();
    assert_eq!(late.status, "stale");
    assert_eq!(late.epoch, 1);
    assert_eq!(late.activity_since.as_deref(), Some("2026-10-01T10:00:00Z"));
    let mut replay = original.clone();
    replay.privacy_epoch = Some(2);
    assert!(
        matches!(
            vorschlag_einreichen(pool, &replay).await,
            Err(VorschlagFehler::PrivacyGesperrt)
        ),
        "Auch ein umgebundener alter Key bleibt gelöscht"
    );
    let mut new = entry(A, "neue-einwilligung", "1003", "frischer");
    new.privacy_epoch = Some(2);
    vorschlag_einreichen(pool, &new).await.unwrap();
    assert_eq!(export(pool, A).await.unwrap().suggestions.len(), 1);
    let mut late_old = entry(A, "alter-offener-auftrag", "1004", "verspaetet");
    late_old.submitted_at = Some(at() - chrono::Duration::seconds(1));
    late_old.privacy_epoch = Some(2);
    assert!(matches!(
        vorschlag_einreichen(pool, &late_old).await,
        Err(VorschlagFehler::PrivacyGesperrt)
    ));
    apply_operation(pool, A, uuid::Uuid::new_v4(), 3, None)
        .await
        .unwrap();
    assert_eq!(
        apply_operation(pool, A, consent_id, 2, Some(at()))
            .await
            .unwrap()
            .status,
        "stale"
    );
    assert!(matches!(
        vorschlag_einreichen(pool, &new).await,
        Err(VorschlagFehler::PrivacyGesperrt)
    ));
}

async fn wait_on_lock(pool: &PgPool, objid: i64) {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE locktype='advisory' AND objid::bigint=$1 AND NOT granted)")
            .bind(objid).fetch_one(pool).await.unwrap();
        if waiting {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Erwarteter echter Advisory-Wartezustand fehlt"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}
async fn race(erasure_first: bool) {
    let db = super::tests::migrated_pool("tb_scout_privacy_race").await;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect_with((*db.pool.connect_options()).clone())
        .await
        .unwrap();
    sqlx::raw_sql("CREATE FUNCTION privacy_fixture_pause() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_xact_lock(99998); RETURN NEW; END $$;")
        .execute(&pool).await.unwrap();
    let table = if erasure_first {
        "twitch_scout_community_privacy"
    } else {
        "twitch_scout_community_suggestions"
    };
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE TRIGGER privacy_fixture_pause BEFORE INSERT ON {table} FOR EACH ROW EXECUTE FUNCTION privacy_fixture_pause()")))
        .execute(&pool).await.unwrap();
    let mut pause = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(99998)")
        .execute(&mut *pause)
        .await
        .unwrap();
    let first_pool = pool.clone();
    let first_entry = entry(A, "race", "1100", "rennen");
    let first = tokio::spawn(async move {
        if erasure_first {
            apply_operation(&first_pool, A, uuid::Uuid::new_v4(), 1, None)
                .await
                .map(|_| ())
        } else {
            vorschlag_einreichen(&first_pool, &first_entry)
                .await
                .map(|_| ())
        }
    });
    wait_on_lock(&pool, 99998).await;
    let second_pool = pool.clone();
    let second_entry = entry(A, "race", "1100", "rennen");
    let second = tokio::spawn(async move {
        if erasure_first {
            vorschlag_einreichen(&second_pool, &second_entry)
                .await
                .map(|_| ())
        } else {
            apply_operation(&second_pool, A, uuid::Uuid::new_v4(), 1, None)
                .await
                .map(|_| ())
        }
    });
    wait_on_lock(&pool, (0x5c07_c0de_f001_i64 & 0xffff_ffff)).await;
    pause.commit().await.unwrap();
    first.await.unwrap().unwrap();
    let second = second.await.unwrap();
    if erasure_first {
        assert!(matches!(second, Err(VorschlagFehler::PrivacyGesperrt)));
    } else {
        second.unwrap();
    }
    let result = export(&pool, A).await.unwrap();
    assert!(result.suggestions.is_empty());
    assert!(result.candidates.is_empty());
    pool.close().await;
}
#[tokio::test]
async fn schreiben_vor_erasure_wird_vollständig_gelöscht() {
    race(false).await;
}
#[tokio::test]
async fn erasure_vor_schreiben_sperrt_den_neuen_auftrag() {
    race(true).await;
}

#[tokio::test]
async fn erstautor_erasure_erhaelt_fremde_outcomes_mit_eigener_consent_epoche() {
    use tb_scout::community::liste_ergebnisse;
    let db = super::tests::migrated_pool("tb_scout_individual_outcomes").await;
    let pool = &db.pool;
    let original = entry(A, "first-private", "1001", "gemeinsam");
    vorschlag_einreichen(pool, &original).await.unwrap();
    apply_operation(pool, B, uuid::Uuid::new_v4(), 1, Some(at()))
        .await
        .unwrap();
    let mut other = entry(B, "other-private", "1001", "gemeinsam");
    other.privacy_epoch = Some(1);
    vorschlag_einreichen(pool, &other).await.unwrap();
    // Ein weiterer Key derselben Person erzeugt keine zweite Zuordnung.
    let mut duplicate = other.clone();
    duplicate.idempotency_key = "other-second-key".into();
    vorschlag_einreichen(pool, &duplicate).await.unwrap();
    let first = liste_ergebnisse(pool, None, 1).await.unwrap();
    assert_eq!(first.rows.len(), 1);
    assert_eq!(first.rows[0].suggested_by_discord_id, A);
    assert!(first.rows[0].is_first_eligible);
    assert!(first.has_more);
    let cursor = first.rows[0]
        .updated_at
        .parse::<chrono::DateTime<chrono::Utc>>()
        .unwrap();
    let second = liste_ergebnisse(pool, Some(cursor), 1).await.unwrap();
    assert_eq!(second.rows.len(), 1);
    assert_eq!(second.rows[0].suggested_by_discord_id, B);
    assert!(!second.rows[0].is_first_eligible);
    assert_eq!(second.rows[0].privacy_epoch, Some(1));
    assert_eq!(
        second.rows[0].submitted_at.as_deref(),
        Some("2026-10-01T10:00:00Z")
    );
    assert_ne!(first.rows[0].updated_at, second.rows[0].updated_at);
    assert!(!second.has_more);
    let cursor = second.rows[0]
        .updated_at
        .parse::<chrono::DateTime<chrono::Utc>>()
        .unwrap();
    assert!(liste_ergebnisse(pool, Some(cursor), 1)
        .await
        .unwrap()
        .rows
        .is_empty());

    apply_operation(pool, A, uuid::Uuid::new_v4(), 1, None)
        .await
        .unwrap();
    let (author, reason, when): (Option<String>, Option<String>, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as("SELECT suggested_by_discord_id,suggestion_reason,suggested_at FROM twitch_scout_candidates WHERE twitch_user_id='1001'")
        .fetch_one(pool).await.unwrap();
    assert_eq!((author, reason, when), (None, None, None));
    sqlx::query("INSERT INTO twitch_partners (twitch_user_id,twitch_login,status,partnered_at) VALUES ('1001','gemeinsam','active','2026-10-02 10:00:00+00')")
        .execute(pool).await.unwrap();
    // Die Erasure stempelt die fortbestehende fremde Zuordnung hinter den Cursor.
    let remaining = liste_ergebnisse(pool, Some(cursor), 1).await.unwrap();
    assert_eq!(remaining.rows.len(), 1);
    assert_eq!(remaining.rows[0].suggested_by_discord_id, B);
    assert!(remaining.rows[0].is_first_eligible);
    assert_eq!(remaining.rows[0].privacy_epoch, Some(1));
    assert!(remaining.rows[0].is_partner_active);
    assert_eq!(
        remaining.rows[0].partner_since.as_deref(),
        Some("2026-10-02T10:00:00Z")
    );
    assert_eq!(remaining.rows[0].suggested_at, second.rows[0].suggested_at);
    assert_eq!(export(pool, A).await.unwrap().suggestions.len(), 0);
    assert_eq!(export(pool, B).await.unwrap().suggestions.len(), 2);

    // Eine neue Epoche macht frühere fremde Herkunft ungültig.
    let floor = at() + chrono::Duration::minutes(1);
    apply_operation(pool, B, uuid::Uuid::new_v4(), 2, Some(floor))
        .await
        .unwrap();
    assert!(liste_ergebnisse(pool, None, 10)
        .await
        .unwrap()
        .rows
        .is_empty());
    let mut already_partner = entry(B, "other-consent-still-partner", "1001", "gemeinsam");
    already_partner.privacy_epoch = Some(2);
    already_partner.submitted_at = Some(floor);
    assert_eq!(
        vorschlag_einreichen(pool, &already_partner).await.unwrap(),
        VorschlagStatus::AlreadyPartner
    );
    // Für den frischen berechtigten Vorschlag ist der Kanal noch kein Partner.
    sqlx::query("DELETE FROM twitch_partners WHERE twitch_user_id='1001'")
        .execute(pool)
        .await
        .unwrap();
    let mut fresh = entry(B, "other-new-consent", "1001", "gemeinsam");
    fresh.privacy_epoch = Some(2);
    fresh.submitted_at = Some(floor);
    assert_eq!(
        vorschlag_einreichen(pool, &fresh).await.unwrap(),
        VorschlagStatus::AlreadyKnown
    );
    let page = liste_ergebnisse(pool, None, 10).await.unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].suggested_by_discord_id, B);
    assert_eq!(page.rows[0].privacy_epoch, Some(2));
    assert_eq!(
        page.rows[0].submitted_at.as_deref(),
        Some("2026-10-01T10:01:00Z")
    );
    let author: Option<String> = sqlx::query_scalar(
        "SELECT suggested_by_discord_id FROM twitch_scout_candidates WHERE twitch_user_id='1001'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(author, None);
    apply_operation(pool, B, uuid::Uuid::new_v4(), 3, None)
        .await
        .unwrap();
    assert!(liste_ergebnisse(pool, None, 10)
        .await
        .unwrap()
        .rows
        .is_empty());
}

#[tokio::test]
async fn frueherer_neuer_schluessel_stempelt_beide_paare_hinter_cursor() {
    use tb_scout::community::liste_ergebnisse;
    let db = super::tests::migrated_pool("tb_scout_earlier_key_cursor").await;
    let pool = &db.pool;
    let mut first = entry(A, "first-origin", "1001", "chronologie");
    first.submitted_at = Some(at() + chrono::Duration::seconds(1));
    vorschlag_einreichen(pool, &first).await.unwrap();
    let mut later = entry(B, "later-origin", "1001", "chronologie");
    later.submitted_at = Some(at() + chrono::Duration::seconds(2));
    vorschlag_einreichen(pool, &later).await.unwrap();
    let initial = liste_ergebnisse(pool, None, 10).await.unwrap();
    assert_eq!(initial.rows.len(), 2);
    assert_eq!(initial.rows[0].suggested_by_discord_id, A);
    assert!(initial.rows[0].is_first_eligible);
    assert!(!initial.rows[1].is_first_eligible);
    let cursor = initial.next_updated_since.unwrap().parse().unwrap();
    let earlier = entry(B, "earlier-new-key", "1001", "chronologie");
    assert_eq!(vorschlag_einreichen(pool, &earlier).await.unwrap(), VorschlagStatus::AlreadyKnown);
    let changed = liste_ergebnisse(pool, Some(cursor), 1).await.unwrap();
    assert_eq!(changed.rows.len(), 1);
    assert_eq!(changed.rows[0].suggested_by_discord_id, B);
    assert!(changed.rows[0].is_first_eligible);
    assert_eq!(changed.rows[0].submitted_at.as_deref(), Some("2026-10-01T10:00:00Z"));
    assert_eq!(changed.rows[0].privacy_epoch, Some(0));
    assert_eq!(changed.rows[0].suggestion_count, 2);
    assert!(changed.has_more);
    let cursor = changed.next_updated_since.unwrap().parse().unwrap();
    let displaced = liste_ergebnisse(pool, Some(cursor), 1).await.unwrap();
    assert_eq!(displaced.rows.len(), 1);
    assert_eq!(displaced.rows[0].suggested_by_discord_id, A);
    assert!(!displaced.rows[0].is_first_eligible);
    assert_eq!(displaced.rows[0].submitted_at, initial.rows[0].submitted_at);
    assert_eq!(displaced.rows[0].privacy_epoch, Some(0));
    assert_eq!(displaced.rows[0].suggestion_count, 2);
    assert!(!displaced.has_more);
    let cursor = displaced.next_updated_since.unwrap().parse().unwrap();
    assert!(liste_ergebnisse(pool, Some(cursor), 1).await.unwrap().rows.is_empty());
    assert_eq!(vorschlag_einreichen(pool, &earlier).await.unwrap(), VorschlagStatus::AlreadyKnown);
    assert!(liste_ergebnisse(pool, Some(cursor), 1).await.unwrap().rows.is_empty());
}

#[tokio::test]
async fn consentwechsel_erhaelt_fremde_berechtigung_hinter_cursor_ohne_ersatzautor() {
    use tb_scout::community::liste_ergebnisse;
    let db = super::tests::migrated_pool("tb_scout_consent_outcome_cursor").await;
    let pool = &db.pool;
    let mut later = entry(A, "created-first-later-origin", "1001", "chronologie");
    later.submitted_at = Some(at() + chrono::Duration::seconds(1));
    vorschlag_einreichen(pool, &later).await.unwrap();
    let earlier = entry(B, "created-second-earlier-origin", "1001", "chronologie");
    vorschlag_einreichen(pool, &earlier).await.unwrap();
    // Die ursprüngliche Zeit entscheidet unabhängig von Insert- oder Seitenfolge.
    let first = liste_ergebnisse(pool, None, 1).await.unwrap();
    assert_eq!(first.rows[0].suggested_by_discord_id, B);
    assert!(first.rows[0].is_first_eligible);
    let cursor = first.rows[0]
        .updated_at
        .parse::<chrono::DateTime<chrono::Utc>>()
        .unwrap();
    let second = liste_ergebnisse(pool, Some(cursor), 1).await.unwrap();
    assert_eq!(second.rows[0].suggested_by_discord_id, A);
    assert!(!second.rows[0].is_first_eligible);
    let cursor = second.rows[0]
        .updated_at
        .parse::<chrono::DateTime<chrono::Utc>>()
        .unwrap();
    apply_operation(
        pool,
        B,
        uuid::Uuid::new_v4(),
        1,
        Some(at() + chrono::Duration::minutes(1)),
    )
    .await
    .unwrap();
    let changed = liste_ergebnisse(pool, Some(cursor), 1).await.unwrap();
    assert_eq!(changed.rows.len(), 1);
    assert_eq!(changed.rows[0].suggested_by_discord_id, A);
    assert!(changed.rows[0].is_first_eligible);
    assert_eq!(changed.rows[0].submitted_at, second.rows[0].submitted_at);
    assert_eq!(changed.rows[0].privacy_epoch, Some(0));
    assert_ne!(changed.rows[0].updated_at, second.rows[0].updated_at);
}
