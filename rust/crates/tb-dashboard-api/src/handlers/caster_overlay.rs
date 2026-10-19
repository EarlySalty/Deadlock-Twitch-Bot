use super::caster_overlay_assets;
use crate::auth::level::DashboardAuthLevel;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};
use tb_http_core::ApiError;
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

const STEAM64_BASE: u64 = 76_561_197_960_265_728;
const MAX_CASTERS: usize = 100;
const MAX_CAMERA_URL: usize = 2048;
const MAX_TEAM_PLAYERS: usize = 12;
const MAX_TEAM_SHORT: usize = 8;
const MAX_COUNTDOWN_TITLE: usize = 60;
const BRAND_URL: &str = "/twitch/caster-overlay/builtin/dachlock.png";

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CasterLayout {
    Solo,
    #[default]
    Duo,
    Trio,
}
impl CasterLayout {
    fn slot_count(self) -> usize {
        match self {
            Self::Solo => 1,
            Self::Duo => 2,
            Self::Trio => 3,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Caster {
    pub id: String,
    pub name: String,
    pub handle: String,
    #[serde(default, rename = "accountLogin", skip_serializing_if = "Option::is_none")]
    pub account_login: Option<String>,
    #[serde(default, rename = "cameraUrl")]
    pub camera_url: String,
    #[serde(default, rename = "cameraId", skip_serializing_if = "Option::is_none")]
    pub camera_id: Option<String>,
    #[serde(default, rename = "steamAccountId", skip_serializing_if = "Option::is_none")]
    pub steam_account_id: Option<i64>,
    #[serde(default, rename = "teamId", skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ScenePlayer {
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(default, rename = "steamId64", skip_serializing_if = "Option::is_none")]
    pub steam_id64: Option<String>,
    #[serde(default, rename = "accountId", skip_serializing_if = "Option::is_none")]
    pub account_id: Option<i64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SceneTeam {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short: Option<String>,
    #[serde(
        default,
        rename = "logoAssetId",
        skip_serializing_if = "Option::is_none"
    )]
    pub logo_asset_id: Option<String>,
    #[serde(default)]
    pub players: Vec<ScenePlayer>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MatchContext {
    #[serde(default, rename = "teamA", skip_serializing_if = "Option::is_none")]
    pub team_a: Option<SceneTeam>,
    #[serde(default, rename = "teamB", skip_serializing_if = "Option::is_none")]
    pub team_b: Option<SceneTeam>,
    #[serde(default, rename = "observerAccountId", skip_serializing_if = "Option::is_none")]
    pub observer_account_id: Option<i64>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinEventLogo {
    #[default]
    Wochenende,
    Community,
    FunTurnier,
}
impl BuiltinEventLogo {
    fn url(self) -> &'static str {
        match self {
            Self::Wochenende => "/twitch/caster-overlay/builtin/wochenende.webp",
            Self::Community => "/twitch/caster-overlay/builtin/community.webp",
            Self::FunTurnier => "/twitch/caster-overlay/builtin/fun-turnier.webp",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventLogo {
    None,
    Builtin {
        id: BuiltinEventLogo,
    },
    Asset {
        #[serde(rename = "assetId")]
        asset_id: String,
    },
}
impl Default for EventLogo {
    fn default() -> Self {
        Self::Builtin {
            id: BuiltinEventLogo::default(),
        }
    }
}
impl EventLogo {
    fn url(&self) -> Option<String> {
        match self {
            Self::None => None,
            Self::Builtin { id } => Some(id.url().to_owned()),
            Self::Asset { asset_id } => Some(caster_overlay_assets::asset_url(asset_id)),
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EventSettings {
    #[serde(default)]
    pub logo: EventLogo,
    #[serde(default = "default_true", rename = "showBrand")]
    pub show_brand: bool,
}
impl Default for EventSettings {
    fn default() -> Self {
        Self {
            logo: EventLogo::default(),
            show_brand: true,
        }
    }
}

fn default_countdown_title() -> String {
    "Stream startet in".to_owned()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Countdown {
    #[serde(default = "default_countdown_title")]
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<DateTime<Utc>>,
}
impl Default for Countdown {
    fn default() -> Self {
        Self {
            title: default_countdown_title(),
            target: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Scene {
    pub roster: Vec<Caster>,
    #[serde(default)]
    pub slots: Vec<Option<String>>,
    #[serde(default)]
    pub layout: CasterLayout,
    #[serde(default, rename = "matchContext")]
    pub match_context: MatchContext,
    #[serde(default)]
    pub event: EventSettings,
    #[serde(default)]
    pub countdown: Countdown,
    #[serde(default, rename = "sidesSwapped")]
    pub sides_swapped: bool,
}

#[derive(Deserialize)]
pub struct SaveRequest {
    pub revision: i64,
    pub scene: Scene,
}

fn normalize_scene(scene: &mut Scene) {
    let count = scene.layout.slot_count();
    scene.slots.resize(count, None);
    scene.slots.truncate(count);
    for caster in &mut scene.roster {
        caster.name = caster.name.trim().to_owned();
        caster.handle = caster.handle.trim().trim_start_matches('@').to_owned();
        caster.account_login = caster
            .account_login
            .take()
            .map(|login| login.trim().trim_start_matches('@').to_ascii_lowercase())
            .filter(|login| !login.is_empty());
        caster.camera_url = caster.camera_url.trim().to_owned();
        caster.camera_id = caster
            .camera_id
            .take()
            .map(|id| id.trim().to_owned())
            .filter(|id| !id.is_empty());
        caster.team_id = caster
            .team_id
            .take()
            .map(|id| id.trim().to_owned())
            .filter(|id| !id.is_empty());
    }
    for team in [&mut scene.match_context.team_a, &mut scene.match_context.team_b]
        .into_iter()
        .flatten()
    {
        team.name = team.name.trim().to_owned();
        team.id = team
            .id
            .take()
            .map(|id| id.trim().to_owned())
            .filter(|id| !id.is_empty());
        team.short = trimmed_option(team.short.take());
        team.logo_asset_id = trimmed_option(team.logo_asset_id.take());
        team.players.truncate(MAX_TEAM_PLAYERS);
        for player in &mut team.players {
            player.display_name = player.display_name.trim().to_owned();
        }
    }
    normalize_event_and_countdown(scene);
}

fn trimmed_option(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn normalize_event_and_countdown(scene: &mut Scene) {
    if let EventLogo::Asset { asset_id } = &mut scene.event.logo {
        *asset_id = asset_id.trim().to_owned();
    }
    scene.countdown.title = scene.countdown.title.trim().to_owned();
    if scene.countdown.title.is_empty() {
        scene.countdown.title = default_countdown_title();
    }
}

fn valid_text(value: &str, max: usize) -> bool {
    value.chars().count() <= max && !value.chars().any(char::is_control)
}

fn team_extras_valid(team: &SceneTeam) -> bool {
    team.short
        .as_deref()
        .is_none_or(|short| valid_text(short, MAX_TEAM_SHORT))
        && team
            .logo_asset_id
            .as_deref()
            .is_none_or(caster_overlay_assets::is_asset_id)
}

fn event_and_countdown_valid(scene: &Scene) -> bool {
    let logo_valid = match &scene.event.logo {
        EventLogo::Asset { asset_id } => caster_overlay_assets::is_asset_id(asset_id),
        EventLogo::None | EventLogo::Builtin { .. } => true,
    };
    logo_valid && valid_text(&scene.countdown.title, MAX_COUNTDOWN_TITLE)
}

fn validate(scene: &mut Scene) -> Result<(), ApiError> {
    normalize_scene(scene);
    let invalid = || {
        ApiError::bad_request_with_body(json!({
            "message":"Ungültige Caster-Szene. Erlaubt sind Solo/Duo/Trio, höchstens 100 Personen, sichere HTTPS-Kameraquellen, maximal zwölf Spieler je Team, Teamkürzel bis acht Zeichen und ein Countdown-Titel bis 60 Zeichen."
        }))
    };
    if scene.roster.len() > MAX_CASTERS {
        return Err(invalid());
    }
    let mut ids = std::collections::HashSet::new();
    for caster in &scene.roster {
        let valid_camera_url = caster.camera_url.is_empty()
            || ((caster.camera_url.starts_with("https://")
                || (caster.camera_url.starts_with('/') && !caster.camera_url.starts_with("//")))
                && !caster.camera_url.chars().any(char::is_whitespace));
        let valid_camera_id = caster.camera_id.as_ref().is_none_or(|id| {
            id.len() <= 80
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        });
        if caster.id.is_empty()
            || caster.id.len() > 64
            || !caster
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || !ids.insert(caster.id.clone())
            || caster.name.is_empty()
            || caster.name.chars().count() > 60
            || caster.handle.chars().count() > 60
            || caster
                .account_login
                .as_ref()
                .is_some_and(|login| login.len() > 60 || !login.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
            || caster.camera_url.len() > MAX_CAMERA_URL
            || !valid_camera_url
            || !valid_camera_id
            || caster.name.chars().any(char::is_control)
            || caster.handle.chars().any(char::is_control)
            || caster.camera_url.chars().any(char::is_control)
        {
            return Err(invalid());
        }
    }
    if scene.slots.iter().flatten().any(|id| !ids.contains(id)) {
        return Err(invalid());
    }
    for team in [&scene.match_context.team_a, &scene.match_context.team_b]
        .into_iter()
        .flatten()
    {
        if !valid_text(&team.name, 80)
            || !team_extras_valid(team)
            || team.players.len() > MAX_TEAM_PLAYERS
            || team.players.iter().any(|player| {
                player.display_name.is_empty()
                    || player.display_name.chars().count() > 80
                    || player.display_name.chars().any(char::is_control)
            })
        {
            return Err(invalid());
        }
    }
    if !event_and_countdown_valid(scene) {
        return Err(invalid());
    }
    Ok(())
}

fn db_error(error: sqlx::Error) -> ApiError {
    tracing::error!(%error, "Caster-Szene konnte nicht gespeichert oder geladen werden");
    ApiError::internal()
}

async fn load(pool: &PgPool) -> Result<(i64, Scene), ApiError> {
    let (revision, value): (i64, Value) =
        sqlx::query_as("SELECT revision, scene FROM twitch_caster_overlay WHERE id = true")
            .fetch_one(pool)
            .await
            .map_err(db_error)?;
    let mut scene: Scene = serde_json::from_value(value).map_err(|error| {
        tracing::error!(%error, "Ungültige gespeicherte Caster-Szene");
        ApiError::internal()
    })?;
    normalize_scene(&mut scene);
    Ok((revision, scene))
}

pub async fn get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    let (revision, scene) = load(&pool).await?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({"revision": revision,"scene":scene})),
    ))
}

pub async fn save_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Json(mut request): Json<SaveRequest>,
) -> Result<Response, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    validate(&mut request.scene)?;
    let scene = serde_json::to_value(&request.scene).map_err(|_| ApiError::internal())?;
    let revision: Option<i64> = sqlx::query_scalar(
        "UPDATE twitch_caster_overlay SET scene = $1, revision = revision + 1 WHERE id = true AND revision = $2 RETURNING revision",
    )
    .bind(scene)
    .bind(request.revision)
    .fetch_optional(&pool)
    .await
    .map_err(db_error)?;
    let Some(revision) = revision else {
        return Ok((
            StatusCode::CONFLICT,
            Json(json!({"message":"Die Szene wurde inzwischen in einem anderen Fenster geändert. Bitte neu laden und deine Änderung erneut vornehmen."})),
        )
            .into_response());
    };
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({"revision":revision,"scene":request.scene})),
    )
        .into_response())
}

pub async fn public_handler(State(pool): State<PgPool>) -> Result<impl IntoResponse, ApiError> {
    let (revision, scene) = load(&pool).await?;
    let slots: Vec<_> = scene
        .slots
        .iter()
        .map(|id| {
            id.as_ref()
                .and_then(|id| scene.roster.iter().find(|caster| &caster.id == id))
                .map(|caster| {
                    json!({
                        "name":caster.name,
                        "handle":caster.handle,
                        "cameraUrl":caster.camera_url,
                        "cameraId":caster.camera_id,
                        "steamAccountId":caster.steam_account_id,
                        "teamId":caster.team_id,
                    })
                })
        })
        .collect();
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({
            "revision":revision,
            "layout":scene.layout,
            "slots":slots,
            "matchContext":scene.match_context,
            "teams":public_teams(&scene),
            "sidesSwapped":scene.sides_swapped,
            "event":{
                "logoUrl":scene.event.logo.url(),
                "showBrand":scene.event.show_brand,
                "brandUrl":BRAND_URL,
            },
            "countdown":scene.countdown,
        })),
    ))
}

fn public_team(team: Option<&SceneTeam>) -> Value {
    team.map_or(Value::Null, |team| {
        json!({
            "name":team.name,
            "short":team.short,
            "logoUrl":team.logo_asset_id.as_deref().map(caster_overlay_assets::asset_url),
            "players":team.players,
        })
    })
}

fn public_teams(scene: &Scene) -> Value {
    let team_a = public_team(scene.match_context.team_a.as_ref());
    let team_b = public_team(scene.match_context.team_b.as_ref());
    if scene.sides_swapped {
        json!({"left":team_b,"right":team_a})
    } else {
        json!({"left":team_a,"right":team_b})
    }
}

fn overlay_page(html: &'static str) -> impl IntoResponse {
    (
        [
            (header::CACHE_CONTROL, "no-cache"),
            (header::X_FRAME_OPTIONS, "SAMEORIGIN"),
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'none'; img-src 'self'; frame-src 'self' https:; media-src 'self' blob:; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self' ws: wss:; base-uri 'none'; frame-ancestors 'self' https://admin.deutsche-deadlock-community.de",
            ),
        ],
        Html(html),
    )
}

pub async fn html_handler() -> impl IntoResponse {
    overlay_page(include_str!("caster_overlay.html"))
}

pub async fn scoreboard_handler() -> impl IntoResponse {
    overlay_page(include_str!("caster_scoreboard.html"))
}

pub async fn background_handler() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        include_bytes!("caster_background.png").as_slice(),
    )
}

#[derive(Debug, Deserialize)]
pub struct CreateCameraRequest {
    #[serde(rename = "ownerLogin")]
    owner_login: String,
    label: String,
}

fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn camera_id_valid(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

fn normalize_owner_login(raw: &str) -> Option<String> {
    let value = raw.trim().trim_start_matches('@').to_ascii_lowercase();
    (!value.is_empty()
        && value.len() <= 60
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
    .then_some(value)
}

pub async fn list_cameras_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    let rows = sqlx::query(
        "SELECT camera_id, owner_login, label, consent_enabled, created_at, last_connected_at FROM twitch_caster_cameras ORDER BY owner_login, label, camera_id",
    )
    .fetch_all(&pool)
    .await
    .map_err(db_error)?;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let camera_id: String = row.try_get("camera_id").map_err(db_error)?;
        items.push(json!({
            "cameraId": camera_id,
            "ownerLogin": row.try_get::<String,_>("owner_login").map_err(db_error)?,
            "label": row.try_get::<String,_>("label").map_err(db_error)?,
            "consentEnabled": row.try_get::<bool,_>("consent_enabled").map_err(db_error)?,
            "online": camera_online(&camera_id).await,
            "createdAt": row.try_get::<chrono::DateTime<chrono::Utc>,_>("created_at").map_err(db_error)?,
            "lastConnectedAt": row.try_get::<Option<chrono::DateTime<chrono::Utc>>,_>("last_connected_at").map_err(db_error)?,
        }));
    }
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(json!({"items":items}))))
}

pub async fn create_camera_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Json(request): Json<CreateCameraRequest>,
) -> Result<Response, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    let Some(owner_login) = normalize_owner_login(&request.owner_login) else {
        return Ok((StatusCode::BAD_REQUEST, Json(json!({"message":"Ungültiger Twitch-Login."}))).into_response());
    };
    let label = request.label.trim();
    if label.is_empty() || label.chars().count() > 80 || label.chars().any(char::is_control) {
        return Ok((StatusCode::BAD_REQUEST, Json(json!({"message":"Kamera-Name muss 1 bis 80 Zeichen lang sein."}))).into_response());
    }
    let camera_id = format!("cam_{}", Uuid::new_v4().simple());
    let publish_token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let hash = token_hash(&publish_token);
    sqlx::query(
        "INSERT INTO twitch_caster_cameras(camera_id, owner_login, label, publish_token_hash) VALUES($1,$2,$3,$4)",
    )
    .bind(&camera_id)
    .bind(&owner_login)
    .bind(label)
    .bind(hash)
    .execute(&pool)
    .await
    .map_err(db_error)?;
    let invite_url = format!(
        "https://deutsche-deadlock-community.de/twitch/caster-camera/{camera_id}#token={publish_token}"
    );
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "cameraId":camera_id,
            "ownerLogin":owner_login,
            "label":label,
            "inviteUrl":invite_url,
            "consentEnabled":false,
            "online":false,
        })),
    )
        .into_response())
}

pub async fn revoke_camera_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(camera_id): Path<String>,
) -> Result<Response, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    if !camera_id_valid(&camera_id) {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    let affected = sqlx::query(
        "UPDATE twitch_caster_cameras SET consent_enabled=FALSE, revoked_at=now() WHERE camera_id=$1",
    )
    .bind(&camera_id)
    .execute(&pool)
    .await
    .map_err(db_error)?
    .rows_affected();
    disconnect_camera(&camera_id).await;
    if affected == 0 {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    Ok(Json(json!({"ok":true,"cameraId":camera_id})).into_response())
}

pub async fn camera_page_handler(
    State(pool): State<PgPool>,
    Path(camera_id): Path<String>,
) -> Response {
    if !camera_id_valid(&camera_id) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM twitch_caster_cameras WHERE camera_id=$1)",
    )
    .bind(&camera_id)
    .fetch_one(&pool)
    .await
    .unwrap_or(false);
    if !exists {
        return StatusCode::NOT_FOUND.into_response();
    }
    (
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::X_FRAME_OPTIONS, "DENY"),
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'none'; media-src 'self' blob:; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self' ws: wss:; base-uri 'none'; frame-ancestors 'none'",
            ),
            (header::REFERRER_POLICY, "no-referrer"),
        ],
        Html(include_str!("caster_camera.html")),
    )
        .into_response()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CameraRole {
    Publisher,
    Subscriber,
}

#[derive(Clone)]
struct CameraPeer {
    role: CameraRole,
    sender: mpsc::UnboundedSender<Message>,
}

#[derive(Default)]
struct CameraRoom {
    publisher: Option<String>,
    peers: HashMap<String, CameraPeer>,
}

type CameraHub = Arc<Mutex<HashMap<String, CameraRoom>>>;
static CAMERA_HUB: OnceLock<CameraHub> = OnceLock::new();
fn camera_hub() -> &'static CameraHub {
    CAMERA_HUB.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

async fn camera_online(camera_id: &str) -> bool {
    camera_hub()
        .lock()
        .await
        .get(camera_id)
        .and_then(|room| room.publisher.as_ref())
        .is_some()
}

async fn disconnect_camera(camera_id: &str) {
    let room = camera_hub().lock().await.remove(camera_id);
    if let Some(room) = room {
        for peer in room.peers.into_values() {
            let _ = peer.sender.send(Message::Text(
                json!({"type":"revoked"}).to_string().into(),
            ));
        }
    }
}

#[derive(Deserialize)]
pub struct CameraWsQuery {
    #[serde(rename = "cameraId")]
    camera_id: String,
    role: String,
}

fn allowed_camera_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|value| value.to_str().ok()) else {
        return false;
    };
    matches!(
        origin,
        "https://deutsche-deadlock-community.de" | "https://admin.deutsche-deadlock-community.de"
    ) || cfg!(test) && origin == "http://localhost"
}

pub async fn camera_ws_handler(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Query(query): Query<CameraWsQuery>,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !camera_id_valid(&query.camera_id) || !allowed_camera_origin(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let role = match query.role.as_str() {
        "publisher" => CameraRole::Publisher,
        "subscriber" => CameraRole::Subscriber,
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };
    let camera_id = query.camera_id;
    upgrade
        .on_upgrade(move |socket| camera_socket(socket, pool, camera_id, role))
        .into_response()
}

async fn publisher_authorized(pool: &PgPool, camera_id: &str, token: &str) -> bool {
    let Ok(Some(stored)) = sqlx::query_scalar::<_, String>(
        "SELECT publish_token_hash FROM twitch_caster_cameras WHERE camera_id=$1 AND revoked_at IS NULL",
    )
    .bind(camera_id)
    .fetch_optional(pool)
    .await
    else {
        return false;
    };
    let presented = token_hash(token);
    tb_crypto::constant_time_eq(stored.as_bytes(), presented.as_bytes())
}

async fn subscriber_authorized(pool: &PgPool, camera_id: &str) -> bool {
    sqlx::query_scalar::<_, bool>(
        "SELECT COALESCE((SELECT consent_enabled AND revoked_at IS NULL FROM twitch_caster_cameras WHERE camera_id=$1), FALSE)",
    )
    .bind(camera_id)
    .fetch_one(pool)
    .await
    .unwrap_or(false)
}

async fn camera_socket(
    mut socket: WebSocket,
    pool: PgPool,
    camera_id: String,
    role: CameraRole,
) {
    if role == CameraRole::Publisher {
        let auth = tokio::time::timeout(std::time::Duration::from_secs(10), socket.next()).await;
        let token = match auth {
            Ok(Some(Ok(Message::Text(text)))) => serde_json::from_str::<Value>(&text)
                .ok()
                .filter(|value| value.get("type").and_then(Value::as_str) == Some("auth"))
                .filter(|value| value.get("consent").and_then(Value::as_bool) == Some(true))
                .and_then(|value| value.get("token").and_then(Value::as_str).map(str::to_owned)),
            _ => None,
        };
        let Some(token) = token else {
            let _ = socket.send(Message::Text(json!({"type":"auth-error"}).to_string().into())).await;
            return;
        };
        if !publisher_authorized(&pool, &camera_id, &token).await {
            let _ = socket.send(Message::Text(json!({"type":"auth-error"}).to_string().into())).await;
            return;
        }
        let _ = sqlx::query(
            "UPDATE twitch_caster_cameras SET consent_enabled=TRUE, revoked_at=NULL, last_connected_at=now() WHERE camera_id=$1",
        )
        .bind(&camera_id)
        .execute(&pool)
        .await;
    } else if !subscriber_authorized(&pool, &camera_id).await {
        let _ = socket
            .send(Message::Text(json!({"type":"not-available"}).to_string().into()))
            .await;
        return;
    }

    let peer_id = Uuid::new_v4().simple().to_string();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();
    let peer = CameraPeer {
        role,
        sender: out_tx.clone(),
    };

    {
        let mut hub = camera_hub().lock().await;
        let room = hub.entry(camera_id.clone()).or_default();
        if role == CameraRole::Publisher {
            if let Some(previous) = room.publisher.take() {
                if let Some(peer) = room.peers.remove(&previous) {
                    let _ = peer.sender.send(Message::Text(
                        json!({"type":"replaced"}).to_string().into(),
                    ));
                }
            }
            room.publisher = Some(peer_id.clone());
            room.peers.insert(peer_id.clone(), peer);
            let subscribers = room
                .peers
                .iter()
                .filter(|(_, peer)| peer.role == CameraRole::Subscriber)
                .map(|(id, peer)| (id.clone(), peer.sender.clone()))
                .collect::<Vec<_>>();
            for (subscriber_id, sender) in subscribers {
                let _ = out_tx.send(Message::Text(
                    json!({"type":"subscriber-joined","peerId":subscriber_id})
                        .to_string()
                        .into(),
                ));
                let _ = sender.send(Message::Text(
                    json!({"type":"publisher-ready"}).to_string().into(),
                ));
            }
        } else {
            room.peers.insert(peer_id.clone(), peer);
            if let Some(publisher_id) = room.publisher.clone() {
                if let Some(publisher) = room.peers.get(&publisher_id) {
                    let _ = publisher.sender.send(Message::Text(
                        json!({"type":"subscriber-joined","peerId":peer_id})
                            .to_string()
                            .into(),
                    ));
                    let _ = out_tx.send(Message::Text(
                        json!({"type":"publisher-ready"}).to_string().into(),
                    ));
                }
            }
        }
    }
    let _ = out_tx.send(Message::Text(
        json!({"type":"ready","peerId":peer_id,"role": if role == CameraRole::Publisher {"publisher"} else {"subscriber"}})
            .to_string()
            .into(),
    ));

    let (mut sink, mut stream) = socket.split();
    loop {
        tokio::select! {
            outgoing = out_rx.recv() => {
                let Some(message) = outgoing else { break; };
                if sink.send(message).await.is_err() { break; }
            }
            incoming = stream.next() => {
                let Some(Ok(message)) = incoming else { break; };
                match message {
                    Message::Text(text) => {
                        if role == CameraRole::Publisher
                            && serde_json::from_str::<Value>(&text)
                                .ok()
                                .and_then(|value| value.get("type").and_then(Value::as_str).map(str::to_owned))
                                .as_deref()
                                == Some("revoke")
                        {
                            let _ = sqlx::query(
                                "UPDATE twitch_caster_cameras SET consent_enabled=FALSE, revoked_at=now() WHERE camera_id=$1",
                            )
                            .bind(&camera_id)
                            .execute(&pool)
                            .await;
                            let _ = sink
                                .send(Message::Text(json!({"type":"revoke-ok"}).to_string().into()))
                                .await;
                            break;
                        }
                        if !forward_signal(&camera_id, &peer_id, &text).await {
                            continue;
                        }
                    }
                    Message::Ping(data) => {
                        if sink.send(Message::Pong(data)).await.is_err() { break; }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        }
    }
    remove_camera_peer(&camera_id, &peer_id, role).await;
}

async fn forward_signal(camera_id: &str, from_peer_id: &str, raw: &str) -> bool {
    if raw.len() > 64 * 1024 {
        return false;
    }
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return false;
    };
    if value.get("type").and_then(Value::as_str) != Some("signal") {
        return false;
    }
    let Some(target) = value.get("targetPeerId").and_then(Value::as_str) else {
        return false;
    };
    let Some(data) = value.get("data") else {
        return false;
    };
    let hub = camera_hub().lock().await;
    let Some(room) = hub.get(camera_id) else {
        return false;
    };
    if !room.peers.contains_key(from_peer_id) {
        return false;
    }
    let Some(target_peer) = room.peers.get(target) else {
        return false;
    };
    let payload = json!({
        "type":"signal",
        "fromPeerId":from_peer_id,
        "data":data,
    });
    target_peer
        .sender
        .send(Message::Text(payload.to_string().into()))
        .is_ok()
}

async fn remove_camera_peer(camera_id: &str, peer_id: &str, role: CameraRole) {
    let mut hub = camera_hub().lock().await;
    let Some(room) = hub.get_mut(camera_id) else {
        return;
    };
    room.peers.remove(peer_id);
    if role == CameraRole::Publisher && room.publisher.as_deref() == Some(peer_id) {
        room.publisher = None;
        for peer in room.peers.values() {
            let _ = peer.sender.send(Message::Text(
                json!({"type":"publisher-left"}).to_string().into(),
            ));
        }
    } else if let Some(publisher_id) = room.publisher.clone() {
        if let Some(publisher) = room.peers.get(&publisher_id) {
            let _ = publisher.sender.send(Message::Text(
                json!({"type":"subscriber-left","peerId":peer_id})
                    .to_string()
                    .into(),
            ));
        }
    }
    if room.peers.is_empty() {
        hub.remove(camera_id);
    }
}

#[derive(Debug, Deserialize)]
struct TurnierTeam {
    id: Value,
    name: String,
    #[serde(default)]
    members: Vec<TurnierMember>,
}
#[derive(Debug, Deserialize)]
struct TurnierMember {
    display_name: String,
    discord_id: Option<String>,
    #[serde(default)]
    is_bench: bool,
}

fn value_id(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        _ => String::new(),
    }
}

async fn steam_links_for_discord(pool: &PgPool, discord_ids: &[String]) -> HashMap<String, (String, i64)> {
    if discord_ids.is_empty() {
        return HashMap::new();
    }
    let rows = sqlx::query(
        "SELECT discord_id::text AS discord_id, steam_id64::text AS steam_id64 \
         FROM core.steam_links \
         WHERE discord_id::text = ANY($1) AND verified=TRUE AND steam_id64 IS NOT NULL \
         ORDER BY primary_account DESC NULLS LAST, linked_at DESC NULLS LAST",
    )
    .bind(discord_ids)
    .fetch_all(pool)
    .await;
    let Ok(rows) = rows else {
        return HashMap::new();
    };
    let mut result = HashMap::new();
    for row in rows {
        let Ok(discord_id) = row.try_get::<String, _>("discord_id") else {
            continue;
        };
        if result.contains_key(&discord_id) {
            continue;
        }
        let Ok(steam_id64) = row.try_get::<String, _>("steam_id64") else {
            continue;
        };
        let Some(account_id) = steam_id64
            .parse::<u64>()
            .ok()
            .and_then(|value| value.checked_sub(STEAM64_BASE))
            .and_then(|value| i64::try_from(value).ok())
        else {
            continue;
        };
        result.insert(discord_id, (steam_id64, account_id));
    }
    result
}

pub async fn context_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    let token = std::env::var("TURNIER_INTERNAL_API_TOKEN")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let Some(token) = token else {
        return Ok((
            [(header::CACHE_CONTROL, "no-store")],
            Json(json!({"available":false,"reason":"turnier_token_missing","teams":[]})),
        ));
    };
    let base = std::env::var("TURNIER_INTERNAL_API_BASE_URL")
        .ok()
        .map(|value| value.trim_end_matches('/').to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:8900".to_string());
    let url = format!("{base}/internal/turnier/v1/caster/teams");
    let response = match reqwest::Client::new()
        .get(url)
        .header("X-Internal-Token", token)
        .timeout(std::time::Duration::from_secs(4))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => response,
        Ok(response) => {
            tracing::warn!(status=%response.status(), "Caster-Kontext: Turnier-Team-API abgelehnt");
            return Ok((
                [(header::CACHE_CONTROL, "no-store")],
                Json(json!({"available":false,"reason":"turnier_api_rejected","teams":[]})),
            ));
        }
        Err(error) => {
            tracing::warn!(%error, "Caster-Kontext: Turnier-Team-API nicht erreichbar");
            return Ok((
                [(header::CACHE_CONTROL, "no-store")],
                Json(json!({"available":false,"reason":"turnier_api_unreachable","teams":[]})),
            ));
        }
    };
    let teams: Vec<TurnierTeam> = match response.json().await {
        Ok(teams) => teams,
        Err(error) => {
            tracing::warn!(%error, "Caster-Kontext: ungültige Team-Antwort");
            return Ok((
                [(header::CACHE_CONTROL, "no-store")],
                Json(json!({"available":false,"reason":"turnier_api_invalid","teams":[]})),
            ));
        }
    };
    let discord_ids = teams
        .iter()
        .flat_map(|team| team.members.iter())
        .filter_map(|member| member.discord_id.clone())
        .collect::<Vec<_>>();
    let steam = steam_links_for_discord(&pool, &discord_ids).await;
    let teams = teams
        .into_iter()
        .map(|team| {
            let players = team
                .members
                .into_iter()
                .map(|member| {
                    let ids = member.discord_id.as_ref().and_then(|id| steam.get(id));
                    json!({
                        "displayName":member.display_name,
                        "discordId":member.discord_id,
                        "steamId64":ids.map(|ids| ids.0.clone()),
                        "accountId":ids.map(|ids| ids.1),
                        "isBench":member.is_bench,
                    })
                })
                .collect::<Vec<_>>();
            json!({"id":value_id(&team.id),"name":team.name,"players":players})
        })
        .collect::<Vec<_>>();
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({"available":true,"source":"turnier+steam","teams":teams})),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scene() -> Scene {
        Scene {
            roster: vec![Caster {
                id: "caster-1".into(),
                name: " ZeRo ".into(),
                handle: "@zro_dl".into(),
                account_login: Some("@ZRO_DL".into()),
                camera_url: " https://vdo.ninja/?view=test ".into(),
                camera_id: Some("cam_test".into()),
                steam_account_id: Some(42),
                team_id: Some("7".into()),
            }],
            slots: vec![Some("caster-1".into()), None],
            layout: CasterLayout::Duo,
            match_context: MatchContext::default(),
            event: EventSettings::default(),
            countdown: Countdown::default(),
            sides_swapped: false,
        }
    }
    #[test]
    fn validates_and_normalizes_names() {
        let mut value = scene();
        validate(&mut value).unwrap();
        assert_eq!(value.roster[0].name, "ZeRo");
        assert_eq!(value.roster[0].handle, "zro_dl");
        assert_eq!(value.roster[0].account_login.as_deref(), Some("zro_dl"));
        assert_eq!(value.roster[0].camera_url, "https://vdo.ninja/?view=test");
    }
    #[test]
    fn layouts_normalisieren_slotanzahl() {
        let mut value = scene();
        value.layout = CasterLayout::Solo;
        validate(&mut value).unwrap();
        assert_eq!(value.slots.len(), 1);
        value.layout = CasterLayout::Trio;
        validate(&mut value).unwrap();
        assert_eq!(value.slots.len(), 3);
    }
    #[test]
    fn rejects_dangling_slots_and_duplicate_ids() {
        let mut value = scene();
        value.slots[1] = Some("missing".into());
        assert!(validate(&mut value).is_err());
        let mut value = scene();
        value.roster.push(value.roster[0].clone());
        assert!(validate(&mut value).is_err());
    }
    #[test]
    fn rejects_control_characters_and_long_names() {
        let mut value = scene();
        value.roster[0].name = "x\ny".into();
        assert!(validate(&mut value).is_err());
        value.roster[0].name = "ü".repeat(61);
        assert!(validate(&mut value).is_err());
    }
    #[test]
    fn rejects_unsafe_camera_urls() {
        let mut value = scene();
        value.roster[0].camera_url = "javascript:alert(1)".into();
        assert!(validate(&mut value).is_err());
        let mut value = scene();
        value.roster[0].camera_url = "https://example.invalid/cam with-space".into();
        assert!(validate(&mut value).is_err());
    }
    #[test]
    fn alte_szenen_bekommen_duo_defaults() {
        let mut value: Scene = serde_json::from_value(json!({
            "roster":[{"id":"caster-1","name":"Nimo","handle":"nimo"}],
            "slots":["caster-1",null]
        }))
        .unwrap();
        normalize_scene(&mut value);
        assert_eq!(value.layout, CasterLayout::Duo);
        assert_eq!(value.slots.len(), 2);
        assert!(value.roster[0].camera_id.is_none());
        assert!(value.event.show_brand);
        assert_eq!(
            value.event.logo,
            EventLogo::Builtin {
                id: BuiltinEventLogo::Wochenende
            }
        );
        assert_eq!(value.countdown.title, "Stream startet in");
        assert!(!value.sides_swapped);
    }
    #[test]
    fn freie_teams_und_eventlogo_werden_geprueft() {
        let mut value = scene();
        value.match_context.team_a = Some(SceneTeam {
            name: "Team Rot".into(),
            short: Some(" ROT ".into()),
            logo_asset_id: Some("6f1c2a52-6a8e-4c43-9f43-1d5d0c6b8a11".into()),
            ..SceneTeam::default()
        });
        value.event.logo = EventLogo::Asset {
            asset_id: "6f1c2a52-6a8e-4c43-9f43-1d5d0c6b8a12".into(),
        };
        validate(&mut value).unwrap();
        assert_eq!(
            value
                .match_context
                .team_a
                .as_ref()
                .and_then(|t| t.short.as_deref()),
            Some("ROT")
        );
        let mut broken = value.clone();
        if let Some(team) = broken.match_context.team_a.as_mut() {
            team.logo_asset_id = Some("../etc/passwd".into());
        }
        assert!(validate(&mut broken).is_err());
        let mut broken = value.clone();
        if let Some(team) = broken.match_context.team_a.as_mut() {
            team.short = Some("VIELZULANG".into());
        }
        assert!(validate(&mut broken).is_err());
        let mut broken = value;
        broken.event.logo = EventLogo::Asset {
            asset_id: "kein-bild".into(),
        };
        assert!(validate(&mut broken).is_err());
    }
    #[test]
    fn seitentausch_dreht_die_teams() {
        let mut value = scene();
        value.match_context.team_a = Some(SceneTeam {
            name: "Links".into(),
            ..SceneTeam::default()
        });
        value.match_context.team_b = Some(SceneTeam {
            name: "Rechts".into(),
            ..SceneTeam::default()
        });
        assert_eq!(public_teams(&value)["left"]["name"], "Links");
        value.sides_swapped = true;
        assert_eq!(public_teams(&value)["left"]["name"], "Rechts");
        assert_eq!(public_teams(&value)["right"]["name"], "Links");
    }
    #[test]
    fn token_hash_ist_stabil_und_kamera_id_strikt() {
        assert_eq!(token_hash("abc"), token_hash("abc"));
        assert_ne!(token_hash("abc"), token_hash("abd"));
        assert!(camera_id_valid("cam_123-abc"));
        assert!(!camera_id_valid("../cam"));
    }
}
