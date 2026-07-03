//! Affiliate-Portal für den eingeloggten Twitch-Account.
//!
//! - `portal_handler` — JSON-API (`/twitch/api/v2/affiliate/portal`).
//! - `portal_page_handler` — HTML-Seite (`/twitch/affiliate/portal`, P1.26):
//!   serviert das dedizierte Bundle aus `website/dist/affiliate-portal`.
//!   Assets daraus laufen über die bestehende `/streamer/*`-Route.

use axum::{
    extract::{Extension, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{Datelike, Utc};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;

use crate::auth::level::DashboardAuthLevel;
use crate::auth::session::DashboardAuthState;
use crate::handlers::affiliate::affiliate_session_from_headers;
use crate::handlers::website::website_dist_root;

async fn authenticated_login(
    headers: &HeaderMap,
    auth_state: Option<&DashboardAuthState>,
) -> Option<String> {
    affiliate_session_from_headers(auth_state, headers)
        .await
        .map(|session| session.twitch_login)
        .filter(|login| !login.trim().is_empty())
}

pub async fn portal_handler(
    _auth: DashboardAuthLevel,
    auth_state: Option<Extension<DashboardAuthState>>,
    headers: HeaderMap,
    State(pool): State<PgPool>,
) -> impl IntoResponse {
    let Some(login) =
        authenticated_login(&headers, auth_state.as_ref().map(|state| &state.0)).await
    else {
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
    _auth: DashboardAuthLevel,
    auth_state: Option<Extension<DashboardAuthState>>,
    headers: HeaderMap,
    State(pool): State<PgPool>,
    Query(query): Query<CommissionsQuery>,
) -> impl IntoResponse {
    let Some(login) =
        authenticated_login(&headers, auth_state.as_ref().map(|state| &state.0)).await
    else {
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

/// `GET /twitch/affiliate/portal` (P1.26) — serviert das dedizierte
/// Affiliate-Portal-Bundle aus `website/dist/affiliate-portal`.
///
/// Das gebaute `index.html` referenziert Assets unter `/streamer/assets/...`;
/// diese werden bereits über `website::streamer_asset_handler` ausgeliefert.
/// Die JSON-API bleibt unter `/twitch/api/v2/affiliate/portal`.
pub async fn portal_page_handler() -> Response {
    let index = website_dist_root().join("affiliate-portal/index.html");
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
    use crate::auth::session::{DashboardAuthState, AFFILIATE_COOKIE_NAME};
    use axum::http::header::COOKIE;
    use axum::{body::to_bytes, response::IntoResponse};
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    const TEST_FERNET_KEY: &str = "dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU=";

    async fn pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .ok()?;
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .ok()?;
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .ok()?;
        admin.close().await;
        let options = PgConnectOptions::from_str(&dsn)
            .ok()?
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .ok()?;
        for ddl in [
            "CREATE TABLE dashboard_sessions (session_id TEXT PRIMARY KEY, session_type TEXT NOT NULL, payload_enc BYTEA NOT NULL, created_at DOUBLE PRECISION NOT NULL, expires_at DOUBLE PRECISION NOT NULL)",
            "CREATE TABLE affiliate_accounts (twitch_login TEXT PRIMARY KEY, display_name TEXT, is_active INTEGER)",
            "CREATE TABLE affiliate_streamer_claims (affiliate_twitch_login TEXT, claimed_streamer_login TEXT, claimed_at TEXT)",
            "CREATE TABLE affiliate_commissions (id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY, affiliate_twitch_login TEXT NOT NULL, streamer_login TEXT NOT NULL, stripe_event_id TEXT, stripe_invoice_id TEXT, stripe_customer_id TEXT, stripe_transfer_id TEXT, brutto_cents INTEGER NOT NULL DEFAULT 0, commission_cents INTEGER NOT NULL, currency TEXT NOT NULL DEFAULT 'eur', status TEXT NOT NULL DEFAULT 'pending', period_start TEXT, period_end TEXT, created_at TEXT NOT NULL, transferred_at TEXT, error_message TEXT)",
            "CREATE TABLE twitch_streamers (twitch_login TEXT, twitch_user_id TEXT, display_name TEXT)",
        ] {
            sqlx::query(ddl).execute(&pool).await.ok()?;
        }
        Some(pool)
    }

    fn state(pool: PgPool) -> DashboardAuthState {
        DashboardAuthState::new(pool, TEST_FERNET_KEY.to_string())
    }

    fn partner_auth() -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: "nani".into(),
            twitch_user_id: "1".into(),
            display_name: "Nani".into(),
        }
    }

    async fn affiliate_cookie_headers(state: &DashboardAuthState) -> HeaderMap {
        let session = state
            .create_affiliate_session("nani", "1", "Nani", "")
            .await
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            format!("{}={}", AFFILIATE_COOKIE_NAME, session.session_id)
                .parse()
                .unwrap(),
        );
        headers
    }

    #[tokio::test]
    async fn dashboard_session_ohne_affiliate_cookie_wird_abgewiesen() {
        let Some(pool) = pool("t_affiliate_portal_auth").await else {
            return;
        };
        sqlx::query("INSERT INTO affiliate_accounts VALUES ('nani','Nani',1)")
            .execute(&pool)
            .await
            .unwrap();
        let auth_state = state(pool.clone());

        let response = portal_handler(
            partner_auth(),
            Some(Extension(auth_state.clone())),
            HeaderMap::new(),
            State(pool.clone()),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let response = commissions_handler(
            partner_auth(),
            Some(Extension(auth_state)),
            HeaderMap::new(),
            State(pool),
            Query(CommissionsQuery::default()),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn portal_liefert_claims_und_provisionen() {
        let Some(pool) = pool("t_affiliate_portal_stats").await else {
            return;
        };
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
        let auth_state = state(pool.clone());
        let headers = affiliate_cookie_headers(&auth_state).await;

        let response = portal_handler(
            partner_auth(),
            Some(Extension(auth_state)),
            headers,
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
        let Some(pool) = pool("t_affiliate_portal_commissions").await else {
            return;
        };
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
        let auth_state = state(pool.clone());
        let headers = affiliate_cookie_headers(&auth_state).await;

        let response = commissions_handler(
            partner_auth(),
            Some(Extension(auth_state)),
            headers,
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

    // P1.26: Portal-HTML-Seite wird nativ aus dem website-dist-Verzeichnis serviert.
    #[tokio::test]
    async fn portal_page_serviert_html() {
        use std::time::{SystemTime, UNIX_EPOCH};
        struct EnvGuard {
            key: &'static str,
            previous: Option<String>,
        }
        impl EnvGuard {
            fn set_path(key: &'static str, value: &std::path::Path) -> Self {
                let previous = std::env::var(key).ok();
                std::env::set_var(key, value);
                Self { key, previous }
            }
        }
        impl Drop for EnvGuard {
            fn drop(&mut self) {
                match self.previous.as_ref() {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("tb_affiliate_portal_{unique}"));
        let portal_root = root.join("affiliate-portal");
        tokio::fs::create_dir_all(&portal_root).await.unwrap();
        tokio::fs::write(portal_root.join("index.html"), b"<html>portal</html>")
            .await
            .unwrap();
        let _guard = EnvGuard::set_path("WEBSITE_DIST_PATH", &root);

        let res = portal_page_handler().await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/html; charset=utf-8"
        );

        tokio::fs::remove_dir_all(root).await.ok();
    }
}
