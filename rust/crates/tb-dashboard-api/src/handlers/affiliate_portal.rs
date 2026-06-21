//! Affiliate-Portal für den eingeloggten Twitch-Account.
//!
//! - `portal_handler` — JSON-API (`/twitch/api/v2/affiliate/portal`).
//! - `portal_page_handler` — HTML-Seite (`/twitch/affiliate/portal`, P1.26):
//!   serviert die dashboard_v2-SPA-Shell nativ statt via Python-Fallback.

use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{Datelike, Utc};
use serde_json::json;
use sqlx::PgPool;

use crate::auth::level::DashboardAuthLevel;
use crate::handlers::spa::dist_root;

fn authenticated_login(auth: &DashboardAuthLevel) -> Option<String> {
    match auth {
        DashboardAuthLevel::Partner { twitch_login, .. } => {
            Some(twitch_login.trim().to_lowercase())
        }
        DashboardAuthLevel::Admin { actor: Some(actor) } => {
            Some(actor.twitch_login.trim().to_lowercase())
        }
        _ => None,
    }
    .filter(|login| !login.is_empty())
}

pub async fn portal_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> impl IntoResponse {
    let Some(login) = authenticated_login(&auth) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"unauthorized"})),
        )
            .into_response();
    };

    let account = sqlx::query_as::<_, (String, Option<String>, i32)>(
        "SELECT twitch_login, display_name, is_active \
         FROM affiliate_accounts WHERE LOWER(twitch_login) = $1 LIMIT 1",
    )
    .bind(&login)
    .fetch_optional(&pool)
    .await;
    let account = match account {
        Ok(Some(account)) => account,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, Json(json!({"error":"not_found"}))).into_response()
        }
        Err(e) => {
            tracing::error!("affiliate account lookup fehlgeschlagen: {e}");
            return (StatusCode::NOT_FOUND, Json(json!({"error":"not_found"}))).into_response();
        }
    };

    let now = Utc::now();
    let month_start = format!("{:04}-{:02}-01T00:00:00+00:00", now.year(), now.month());
    let claims = sqlx::query_as::<_, (i64, i64)>(
        "SELECT COUNT(*)::bigint, \
                COUNT(*) FILTER (WHERE claimed_at >= $1)::bigint \
         FROM affiliate_streamer_claims \
         WHERE LOWER(affiliate_twitch_login) = $2",
    )
    .bind(&month_start)
    .bind(&login)
    .fetch_one(&pool)
    .await
    .unwrap_or((0, 0));
    let commissions = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT \
            COALESCE(SUM(commission_cents) FILTER (WHERE status IN ('pending','transferred')), 0)::bigint, \
            COALESCE(SUM(commission_cents) FILTER (WHERE created_at >= $1 AND status IN ('pending','transferred')), 0)::bigint, \
            COALESCE(SUM(commission_cents) FILTER (WHERE status = 'pending'), 0)::bigint \
         FROM affiliate_commissions WHERE LOWER(affiliate_twitch_login) = $2",
    )
    .bind(&month_start)
    .bind(&login)
    .fetch_one(&pool)
    .await
    .unwrap_or((0, 0, 0));
    let rows = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT c.claimed_streamer_login, c.claimed_at, \
                COALESCE(SUM(co.commission_cents), 0)::bigint \
         FROM affiliate_streamer_claims c \
         LEFT JOIN affiliate_commissions co \
           ON LOWER(co.affiliate_twitch_login) = LOWER(c.affiliate_twitch_login) \
          AND LOWER(co.streamer_login) = LOWER(c.claimed_streamer_login) \
          AND co.status IN ('pending','transferred') \
         WHERE LOWER(c.affiliate_twitch_login) = $1 \
         GROUP BY c.claimed_streamer_login, c.claimed_at \
         ORDER BY c.claimed_at DESC LIMIT 10",
    )
    .bind(&login)
    .fetch_all(&pool)
    .await
    .unwrap_or_default();

    let mut recent = Vec::with_capacity(rows.len());
    for (customer_login, created_at, amount_cents) in rows {
        let display_name: Option<String> = sqlx::query_scalar(
            "SELECT display_name FROM twitch_streamers \
             WHERE LOWER(twitch_login) = LOWER($1) LIMIT 1",
        )
        .bind(&customer_login)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
        .flatten();
        let plan_name = tb_analytics::plan::resolve_plan_snapshot(&pool, &customer_login, "")
            .await
            .ok()
            .map(|snapshot| snapshot.plan_name);
        recent.push(json!({
            "customer_display_name": display_name.unwrap_or_else(|| customer_login.clone()),
            "plan_name": plan_name,
            "amount": amount_cents as f64 / 100.0,
            "created_at": created_at,
        }));
    }

    let ref_code = std::env::var("TWITCH_DISCORD_REF_CODE")
        .unwrap_or_else(|_| "DE-Deadlock-Discord".to_string());
    let referral_url = if ref_code.trim().is_empty() {
        format!("https://www.twitch.tv/{login}")
    } else {
        format!(
            "https://www.twitch.tv/{login}?ref={}",
            url::form_urlencoded::byte_serialize(ref_code.trim().as_bytes()).collect::<String>()
        )
    };

    Json(json!({
        "affiliate": {
            "login": account.0,
            "display_name": account.1,
            "active": account.2 != 0,
            "referral_code": ref_code,
            "referral_url": referral_url,
        },
        "stats": {
            "total_claims": claims.0,
            "total_provision": commissions.0 as f64 / 100.0,
            "this_month_claims": claims.1,
            "this_month_provision": commissions.1 as f64 / 100.0,
            "pending_payout": commissions.2 as f64 / 100.0,
        },
        "recent_claims": recent,
    }))
    .into_response()
}

/// `GET /twitch/affiliate/portal` (P1.26) — serviert die Portal-HTML-Seite
/// (dashboard_v2-SPA-Shell) nativ. Die JSON-API bleibt unter
/// `/twitch/api/v2/affiliate/portal`.
pub async fn portal_page_handler() -> Response {
    let index = dist_root().join("index.html");
    let html = match tokio::fs::read_to_string(&index).await {
        Ok(s) => s,
        Err(_) => {
            // 404, wenn das Affiliate-Portal-Bundle (index.html) nicht gebaut ist.
            return (StatusCode::NOT_FOUND, "Das Affiliate-Portal ist derzeit nicht verfügbar.").into_response();
        }
    };
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        html,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::level::AdminActor;
    use axum::{body::to_bytes, response::IntoResponse};
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    #[test]
    fn login_nur_aus_twitch_session() {
        assert_eq!(
            authenticated_login(&DashboardAuthLevel::Partner {
                twitch_login: "Nani".into(),
                twitch_user_id: "1".into(),
                display_name: "Nani".into(),
            })
            .as_deref(),
            Some("nani")
        );
        assert!(authenticated_login(&DashboardAuthLevel::admin()).is_none());
        assert_eq!(
            authenticated_login(&DashboardAuthLevel::Admin {
                actor: Some(AdminActor {
                    twitch_login: "EarlySalty".into(),
                    twitch_user_id: "2".into(),
                }),
            })
            .as_deref(),
            Some("earlysalty")
        );
    }

    async fn pool() -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .ok()?;
        sqlx::query("DROP SCHEMA IF EXISTS t_affiliate_portal CASCADE")
            .execute(&admin)
            .await
            .ok()?;
        sqlx::query("CREATE SCHEMA t_affiliate_portal")
            .execute(&admin)
            .await
            .ok()?;
        admin.close().await;
        let options = PgConnectOptions::from_str(&dsn)
            .ok()?
            .options([("search_path", "t_affiliate_portal")]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .ok()?;
        for ddl in [
            "CREATE TABLE affiliate_accounts (twitch_login TEXT PRIMARY KEY, display_name TEXT, is_active INTEGER)",
            "CREATE TABLE affiliate_streamer_claims (affiliate_twitch_login TEXT, claimed_streamer_login TEXT, claimed_at TEXT)",
            "CREATE TABLE affiliate_commissions (affiliate_twitch_login TEXT, streamer_login TEXT, commission_cents INTEGER, status TEXT, created_at TEXT)",
            "CREATE TABLE twitch_streamers (twitch_login TEXT, twitch_user_id TEXT, display_name TEXT)",
        ] {
            sqlx::query(ddl).execute(&pool).await.ok()?;
        }
        Some(pool)
    }

    #[tokio::test]
    async fn portal_liefert_claims_und_provisionen() {
        let Some(pool) = pool().await else { return };
        sqlx::query("INSERT INTO affiliate_accounts VALUES ('nani','Nani',1)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO twitch_streamers VALUES ('kunde','42','Kunde')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO affiliate_streamer_claims VALUES ('nani','kunde','2099-01-01T00:00:00+00:00')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO affiliate_commissions VALUES ('nani','kunde',300,'pending','2099-01-01T00:00:00+00:00')")
            .execute(&pool)
            .await
            .unwrap();

        let response = portal_handler(
            DashboardAuthLevel::Partner {
                twitch_login: "nani".into(),
                twitch_user_id: "1".into(),
                display_name: "Nani".into(),
            },
            State(pool),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["stats"]["total_claims"], 1);
        assert_eq!(value["stats"]["pending_payout"], 3.0);
        assert_eq!(value["recent_claims"][0]["customer_display_name"], "Kunde");
    }

    // P1.26: Portal-HTML-Seite wird nativ aus dem dist-Verzeichnis serviert.
    #[tokio::test]
    async fn portal_page_serviert_html() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("tb_affiliate_portal_{unique}"));
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(root.join("index.html"), b"<html>portal</html>")
            .await
            .unwrap();
        std::env::set_var("DASHBOARD_V2_DIST_PATH", &root);

        let res = portal_page_handler().await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/html; charset=utf-8"
        );

        std::env::remove_var("DASHBOARD_V2_DIST_PATH");
        tokio::fs::remove_dir_all(root).await.ok();
    }
}
