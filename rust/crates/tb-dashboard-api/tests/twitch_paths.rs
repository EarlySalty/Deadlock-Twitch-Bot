use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
    Router,
};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

fn app() -> Router {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(std::time::Duration::from_millis(50))
        .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/none")
        .unwrap();
    tb_dashboard_api::build_router(pool, "test-only".into())
}

async fn request(app: &Router, method: &str, path: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header(header::HOST, "deutsche-deadlock-community.de")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn legacy_pages_redirect_once_with_suffix_query_and_method_preserved() {
    let app = app();
    for (source, target) in [
        ("/social-media", "/twitch/social-media"),
        ("/social-media/", "/twitch/social-media/"),
        ("/social-media-admin", "/twitch/social-media"),
        ("/social-media-admin/", "/twitch/social-media/"),
        (
            "/social-media-admin/archiv?streamer=abc",
            "/twitch/social-media/archiv?streamer=abc",
        ),
        (
            "/social-media/clips/a%2Fb?view=archiv&x=a%2Fb",
            "/twitch/social-media/clips/a%2Fb?view=archiv&x=a%2Fb",
        ),
        ("/analyse", "/twitch/analyse"),
        ("/analyse/", "/twitch/analyse/"),
        ("/analyse/a%2Fb?x=%2F", "/twitch/analyse/a%2Fb?x=%2F"),
        (
            "/analyse/assets/app.js?x=1",
            "/twitch/analyse/assets/app.js?x=1",
        ),
    ] {
        for method in ["GET", "HEAD", "POST"] {
            let response = request(&app, method, source).await;
            assert_eq!(
                response.status(),
                StatusCode::PERMANENT_REDIRECT,
                "{method} {source}"
            );
            assert_eq!(response.headers()[header::LOCATION], target, "{source}");
            if source.starts_with("/analyse") {
                assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
                assert_eq!(response.headers()["clear-site-data"], "\"cache\"");
            }
        }
    }
}

#[tokio::test]
async fn canonical_pages_use_their_own_login_destination() {
    let app = app();
    for (source, target) in [
        (
            "/twitch/analyse",
            "/twitch/auth/login?next=%2Ftwitch%2Fanalyse",
        ),
        (
            "/twitch/social-media",
            "/twitch/auth/login?next=%2Ftwitch%2Fsocial-media",
        ),
        (
            "/twitch/social-media/archiv?view=archiv",
            "/twitch/auth/login?next=%2Ftwitch%2Fsocial-media%2Farchiv%3Fview%3Darchiv",
        ),
    ] {
        let response = request(&app, "GET", source).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "{source}");
        assert_eq!(response.headers()[header::LOCATION], target, "{source}");
        let next = url::form_urlencoded::parse(target.split_once('?').unwrap().1.as_bytes())
            .find(|(key, _)| key == "next")
            .unwrap()
            .1
            .into_owned();
        assert_eq!(
            tb_dashboard_api::auth::oauth_login::sanitize_next_path(Some(&next)),
            next
        );
    }
}

#[tokio::test]
async fn legal_pages_are_directly_served_on_old_and_new_addresses() {
    let app = app();
    for suffix in ["terms", "privacy"] {
        let old = request(&app, "GET", &format!("/social-media/{suffix}")).await;
        let new = request(&app, "GET", &format!("/twitch/social-media/{suffix}")).await;
        assert_eq!(old.status(), StatusCode::OK);
        assert_eq!(new.status(), StatusCode::OK);
        assert!(old.headers().get(header::LOCATION).is_none());
        assert!(new.headers().get(header::LOCATION).is_none());
        assert!(old.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .contains("text/html"));
        assert_eq!(
            to_bytes(old.into_body(), 128 * 1024).await.unwrap(),
            to_bytes(new.into_body(), 128 * 1024).await.unwrap()
        );
    }
    let response = request(&app, "GET", "/privacy").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get(header::LOCATION).is_none());
}

#[tokio::test]
async fn platform_callbacks_are_served_without_a_path_redirect() {
    let app = app();
    for prefix in ["/social-media", "/twitch/social-media"] {
        for suffix in ["", "/tiktok", "/youtube"] {
            let response = request(&app, "GET", &format!("{prefix}/oauth/callback{suffix}")).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert!(response.headers().get(header::LOCATION).is_none());
            assert_eq!(
                to_bytes(response.into_body(), 1024).await.unwrap(),
                "Missing code or state"
            );
        }
    }
}

#[tokio::test]
async fn old_tabs_and_new_clients_share_api_auth_and_csrf_guards() {
    let app = app();
    for suffix in [
        "/api/access/me",
        "/api/stats",
        "/api/admin/clips",
        "/api/vod-archive",
    ] {
        let old = request(&app, "GET", &format!("/social-media{suffix}")).await;
        let new = request(&app, "GET", &format!("/twitch/social-media{suffix}")).await;
        assert_eq!(old.status(), StatusCode::UNAUTHORIZED, "{suffix}");
        assert_eq!(new.status(), old.status(), "{suffix}");
        assert!(old.headers().get(header::LOCATION).is_none());
        assert!(new.headers().get(header::LOCATION).is_none());
    }
    for prefix in ["/social-media", "/twitch/social-media"] {
        let response = request(&app, "POST", &format!("{prefix}/api/vod-archive")).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let data: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
        assert_eq!(data["error"], "invalid_csrf");
    }
}
