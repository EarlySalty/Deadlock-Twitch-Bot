use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, Request, StatusCode},
};
use sqlx::postgres::PgPoolOptions;
use tb_dashboard_api::{build_router, handlers::spa, DashboardAuthLevel};
use tower::ServiceExt;

#[tokio::test]
async fn manager_routes_keep_login_aliases_and_reserved_handlers_separate() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/none")
        .unwrap();
    let app = build_router(pool.clone(), "route-test".into());
    for path in [
        "/social-media",
        "/social-media/",
        "/social-media/clips?oauth_success=youtube&twitch_user_id=42",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header(header::HOST, "deutsche-deadlock-community.de")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "{path}");
        let encoded: String = url::form_urlencoded::byte_serialize(path.as_bytes()).collect();
        assert_eq!(
            response.headers()[header::LOCATION],
            format!("/twitch/auth/login?next={encoded}")
        );
    }
    for (method, path, target) in [
        ("GET", "/social-media-admin", "/social-media"),
        ("GET", "/social-media-admin/", "/social-media/"),
        ("GET", "/social-media-admin/xyz", "/social-media/xyz"),
        (
            "POST",
            "/social-media-admin/clips/a%2Fb?tab=konten",
            "/social-media/clips/a%2Fb?tab=konten",
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(header::HOST, "deutsche-deadlock-community.de")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PERMANENT_REDIRECT, "{path}");
        assert_eq!(response.headers()[header::LOCATION], target);
    }
    for path in ["/social-media/terms", "/social-media/privacy"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header(header::HOST, "deutsche-deadlock-community.de")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert!(response.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/html"));
        assert!(response.headers().get(header::LOCATION).is_none());
    }
    for path in [
        "/social-media/api/stats",
        "/social-media/oauth/start/youtube",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header(header::HOST, "deutsche-deadlock-community.de")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
    for path in ["/social-media/api/missing", "/social-media/oauth/missing"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header(header::HOST, "deutsche-deadlock-community.de")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
    for path in ["/social-media", "/social-media/clips?oauth_success=youtube"] {
        let response = spa::social_media_manager_handler(
            HeaderMap::new(),
            path.parse().unwrap(),
            DashboardAuthLevel::admin(),
            State(pool.clone()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert!(response.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/html"));
        let bytes = axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap();
        let html = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(html.contains("__TWITCH_DASHBOARD_RUNTIME__"));
        assert!(html.contains("/twitch/dashboard-v2/assets/"));
    }
}
