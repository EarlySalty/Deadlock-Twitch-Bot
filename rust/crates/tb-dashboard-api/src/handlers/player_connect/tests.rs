use super::*;
use crate::auth::{
    oauth_login::TwitchOAuthClient,
    session::{OAuthLoginState, SessionCreation},
};
use crate::handlers::auth_login::{callback_handler, CallbackQuery};
use axum::{
    body::{to_bytes, Body},
    extract::Query,
    http::{header::COOKIE, Request},
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tower::ServiceExt;
use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};

struct FakeOAuth(Arc<AtomicUsize>);
#[async_trait::async_trait]
impl TwitchOAuthClient for FakeOAuth {
    async fn exchange_code_for_identity(
        &self,
        _: &str,
        _: &str,
    ) -> Result<TwitchIdentity, tb_transport_twitch::user_token::UserTokenError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(TwitchIdentity {
            twitch_user_id: "111".into(),
            twitch_login: "viewer".into(),
            display_name: "Viewer".into(),
            email: String::new(),
        })
    }
}
fn config() -> OAuthLoginConfig {
    OAuthLoginConfig {
        client_id: "test-client".into(),
        redirect_uri: "https://example.test/twitch/auth/callback".into(),
        cookie_secure: true,
        client: Arc::new(FakeOAuth(Arc::new(AtomicUsize::new(0)))),
        raid_callback: None,
    }
}
async fn fixture() -> (
    crate::test_postgres::TestPostgres,
    DashboardAuthState,
    SessionCreation,
) {
    let db = crate::test_postgres::TestPostgres::start().await;
    sqlx::raw_sql("CREATE TABLE dashboard_sessions (session_id TEXT PRIMARY KEY, session_type TEXT NOT NULL, payload_enc BYTEA NOT NULL, created_at DOUBLE PRECISION NOT NULL, expires_at DOUBLE PRECISION NOT NULL);")
        .execute(&db.pool).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../../../../migrations/20260918100000_twitch_player_steam_links.sql"
    ))
    .execute(&db.pool)
    .await
    .unwrap();
    let state = DashboardAuthState::new(
        db.pool.clone(),
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
    );
    let created = state.create_player_session("111", "viewer").await.unwrap();
    (db, state, created)
}
fn headers(created: &SessionCreation) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        COOKIE,
        HeaderValue::from_str(&format!("{PLAYER_COOKIE_NAME}={}", created.session_id)).unwrap(),
    );
    headers.insert("origin", HeaderValue::from_static("https://example.test"));
    headers
}
fn form(created: &SessionCreation) -> Form<ConnectForm> {
    Form(ConnectForm {
        csrf_token: created.csrf_token.clone(),
    })
}
async fn text(response: Response) -> String {
    String::from_utf8(
        to_bytes(response.into_body(), 100_000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}
async fn start(state: &DashboardAuthState, created: &SessionCreation) -> (String, String) {
    let response = steam_start(
        Some(Extension(state.clone())),
        Some(Extension(config())),
        headers(created),
        form(created),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let url = url::Url::parse(response.headers()[LOCATION].to_str().unwrap()).unwrap();
    assert_eq!(url.host_str(), Some("steamcommunity.com"));
    let callback = url
        .query_pairs()
        .find(|(key, _)| key == "openid.return_to")
        .unwrap()
        .1
        .to_string();
    let parsed = url::Url::parse(&callback).unwrap();
    let token = parsed
        .query_pairs()
        .find(|(key, _)| key == "state")
        .unwrap()
        .1
        .to_string();
    (token, callback)
}
fn assertion(token: &str, callback: &str) -> RawQuery {
    let nonce = format!(
        "{}{}",
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
        token
    );
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query.extend_pairs([
        ("state", token),
        ("openid.ns", "http://specs.openid.net/auth/2.0"),
        ("openid.mode", "id_res"),
        (
            "openid.op_endpoint",
            "https://steamcommunity.com/openid/login",
        ),
        (
            "openid.claimed_id",
            "https://steamcommunity.com/openid/id/76561197960265770",
        ),
        (
            "openid.identity",
            "https://steamcommunity.com/openid/id/76561197960265770",
        ),
        ("openid.return_to", callback),
        ("openid.response_nonce", &nonce),
        ("openid.sig", "test-signature"),
        ("openid.assoc_handle", "test-handle"),
        (
            "openid.signed",
            "op_endpoint,claimed_id,identity,return_to,response_nonce,assoc_handle",
        ),
    ]);
    RawQuery(Some(query.finish()))
}
async fn verifier(valid: bool) -> (MockServer, SteamOpenIdClient) {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            "ns:http://specs.openid.net/auth/2.0\nis_valid:{valid}\n"
        )))
        .mount(&server)
        .await;
    let client = SteamOpenIdClient::at(server.uri());
    (server, client)
}

#[tokio::test]
async fn player_connect_public_page_is_accessible_without_partner_and_never_cached() {
    let (_db, state, _) = fixture().await;
    let response = router()
        .layer(Extension(state))
        .oneshot(
            Request::builder()
                .uri(CONNECT_PATH)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(response.headers()["referrer-policy"], "no-referrer");
    assert!(response.headers()["content-security-policy"]
        .to_str()
        .unwrap()
        .contains("font-src 'self'"));
    let body = text(response).await;
    assert!(body.contains("Mit Twitch anmelden"));
    assert!(body.contains("flow-steps"));
    assert!(body.contains("Sora"));
    assert!(body.contains("Twitch-Konto bestätigen."));
}
#[tokio::test]
async fn player_connect_twitch_login_uses_existing_callback_without_raid_scopes() {
    let (_db, state, _) = fixture().await;
    let response = twitch_login(Some(Extension(state)), Some(Extension(config()))).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let url = url::Url::parse(response.headers()[LOCATION].to_str().unwrap()).unwrap();
    assert_eq!(url.host_str(), Some("id.twitch.tv"));
    let q = url
        .query_pairs()
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(q["redirect_uri"], config().redirect_uri);
    assert_eq!(q["force_verify"], "true");
    assert!(!q
        .get("scope")
        .is_some_and(|scope| scope.contains("manage") || scope.contains("chat:")));
}
#[tokio::test]
async fn player_connect_callback_creates_only_viewer_session_for_non_partner() {
    let (_db, state, _) = fixture().await;
    state
        .save_oauth_login_state(
            "oauth-state",
            &OAuthLoginState {
                next_path: CONNECT_PATH.into(),
                redirect_uri: config().redirect_uri,
                context_token: "browser-context".into(),
            },
        )
        .await
        .unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        COOKIE,
        HeaderValue::from_static("twitch_dash_session_oauth_ctx=browser-context"),
    );
    let response = callback_handler(
        Some(Extension(state.clone())),
        Some(Extension(config())),
        headers,
        Query(CallbackQuery {
            code: Some("test-code".into()),
            state: Some("oauth-state".into()),
            error: None,
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()[LOCATION], CONNECT_PATH);
    let cookies = response
        .headers()
        .get_all(SET_COOKIE)
        .iter()
        .map(|c| c.to_str().unwrap())
        .collect::<Vec<_>>();
    assert!(!cookies
        .iter()
        .any(|c| c.starts_with("twitch_dash_session=")));
    let player = cookies
        .iter()
        .find(|c| c.starts_with("twitch_player_session="))
        .unwrap();
    assert!(
        player.contains("Secure") && player.contains("HttpOnly") && player.contains("SameSite=Lax")
    );
    let token = player.split(';').next().unwrap().split_once('=').unwrap().1;
    assert_eq!(
        state
            .load_player_session(token)
            .await
            .unwrap()
            .unwrap()
            .twitch_user_id,
        "111"
    );
    assert!(state.load_partner_session(token).await.unwrap().is_none());
    assert!(state.load_admin_session(token).await.unwrap().is_none());
}
#[tokio::test]
async fn player_connect_twitch_callback_requires_browser_context_before_exchange() {
    let (_db, state, _) = fixture().await;
    state
        .save_oauth_login_state(
            "oauth-state",
            &OAuthLoginState {
                next_path: CONNECT_PATH.into(),
                redirect_uri: config().redirect_uri,
                context_token: "browser-context".into(),
            },
        )
        .await
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut config = config();
    config.client = Arc::new(FakeOAuth(calls.clone()));
    let response = callback_handler(
        Some(Extension(state)),
        Some(Extension(config)),
        HeaderMap::new(),
        Query(CallbackQuery {
            code: Some("test-code".into()),
            state: Some("oauth-state".into()),
            error: None,
        }),
    )
    .await;
    assert!(response.status().is_client_error());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn player_connect_steam_start_requires_login_csrf_and_same_origin() {
    let (_db, state, created) = fixture().await;
    let response = steam_start(
        Some(Extension(state.clone())),
        Some(Extension(config())),
        HeaderMap::new(),
        form(&created),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = steam_start(
        Some(Extension(state.clone())),
        Some(Extension(config())),
        headers(&created),
        Form(ConnectForm {
            csrf_token: "wrong".into(),
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let mut foreign = headers(&created);
    foreign.insert("origin", HeaderValue::from_static("https://evil.test"));
    let response = steam_start(
        Some(Extension(state.clone())),
        Some(Extension(config())),
        foreign,
        form(&created),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(player_links::load(state.pool(), "111")
        .await
        .unwrap()
        .is_none());
}
#[tokio::test]
async fn player_connect_steam_start_accepts_public_connect_origin() {
    let (_db, state, created) = fixture().await;
    let mut apex = headers(&created);
    apex.insert(
        "origin",
        HeaderValue::from_static("https://deutsche-deadlock-community.de"),
    );
    let response = steam_start(
        Some(Extension(state.clone())),
        Some(Extension(config())),
        apex,
        form(&created),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn player_connect_steam_start_trusts_same_site_fetch_metadata_with_valid_csrf() {
    let (_db, state, created) = fixture().await;
    let mut proxied = headers(&created);
    proxied.insert(
        "origin",
        HeaderValue::from_static("https://www.deutsche-deadlock-community.de"),
    );
    proxied.insert("sec-fetch-site", HeaderValue::from_static("same-site"));
    let response = steam_start(
        Some(Extension(state.clone())),
        Some(Extension(config())),
        proxied,
        form(&created),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn player_connect_steam_start_rejects_cross_site_fetch_metadata() {
    let (_db, state, created) = fixture().await;
    let mut cross_site = headers(&created);
    cross_site.insert("sec-fetch-site", HeaderValue::from_static("cross-site"));
    let response = steam_start(
        Some(Extension(state.clone())),
        Some(Extension(config())),
        cross_site,
        form(&created),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn player_connect_steam_verified_roundtrip_and_replay_rejection() {
    let (_db, state, created) = fixture().await;
    let (token, callback) = start(&state, &created).await;
    assert_eq!(
        player_links::load(state.pool(), "111")
            .await
            .unwrap()
            .unwrap()
            .steam_id64,
        None
    );
    let (_server, client) = verifier(true).await;
    let response = steam_callback(
        Some(Extension(state.clone())),
        Some(Extension(client.clone())),
        headers(&created),
        assertion(&token, &callback),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        player_links::load(state.pool(), "111")
            .await
            .unwrap()
            .unwrap()
            .account_id(),
        Some(42)
    );
    let response = steam_callback(
        Some(Extension(state)),
        Some(Extension(client)),
        headers(&created),
        assertion(&token, &callback),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
#[tokio::test]
async fn player_connect_steam_rejects_cross_browser_and_failed_verification() {
    let (_db, state, created) = fixture().await;
    let (token, callback) = start(&state, &created).await;
    let other = state.create_player_session("222", "other").await.unwrap();
    let (_server, client) = verifier(true).await;
    let response = steam_callback(
        Some(Extension(state.clone())),
        Some(Extension(client)),
        headers(&other),
        assertion(&token, &callback),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let (token, callback) = start(&state, &created).await;
    let (_server, client) = verifier(false).await;
    let response = steam_callback(
        Some(Extension(state.clone())),
        Some(Extension(client)),
        headers(&created),
        assertion(&token, &callback),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        player_links::load(state.pool(), "111")
            .await
            .unwrap()
            .unwrap()
            .steam_id64,
        None
    );
}
#[tokio::test]
async fn player_connect_disconnect_invalidates_pending_steam_and_removes_id() {
    let (_db, state, created) = fixture().await;
    let (token, callback) = start(&state, &created).await;
    let response = unlink(
        Some(Extension(state.clone())),
        Some(Extension(config())),
        headers(&created),
        form(&created),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let (_server, client) = verifier(true).await;
    let response = steam_callback(
        Some(Extension(state.clone())),
        Some(Extension(client)),
        headers(&created),
        assertion(&token, &callback),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let link = player_links::load(state.pool(), "111")
        .await
        .unwrap()
        .unwrap();
    assert!(!link.lookup_enabled);
    assert_eq!(link.steam_id64, None);
}
#[tokio::test]
async fn player_connect_unlink_get_and_foreign_csrf_cannot_mutate_link() {
    let (_db, state, created) = fixture().await;
    let response = router()
        .layer(Extension(state.clone()))
        .layer(Extension(config()))
        .oneshot(
            Request::builder()
                .uri("/twitch/connect/unlink")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    let response = unlink(
        Some(Extension(state.clone())),
        Some(Extension(config())),
        headers(&created),
        Form(ConnectForm {
            csrf_token: "wrong".into(),
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(player_links::load(state.pool(), "111")
        .await
        .unwrap()
        .is_none());
}
#[tokio::test]
async fn player_connect_expired_session_and_flow_cannot_link() {
    let (_db, state, created) = fixture().await;
    let (token, callback) = start(&state, &created).await;
    sqlx::query("UPDATE dashboard_sessions SET expires_at = 0 WHERE session_type = 'twitch_player_steam_flow'").execute(state.pool()).await.unwrap();
    let response = steam_callback(
        Some(Extension(state.clone())),
        None,
        headers(&created),
        assertion(&token, &callback),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    sqlx::query("UPDATE dashboard_sessions SET expires_at = 0")
        .execute(state.pool())
        .await
        .unwrap();
    let response = steam_start(
        Some(Extension(state)),
        Some(Extension(config())),
        headers(&created),
        form(&created),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
#[test]
fn player_connect_duplicate_cookies_rejected_and_html_escaped() {
    let mut headers = HeaderMap::new();
    headers.insert(
        COOKIE,
        HeaderValue::from_static("twitch_player_session=a; twitch_player_session=b"),
    );
    assert!(cookie(&headers, PLAYER_COOKIE_NAME).is_none());
    assert_eq!(escape("<script>\"'&"), "&lt;script&gt;&quot;&#39;&amp;");
}
