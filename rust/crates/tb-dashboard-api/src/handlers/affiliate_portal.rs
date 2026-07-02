//! Affiliate-Portal für den eingeloggten Twitch-Account.
//!
//! - `portal_handler` — JSON-API (`/twitch/api/v2/affiliate/portal`).
//! - `portal_page_handler` — HTML-Seite (`/twitch/affiliate/portal`, P1.26):
//!   serviert die dashboard_v2-SPA-Shell nativ statt via Python-Fallback.

use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{Datelike, Utc};
use serde::Deserialize;
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

    let account = sqlx::query!(
        "SELECT twitch_login, display_name, is_active \
         FROM affiliate_accounts WHERE LOWER(twitch_login) = $1 LIMIT 1",
        &login
    )
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
    let claims = sqlx::query!(
        "SELECT COUNT(*)::bigint AS \"total_claims!\", \
                COUNT(*) FILTER (WHERE claimed_at >= $1)::bigint AS \"month_claims!\" \
         FROM affiliate_streamer_claims \
         WHERE LOWER(affiliate_twitch_login) = $2",
        &month_start,
        &login
    )
    .fetch_one(&pool)
    .await
    .ok();
    let commissions = sqlx::query!(
        "SELECT \
            COALESCE(SUM(commission_cents) FILTER (WHERE status IN ('pending','transferred')), 0)::bigint AS \"total_cents!\", \
            COALESCE(SUM(commission_cents) FILTER (WHERE created_at >= $1 AND status IN ('pending','transferred')), 0)::bigint AS \"month_cents!\", \
            COALESCE(SUM(commission_cents) FILTER (WHERE status = 'pending'), 0)::bigint AS \"pending_cents!\" \
         FROM affiliate_commissions WHERE LOWER(affiliate_twitch_login) = $2",
        &month_start,
        &login
    )
    .fetch_one(&pool)
    .await
    .ok();
    let rows = sqlx::query!(
        "SELECT c.claimed_streamer_login, c.claimed_at, \
                COALESCE(SUM(co.commission_cents), 0)::bigint AS \"amount_cents!\" \
         FROM affiliate_streamer_claims c \
         LEFT JOIN affiliate_commissions co \
           ON LOWER(co.affiliate_twitch_login) = LOWER(c.affiliate_twitch_login) \
          AND LOWER(co.streamer_login) = LOWER(c.claimed_streamer_login) \
          AND co.status IN ('pending','transferred') \
         WHERE LOWER(c.affiliate_twitch_login) = $1 \
         GROUP BY c.claimed_streamer_login, c.claimed_at \
         ORDER BY c.claimed_at DESC LIMIT 10",
        &login
    )
    .fetch_all(&pool)
    .await
    .unwrap_or_default();

    let mut recent = Vec::with_capacity(rows.len());
    for row in rows {
        let customer_login = row.claimed_streamer_login;
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
            "amount": row.amount_cents as f64 / 100.0,
            "created_at": row.claimed_at,
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
            "login": account.twitch_login,
            "display_name": account.display_name,
            "active": account.is_active != 0,
            "referral_code": ref_code,
            "referral_url": referral_url,
        },
        "stats": {
            "total_claims": claims.as_ref().map(|row| row.total_claims).unwrap_or(0),
            "total_provision": commissions.as_ref().map(|row| row.total_cents).unwrap_or(0) as f64 / 100.0,
            "this_month_claims": claims.as_ref().map(|row| row.month_claims).unwrap_or(0),
            "this_month_provision": commissions.as_ref().map(|row| row.month_cents).unwrap_or(0) as f64 / 100.0,
            "pending_payout": commissions.as_ref().map(|row| row.pending_cents).unwrap_or(0) as f64 / 100.0,
        },
        "recent_claims": recent,
    }))
    .into_response()
}

#[derive(Debug, Deserialize, Default)]
pub struct CommissionsQuery {
    #[serde(default)]
    pub page: Option<i64>,
    #[serde(default)]
    pub page_size: Option<i64>,
}

pub async fn commissions_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<CommissionsQuery>,
) -> impl IntoResponse {
    let Some(login) = authenticated_login(&auth) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"unauthorized"})),
        )
            .into_response();
    };
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(25).clamp(1, 100);
    let offset = (page - 1) * page_size;

    let total = match sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint FROM affiliate_commissions \
         WHERE LOWER(affiliate_twitch_login) = $1",
    )
    .bind(&login)
    .fetch_one(&pool)
    .await
    {
        Ok(total) => total,
        Err(error) => {
            tracing::error!(%error, "affiliate commissions count failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":"db"})),
            )
                .into_response();
        }
    };

    type CommissionRow = (
        i64,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
        i64,
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
    );
    let rows = match sqlx::query_as::<_, CommissionRow>(
        r#"
        SELECT id::bigint, affiliate_twitch_login, streamer_login,
               stripe_event_id, stripe_invoice_id, stripe_customer_id,
               COALESCE(brutto_cents, 0)::bigint,
               COALESCE(commission_cents, 0)::bigint,
               COALESCE(currency, 'eur'),
               COALESCE(status, 'pending'),
               period_start, period_end, created_at, transferred_at, error_message
        FROM affiliate_commissions
        WHERE LOWER(affiliate_twitch_login) = $1
        ORDER BY created_at DESC, id DESC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(&login)
    .bind(page_size)
    .bind(offset)
    .fetch_all(&pool)
    .await
    {
        Ok(rows) => rows,
        Err(error) => {
            tracing::error!(%error, "affiliate commissions lookup failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":"db"})),
            )
                .into_response();
        }
    };

    let items: Vec<_> = rows
        .into_iter()
        .map(
            |(
                id,
                affiliate_twitch_login,
                streamer_login,
                stripe_event_id,
                stripe_invoice_id,
                stripe_customer_id,
                brutto_cents,
                commission_cents,
                currency,
                status,
                period_start,
                period_end,
                created_at,
                transferred_at,
                error_message,
            )| {
                json!({
                    "id": id,
                    "affiliate_twitch_login": affiliate_twitch_login,
                    "streamer_login": streamer_login,
                    "stripe_event_id": stripe_event_id,
                    "stripe_invoice_id": stripe_invoice_id,
                    "stripe_customer_id": stripe_customer_id,
                    "brutto_cents": brutto_cents,
                    "commission_cents": commission_cents,
                    "amount": commission_cents as f64 / 100.0,
                    "currency": currency,
                    "status": status,
                    "period_start": period_start,
                    "period_end": period_end,
                    "created_at": created_at,
                    "transferred_at": transferred_at,
                    "error_message": error_message,
                })
            },
        )
        .collect();

    Json(json!({
        "page": page,
        "page_size": page_size,
        "total": total,
        "commissions": items,
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
            return (
                StatusCode::NOT_FOUND,
                "Das Affiliate-Portal ist derzeit nicht verfügbar.",
            )
                .into_response();
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
            "CREATE TABLE affiliate_commissions (id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY, affiliate_twitch_login TEXT NOT NULL, streamer_login TEXT NOT NULL, stripe_event_id TEXT, stripe_invoice_id TEXT, stripe_customer_id TEXT, stripe_transfer_id TEXT, brutto_cents INTEGER NOT NULL DEFAULT 0, commission_cents INTEGER NOT NULL, currency TEXT NOT NULL DEFAULT 'eur', status TEXT NOT NULL DEFAULT 'pending', period_start TEXT, period_end TEXT, created_at TEXT NOT NULL, transferred_at TEXT, error_message TEXT)",
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
        sqlx::query("INSERT INTO affiliate_commissions (affiliate_twitch_login, streamer_login, stripe_event_id, brutto_cents, commission_cents, status, created_at) VALUES ('nani','kunde','evt_portal',1000,300,'pending','2099-01-01T00:00:00+00:00')")
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

    #[tokio::test]
    async fn commissions_route_liefert_page_total_und_sortierung() {
        let Some(pool) = pool().await else { return };
        sqlx::query("INSERT INTO affiliate_accounts VALUES ('nani','Nani',1)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO affiliate_commissions \
             (affiliate_twitch_login, streamer_login, stripe_event_id, brutto_cents, commission_cents, currency, status, created_at) \
             VALUES \
             ('nani','a','evt_old',1000,300,'eur','pending','2026-06-01T00:00:00+00:00'), \
             ('nani','b','evt_new',2000,600,'eur','transferred','2026-06-02T00:00:00+00:00'), \
             ('other','x','evt_other',500,150,'eur','pending','2026-06-03T00:00:00+00:00')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let response = commissions_handler(
            DashboardAuthLevel::Partner {
                twitch_login: "nani".into(),
                twitch_user_id: "1".into(),
                display_name: "Nani".into(),
            },
            State(pool),
            Query(CommissionsQuery {
                page: Some(1),
                page_size: Some(1),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["page"], 1);
        assert_eq!(value["page_size"], 1);
        assert_eq!(value["total"], 2);
        assert_eq!(value["commissions"][0]["streamer_login"], "b");
        assert_eq!(value["commissions"][0]["commission_cents"], 600);
    }

    // P1.26: Portal-HTML-Seite wird nativ aus dem dist-Verzeichnis serviert.
    #[tokio::test]
    async fn portal_page_serviert_html() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
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
