use axum::{
    extract::{Request, State},
    http::{header::LOCATION, request::Parts, HeaderMap, Method},
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
        || path.starts_with("/twitch/social-media/api/admin/")
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

async fn actor_from_request(parts: &mut Parts) -> String {
    use crate::auth::level::{AuthenticatedAdminSessionId, AuthenticatedPartnerSessionId};
    if let Some(state) = parts
        .extensions
        .get::<crate::auth::session::DashboardAuthState>()
    {
        if let Some(session_id) = parts.extensions.get::<AuthenticatedAdminSessionId>() {
            if let Ok(Some(user_id)) = state.load_admin_session_user_id(&session_id.0).await {
                return format!("discord:{user_id}");
            }
        }
        let partner_session_id = parts
            .extensions
            .get::<AuthenticatedPartnerSessionId>()
            .map(|session| session.0.as_str())
            .or_else(|| cookie_value(&parts.headers, crate::auth::session::PARTNER_COOKIE_NAME));
        if let Some(session_id) = partner_session_id {
            if let Ok(Some(session)) = state.load_partner_session(session_id).await {
                if !session.twitch_login.trim().is_empty() {
                    return session.twitch_login.trim().to_lowercase();
                }
            }
        }
    }
    if parts.headers.contains_key("x-internal-token") {
        "internal".to_string()
    } else {
        "admin".to_string()
    }
}

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
    let (mut parts, body) = request.into_parts();
    let selection = crate::auth::level::AuditSessionSelection::default();
    parts.extensions.insert(selection.clone());
    let fallback_actor = actor_from_request(&mut parts).await;
    let response = next.run(Request::from_parts(parts, body)).await;
    let status = response.status();
    if response_marks_success(&path, &response) {
        let actor = selection.0.lock().await.clone().unwrap_or(fallback_actor);
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
    use crate::auth::discord_admin_login::{
        DiscordAdminLoginConfig, DiscordAdminOAuthClient, DiscordAdminOAuthError,
        DiscordAdminSession, DiscordAuthorize, ValidatedAdminSession,
    };
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        middleware::{from_fn, from_fn_with_state},
        response::{IntoResponse, Redirect},
        routing::{get, post},
        Extension, Router,
    };
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use tower::ServiceExt;

    async fn pool_or_skip(schema: &str) -> Option<sqlx::PgPool> {
        let dsn = match std::env::var("TB_TEST_DATABASE_URL") {
            Ok(dsn) => dsn,
            Err(error) => {
                assert_ne!(
                    std::env::var("TB_TEST_REQUIRE_DB").as_deref(),
                    Ok("1"),
                    "TB_TEST_DATABASE_URL muss bei TB_TEST_REQUIRE_DB=1 gesetzt sein: {error}",
                );
                return None;
            }
        };
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE"
        )))
        .execute(&admin)
        .await
        .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
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

    #[derive(Default)]
    struct CentralSessionClient {
        recovery_calls: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
    }

    #[async_trait::async_trait]
    impl DiscordAdminOAuthClient for CentralSessionClient {
        async fn initiate(
            &self,
            _scope: &str,
            _redirect_after: &str,
            _requesting_service: &str,
            _metadata: serde_json::Value,
        ) -> Result<DiscordAuthorize, DiscordAdminOAuthError> {
            panic!("Audit startet keine Anmeldung");
        }

        async fn consume_result(
            &self,
            _state_id: &str,
        ) -> Result<DiscordAdminSession, DiscordAdminOAuthError> {
            panic!("Audit konsumiert keine Anmeldung");
        }

        async fn validate_session(
            &self,
            session_id: &str,
        ) -> Result<ValidatedAdminSession, DiscordAdminOAuthError> {
            let user_id = match session_id {
                "zentral-42-einzel" | "zentral-42-doppelt" | "zentral-42-getrennt" => 42,
                "zentral-99-doppelt" | "zentral-99-getrennt" => 99,
                "zentral-99-nach-ausfall" => {
                    if self
                        .recovery_calls
                        .as_ref()
                        .unwrap()
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                        == 0
                    {
                        return Err(DiscordAdminOAuthError);
                    }
                    99
                }
                _ => return Err(DiscordAdminOAuthError),
            };
            Ok(ValidatedAdminSession {
                user_id,
                username: "admin".into(),
                display_name: "Admin".into(),
                expires_at: 9_999_999_999.0,
            })
        }

        async fn import_session(
            &self,
            _session_id: &str,
            _session: &ValidatedAdminSession,
        ) -> Result<(), DiscordAdminOAuthError> {
            panic!("Audit überträgt keine Sitzung zum Broker");
        }

        async fn revoke_session(&self, _session_id: &str) -> Result<(), DiscordAdminOAuthError> {
            panic!("Audit widerruft keine Sitzung");
        }
    }

    #[tokio::test]
    async fn audit_ordnet_einzel_doppelte_und_getrennte_admin_cookies_zu() {
        use crate::auth::{
            level::{AuthenticatedAdminSessionId, DashboardAuthLevel},
            session::{DashboardAuthState, ADMIN_COOKIE_NAME},
        };

        let schema = crate::auth::session::test_schema_name("admin_audit_actor");
        let Some(pool) = pool_or_skip(&schema).await else {
            return;
        };
        sqlx::query(
            r#"CREATE TABLE dashboard_sessions (
                session_id TEXT PRIMARY KEY,
                session_type TEXT NOT NULL,
                payload_enc BYTEA NOT NULL,
                created_at DOUBLE PRECISION NOT NULL,
                expires_at DOUBLE PRECISION NOT NULL
            )"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        let state = DashboardAuthState::new(
            pool.clone(),
            "dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU=".to_string(),
        );
        state
            .import_central_admin_session("veraltet", "99", "Alt", "Alt", 0.0)
            .await
            .unwrap();
        let session = state
            .create_admin_session("42", "Audit Admin")
            .await
            .unwrap();
        let other_session = state
            .create_admin_session("99", "Zweiter Admin")
            .await
            .unwrap();
        let config = DiscordAdminLoginConfig {
            admin_base_url: "https://admin.test".into(),
            cookie_secure: true,
            cookie_domain: None,
            owner_user_id: None,
            moderator_role_id: 1,
            admin_role_ids: Vec::new(),
            admin_guild_ids: Vec::new(),
            client: std::sync::Arc::new(CentralSessionClient::default()),
        };
        let app = Router::new()
            .route(
                "/twitch/api/admin/test",
                post(
                    |auth: DashboardAuthLevel, request: Request<Body>| async move {
                        assert!(auth.is_privileged());
                        let selected = request
                            .extensions()
                            .get::<AuthenticatedAdminSessionId>()
                            .unwrap()
                            .clone();
                        let mut response = StatusCode::NO_CONTENT.into_response();
                        response.extensions_mut().insert(selected);
                        response
                    },
                ),
            )
            .layer(from_fn(crate::auth::csrf::csrf_protect))
            .layer(from_fn(crate::auth::require_admin_before_csrf))
            .layer(from_fn(crate::auth::level::promote_dashboard_admin_session))
            .layer(Extension(tb_http_core::ExpectedToken(
                "test-internal".into(),
            )))
            .layer(from_fn_with_state(pool.clone(), audit_admin_mutations))
            .layer(Extension(config))
            .layer(Extension(state.clone()));

        let valid_cookie = format!("{ADMIN_COOKIE_NAME}={}", session.session_id);
        let other_cookie = format!("{ADMIN_COOKIE_NAME}={}", other_session.session_id);
        let stale_cookie = format!("{ADMIN_COOKIE_NAME}=veraltet");
        let mut cases = vec![
            (
                vec![valid_cookie.clone()],
                "discord:42",
                session.session_id.clone(),
            ),
            (
                vec![format!("{stale_cookie}; {valid_cookie}")],
                "discord:42",
                session.session_id.clone(),
            ),
            (
                vec![stale_cookie.clone(), valid_cookie.clone()],
                "discord:42",
                session.session_id.clone(),
            ),
            (
                vec![format!("{other_cookie}; {valid_cookie}")],
                "discord:99",
                other_session.session_id.clone(),
            ),
        ];
        for (central_id, first_cookie, last_cookie, separated, actor) in [
            ("zentral-42-einzel", None, None, false, "discord:42"),
            (
                "zentral-42-doppelt",
                Some(&stale_cookie),
                None,
                false,
                "discord:42",
            ),
            (
                "zentral-42-getrennt",
                Some(&stale_cookie),
                None,
                true,
                "discord:42",
            ),
            (
                "zentral-99-doppelt",
                None,
                Some(&valid_cookie),
                false,
                "discord:99",
            ),
            (
                "zentral-99-getrennt",
                None,
                Some(&valid_cookie),
                true,
                "discord:99",
            ),
        ] {
            assert!(state
                .load_admin_session(central_id)
                .await
                .unwrap()
                .is_none());
            let mut cookies = Vec::new();
            if let Some(cookie) = first_cookie {
                cookies.push(cookie.clone());
            }
            cookies.push(format!("{ADMIN_COOKIE_NAME}={central_id}"));
            if let Some(cookie) = last_cookie {
                cookies.push(cookie.clone());
            }
            if !separated {
                cookies = vec![cookies.join("; ")];
            }
            cases.push((cookies, actor, central_id.to_string()));
        }
        let expected_actors: Vec<_> = cases.iter().map(|(_, actor, _)| *actor).collect();
        for (cookies, _, selected_id) in &cases {
            let mut request = Request::builder()
                .method("POST")
                .uri("/twitch/api/admin/test")
                .header("host", "admin.test")
                .header("origin", "https://admin.test")
                .header("x-dashboard-context", "admin");
            for cookie in cookies {
                request = request.header(axum::http::header::COOKIE, cookie);
            }
            let response = app
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NO_CONTENT);
            assert_eq!(
                &response
                    .extensions()
                    .get::<AuthenticatedAdminSessionId>()
                    .unwrap()
                    .0,
                selected_id,
            );
            assert!(state
                .load_admin_session(selected_id)
                .await
                .unwrap()
                .is_some());
        }
        for cookies in [vec![], vec![format!("{ADMIN_COOKIE_NAME}=unbekannt")]] {
            let mut request = Request::builder()
                .method("POST")
                .uri("/twitch/api/admin/test")
                .header("host", "admin.test")
                .header("x-dashboard-context", "admin");
            for cookie in cookies {
                request = request.header(axum::http::header::COOKIE, cookie);
            }
            let response = app
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }

        let actors: Vec<String> =
            sqlx::query_scalar("SELECT actor FROM dashboard_admin_audit_events ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(actors, expected_actors);
        for internal in [false, true] {
            for with_state in [false, true] {
                let mut request = Request::builder()
                    .header("x-dashboard-context", "admin")
                    .header("cookie", format!("{ADMIN_COOKIE_NAME}=unbekannt"));
                if with_state {
                    request = request.extension(state.clone());
                }
                if internal {
                    request = request.header("x-internal-token", "test-internal");
                }
                let (mut parts, _) = request.body(()).unwrap().into_parts();
                assert_eq!(
                    super::actor_from_request(&mut parts).await,
                    if internal { "internal" } else { "admin" },
                );
            }
        }
        let (mut parts, _) = Request::builder()
            .header("cookie", format!("{other_cookie}; {valid_cookie}"))
            .extension(state)
            .extension(AuthenticatedAdminSessionId(session.session_id))
            .body(())
            .unwrap()
            .into_parts();
        assert_eq!(super::actor_from_request(&mut parts).await, "discord:42");
    }

    #[tokio::test]
    async fn audit_behaelt_tatsaechliche_auswahl_bei_broker_erholung() {
        use crate::auth::{
            level::{AuthenticatedAdminSessionId, DashboardAuthLevel},
            session::{DashboardAuthState, ADMIN_COOKIE_NAME},
        };
        use std::sync::{atomic::Ordering, Arc};

        for separated in [false, true] {
            let schema = crate::auth::session::test_schema_name("admin_audit_recovery");
            let Some(pool) = pool_or_skip(&schema).await else {
                return;
            };
            sqlx::query(
                r#"CREATE TABLE dashboard_sessions (
                    session_id TEXT PRIMARY KEY,
                    session_type TEXT NOT NULL,
                    payload_enc BYTEA NOT NULL,
                    created_at DOUBLE PRECISION NOT NULL,
                    expires_at DOUBLE PRECISION NOT NULL
                )"#,
            )
            .execute(&pool)
            .await
            .unwrap();
            let state = DashboardAuthState::new(
                pool.clone(),
                "dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU=".to_string(),
            );
            let local = state
                .create_admin_session("42", "Audit Admin")
                .await
                .unwrap();
            let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let config = DiscordAdminLoginConfig {
                admin_base_url: "https://admin.test".into(),
                cookie_secure: true,
                cookie_domain: None,
                owner_user_id: None,
                moderator_role_id: 1,
                admin_role_ids: Vec::new(),
                admin_guild_ids: Vec::new(),
                client: Arc::new(CentralSessionClient {
                    recovery_calls: Some(calls.clone()),
                }),
            };
            let app = Router::new()
                .route(
                    "/twitch/api/admin/test",
                    post(
                        |auth: DashboardAuthLevel, request: Request<Body>| async move {
                            assert!(auth.is_privileged());
                            let selected = request
                                .extensions()
                                .get::<AuthenticatedAdminSessionId>()
                                .unwrap()
                                .clone();
                            if request.headers().contains_key("x-test-logout") {
                                request
                                    .extensions()
                                    .get::<DashboardAuthState>()
                                    .unwrap()
                                    .invalidate_session(&selected.0)
                                    .await;
                            }
                            let mut response = StatusCode::NO_CONTENT.into_response();
                            response.extensions_mut().insert(selected);
                            response
                        },
                    ),
                )
                .layer(from_fn(crate::auth::csrf::csrf_protect))
                .layer(from_fn(crate::auth::require_admin_before_csrf))
                .layer(from_fn(crate::auth::level::promote_dashboard_admin_session))
                .layer(Extension(tb_http_core::ExpectedToken(
                    "test-internal".into(),
                )))
                .layer(from_fn_with_state(pool.clone(), audit_admin_mutations))
                .layer(Extension(config))
                .layer(Extension(state.clone()));
            let central_id = "zentral-99-nach-ausfall";
            let cookies = [
                format!("{ADMIN_COOKIE_NAME}={central_id}"),
                format!("{ADMIN_COOKIE_NAME}={}", local.session_id),
            ];
            for (index, expected_id) in [local.session_id.as_str(), central_id]
                .into_iter()
                .enumerate()
            {
                let mut request = Request::builder()
                    .method("POST")
                    .uri("/twitch/api/admin/test")
                    .header("host", "admin.test")
                    .header("origin", "https://admin.test")
                    .header("x-dashboard-context", "admin");
                if separated {
                    for cookie in &cookies {
                        request = request.header(axum::http::header::COOKIE, cookie);
                    }
                } else {
                    request = request.header(axum::http::header::COOKIE, cookies.join("; "));
                }
                let response = app
                    .clone()
                    .oneshot(request.body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::NO_CONTENT);
                assert_eq!(
                    response
                        .extensions()
                        .get::<AuthenticatedAdminSessionId>()
                        .unwrap()
                        .0,
                    expected_id,
                );
                assert_eq!(calls.load(Ordering::SeqCst), index + 1);
                assert_eq!(
                    state
                        .load_admin_session(central_id)
                        .await
                        .unwrap()
                        .is_some(),
                    index == 1,
                );
                let actors: Vec<String> = sqlx::query_scalar(
                    "SELECT actor FROM dashboard_admin_audit_events ORDER BY id",
                )
                .fetch_all(&pool)
                .await
                .unwrap();
                assert_eq!(actors, ["discord:42", "discord:99"][..=index]);
            }
            sqlx::query(
                "CREATE TABLE twitch_partners (
                    id BIGINT PRIMARY KEY, twitch_login TEXT NOT NULL,
                    twitch_user_id TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'active',
                    technical_pause_reason TEXT, admin_archived_at TEXT,
                    departnered_at TEXT, partnered_at TEXT DEFAULT CURRENT_TIMESTAMP
                )",
            )
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO twitch_partners (id, twitch_login, twitch_user_id)
                 VALUES (777, 'fremderpartner', '777')",
            )
            .execute(&pool)
            .await
            .unwrap();
            let foreign = state
                .create_partner_session("fremderpartner", "777", "Fremder Partner")
                .await
                .unwrap();
            assert!(state
                .load_partner_session(&foreign.session_id)
                .await
                .unwrap()
                .is_some());
            let response = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/twitch/api/admin/test")
                        .header("host", "admin.test")
                        .header("origin", "https://admin.test")
                        .header("x-dashboard-context", "admin")
                        .header("x-test-logout", "1")
                        .header(
                            "cookie",
                            format!(
                                "{ADMIN_COOKIE_NAME}={central_id}; {}={}",
                                crate::auth::session::PARTNER_COOKIE_NAME,
                                foreign.session_id,
                            ),
                        )
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NO_CONTENT);
            assert!(state
                .load_admin_session(central_id)
                .await
                .unwrap()
                .is_none());
            let actors: Vec<String> =
                sqlx::query_scalar("SELECT actor FROM dashboard_admin_audit_events ORDER BY id")
                    .fetch_all(&pool)
                    .await
                    .unwrap();
            assert_eq!(actors, ["discord:42", "discord:99", "discord:99"]);
            pool.close().await;
        }
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
