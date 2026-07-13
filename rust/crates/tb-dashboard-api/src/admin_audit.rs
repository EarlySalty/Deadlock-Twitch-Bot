use axum::{
    extract::{Request, State},
    http::{header::LOCATION, HeaderMap, Method},
    middleware::Next,
    response::Response,
};
use sqlx::PgPool;

fn is_write_method(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

fn is_admin_path(path: &str) -> bool {
    path.starts_with("/twitch/api/admin/")
        || path.starts_with("/twitch/api/v2/admin/")
        || path.starts_with("/social-media/api/admin/")
        || path == "/twitch/api/v2/internal-home/changelog"
        || path == "/twitch/api/v2/roadmap"
        || path.starts_with("/twitch/api/v2/roadmap/")
        || is_legacy_admin_path(path)
}

fn is_legacy_admin_path(path: &str) -> bool {
    path.starts_with("/twitch/admin/")
        || matches!(
            path,
            "/twitch/add_streamer"
                | "/twitch/add_url"
                | "/twitch/add_login"
                | "/twitch/add_any"
                | "/twitch/remove"
                | "/twitch/discord_link"
                | "/twitch/verify"
                | "/twitch/archive"
                | "/twitch/discord_flag"
        )
}

fn response_marks_success(path: &str, response: &Response) -> bool {
    response.status().is_success()
        || (is_legacy_admin_path(path)
            && response.status().is_redirection()
            && response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|location| location.starts_with("/twitch/admin?ok=")))
}

fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let cookies = headers
        .get(axum::http::header::COOKIE)
        .and_then(|value| value.to_str().ok())?;
    cookies.split(';').find_map(|pair| {
        let (key, value) = pair.trim().split_once('=')?;
        (key.trim() == name).then_some(value.trim())
    })
}

async fn actor_from_request(
    state: Option<crate::auth::session::DashboardAuthState>,
    admin_session_id: Option<String>,
    partner_session_id: Option<String>,
    internal: bool,
) -> String {
    if let Some(state) = state {
        if let Some(session_id) = admin_session_id {
            if let Ok(Some(user_id)) = state.load_admin_session_user_id(&session_id).await {
                return format!("discord:{user_id}");
            }
        }
        if let Some(session_id) = partner_session_id {
            if let Ok(Some(session)) = state.load_partner_session(&session_id).await {
                if !session.twitch_login.trim().is_empty() {
                    return session.twitch_login.trim().to_lowercase();
                }
            }
        }
    }
    if internal {
        "internal".to_string()
    } else {
        "admin".to_string()
    }
}

/// Persistiert erfolgreiche mutierende Admin-Requests ohne Query oder Body.
///
/// Die Auditierung ist best-effort: ein Schreibfehler darf die bereits
/// ausgeführte Admin-Aktion nicht in einen HTTP-Fehler umwandeln.
pub async fn audit_admin_mutations(
    State(pool): State<PgPool>,
    request: Request,
    next: Next,
) -> Response {
    if !is_write_method(request.method()) || !is_admin_path(request.uri().path()) {
        return next.run(request).await;
    }

    let method = request.method().as_str().to_string();
    let path: String = request.uri().path().chars().take(512).collect();
    let auth_state = request
        .extensions()
        .get::<crate::auth::session::DashboardAuthState>()
        .cloned();
    let admin_session_id = cookie_value(request.headers(), crate::auth::session::ADMIN_COOKIE_NAME)
        .map(str::to_string);
    let partner_session_id =
        cookie_value(request.headers(), crate::auth::session::PARTNER_COOKIE_NAME)
            .map(str::to_string);
    let internal = request.headers().contains_key("x-internal-token");
    let actor =
        actor_from_request(auth_state, admin_session_id, partner_session_id, internal).await;
    let response = next.run(request).await;
    let status = response.status();
    if response_marks_success(&path, &response) {
        if let Err(error) = sqlx::query(
            r#"INSERT INTO dashboard_admin_audit_events
                (actor, method, path, status_code)
               VALUES ($1, $2, $3, $4)"#,
        )
        .bind(&actor)
        .bind(&method)
        .bind(&path)
        .bind(i32::from(status.as_u16()))
        .execute(&pool)
        .await
        {
            tracing::warn!(%error, %method, %path, "Admin-Audit konnte nicht gespeichert werden");
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::audit_admin_mutations;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        middleware::from_fn_with_state,
        response::Redirect,
        routing::{get, post},
        Router,
    };
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use tower::ServiceExt;

    async fn pool_or_skip(schema: &str) -> Option<sqlx::PgPool> {
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
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            r#"CREATE TABLE dashboard_admin_audit_events (
                id BIGSERIAL PRIMARY KEY,
                occurred_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                actor TEXT NOT NULL,
                method TEXT NOT NULL,
                path TEXT NOT NULL,
                status_code INTEGER NOT NULL
            )"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn nur_erfolgreiche_mutierende_admin_requests_werden_gespeichert() {
        let Some(pool) = pool_or_skip("admin_audit_middleware").await else {
            return;
        };
        let app = Router::new()
            .route(
                "/twitch/api/admin/test",
                get(|| async { "read" }).post(|| async { StatusCode::NO_CONTENT }),
            )
            .route(
                "/twitch/api/admin/failing-test",
                post(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
            )
            .route(
                "/twitch/api/v2/roadmap",
                post(|| async { StatusCode::CREATED }),
            )
            .route(
                "/twitch/verify",
                post(|| async { Redirect::to("/twitch/admin?ok=gespeichert") }),
            )
            .route(
                "/twitch/archive",
                post(|| async { Redirect::to("/twitch/admin?err=abgelehnt") }),
            )
            .route(
                "/twitch/admin/manual-plan",
                post(|| async { Redirect::to("/twitch/admin?ok=plan") }),
            )
            .route("/public-write", post(|| async { StatusCode::NO_CONTENT }))
            .layer(from_fn_with_state(pool.clone(), audit_admin_mutations));

        for (method, uri) in [
            ("GET", "/twitch/api/admin/test"),
            ("POST", "/public-write"),
            ("POST", "/twitch/api/admin/failing-test"),
            ("POST", "/twitch/api/admin/test"),
            ("POST", "/twitch/api/v2/roadmap"),
            ("POST", "/twitch/verify"),
            ("POST", "/twitch/archive"),
            ("POST", "/twitch/admin/manual-plan"),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            if uri == "/twitch/api/admin/failing-test" {
                assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
            } else if matches!(
                uri,
                "/twitch/verify" | "/twitch/archive" | "/twitch/admin/manual-plan"
            ) {
                assert!(response.status().is_redirection());
            } else {
                assert!(response.status().is_success());
            }
        }

        let rows: Vec<(String, String, String, i32)> = sqlx::query_as(
            "SELECT actor, method, path, status_code FROM dashboard_admin_audit_events ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(rows.len(), 4);
        assert_eq!(
            rows[0],
            (
                "admin".to_string(),
                "POST".to_string(),
                "/twitch/api/admin/test".to_string(),
                204,
            )
        );
        assert_eq!(rows[1].2, "/twitch/api/v2/roadmap");
        assert_eq!(rows[1].3, 201);
        assert_eq!(rows[2].2, "/twitch/verify");
        assert_eq!(rows[3].2, "/twitch/admin/manual-plan");
    }
}
