//! Abnahme fuer die serverseitigen Plan-Sperren (Spec M1).
//!
//! Fuer jeden Endpunkt, der neu an der Stufe haengt, steht hier ein Test mit
//! beiden Seiten: ohne Netzwerk Plus die gesperrte beziehungsweise verkuerzte
//! Antwort, mit Plus die volle. Die Stop-Regel lautet: was im Frontend gesperrt
//! aussieht, aber per direktem API-Aufruf voll antwortet, gilt als nicht
//! erledigt. Diese Tests rufen die Handler direkt auf, also genau so, wie ein
//! Umgeher es tun wuerde.
//!
//! Braucht `TB_TEST_DATABASE_URL`; ohne die Variable ueberspringen die Tests.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use std::str::FromStr;

use tb_dashboard_api::auth::level::DashboardAuthLevel;
use tb_dashboard_api::handlers::{
    category_comparison, chat_analytics, clip_command_settings, coaching, performance, social_media,
};

// ── Aufbau ──────────────────────────────────────────────────────────────────

/// Alle Tabellen, die die geprueften Handler und der Plan-Resolver anfassen.
const DDL: &[&str] = &[
    "CREATE TABLE streamer_plans (twitch_user_id TEXT, twitch_login TEXT, manual_plan_id TEXT, \
     manual_plan_expires_at TEXT, manual_plan_notes TEXT, manual_plan_updated_at TEXT, \
     clip_command_enabled INTEGER DEFAULT 1)",
    "CREATE TABLE twitch_billing_subscriptions (customer_reference TEXT, plan_id TEXT, \
     status TEXT, current_period_end TEXT, updated_at TEXT)",
    "CREATE TABLE twitch_stream_sessions (id BIGSERIAL PRIMARY KEY, streamer_login TEXT, \
     started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ, duration_seconds INTEGER, avg_viewers REAL, \
     peak_viewers INTEGER, start_viewers INTEGER, retention_10m DOUBLE PRECISION, retention_5m REAL, \
     unique_chatters INTEGER, follower_delta INTEGER, followers_start INTEGER, \
     followers_end INTEGER, stream_title TEXT, tags TEXT)",
    "CREATE TABLE twitch_session_viewers (session_id BIGINT, ts_utc TIMESTAMPTZ, \
     minutes_from_start INTEGER, viewer_count INTEGER)",
    "CREATE TABLE twitch_session_chatters (session_id BIGINT, streamer_login TEXT, \
     chatter_login TEXT, chatter_id TEXT, messages INTEGER DEFAULT 0, \
     seen_via_chatters_api BOOLEAN DEFAULT FALSE, is_first_time_streamer BOOLEAN DEFAULT FALSE)",
    "CREATE TABLE twitch_chat_messages (id BIGSERIAL PRIMARY KEY, session_id BIGINT, \
     streamer_login TEXT, chatter_login TEXT, chatter_id TEXT, content TEXT, \
     is_command BOOLEAN, message_ts TIMESTAMPTZ)",
    "CREATE TABLE twitch_chatter_rollup (streamer_login TEXT NOT NULL, chatter_login TEXT NOT NULL, \
     chatter_id TEXT, first_seen_at TIMESTAMPTZ NOT NULL, last_seen_at TIMESTAMPTZ NOT NULL, \
     total_messages INTEGER DEFAULT 0, total_sessions INTEGER DEFAULT 0, \
     PRIMARY KEY (streamer_login, chatter_login))",
    "CREATE TABLE twitch_raw_chat_ingest_health (streamer_login TEXT PRIMARY KEY, \
     last_raw_chat_message_at TEXT, last_raw_chat_insert_ok_at TEXT, \
     last_raw_chat_insert_error_at TEXT, last_raw_chat_error TEXT)",
    "CREATE TABLE twitch_stats_tracked (streamer TEXT, ts_utc TIMESTAMPTZ, viewer_count INTEGER)",
    "CREATE TABLE twitch_stats_category (id BIGSERIAL PRIMARY KEY, ts_utc TIMESTAMPTZ, \
     streamer TEXT, viewer_count INTEGER)",
    // Spaltensatz wie `CLIP_COLUMNS` in `social_media.rs`: der Approval-Pfad
    // laedt die volle Zeile, eine schmale Testtabelle liefert sonst 500 statt
    // der Plan-Antwort.
    "CREATE TABLE twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, clip_id TEXT, \
     clip_url TEXT, clip_title TEXT, clip_thumbnail_url TEXT, streamer_login TEXT, \
     twitch_user_id TEXT, created_at TIMESTAMPTZ DEFAULT NOW(), duration_seconds DOUBLE PRECISION, \
     view_count INTEGER, game_name TEXT, status TEXT DEFAULT 'pending', \
     source_kind TEXT DEFAULT 'twitch', upload_local_path TEXT, retention_until TIMESTAMPTZ, \
     discarded_at TIMESTAMPTZ, kontingent_verbraucht_at TIMESTAMPTZ, layout_override_json JSONB, \
     uploaded_tiktok BOOLEAN DEFAULT FALSE, uploaded_youtube BOOLEAN DEFAULT FALSE, \
     uploaded_instagram BOOLEAN DEFAULT FALSE)",
    "CREATE TABLE social_media_partner_access (streamer_login TEXT PRIMARY KEY, \
     granted BOOLEAN NOT NULL DEFAULT TRUE, updated_at TIMESTAMPTZ DEFAULT NOW(), \
     updated_by TEXT)",
    "CREATE TABLE twitch_raid_history (from_broadcaster_login TEXT, to_broadcaster_login TEXT, \
     viewer_count INTEGER, executed_at TIMESTAMPTZ, success BOOLEAN)",
    "CREATE TABLE twitch_clips_upload_queue (id SERIAL PRIMARY KEY, clip_id INTEGER, \
     platform TEXT, status TEXT DEFAULT 'pending', priority INTEGER DEFAULT 0, title TEXT, \
     description TEXT, hashtags TEXT, scheduled_at TIMESTAMPTZ, attempts INTEGER DEFAULT 0, \
     last_error TEXT, last_attempt_at TIMESTAMPTZ, \
     created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP, completed_at TIMESTAMPTZ)",
    "CREATE TABLE social_media_clip_approval (clip_db_id INTEGER PRIMARY KEY, \
     state TEXT NOT NULL DEFAULT 'awaiting_approval', \
     approved_platforms JSONB NOT NULL DEFAULT '[]'::jsonb, approver_user_id TEXT, \
     decided_at TIMESTAMPTZ, dm_message_id TEXT, dm_channel_id TEXT, last_sent_at TIMESTAMPTZ)",
    "CREATE TABLE social_media_clip_enrichment (clip_db_id INTEGER PRIMARY KEY, \
     transcript_raw TEXT, transcript_corrected TEXT, transcript_segments JSONB, \
     transcript_lang TEXT, detected_terms JSONB DEFAULT '[]'::jsonb, title_youtube TEXT, \
     title_tiktok TEXT, title_instagram TEXT, description_youtube TEXT, \
     description_tiktok TEXT, description_instagram TEXT, \
     hashtags_youtube JSONB DEFAULT '[]'::jsonb, hashtags_tiktok JSONB DEFAULT '[]'::jsonb, \
     hashtags_instagram JSONB DEFAULT '[]'::jsonb, llm_provider TEXT, llm_model TEXT, \
     cost_usd_estimate NUMERIC(10,6), status TEXT DEFAULT 'pending', error_message TEXT, \
     started_at TIMESTAMPTZ, completed_at TIMESTAMPTZ, edited_by TEXT, \
     updated_at TIMESTAMPTZ DEFAULT NOW())",
    "CREATE TABLE social_media_settings (key TEXT PRIMARY KEY, value JSONB, \
     updated_at TIMESTAMPTZ, updated_by TEXT)",
];

/// Frisches Schema mit allen Tabellen. `None` ohne `TB_TEST_DATABASE_URL`.
async fn pool(schema: &str) -> Option<PgPool> {
    let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
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
    let opts = PgConnectOptions::from_str(&dsn)
        .unwrap()
        .options([("search_path", schema)]);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(opts)
        .await
        .unwrap();
    for ddl in DDL {
        sqlx::query(ddl).execute(&pool).await.unwrap();
    }
    Some(pool)
}

/// Partner-Session ohne `twitch_user_id`: der Trial-Auto-Grant im Resolver
/// braucht beide Werte und springt so nicht an, die Stufe kommt allein aus dem
/// Manual-Override.
fn partner(login: &str) -> DashboardAuthLevel {
    DashboardAuthLevel::Partner {
        twitch_login: login.to_string(),
        twitch_user_id: String::new(),
        display_name: login.to_string(),
    }
}

/// Traegt eine Stufe als Manual-Override ein. Ohne Aufruf steht der Streamer
/// auf dem Default und ist damit Free.
async fn stufe_setzen(pool: &PgPool, login: &str, plan_id: &str) {
    sqlx::query("INSERT INTO streamer_plans (twitch_login, manual_plan_id) VALUES ($1, $2)")
        .bind(login)
        .bind(plan_id)
        .execute(pool)
        .await
        .unwrap();
}

/// Zwei Sessions: eine von gestern, eine von vor 200 Tagen. Free sieht nur die
/// erste, Plus beide.
async fn zwei_sessions(pool: &PgPool, login: &str) {
    for (tage, viewers) in [(1_i32, 10_f32), (200_i32, 20_f32)] {
        sqlx::query(
            "INSERT INTO twitch_stream_sessions \
             (streamer_login, started_at, ended_at, duration_seconds, avg_viewers, peak_viewers, \
              retention_10m, unique_chatters, follower_delta, followers_start, followers_end) \
             VALUES ($1, NOW() - ($2 || ' days')::interval, \
                     NOW() - ($2 || ' days')::interval + INTERVAL '3 hours', \
                     10800, $3, 30, 0.5, 5, 2, 100, 102)",
        )
        .bind(login)
        .bind(tage.to_string())
        .bind(viewers)
        .execute(pool)
        .await
        .unwrap();
    }
}

async fn body_json(resp: Response) -> Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

/// `(status, gekuerzt-Header, Body)` in einem Rutsch.
async fn zerlegen(resp: Response) -> (StatusCode, bool, Value) {
    let status = resp.status();
    let gekuerzt = resp.headers().get("x-plan-gekuerzt").is_some();
    (status, gekuerzt, body_json(resp).await)
}

// ── Harte Sperre: kein Free-Angebot vorhanden ───────────────────────────────

/// Coaching gibt es nur ab Plus, hier ist 403 richtig.
#[tokio::test]
async fn coaching_ohne_plus_403_mit_plus_200() {
    let Some(pool) = pool("t_gate_coaching").await else {
        return;
    };
    zwei_sessions(&pool, "freistreamer").await;
    zwei_sessions(&pool, "plusstreamer").await;
    stufe_setzen(&pool, "plusstreamer", "plus").await;

    let frei = coaching::coaching_handler(
        partner("freistreamer"),
        State(pool.clone()),
        Query(coaching::CoachingQuery {
            streamer: None,
            days: Some("30".into()),
        }),
    )
    .await
    .into_response();
    assert_eq!(frei.status(), StatusCode::FORBIDDEN);
    let body = body_json(frei).await;
    assert_eq!(body["error"], "plan_required");
    assert_eq!(body["required_stufe"], "plus");

    let bezahlt = coaching::coaching_handler(
        partner("plusstreamer"),
        State(pool),
        Query(coaching::CoachingQuery {
            streamer: None,
            days: Some("30".into()),
        }),
    )
    .await
    .into_response();
    assert_eq!(bezahlt.status(), StatusCode::OK);
}

/// KI-Analyse: die Modellwahl ist das Gate, `None` heisst 403 im Handler.
#[tokio::test]
async fn ki_analyse_erst_ab_plus() {
    let Some(pool) = pool("t_gate_ki").await else {
        return;
    };
    stufe_setzen(&pool, "plusstreamer", "plus").await;

    assert_eq!(
        tb_analytics::ai_analysis::plan_ai_model(&pool, "freistreamer")
            .await
            .unwrap(),
        None,
        "Free darf keine KI-Analyse bekommen"
    );
    assert!(
        tb_analytics::ai_analysis::plan_ai_model(&pool, "plusstreamer")
            .await
            .unwrap()
            .is_some(),
        "Plus muss KI-Analyse bekommen"
    );
}

// ── Pro-Grenze: automatisches Posten ────────────────────────────────────────

/// Automatisches Posten bleibt fuer alle offen, solange Creator Pro nicht
/// buchbar ist.
///
/// Die Stufe steht im Katalog auf `buchbar = false`, und
/// `checkout_start_handler` schickt jeden Pro-Kaufversuch auf `/twitch/pricing`
/// zurueck. Eine Pro-Sperre wuerde einem Partner also eine heute laufende
/// Funktion nehmen, ohne ihm einen Weg zurueck zu lassen. Deshalb haengt sie an
/// `stufe::sperre_greift(Stufe::Pro)` und ist inaktiv.
///
/// Geprueft werden alle drei Wege in die Upload-Warteschlange (Sammel-Upload,
/// direktes Einreihen, Approval "approve"), und zwar auf Wirkung: es kommt
/// wirklich etwas in der Warteschlange an.
#[tokio::test]
async fn auto_posting_offen_solange_pro_nicht_buchbar() {
    // Die Voraussetzung des Tests, sichtbar statt stillschweigend.
    assert!(
        !tb_analytics::stufe::sperre_greift(tb_analytics::stufe::Stufe::Pro),
        "Creator Pro ist buchbar geworden: dieser Test und die Sperre gehoeren umgestellt"
    );

    let Some(pool) = pool("t_gate_autopost").await else {
        return;
    };
    for login in ["freistreamer", "plusstreamer", "prostreamer"] {
        sqlx::query(
            "INSERT INTO social_media_partner_access (streamer_login, granted) VALUES ($1, TRUE)",
        )
        .bind(login)
        .execute(&pool)
        .await
        .unwrap();
    }
    stufe_setzen(&pool, "plusstreamer", "plus").await;
    stufe_setzen(&pool, "prostreamer", "pro").await;
    sqlx::query(
        "INSERT INTO twitch_clips_social_media (id, clip_id, clip_url, streamer_login) \
         VALUES (1, 'c1', 'https://clips.twitch.tv/c1', 'freistreamer'), \
                (2, 'c2', 'https://clips.twitch.tv/c2', 'plusstreamer'), \
                (3, 'c3', 'https://clips.twitch.tv/c3', 'prostreamer')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Weg 1: Sammel-Einreihung.
    for login in ["freistreamer", "plusstreamer", "prostreamer"] {
        let resp = social_media::batch_upload_handler(
            partner(login),
            State(pool.clone()),
            Query(social_media::StreamerQuery { streamer: None }),
            Json(serde_json::from_value(json!({ "platforms": ["tiktok"] })).unwrap()),
        )
        .await;
        assert_ne!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "{login} darf sammelweise posten, solange Pro nicht kaufbar ist"
        );
    }

    // Weg 2: direktes Einreihen, auch ohne `schedule` und mit `platforms: all`.
    for (login, clip_id, body) in [
        (
            "freistreamer",
            1,
            json!({ "clip_id": 1, "platforms": ["tiktok"], "schedule": "auto" }),
        ),
        (
            "plusstreamer",
            2,
            json!({ "clip_id": 2, "platforms": ["tiktok"] }),
        ),
        (
            "prostreamer",
            3,
            json!({ "clip_id": 3, "platforms": "all" }),
        ),
    ] {
        let resp = social_media::queue_upload_handler(
            partner(login),
            State(pool.clone()),
            Query(social_media::StreamerQuery { streamer: None }),
            Json(serde_json::from_value(body.clone()).unwrap()),
        )
        .await;
        assert_ne!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "{login} darf einreihen: {body}"
        );
        let eingereiht: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clips_upload_queue WHERE clip_id = $1")
                .bind(clip_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(
            eingereiht > 0,
            "{login}: Status ohne Wirkung, nichts in der Warteschlange"
        );
    }

    // Weg 3: Approval "approve" reiht ueber `handle_decision` selbst ein.
    let approve = social_media::approval_decision_handler(
        partner("freistreamer"),
        State(pool.clone()),
        axum::extract::Path("1".to_string()),
        json!({ "decision": "approve", "platforms": ["tiktok"] }).to_string(),
    )
    .await;
    assert_ne!(
        approve.status(),
        StatusCode::FORBIDDEN,
        "Freigabe ist keine gesperrte Funktion, solange Pro nicht kaufbar ist"
    );

    // Gegenprobe zur Mechanik: waere die Sperre scharf, bliebe nur Pro uebrig.
    assert!(!tb_analytics::stufe::Stufe::Free.hat_pro());
    assert!(!tb_analytics::stufe::Stufe::Plus.hat_pro());
    assert!(tb_analytics::stufe::Stufe::Pro.hat_pro());
}

// ── Clip-Kontingent ─────────────────────────────────────────────────────────

/// Das Monatskontingent zaehlt, sperrt aber niemanden.
///
/// Der Ausweg aus der Grenze ist Creator Pro (unbegrenzt), und die Stufe ist
/// nicht buchbar. In keiner Feature-Liste steht ein Clip-Kontingent, also darf
/// es auch niemanden aus einer heute unbegrenzten Funktion aussperren. Die
/// Zaehlung ueber `kontingent_verbraucht_at` laeuft weiter, sie ist der Unterbau
/// fuer spaeter.
#[tokio::test]
async fn clip_kontingent_zaehlt_ohne_zu_sperren() {
    let Some(pool) = pool("t_gate_clipfetch").await else {
        return;
    };
    sqlx::query("INSERT INTO social_media_partner_access (streamer_login, granted) VALUES ('freistreamer', TRUE)")
        .execute(&pool)
        .await
        .unwrap();
    // Weit ueber der Free-Grenze von 3, alle selbst geholt.
    for i in 0..12 {
        sqlx::query(
            "INSERT INTO twitch_clips_social_media \
                 (clip_id, clip_url, streamer_login, kontingent_verbraucht_at) \
             VALUES ($1, 'https://clips.twitch.tv/x', 'freistreamer', NOW())",
        )
        .bind(format!("selbst-{i}"))
        .execute(&pool)
        .await
        .unwrap();
    }

    let resp = social_media::fetch_clips_handler(
        partner("freistreamer"),
        State(pool.clone()),
        Json(serde_json::from_value(json!({ "limit": 20 })).unwrap()),
    )
    .await;
    assert_ne!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "kein Kontingent im Angebot, also keine Sperre"
    );

    // Gezaehlt wird trotzdem, und zwar richtig.
    let stand = clip_command_settings::get_handler(
        partner("freistreamer"),
        State(pool),
        Query(clip_command_settings::ClipCommandQuery { streamer: None }),
    )
    .await;
    let body = body_json(stand).await;
    assert_eq!(body["kontingent"]["genutzt"], 12, "Zaehlung muss laufen");
    assert_eq!(body["kontingent"]["erzwungen"], false);
    assert!(
        body["kontingent"]["limit"].is_null(),
        "keine Schranke behaupten, die es nicht gibt"
    );
    assert!(body["kontingent"]["rest"].is_null());
    assert!(
        body["kontingent"].get("wasserzeichen").is_none(),
        "Wasserzeichen wird nirgends angewendet und darf nicht gemeldet werden"
    );
}

/// Gezaehlt wird die Aufnahme in unsere DB, nicht der Twitch-Zeitstempel.
///
/// Beide Fehlrichtungen der alten `created_at`-Zaehlung sind hier festgenagelt:
/// Zuschauer-Clips aus diesem Monat, die der Hintergrund-Fetcher geholt hat,
/// duerfen nichts verbrauchen, und selbst geholte Clips zaehlen auch dann, wenn
/// Twitch sie auf einen alten Tag datiert hat.
#[tokio::test]
async fn kontingent_zaehlt_aufnahme_nicht_twitch_datum() {
    let Some(pool) = pool("t_gate_clipzaehlung").await else {
        return;
    };
    sqlx::query("INSERT INTO social_media_partner_access (streamer_login, granted) VALUES ('freistreamer', TRUE)")
        .execute(&pool)
        .await
        .unwrap();

    let stand = |pool: PgPool| async move {
        let resp = clip_command_settings::get_handler(
            partner("freistreamer"),
            State(pool),
            Query(clip_command_settings::ClipCommandQuery { streamer: None }),
        )
        .await;
        body_json(resp).await["kontingent"]["genutzt"].clone()
    };

    // Zehn Clips, die Zuschauer in diesem Monat auf Twitch erstellt haben und
    // die der Hintergrund-Fetcher eingelesen hat: kein Verbrauch.
    for i in 0..10 {
        sqlx::query(
            "INSERT INTO twitch_clips_social_media \
                 (clip_id, clip_url, streamer_login, created_at) \
             VALUES ($1, 'https://clips.twitch.tv/x', 'freistreamer', NOW())",
        )
        .bind(format!("zuschauer-{i}"))
        .execute(&pool)
        .await
        .unwrap();
    }
    assert_eq!(
        stand(pool.clone()).await,
        0,
        "Zuschauer-Clips duerfen das Kontingent nicht aufbrauchen"
    );

    // Drei selbst geholte Clips, von Twitch auf den Vormonat datiert: zaehlen.
    for i in 0..3 {
        sqlx::query(
            "INSERT INTO twitch_clips_social_media \
                 (clip_id, clip_url, streamer_login, created_at, kontingent_verbraucht_at) \
             VALUES ($1, 'https://clips.twitch.tv/x', 'freistreamer', \
                     NOW() - INTERVAL '40 days', NOW())",
        )
        .bind(format!("selbst-{i}"))
        .execute(&pool)
        .await
        .unwrap();
    }
    assert_eq!(stand(pool.clone()).await, 3);

    // Verworfene Clips zaehlen nicht mehr mit.
    sqlx::query(
        "UPDATE twitch_clips_social_media SET discarded_at = NOW() WHERE clip_id = 'selbst-0'",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        stand(pool).await,
        2,
        "ein Fehlgriff darf nicht teuer bleiben"
    );
}

/// Der `!clip`-Schalter bleibt ohne 403 (Chat-Befehle sind Free) und traegt den
/// Zaehlerstand mit, aber keine Grenze, die es noch nicht gibt.
#[tokio::test]
async fn clip_command_settings_zeigt_den_zaehlerstand() {
    let Some(pool) = pool("t_gate_clipcmd").await else {
        return;
    };
    stufe_setzen(&pool, "plusstreamer", "plus").await;

    for (login, stufe) in [("freistreamer", "free"), ("plusstreamer", "plus")] {
        let resp = clip_command_settings::get_handler(
            partner(login),
            State(pool.clone()),
            Query(clip_command_settings::ClipCommandQuery { streamer: None }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        assert_eq!(body["clip_command_enabled"], true);
        assert_eq!(body["kontingent"]["plan_stufe"], stufe);
        assert_eq!(body["kontingent"]["genutzt"], 0);
        assert!(body["kontingent"]["limit"].is_null(), "{login}");
        assert_eq!(body["kontingent"]["erzwungen"], false, "{login}");
    }
}

// ── Weiche Sperre: verkuerztes Fenster statt 403 ────────────────────────────

/// Monatsstatistik: Free bekommt kein 403, sondern nur den letzten Stream.
#[tokio::test]
async fn monthly_stats_ohne_plus_nur_letzter_stream() {
    let Some(pool) = pool("t_gate_monthly").await else {
        return;
    };
    zwei_sessions(&pool, "freistreamer").await;
    zwei_sessions(&pool, "plusstreamer").await;
    stufe_setzen(&pool, "plusstreamer", "plus").await;

    let frei = performance::monthly_stats_handler(
        partner("freistreamer"),
        State(pool.clone()),
        Query(performance::MonthlyQuery {
            streamer: None,
            months: Some("12".into()),
        }),
    )
    .await
    .into_response();
    let (status, gekuerzt, body) = zerlegen(frei).await;
    assert_eq!(status, StatusCode::OK, "Free darf kein 403 sehen");
    assert!(gekuerzt, "Free-Antwort muss als gekuerzt markiert sein");
    assert_eq!(
        body.as_array().unwrap().len(),
        1,
        "Free sieht genau den letzten Stream-Monat"
    );

    let bezahlt = performance::monthly_stats_handler(
        partner("plusstreamer"),
        State(pool),
        Query(performance::MonthlyQuery {
            streamer: None,
            months: Some("12".into()),
        }),
    )
    .await
    .into_response();
    let (status, gekuerzt, body) = zerlegen(bezahlt).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!gekuerzt, "Plus-Antwort darf nicht gekuerzt sein");
    assert_eq!(
        body.as_array().unwrap().len(),
        2,
        "Plus sieht beide Stream-Monate"
    );
}

/// Wochentagsanalyse, stuendliches und Kalender-Heatmap: dieselbe Regel.
#[tokio::test]
async fn weekly_hourly_calendar_ohne_plus_verkuerzt() {
    let Some(pool) = pool("t_gate_perf").await else {
        return;
    };
    zwei_sessions(&pool, "freistreamer").await;
    zwei_sessions(&pool, "plusstreamer").await;
    stufe_setzen(&pool, "plusstreamer", "plus").await;
    // Tracked-Punkte fuer das stuendliche Heatmap.
    for tage in [1_i32, 200_i32] {
        sqlx::query(
            "INSERT INTO twitch_stats_tracked (streamer, ts_utc, viewer_count) \
             VALUES ('freistreamer', NOW() - ($1 || ' days')::interval, 10), \
                    ('plusstreamer', NOW() - ($1 || ' days')::interval, 10)",
        )
        .bind(tage.to_string())
        .execute(&pool)
        .await
        .unwrap();
    }

    for name in ["weekly", "hourly", "calendar"] {
        for (login, erwartet_gekuerzt, erwartete_zeilen) in [
            ("freistreamer", true, 1_usize),
            ("plusstreamer", false, 2_usize),
        ] {
            let query = performance::DaysQuery {
                streamer: None,
                days: Some("365".into()),
            };
            let resp = match name {
                "weekly" => performance::weekly_stats_handler(
                    partner(login),
                    State(pool.clone()),
                    Query(query),
                )
                .await
                .into_response(),
                "hourly" => performance::hourly_heatmap_handler(
                    partner(login),
                    State(pool.clone()),
                    Query(query),
                )
                .await
                .into_response(),
                _ => performance::calendar_heatmap_handler(
                    partner(login),
                    State(pool.clone()),
                    Query(query),
                )
                .await
                .into_response(),
            };
            let (status, gekuerzt, body) = zerlegen(resp).await;
            assert_eq!(status, StatusCode::OK, "{name}/{login} darf kein 403 sein");
            assert_eq!(
                gekuerzt, erwartet_gekuerzt,
                "{name}/{login} Kuerzungs-Header"
            );
            // Zwei Sessions an zwei verschiedenen Tagen: Free sieht eine Zeile,
            // Plus zwei. Beim Wochentags-Heatmap gilt das genauso, weil gestern
            // und vor 200 Tagen auf verschiedene Wochentage fallen.
            assert_eq!(
                body.as_array().unwrap().len(),
                erwartete_zeilen,
                "{name}/{login} Zeilenzahl"
            );
        }
    }
}

/// Chat-Analyse: Free bekommt dieselbe Struktur, nur ueber das kurze Fenster,
/// und die Antwort sagt es im Body.
#[tokio::test]
async fn chat_analytics_ohne_plus_verkuerzt_statt_403() {
    let Some(pool) = pool("t_gate_chat").await else {
        return;
    };
    zwei_sessions(&pool, "freistreamer").await;
    zwei_sessions(&pool, "plusstreamer").await;
    stufe_setzen(&pool, "plusstreamer", "plus").await;

    let frei = chat_analytics::chat_analytics_handler(
        partner("freistreamer"),
        State(pool.clone()),
        Query(chat_analytics::ChatAnalyticsQuery {
            streamer: None,
            days: Some("365".into()),
            timezone: None,
        }),
    )
    .await
    .into_response();
    let (status, gekuerzt, body) = zerlegen(frei).await;
    assert_eq!(status, StatusCode::OK, "Free darf kein 403 sehen");
    assert!(gekuerzt);
    assert_eq!(body["plan_limit"]["gekuerzt"], true);
    assert_eq!(body["plan_limit"]["benoetigte_stufe"], "plus");

    let bezahlt = chat_analytics::chat_analytics_handler(
        partner("plusstreamer"),
        State(pool),
        Query(chat_analytics::ChatAnalyticsQuery {
            streamer: None,
            days: Some("365".into()),
            timezone: None,
        }),
    )
    .await
    .into_response();
    let (status, gekuerzt, body) = zerlegen(bezahlt).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!gekuerzt);
    assert!(
        body.get("plan_limit").is_none(),
        "Plus sieht keinen Hinweis"
    );
}

/// Kategorie-Vergleich: ebenfalls kein 403, nur ein kuerzerer Zeitraum.
#[tokio::test]
async fn category_comparison_ohne_plus_verkuerzt_statt_403() {
    let Some(pool) = pool("t_gate_catcmp").await else {
        return;
    };
    zwei_sessions(&pool, "freistreamer").await;
    zwei_sessions(&pool, "plusstreamer").await;
    stufe_setzen(&pool, "plusstreamer", "plus").await;
    for tage in [1_i32, 200_i32] {
        sqlx::query(
            "INSERT INTO twitch_stats_category (streamer, ts_utc, viewer_count) \
             VALUES ('freistreamer', NOW() - ($1 || ' days')::interval, 12), \
                    ('plusstreamer', NOW() - ($1 || ' days')::interval, 12)",
        )
        .bind(tage.to_string())
        .execute(&pool)
        .await
        .unwrap();
    }

    let frei = category_comparison::category_comparison_handler(
        partner("freistreamer"),
        State(pool.clone()),
        Query(category_comparison::ComparisonQuery {
            streamer: None,
            days: Some("365".into()),
            exclude_external: None,
        }),
    )
    .await
    .into_response();
    let (status, gekuerzt, body) = zerlegen(frei).await;
    assert_eq!(status, StatusCode::OK, "Free darf kein 403 sehen");
    assert!(gekuerzt);
    assert_eq!(body["plan_limit"]["gekuerzt"], true);
    assert!(
        body["yourStats"].is_object(),
        "Free sieht dieselbe Struktur"
    );
    // Die eigenen Werte kommen aus dem geklemmten Fenster, die Verteilung aus
    // dem angefragten Zeitraum. Ein Perzentil aus zwei Fenstern waere eine
    // erfundene Zahl, also kommt keine.
    for feld in ["avgViewers", "peakViewers", "retention10m", "chatHealth"] {
        assert!(
            body["percentiles"][feld].is_null(),
            "{feld}: Perzentil aus zwei Zeitfenstern darf nicht ausgeliefert werden"
        );
    }
    assert!(
        body["categoryRank"].is_null(),
        "Rang haengt am Perzentil und faellt mit ihm weg"
    );
    // Was die Kategorie ueber sich selbst sagt, bleibt: das behauptet nichts
    // ueber den Streamer.
    assert!(body["categoryAvg"].is_object());
    assert!(body["categoryTotal"].is_number());

    let bezahlt = category_comparison::category_comparison_handler(
        partner("plusstreamer"),
        State(pool),
        Query(category_comparison::ComparisonQuery {
            streamer: None,
            days: Some("365".into()),
            exclude_external: None,
        }),
    )
    .await
    .into_response();
    let (status, gekuerzt, body) = zerlegen(bezahlt).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!gekuerzt);
    assert!(body.get("plan_limit").is_none());
    // Gegenprobe: bei deckungsgleichem Fenster gibt es die Zahlen sehr wohl.
    assert!(
        body["percentiles"]["avgViewers"].is_number(),
        "Plus vergleicht beide Seiten im selben Fenster und bekommt sein Perzentil"
    );
    assert!(body["categoryRank"].is_number());
}

/// Nebeneingaenge, die der Endpunkt-Durchgang zutage gefoerdert hat: die
/// Session-Detailansicht klemmte Free auf den letzten Stream, die
/// Ereignisliste derselben Session nicht. Und die Zuschauer-Ueberschneidung
/// stand offen, obwohl ihre beiden Nachbarn gesperrt sind.
#[tokio::test]
async fn session_events_und_viewer_overlap_sind_zu() {
    let Some(pool) = pool("t_gate_nebeneingang").await else {
        return;
    };
    zwei_sessions(&pool, "freistreamer").await;
    zwei_sessions(&pool, "plusstreamer").await;
    stufe_setzen(&pool, "plusstreamer", "plus").await;

    // Die aeltere der beiden Sessions ist fuer Free tabu.
    let alte_id: i64 = sqlx::query_scalar(
        "SELECT id FROM twitch_stream_sessions WHERE streamer_login = 'freistreamer'          ORDER BY started_at ASC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let neue_id: i64 = sqlx::query_scalar(
        "SELECT id FROM twitch_stream_sessions WHERE streamer_login = 'freistreamer'          ORDER BY started_at DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let alt = tb_dashboard_api::handlers::session_detail::session_events_handler(
        partner("freistreamer"),
        State(pool.clone()),
        axum::extract::Path(alte_id.to_string()),
    )
    .await
    .into_response();
    assert_eq!(
        alt.status(),
        StatusCode::FORBIDDEN,
        "Free darf die Ereignisse alter Sessions nicht sehen"
    );

    // Der letzte Stream bleibt fuer Free offen (kein 403).
    let neu = tb_dashboard_api::handlers::session_detail::session_events_handler(
        partner("freistreamer"),
        State(pool.clone()),
        axum::extract::Path(neue_id.to_string()),
    )
    .await
    .into_response();
    assert_ne!(
        neu.status(),
        StatusCode::FORBIDDEN,
        "der letzte Stream gehoert zu Free"
    );

    // Zuschauer-Ueberschneidung: Free 403, Plus nicht.
    let frei = tb_dashboard_api::handlers::audience::viewer_overlap_handler(
        partner("freistreamer"),
        State(pool.clone()),
        Query(serde_json::from_value(json!({ "limit": 5 })).unwrap()),
    )
    .await
    .into_response();
    assert_eq!(frei.status(), StatusCode::FORBIDDEN);
    assert_eq!(body_json(frei).await["required_stufe"], "plus");

    let bezahlt = tb_dashboard_api::handlers::audience::viewer_overlap_handler(
        partner("plusstreamer"),
        State(pool),
        Query(serde_json::from_value(json!({ "limit": 5 })).unwrap()),
    )
    .await
    .into_response();
    assert_ne!(bezahlt.status(), StatusCode::FORBIDDEN);
}

/// Gegenprobe zur Stop-Regel: eine Session ohne Auth kommt an keinen der
/// gesperrten Endpunkte, auch nicht per direktem Aufruf.
#[tokio::test]
async fn ohne_auth_kein_zugang() {
    let Some(pool) = pool("t_gate_noauth").await else {
        return;
    };
    let resp = coaching::coaching_handler(
        DashboardAuthLevel::None,
        State(pool.clone()),
        Query(coaching::CoachingQuery {
            streamer: Some("freistreamer".into()),
            days: Some("30".into()),
        }),
    )
    .await
    .into_response();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let resp = chat_analytics::chat_analytics_handler(
        DashboardAuthLevel::None,
        State(pool),
        Query(chat_analytics::ChatAnalyticsQuery {
            streamer: Some("freistreamer".into()),
            days: Some("30".into()),
            timezone: None,
        }),
    )
    .await
    .into_response();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
