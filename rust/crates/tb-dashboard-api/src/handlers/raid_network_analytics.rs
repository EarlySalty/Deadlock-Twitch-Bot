//! Admin-Netzwerk-Raid-Analytics: Partner-Send/Receive-Balance, Leecher
//! ("nie zurückgeraidet") und Manual-Raid-Listing (P2.130).
//!
//! Port der Daten-Sicht hinter der alten server-gerenderten Analytics-Seite
//! (`bot/dashboard/raids/raid_mixin.py:426-520`). Die HTML-Seite selbst wandert
//! per Architektur-Migration ins SPA — dieser Handler liefert die identischen
//! Daten als JSON, damit die im Audit (P2.130) benannten Sichten nicht verloren
//! gehen: Partner-Balance-Tabelle, Leecher-Erkennung und Manual-Raid-Liste
//! (Partner/Extern).
//!
//! Zugriff: admin-only (Python `_require_token`) — `DashboardAuthLevel` muss
//! privileged (Localhost/Admin) sein, sonst 401.
//!
//! Routen-Registrierung erfolgt im Composition-Root (lib.rs) — siehe
//! WIRING-TODO. Dieser Handler ist self-contained und braucht nur den Pool.

use std::collections::{BTreeMap, BTreeSet};

use axum::{extract::State, response::IntoResponse, Json};
use serde_json::{json, Value};
use sqlx::PgPool;
use tb_http_core::ApiError;

use crate::auth::level::DashboardAuthLevel;

/// Aggregat-Zeile pro Login (Sent/Received-Counts + Viewer-Summen).
#[derive(Default, Clone)]
struct LoginAgg {
    cnt: i64,
    viewers: i64,
}

struct LoginAggRow {
    login: Option<String>,
    cnt: i64,
    viewers: i64,
}

/// `GET /twitch/api/raid/analytics`.
///
/// Liefert die Netzwerk-weite Raid-Balance, Leecher und Manual-Raids.
pub async fn raid_network_analytics_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    let payload = load_raid_network_analytics(&pool)
        .await
        .map_err(|_| ApiError::internal())?;
    Ok(Json(payload))
}

/// Baut das Analytics-Payload (separater Loader für DB-Tests ohne Auth-Extractor).
async fn load_raid_network_analytics(pool: &PgPool) -> Result<Value, sqlx::Error> {
    // Aktive Partner (Set, lowercased).
    let partner_rows = sqlx::query!(
        "SELECT LOWER(twitch_login) AS login \
         FROM twitch_streamers_partner_state \
         WHERE is_partner_active = 1",
    )
    .fetch_all(pool)
    .await?;
    let partners: BTreeSet<String> = partner_rows
        .into_iter()
        .filter_map(|r| r.login)
        .filter(|s| !s.is_empty())
        .collect();

    // Sent-Statistik (Quell-Broadcaster).
    let sent_rows = sqlx::query_as!(
        LoginAggRow,
        "SELECT LOWER(from_broadcaster_login) AS login, \
                COUNT(*)::bigint AS \"cnt!\", \
                COALESCE(SUM(viewer_count), 0)::bigint AS \"viewers!\" \
         FROM twitch_raid_history \
         WHERE COALESCE(success, FALSE) IS TRUE \
           AND from_broadcaster_login IS NOT NULL \
         GROUP BY LOWER(from_broadcaster_login)",
    )
    .fetch_all(pool)
    .await?;
    let sent_map = collect_agg(&sent_rows);

    // Received-Statistik (Ziel-Broadcaster).
    let recv_rows = sqlx::query_as!(
        LoginAggRow,
        "SELECT LOWER(to_broadcaster_login) AS login, \
                COUNT(*)::bigint AS \"cnt!\", \
                COALESCE(SUM(viewer_count), 0)::bigint AS \"viewers!\" \
         FROM twitch_raid_history \
         WHERE COALESCE(success, FALSE) IS TRUE \
           AND to_broadcaster_login IS NOT NULL \
         GROUP BY LOWER(to_broadcaster_login)",
    )
    .fetch_all(pool)
    .await?;
    let recv_map = collect_agg(&recv_rows);

    // Per-Partner-Balance (nur aktive Partner für die Haupttabelle).
    let mut partner_stats: Vec<Value> = partners
        .iter()
        .map(|login| {
            let s = sent_map.get(login).cloned().unwrap_or_default();
            let r = recv_map.get(login).cloned().unwrap_or_default();
            json!({
                "login": login,
                "sent": s.cnt,
                "received": r.cnt,
                "balance": s.cnt - r.cnt,
                "viewers_sent": s.viewers,
                "viewers_recv": r.viewers,
            })
        })
        .collect();
    // Sortierung nach balance absteigend (Python: reverse=True).
    partner_stats.sort_by(|a, b| {
        b["balance"]
            .as_i64()
            .unwrap_or(0)
            .cmp(&a["balance"].as_i64().unwrap_or(0))
    });

    // Leecher: aktive Partner mit sent==0 UND received>0.
    let leechers: Vec<Value> = partner_stats
        .iter()
        .filter(|p| p["sent"].as_i64() == Some(0) && p["received"].as_i64().unwrap_or(0) > 0)
        .map(|p| {
            json!({
                "login": p["login"],
                "received": p["received"],
            })
        })
        .collect();

    // Manual-Raids (reason = 'manual_chat_command'), neueste zuerst.
    let manual_rows = sqlx::query!(
        "SELECT LOWER(from_broadcaster_login) AS from_login, \
                LOWER(to_broadcaster_login) AS to_login, \
                COALESCE(viewer_count, 0)::bigint AS \"viewers!\", \
                LEFT(executed_at::text, 16) AS at \
         FROM twitch_raid_history \
         WHERE reason = 'manual_chat_command' \
         ORDER BY executed_at DESC",
    )
    .fetch_all(pool)
    .await?;
    let manual_list: Vec<Value> = manual_rows
        .iter()
        .map(|row| {
            let from = row.from_login.clone().unwrap_or_default();
            let to = row.to_login.clone().unwrap_or_default();
            let viewers = row.viewers;
            let at = row.at.clone().unwrap_or_default();
            json!({
                "from": from,
                "to": to.clone(),
                "viewers": viewers,
                "at": at,
                "is_partner": partners.contains(&to),
            })
        })
        .collect();

    // Datumsbereich + Gesamtzahl erfolgreicher Raids.
    let date_row = sqlx::query!(
        "SELECT LEFT(MIN(executed_at)::text, 10) AS date_min, \
                LEFT(MAX(executed_at)::text, 10) AS date_max, \
                COUNT(*)::bigint AS \"total!\" \
         FROM twitch_raid_history \
         WHERE COALESCE(success, FALSE) IS TRUE",
    )
    .fetch_optional(pool)
    .await?;
    let (date_min, date_max, total) = match date_row {
        Some(r) => (
            r.date_min.unwrap_or_default(),
            r.date_max.unwrap_or_default(),
            r.total,
        ),
        None => (String::new(), String::new(), 0),
    };

    Ok(json!({
        "partner_stats": partner_stats,
        "leechers": leechers,
        "manual_raids": manual_list,
        "date_min": date_min,
        "date_max": date_max,
        "total": total,
        "active_partner_count": partners.len(),
    }))
}

/// Sammelt die Aggregat-Zeilen in eine Login→`LoginAgg`-Map.
fn collect_agg(rows: &[LoginAggRow]) -> BTreeMap<String, LoginAgg> {
    let mut map = BTreeMap::new();
    for row in rows {
        let Some(login) = row.login.clone() else {
            continue;
        };
        if login.is_empty() {
            continue;
        }
        map.insert(
            login,
            LoginAgg {
                cnt: row.cnt,
                viewers: row.viewers,
            },
        );
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::session::{DashboardAuthState, ADMIN_COOKIE_NAME};
    use axum::{
        body::Body,
        http::{header, Request, StatusCode},
        Extension, Router,
    };
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use tower::ServiceExt;

    const TEST_FERNET_KEY: &str = "dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU=";

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        if std::env::var("TB_TEST_REQUIRE_DB").as_deref() != Ok("1") {
            return None;
        }
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
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE dashboard_sessions (\
                 session_id TEXT NOT NULL PRIMARY KEY, session_type TEXT NOT NULL, \
                 payload_enc BYTEA NOT NULL, created_at DOUBLE PRECISION NOT NULL, \
                 expires_at DOUBLE PRECISION NOT NULL)",
            "CREATE TABLE twitch_streamers_partner_state (\
                 twitch_login TEXT, twitch_user_id TEXT, is_partner_active INTEGER NOT NULL DEFAULT 1)",
            "CREATE TABLE twitch_raid_history (\
                 from_broadcaster_login TEXT, to_broadcaster_login TEXT, \
                 viewer_count INTEGER DEFAULT 0, reason TEXT, \
                 executed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), success BOOLEAN DEFAULT TRUE)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    fn router(pool: PgPool, auth_state: DashboardAuthState) -> Router {
        crate::build_raid_pages_router(pool).layer(Extension(auth_state))
    }

    async fn seed(pool: &PgPool) {
        // Aktive Partner: alpha, bravo, charlie. Inaktiv: zeta.
        for (login, active) in [("alpha", 1), ("bravo", 1), ("charlie", 1), ("zeta", 0)] {
            sqlx::query("INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active) VALUES ($1,$2)")
                .bind(login).bind(active).execute(pool).await.unwrap();
        }
        // alpha sendet 2 (an bravo, extern), empfängt 0 → positive balance.
        // bravo sendet 0, empfängt 1 → Leecher.
        // charlie sendet 0, empfängt 0 → kein Leecher (received==0).
        let raids = [
            ("alpha", "bravo", 50, "auto", true),
            ("alpha", "externtarget", 30, "manual_chat_command", true),
            ("nonpartner", "charlie_no", 10, "auto", false), // success=false ignoriert
        ];
        for (from, to, vc, reason, success) in raids {
            sqlx::query("INSERT INTO twitch_raid_history (from_broadcaster_login,to_broadcaster_login,viewer_count,reason,success) VALUES ($1,$2,$3,$4,$5)")
                .bind(from).bind(to).bind(vc).bind(reason).bind(success).execute(pool).await.unwrap();
        }
    }

    #[tokio::test]
    async fn partner_balance_leecher_und_manual() {
        let Some(pool) = make_pool("t_raid_net").await else {
            return;
        };
        seed(&pool).await;

        let payload = load_raid_network_analytics(&pool).await.unwrap();

        assert_eq!(payload["active_partner_count"], 3);
        // total = 2 erfolgreiche Raids (success=false zählt nicht).
        assert_eq!(payload["total"], 2);

        // Partner-Balance: alpha sent=2 (an bravo + extern, beide success), recv=0
        // → balance 2; bravo sent=0 recv=1 → balance -1.
        let stats = payload["partner_stats"].as_array().unwrap();
        assert_eq!(stats.len(), 3);
        let alpha = stats.iter().find(|s| s["login"] == "alpha").unwrap();
        assert_eq!(alpha["sent"], 2);
        assert_eq!(alpha["received"], 0);
        assert_eq!(alpha["balance"], 2);
        assert_eq!(alpha["viewers_sent"], 80);
        let bravo = stats.iter().find(|s| s["login"] == "bravo").unwrap();
        assert_eq!(bravo["sent"], 0);
        assert_eq!(bravo["received"], 1);
        assert_eq!(bravo["balance"], -1);
        // Sortierung: alpha (2) vor charlie (0) vor bravo (-1).
        assert_eq!(stats[0]["login"], "alpha");
        assert_eq!(stats[2]["login"], "bravo");

        // Leecher: nur bravo (sent==0 && received>0). charlie hat received==0.
        let leechers = payload["leechers"].as_array().unwrap();
        assert_eq!(leechers.len(), 1);
        assert_eq!(leechers[0]["login"], "bravo");
        assert_eq!(leechers[0]["received"], 1);

        // Manual-Raids: 1 Eintrag (alpha→externtarget), is_partner=false.
        let manual = payload["manual_raids"].as_array().unwrap();
        assert_eq!(manual.len(), 1);
        assert_eq!(manual[0]["from"], "alpha");
        assert_eq!(manual[0]["to"], "externtarget");
        assert_eq!(manual[0]["is_partner"], false);
        assert_eq!(manual[0]["viewers"], 30);

        sqlx::query("DROP SCHEMA t_raid_net CASCADE")
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    async fn leerer_datensatz_liefert_leere_listen() {
        let Some(pool) = make_pool("t_raid_net_empty").await else {
            return;
        };
        let payload = load_raid_network_analytics(&pool).await.unwrap();
        assert_eq!(payload["total"], 0);
        assert_eq!(payload["active_partner_count"], 0);
        assert!(payload["partner_stats"].as_array().unwrap().is_empty());
        assert!(payload["leechers"].as_array().unwrap().is_empty());
        assert!(payload["manual_raids"].as_array().unwrap().is_empty());
        sqlx::query("DROP SCHEMA t_raid_net_empty CASCADE")
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    async fn route_liefert_json_fuer_admin_session() {
        let Some(pool) = make_pool("t_raid_net_route").await else {
            return;
        };
        seed(&pool).await;

        let auth_state = DashboardAuthState::new(pool.clone(), TEST_FERNET_KEY.to_string());
        let session = auth_state
            .create_admin_session("discord-raid-net", "Raid Admin")
            .await
            .unwrap();
        let resp = router(pool.clone(), auth_state)
            .oneshot(
                Request::builder()
                    .uri("/twitch/api/raid/analytics")
                    .header("x-dashboard-context", "admin")
                    .header(
                        header::COOKIE,
                        format!("{ADMIN_COOKIE_NAME}={}", session.session_id),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["active_partner_count"], 3);
        assert!(payload["partner_stats"].is_array());
        sqlx::query("DROP SCHEMA t_raid_net_route CASCADE")
            .execute(&pool)
            .await
            .ok();
    }
}
