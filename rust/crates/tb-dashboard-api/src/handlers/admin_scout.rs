//! Admin-Endpoints der Scout-Freigaben (`twitch_scout_candidates`, tb-scout).
//!
//! Vertrag:
//! - `GET  /twitch/api/admin/scout/candidates` → offene Kandidaten
//!   (`vorgeschlagen` + `pausiert`) plus die persönliche Besuchsliste
//!   (`persoenlich`, nach Potenzial sortiert); der GET führt vorher einen
//!   Erkennungs-Lauf aus, damit die Liste den aktuellen Bestand zeigt.
//! - `POST /twitch/api/admin/scout/candidates/:login/decision` → Entscheidung
//!   `approve` | `uebersprungen` | `pausiert` | `persoenlich` |
//!   `bekannter_kontakt` mit optionalem Grund; gespeichert wird der
//!   kanonische Status, der Entscheider kommt aus der Admin-Session.
//!
//! Schutzschichten wie die übrigen Admin-Router: die Routen hängen in
//! `build_admin_streamers_router` (`lib.rs`) und tragen damit
//! `require_admin_before_csrf` plus `csrf_protect`; der `require_admin`-Aufruf
//! im Handler ist die zweite, router-unabhängige Sperre. Fachlogik liegt in
//! [`tb_scout`], hier steht nur die Dashboard-Hülle.

use std::collections::BTreeMap;

use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;

use crate::auth::level::DashboardAuthLevel;
use crate::auth::session::DashboardAuthState;
use crate::handlers::admin_actor;
use tb_scout::store::{self, KandidatZeile};
use tb_scout::{normalisiere_login, normalize_entscheidung};

#[derive(Serialize)]
struct ScoutKandidat {
    login: String,
    sessions_count: i32,
    avg_viewers: f32,
    first_seen: Option<DateTime<Utc>>,
    last_seen: Option<DateTime<Utc>>,
    language: Option<String>,
    deadlock_share: f32,
    status: String,
    entscheid_grund: Option<String>,
    approver: Option<String>,
    decided_at: Option<DateTime<Utc>>,
    visited_at: Option<DateTime<Utc>>,
    invite_url: Option<String>,
}

#[derive(Serialize)]
struct ScoutCandidatesResponse {
    items: Vec<ScoutKandidat>,
    persoenlich: Vec<ScoutKandidat>,
}

#[derive(Deserialize)]
pub struct ScoutDecisionRequest {
    pub decision: String,
    #[serde(default, alias = "reason")]
    pub grund: Option<String>,
}

fn db_error(error: sqlx::Error) -> Response {
    tracing::error!(%error, "Admin-Scout-Abfrage fehlgeschlagen");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": "internal_error"})),
    )
        .into_response()
}

/// Twitch-Login-Form wie im Research-Handler (keine Sonderzeichen, ≤ 25).
fn gueltiger_login(login: &str) -> bool {
    (1..=25).contains(&login.len())
        && login
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// Persönliche Invite-URLs je Login aus `twitch_streamer_invites` — dieselbe
/// Quelle wie die Bestands-API `tb-internal-api discord_invite`; die interne
/// API ist token-geschützt, deshalb liefert der Admin-GET die URL direkt mit.
async fn lade_invites(pool: &PgPool, logins: &[String]) -> BTreeMap<String, String> {
    if logins.is_empty() {
        return BTreeMap::new();
    }
    let zeilen: Vec<(String, String)> = sqlx::query_as(
        "SELECT LOWER(streamer_login), MIN(invite_url) \
           FROM twitch_streamer_invites \
          WHERE LOWER(streamer_login) = ANY($1) \
          GROUP BY LOWER(streamer_login)",
    )
    .bind(logins)
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    zeilen.into_iter().collect()
}

fn zu_kandidat(zeile: &KandidatZeile, invites: &BTreeMap<String, String>) -> ScoutKandidat {
    ScoutKandidat {
        login: zeile.login.clone(),
        sessions_count: zeile.sessions_count,
        avg_viewers: zeile.avg_viewers,
        first_seen: zeile.first_seen,
        last_seen: zeile.last_seen,
        language: zeile.language.clone(),
        deadlock_share: zeile.deadlock_share,
        status: zeile.status.clone(),
        entscheid_grund: zeile.entscheid_grund.clone(),
        approver: zeile.approver.clone(),
        decided_at: zeile.decided_at,
        visited_at: zeile.visited_at,
        invite_url: invites.get(&zeile.login).cloned(),
    }
}

/// `GET /twitch/api/admin/scout/candidates`
pub async fn candidates_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>) -> Response {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return err.into_response();
    }

    // Erkennungs-Lauf vor der Liste: neue Kandidaten werden vorgemerkt,
    // Entscheidungen bleiben unangetastet (Upsert-Schutz im Store). Der Lauf
    // protokolliert Fehler selbst und kann die Liste nicht scheitern lassen.
    let _ = tb_scout::detector::laufe_scout_scan(&pool).await;

    let (offen, persoenlich) =
        match tokio::try_join!(store::liste_offen(&pool), store::liste_persoenlich(&pool)) {
            Ok(ergebnis) => ergebnis,
            Err(error) => return db_error(error),
        };

    let mut logins: Vec<String> = offen
        .iter()
        .chain(persoenlich.iter())
        .map(|z| z.login.clone())
        .collect();
    logins.sort_unstable();
    logins.dedup();
    let invites = lade_invites(&pool, &logins).await;

    Json(ScoutCandidatesResponse {
        items: offen.iter().map(|z| zu_kandidat(z, &invites)).collect(),
        persoenlich: persoenlich
            .iter()
            .map(|z| zu_kandidat(z, &invites))
            .collect(),
    })
    .into_response()
}

/// `POST /twitch/api/admin/scout/candidates/:login/decision`
pub async fn decision_handler(
    auth: DashboardAuthLevel,
    config: Option<Extension<DashboardAuthState>>,
    headers: HeaderMap,
    State(pool): State<PgPool>,
    Path(raw_login): Path<String>,
    Json(body): Json<ScoutDecisionRequest>,
) -> Response {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return err.into_response();
    }

    let Some(login) = normalisiere_login(&raw_login).filter(|l| gueltiger_login(l)) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid login"})),
        )
            .into_response();
    };
    let Some(status) = normalize_entscheidung(&body.decision) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "ungueltige Entscheidung"})),
        )
            .into_response();
    };

    let actor = admin_actor::admin_actor_label(config.as_ref(), &headers).await;
    match store::setze_entscheidung(&pool, &login, &body.decision, body.grund.as_deref(), &actor)
        .await
    {
        Ok(true) => Json(json!({"ok": true, "login": login, "status": status})).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "unbekannter Kandidat"})),
        )
            .into_response(),
        Err(error) => db_error(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use serde_json::Value;
    use sqlx::postgres::PgPoolOptions;

    async fn pool_or_skip(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .expect("connect test database");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&pool)
            .await
            .expect("drop schema");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .expect("create schema");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("set search path");
        for ddl in [
            "CREATE TABLE twitch_scout_candidates (streamer_login TEXT PRIMARY KEY, \
             twitch_user_id TEXT, sessions_count INTEGER NOT NULL DEFAULT 0, \
             avg_viewers REAL NOT NULL DEFAULT 0, first_seen TIMESTAMPTZ, last_seen TIMESTAMPTZ, \
             language TEXT, deadlock_share REAL NOT NULL DEFAULT 0, \
             status TEXT NOT NULL DEFAULT 'vorgeschlagen', entscheid_grund TEXT, approver TEXT, \
             decided_at TIMESTAMPTZ, dispatched_at TIMESTAMPTZ, visited_at TIMESTAMPTZ)",
            "CREATE TABLE twitch_streamer_invites (streamer_login TEXT, guild_id BIGINT, \
             channel_id BIGINT, invite_code TEXT, invite_url TEXT, created_at TIMESTAMPTZ)",
            // Tabellen für den Erkennungs-Lauf im GET (Detector-Filter).
            "CREATE TABLE twitch_stats_category (ts_utc TIMESTAMPTZ, streamer TEXT, \
             viewer_count INTEGER, is_partner BOOLEAN DEFAULT FALSE, game_name TEXT, \
             stream_title TEXT, tags TEXT, language TEXT)",
            "CREATE TABLE twitch_stream_sessions (id SERIAL PRIMARY KEY, streamer_login TEXT, \
             started_at TIMESTAMPTZ, twitch_user_id TEXT)",
            "CREATE TABLE twitch_partners (twitch_login TEXT, status TEXT)",
            "CREATE TABLE twitch_raid_blacklist (target_id TEXT, target_login TEXT)",
            "CREATE TABLE twitch_partner_signup_denylist (twitch_user_id TEXT PRIMARY KEY, \
             twitch_login TEXT NOT NULL)",
            "CREATE TABLE twitch_scout_pitch_blacklist (streamer_login TEXT PRIMARY KEY)",
            "CREATE TABLE twitch_outbound_chat_suppressions (target_login TEXT NOT NULL, \
             source TEXT NOT NULL, suppressed_until TIMESTAMPTZ NOT NULL)",
            "CREATE TABLE twitch_partner_outreach (streamer_login TEXT PRIMARY KEY, \
             cooldown_until TIMESTAMPTZ)",
            "CREATE TABLE twitch_chatter_global_ban (chatter_login TEXT, chatter_id TEXT)",
        ] {
            sqlx::query(ddl)
                .execute(&pool)
                .await
                .expect("create test table");
        }
        Some(pool)
    }

    async fn get_candidates(auth: DashboardAuthLevel, pool: PgPool) -> (StatusCode, Value) {
        let response = candidates_handler(auth, State(pool)).await;
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 256 * 1024)
            .await
            .expect("body");
        (status, serde_json::from_slice(&body).expect("json"))
    }

    async fn post_decision(
        auth: DashboardAuthLevel,
        pool: PgPool,
        login: &str,
        decision: &str,
        grund: Option<&str>,
    ) -> (StatusCode, Value) {
        let response = decision_handler(
            auth,
            None,
            HeaderMap::new(),
            State(pool),
            Path(login.to_string()),
            Json(ScoutDecisionRequest {
                decision: decision.into(),
                grund: grund.map(str::to_owned),
            }),
        )
        .await;
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("body");
        (status, serde_json::from_slice(&body).expect("json"))
    }

    /// Kleiner Kanal: 3 Ticks in 20 Minuten → genau eine Session, Ø 5.
    async fn seed_kleinkanal(pool: &PgPool, login: &str) {
        sqlx::query(
            "INSERT INTO twitch_stats_category (streamer, ts_utc, viewer_count, language, game_name) \
             SELECT $1, NOW() - (s * 10 || ' minutes')::interval, 5, 'de', 'Deadlock' \
             FROM generate_series(0, 2) AS s",
        )
        .bind(login)
        .execute(pool)
        .await
        .expect("seed kleinkanal");
    }

    #[tokio::test]
    async fn get_verlangt_admin() {
        let Some(pool) = pool_or_skip("admin_scout_unauth").await else {
            return;
        };
        let (status, body) = get_candidates(DashboardAuthLevel::None, pool).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            body,
            serde_json::json!({"error": "auth_required", "required": "admin"})
        );
    }

    #[tokio::test]
    async fn get_zeigt_erkannte_kandidaten_mit_invite() {
        let Some(pool) = pool_or_skip("admin_scout_get").await else {
            return;
        };
        seed_kleinkanal(&pool, "kleinanfang").await;
        sqlx::query(
            "INSERT INTO twitch_streamer_invites (streamer_login, invite_url) \
             VALUES ('KleinAnfang', 'https://discord.gg/einladung')",
        )
        .execute(&pool)
        .await
        .expect("seed invite");

        let (status, body) = get_candidates(DashboardAuthLevel::admin(), pool).await;

        assert_eq!(status, StatusCode::OK);
        let items = body["items"].as_array().expect("items");
        assert_eq!(items.len(), 1, "Scan hat den Kleinkanal vorgemerkt");
        assert_eq!(items[0]["login"], "kleinanfang");
        assert_eq!(items[0]["status"], "vorgeschlagen");
        assert_eq!(items[0]["sessions_count"], 1);
        assert_eq!(items[0]["avg_viewers"], 5.0);
        assert_eq!(items[0]["language"], "de");
        assert_eq!(items[0]["invite_url"], "https://discord.gg/einladung");
        assert_eq!(
            body["persoenlich"].as_array().expect("persoenlich").len(),
            0
        );
    }

    #[tokio::test]
    async fn get_sortiert_besuchsliste_nach_potenzial() {
        let Some(pool) = pool_or_skip("admin_scout_besuchsliste").await else {
            return;
        };
        for (login, sessions, avg) in [("mager", 1, 2.0), ("fleissig", 4, 8.0), ("mittel", 2, 9.0)]
        {
            sqlx::query(
                "INSERT INTO twitch_scout_candidates \
                     (streamer_login, sessions_count, avg_viewers, status) \
                 VALUES ($1, $2, $3, 'persoenlich')",
            )
            .bind(login)
            .bind(sessions)
            .bind(avg)
            .execute(&pool)
            .await
            .expect("seed persoenlich");
        }

        let (status, body) = get_candidates(DashboardAuthLevel::admin(), pool).await;

        assert_eq!(status, StatusCode::OK);
        let logins: Vec<&str> = body["persoenlich"]
            .as_array()
            .expect("persoenlich")
            .iter()
            .map(|item| item["login"].as_str().expect("login"))
            .collect();
        assert_eq!(logins, vec!["fleissig", "mittel", "mager"]);
        assert!(body["items"].as_array().expect("items").is_empty());
    }

    #[tokio::test]
    async fn decision_setzt_status_grund_und_entscheider() {
        let Some(pool) = pool_or_skip("admin_scout_decision").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_scout_candidates (streamer_login) VALUES ('kandidat')")
            .execute(&pool)
            .await
            .expect("seed kandidat");

        let (status, body) = post_decision(
            DashboardAuthLevel::admin(),
            pool.clone(),
            "kandidat",
            "approve",
            Some("Einzelgänger passt"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], true);
        assert_eq!(body["status"], "approved");

        let zeile = sqlx::query_as::<
            _,
            (
                String,
                Option<String>,
                Option<String>,
                Option<chrono::DateTime<Utc>>,
            ),
        >(
            "SELECT status, entscheid_grund, approver, decided_at \
               FROM twitch_scout_candidates WHERE streamer_login = 'kandidat'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(zeile.0, "approved");
        assert_eq!(zeile.1.as_deref(), Some("Einzelgänger passt"));
        assert_eq!(
            zeile.2.as_deref(),
            Some("admin"),
            "Fallback-Entscheider ohne Session"
        );
        assert!(zeile.3.is_some());

        // Persönlich mit Umlaut-Eingabe wird kanonisch gespeichert.
        let (status, body) = post_decision(
            DashboardAuthLevel::admin(),
            pool.clone(),
            "kandidat",
            "persönlich",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "persoenlich");

        // Bekannter Kontakt (manueller Override) wird ebenso kanonisch gespeichert.
        let (status, body) = post_decision(
            DashboardAuthLevel::admin(),
            pool.clone(),
            "kandidat",
            "bekannter_kontakt",
            Some("alter Bekanter"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "bekannter_kontakt");
    }

    #[tokio::test]
    async fn decision_lehnt_fehlerhafte_aufrufe_ab() {
        let Some(pool) = pool_or_skip("admin_scout_decision_abweisung").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_scout_candidates (streamer_login) VALUES ('kandidat')")
            .execute(&pool)
            .await
            .expect("seed kandidat");

        // Nicht-Admin → 401, kein Schreibzugriff.
        let (status, _) = post_decision(
            DashboardAuthLevel::None,
            pool.clone(),
            "kandidat",
            "approve",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // Unbekannter Kandidat → 404.
        let (status, _) = post_decision(
            DashboardAuthLevel::admin(),
            pool.clone(),
            "gibtsnicht",
            "approve",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // Ungültige Entscheidung → 400, Status bleibt unangetastet.
        let (status, body) = post_decision(
            DashboardAuthLevel::admin(),
            pool.clone(),
            "kandidat",
            "vorgeschlagen",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "ungueltige Entscheidung");

        // Ungültiger Login → 400.
        let (status, _) = post_decision(
            DashboardAuthLevel::admin(),
            pool,
            "kein-gültiger-login",
            "approve",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
