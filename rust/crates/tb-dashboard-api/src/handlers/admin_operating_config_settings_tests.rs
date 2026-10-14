use super::*;
use axum::{
    body::{to_bytes, Body},
    http::Request,
    Extension, Router,
};
use std::fs;
use tb_config::settings_catalog::{self, Kind, CATALOG_VERSION};
use tb_llm::social_editor::fixture::SOCIAL_AI_SECTION;
use tempfile::TempDir;
use tower::ServiceExt;

const BASE: &str = r#"schema_version=1
[twitch]
bot_user_id="1"
notify_channel_id="2"
eventsub_callback_url="https://example.invalid/callback"
[bot]
chat_enabled=false
[bot.brain_client]
mode="legacy" # Ausgangsmodus
endpoint="http://127.0.0.1:8789"
public_scopes=["bot.public"]
timeout_ms=8000
[database]
pool_max=17 # Verbindungen
"#;

fn social_section() -> toml::Table {
    SOCIAL_AI_SECTION.parse().expect("social_ai fixture")
}

fn full_social_section() -> toml::Table {
    let mut section = social_section();
    let runtime = section
        .get_mut("runtime")
        .and_then(toml::Value::as_table_mut)
        .expect("runtime table");
    let brain: toml::Table = r#"endpoint = "http://127.0.0.1:8789/v1/retrieve"
expected_release = "rel-2026.10.14"
deadline_ms = 2000
max_response_bytes = 65536
max_evidence_count = 4
max_evidence_bytes = 2048
allowed_game_terms = ["Haze", "Infernus"]
"#
    .parse()
    .expect("brain fixture");
    let revisions: toml::Table = r#"max_feedback_bytes = 2000
max_revisions = 10
max_auto_changed_percent = 10
max_variants = 3
"#
    .parse()
    .expect("revisions fixture");
    runtime.insert("brain".into(), toml::Value::Table(brain));
    runtime.insert("revisions".into(), toml::Value::Table(revisions));
    section
}

fn config_text(section: &toml::Table) -> String {
    let mut wrapper = toml::Table::new();
    wrapper.insert("social_ai".into(), toml::Value::Table(section.clone()));
    format!(
        "{BASE}\n{}",
        toml::to_string(&wrapper).expect("social_ai text")
    )
}

struct Fixture {
    directory: TempDir,
    original: String,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("temp dir");
        let original = config_text(&social_section());
        fs::write(directory.path().join("bot.toml"), &original).expect("bot.toml");
        Self {
            directory,
            original,
        }
    }

    fn path(&self) -> PathBuf {
        self.directory.path().join("bot.toml")
    }

    fn backup(&self) -> PathBuf {
        self.directory.path().join(".bot.toml.previous")
    }

    fn text(&self) -> String {
        fs::read_to_string(self.path()).expect("saved text")
    }

    async fn catalog(&self) -> Value {
        json_body(read_settings(self.path()).await.expect("catalog")).await
    }

    async fn put(&self, changes: Value) -> Response {
        let revision = self.catalog().await["revision"].clone();
        self.put_at(revision, changes).await
    }

    async fn put_at(&self, revision: Value, changes: Value) -> Response {
        let body = json!({ "revision": revision, "changes": changes }).to_string();
        write_settings(self.path(), Bytes::from(body))
            .await
            .into_response()
    }
}

async fn json_body(response: Response) -> Value {
    serde_json::from_slice(
        &to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .expect("body"),
    )
    .expect("json body")
}

async fn assert_validation(response: Response, field: &str) -> Value {
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = json_body(response).await;
    assert_eq!(body["error"], "validation");
    assert_eq!(body["field"], field);
    assert_eq!(body["path"], field);
    assert!(body["message"]
        .as_str()
        .is_some_and(|text| !text.is_empty()));
    body
}

#[tokio::test]
async fn get_liefert_katalog_mit_ganzzahlen_als_text() {
    let fixture = Fixture::new();
    let response = read_settings(fixture.path()).await.expect("catalog");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(CACHE_CONTROL).expect("cache header"),
        "no-store"
    );
    let body = json_body(response).await;
    assert_eq!(body["version"], CATALOG_VERSION);
    let revision = body["revision"].as_str().expect("revision");
    assert!(revision.len() == 64 && revision.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_eq!(
        body["groups"].as_array().map(Vec::len),
        Some(settings_catalog::groups().len())
    );
    assert_eq!(
        body["fields"].as_array().map(Vec::len),
        Some(settings_catalog::fields().len())
    );
    assert_eq!(body["values"]["bot.brain_client.timeout_ms"], "8000");
    assert!(body["values"].get("database.pool_max").is_none());
    assert_eq!(
        body["values"]["social_ai.provider.model"],
        "claude-opus-5-5"
    );
    assert_eq!(
        body["values"]["social_ai.provider.pricing.input_micro_usd_per_million"],
        "4000000"
    );
    assert!(body["values"]["social_ai.runtime.brain.endpoint"].is_null());
}

#[tokio::test]
async fn put_speichert_mit_kommentaren_und_sicherung() {
    let fixture = Fixture::new();
    let before = fixture.catalog().await;
    let response = fixture
        .put(json!([
            {"path": "bot.brain_client.timeout_ms", "value": "9000"},
            {"path": "social_ai.runtime.list_limit", "value": "40"}
        ]))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(CACHE_CONTROL).expect("cache header"),
        "no-store"
    );
    let saved = json_body(response).await;
    assert_ne!(saved["revision"], before["revision"]);
    assert_eq!(saved["values"]["bot.brain_client.timeout_ms"], "9000");
    assert_eq!(saved["values"]["social_ai.runtime.list_limit"], "40");
    assert_eq!(saved, fixture.catalog().await);
    let text = fixture.text();
    assert!(text.contains("mode=\"legacy\" # Ausgangsmodus"));
    assert!(text.contains("pool_max=17 # Verbindungen"));
    assert_eq!(
        fs::read_to_string(fixture.backup()).expect("backup"),
        fixture.original
    );
}

#[tokio::test]
async fn veralteter_stand_liefert_409() {
    let fixture = Fixture::new();
    let response = fixture
        .put_at(
            json!("0".repeat(64)),
            json!([{"path": "social_ai.runtime.list_limit", "value": "40"}]),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(response).await["error"], "stale_revision");
    assert_eq!(fixture.text(), fixture.original);
    assert!(!fixture.backup().exists());
}

#[tokio::test]
async fn ungueltiger_social_ai_kandidat_liefert_422_ohne_schreiben() {
    let fixture = Fixture::new();
    assert_validation(
        fixture
            .put(json!([{
                "path": "social_ai.provider.pricing.input_micro_usd_per_million",
                "value": "4000001"
            }]))
            .await,
        "social_ai",
    )
    .await;
    assert_validation(
        fixture
            .put(json!([{"path": "social_ai.runtime.resources.max_frames", "value": "8"}]))
            .await,
        "social_ai",
    )
    .await;
    assert_validation(
        fixture
            .put(json!([{"path": "social_ai.runtime.list_limit", "value": "51"}]))
            .await,
        "social_ai.runtime.list_limit",
    )
    .await;
    assert_validation(
        fixture
            .put(json!([{"path": "database.url", "value": "postgres://x"}]))
            .await,
        "settings_catalog",
    )
    .await;
    assert_eq!(fixture.text(), fixture.original);
    assert!(!fixture.backup().exists());
}

#[tokio::test]
async fn kaputte_anfrage_liefert_400() {
    let fixture = Fixture::new();
    let response = write_settings(
        fixture.path(),
        Bytes::from(json!({"revision": "a", "changes": [], "extra": 1}).to_string()),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "invalid_request");
}

#[tokio::test]
async fn nicht_admins_ohne_dateizugriff_abgewiesen() {
    let partner = || DashboardAuthLevel::Partner {
        twitch_login: "fixture".into(),
        twitch_user_id: "101".into(),
        display_name: "Fixture".into(),
    };
    for (auth, status) in [
        (DashboardAuthLevel::None, StatusCode::UNAUTHORIZED),
        (partner(), StatusCode::FORBIDDEN),
    ] {
        assert_eq!(
            settings_get_handler(auth.clone())
                .await
                .into_response()
                .status(),
            status
        );
        assert_eq!(
            settings_save_handler(auth, Bytes::from("{}"))
                .await
                .into_response()
                .status(),
            status
        );
    }
}

#[test]
fn fehlendes_brain_bleibt_gueltig() {
    let fixture = Fixture::new();
    assert!(check_candidate(&fixture.path(), &fixture.original).is_ok());
    assert!(check_candidate(&fixture.path(), &config_text(&full_social_section())).is_ok());
    assert!(check_candidate(&fixture.path(), BASE).is_ok());
}

fn remove_leaf(table: &mut toml::Table, path: &str) {
    let mut parts: Vec<&str> = path.split('.').skip(1).collect();
    let leaf = parts.pop().expect("leaf");
    let mut current = table;
    for part in parts {
        current = current
            .get_mut(part)
            .and_then(toml::Value::as_table_mut)
            .unwrap_or_else(|| panic!("{path} fehlt in der Social-AI-Vorlage"));
    }
    assert!(
        current.remove(leaf).is_some(),
        "{path} fehlt in der Social-AI-Vorlage"
    );
}

#[test]
fn jeder_social_ai_katalogpfad_trifft_die_echten_typen() {
    let source = Path::new("/var/lib/deadlock-twitch/config/bot.toml");
    let full = full_social_section();
    let full_text = config_text(&full);
    check_candidate(source, &full_text).expect("full social_ai fixture");
    let values = settings_catalog::read_catalog(&full_text)
        .expect("catalog")
        .values;
    let fields: Vec<_> = settings_catalog::fields()
        .iter()
        .filter(|field| field.path.starts_with("social_ai."))
        .collect();
    assert!(!fields.is_empty());
    for field in fields {
        let value = values[field.path].clone();
        assert!(!value.is_null(), "{} fehlt in der Vorlage", field.path);
        if field.kind == Kind::Integer {
            assert!(value.is_string(), "{} nicht als Text", field.path);
        }
        let mut reduced = full.clone();
        remove_leaf(&mut reduced, field.path);
        let reduced_text = config_text(&reduced);
        let request = ChangeRequest {
            revision: settings_catalog::read_catalog(&reduced_text)
                .expect("reduced catalog")
                .revision,
            changes: vec![tb_config::settings_catalog::Change {
                path: field.path.to_owned(),
                value: value.clone(),
            }],
        };
        let candidate = settings_catalog::apply_changes(&reduced_text, &request)
            .unwrap_or_else(|error| panic!("{}: {error}", field.path));
        assert!(
            check_candidate(source, &candidate).is_ok(),
            "{} besteht die Social-AI-Prüfung nicht",
            field.path
        );
        assert_eq!(
            settings_catalog::read_catalog(&candidate)
                .expect("candidate catalog")
                .values[field.path],
            value
        );
    }
}

async fn router_fixture() -> (
    crate::test_database::Database,
    crate::auth::session::DashboardAuthState,
    Router,
) {
    let database = crate::test_database::Database::new().await;
    sqlx::raw_sql("CREATE TABLE public.twitch_streamers(twitch_user_id TEXT PRIMARY KEY); CREATE TABLE public.dashboard_sessions(session_id TEXT PRIMARY KEY,session_type TEXT NOT NULL,payload_enc BYTEA NOT NULL,created_at DOUBLE PRECISION NOT NULL,expires_at DOUBLE PRECISION NOT NULL); CREATE TABLE public.twitch_partners(id BIGINT PRIMARY KEY,twitch_login TEXT NOT NULL,twitch_user_id TEXT NOT NULL,status TEXT NOT NULL DEFAULT 'active',technical_pause_reason TEXT,admin_archived_at TEXT,departnered_at TEXT,partnered_at TEXT DEFAULT CURRENT_TIMESTAMP)")
        .execute(&database.pool)
        .await
        .expect("isolated schema");
    let state = crate::auth::session::DashboardAuthState::new(
        database.pool.clone(),
        "dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU=".to_owned(),
    );
    let router =
        crate::build_admin_config_router(database.pool.clone(), "synthetic-internal".into())
            .layer(Extension(state.clone()));
    (database, state, router)
}

async fn put_settings(app: &Router, cookie: Option<&str>, headers: &[(&str, &str)]) -> Response {
    let mut request = Request::builder()
        .method("PUT")
        .uri("/twitch/api/admin/config/operating/settings")
        .header("host", "admin.test")
        .header("x-dashboard-context", "admin")
        .header("Content-Type", "application/json");
    if let Some(cookie) = cookie {
        request = request.header("cookie", cookie);
    }
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    app.clone()
        .oneshot(request.body(Body::from("{}")).expect("request"))
        .await
        .expect("response")
}

#[tokio::test]
async fn route_verlangt_admin_und_csrf_schutz() {
    let (database, state, app) = router_fixture().await;
    assert_eq!(
        put_settings(&app, None, &[]).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let get = Request::builder()
        .uri("/twitch/api/admin/config/operating/settings")
        .header("host", "admin.test")
        .body(Body::empty())
        .expect("request");
    assert_eq!(
        app.clone().oneshot(get).await.expect("response").status(),
        StatusCode::UNAUTHORIZED
    );
    sqlx::query("INSERT INTO public.twitch_partners(id,twitch_login,twitch_user_id) VALUES(101,'fixture','101')")
        .execute(&database.pool)
        .await
        .expect("partner");
    let partner = state
        .create_partner_session("fixture", "101", "Fixture")
        .await
        .expect("partner session");
    let partner_cookie = format!(
        "{}={}",
        crate::auth::session::PARTNER_COOKIE_NAME,
        partner.session_id
    );
    assert_eq!(
        put_settings(
            &app,
            Some(&partner_cookie),
            &[("X-CSRF-Token", partner.csrf_token.as_str())]
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let admin = state
        .create_admin_session("42", "Fixture Admin")
        .await
        .expect("admin session");
    let cookie = format!(
        "{}={}",
        crate::auth::session::ADMIN_COOKIE_NAME,
        admin.session_id
    );
    for headers in [
        [("origin", "https://attacker.example")],
        [("referer", "https://attacker.example/twitch/admin")],
    ] {
        let response = put_settings(&app, Some(&cookie), &headers).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(json_body(response).await["error"], "invalid_csrf");
    }
    let accepted = put_settings(
        &app,
        Some(&cookie),
        &[
            ("origin", "https://admin.test"),
            ("X-CSRF-Token", admin.csrf_token.as_str()),
        ],
    )
    .await
    .status();
    assert!(
        accepted != StatusCode::UNAUTHORIZED && accepted != StatusCode::FORBIDDEN,
        "{accepted}"
    );
}
