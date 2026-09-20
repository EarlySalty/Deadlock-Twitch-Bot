//! Opt-in public partner pages. All public paths use the same live lifecycle gate.
//! No analytics, viewer identities, private Discord data, or tokens are published.
mod html;
#[cfg(test)]
mod tests;

use super::community::matching::{schedule, Session};
use crate::auth::{level::DashboardAuthLevel, streamer_scope::resolve_settings_target};
use axum::{
    extract::{OriginalUri, Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc};
use chrono_tz::Europe::Berlin;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{types::Json as SqlJson, PgPool};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration as StdDuration, Instant},
};
use tb_transport_twitch::{HelixClient, HelixConfig};
use tokio::sync::Mutex;

// Visibility deliberately does not use an auth/session or profile cache. A revoked
// channel must disappear on the next request, including calendar and directory.
// The optional Twitch avatar cache contains image metadata only and never decides visibility.
const ACTIVE: &str = "p.status='active' AND p.departnered_at IS NULL AND p.admin_archived_at IS NULL AND COALESCE(p.manual_partner_opt_out,0)=0 AND COALESCE(p.raid_bot_enabled,0)=1 AND COALESCE(p.technical_pause_reason,'')=''";

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Accent {
    #[default]
    Gold,
    Violet,
    Teal,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Social {
    pub label: String,
    pub url: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub id: uuid::Uuid,
    pub title: String,
    pub description: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Content {
    pub headline: String,
    pub about: String,
    pub avatar_url: String,
    pub banner_url: String,
    pub accent: Accent,
    pub socials: Vec<Social>,
    pub featured: Vec<String>,
    pub main_heroes: Vec<String>,
    pub rank: String,
    pub playstyles: Vec<String>,
    pub preferred_times: Vec<String>,
    pub show_history: bool,
    pub sync_twitch_schedule: bool,
    pub events: Vec<Event>,
}
impl Default for Content {
    fn default() -> Self {
        Self {
            headline: String::new(),
            about: String::new(),
            avatar_url: String::new(),
            banner_url: String::new(),
            accent: Accent::Gold,
            socials: vec![],
            featured: vec![],
            main_heroes: vec![],
            rank: String::new(),
            playstyles: vec![],
            preferred_times: vec![],
            show_history: true,
            sync_twitch_schedule: true,
            events: vec![],
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    pub revision: i64,
    pub published: bool,
    pub profile: Content,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerParams {
    pub streamer: Option<String>,
    pub refresh_twitch: Option<bool>,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageParams {
    pub month: Option<String>,
}
#[derive(sqlx::FromRow)]
struct Record {
    twitch_user_id: String,
    login: String,
    active: bool,
    published: bool,
    revision: i64,
    content: SqlJson<Content>,
    is_live: i32,
    last_seen_at: Option<String>,
    last_game: Option<String>,
}
#[derive(Serialize, sqlx::FromRow)]
struct DirectoryEntry {
    login: String,
    headline: String,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct TwitchLiveProfile {
    pub title: String,
    pub game_name: String,
    pub thumbnail_url: String,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct TwitchClipProfile {
    pub id: String,
    pub title: String,
    pub url: String,
    pub thumbnail_url: String,
    pub view_count: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct TwitchScheduleProfile {
    pub id: String,
    pub title: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub is_recurring: bool,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct TwitchProfileSnapshot {
    pub available: bool,
    pub display_name: String,
    pub description: String,
    pub profile_image_url: String,
    pub banner_url: String,
    pub live: Option<TwitchLiveProfile>,
    pub clips: Vec<TwitchClipProfile>,
    pub schedule: Vec<TwitchScheduleProfile>,
}

#[derive(Clone)]
pub struct TwitchProfileCache {
    helix: HelixClient,
    ttl: StdDuration,
    entries: Arc<Mutex<HashMap<String, CachedTwitchProfile>>>,
}

#[derive(Clone)]
struct CachedTwitchProfile {
    value: TwitchProfileSnapshot,
    expires_at: Instant,
}

impl TwitchProfileCache {
    const TTL: StdDuration = StdDuration::from_secs(60);

    pub fn from_env() -> Option<Self> {
        let client_id = std::env::var("TWITCH_CLIENT_ID")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let client_secret = std::env::var("TWITCH_CLIENT_SECRET")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let client_id_present = client_id.is_some();
        let client_secret_present = client_secret.is_some();
        let (Some(client_id), Some(client_secret)) = (client_id, client_secret) else {
            tracing::warn!(
                client_id_present,
                client_secret_present,
                "Partnerprofil Twitch Cache deaktiviert: Client Zugangsdaten fehlen"
            );
            return None;
        };
        match HelixClient::new(HelixConfig::new(client_id, client_secret)) {
            Ok(helix) => Some(Self {
                helix,
                ttl: Self::TTL,
                entries: Arc::new(Mutex::new(HashMap::new())),
            }),
            Err(error) => {
                tracing::warn!(%error, "Partnerprofil Twitch Cache konnte nicht erstellt werden");
                None
            }
        }
    }

    pub async fn snapshot(
        &self,
        twitch_user_id: &str,
        login: &str,
        now: DateTime<Utc>,
        force_refresh: bool,
    ) -> TwitchProfileSnapshot {
        let key = twitch_user_id.trim().to_string();
        if key.is_empty() {
            return TwitchProfileSnapshot::default();
        }
        if !force_refresh {
            if let Some(cached) = self.entries.lock().await.get(&key) {
                if cached.expires_at > Instant::now() {
                    return cached.value.clone();
                }
            }
        }

        let user_ids = vec![key.clone()];
        let user_id_refs = [key.as_str()];
        let clips_since = now - Duration::days(30);
        let schedule_until = now + Duration::days(30);
        let (users_result, streams_result, schedule_result, clips_result) = tokio::join!(
            self.helix.get_users_by_id(&user_id_refs),
            self.helix.get_streams_by_user_ids(&user_ids, None),
            self.helix.get_channel_stream_schedule(&key, now, 50),
            self.helix
                .get_clips_by_broadcaster_range(&key, clips_since, now, 3),
        );

        let mut snapshot = TwitchProfileSnapshot::default();
        match users_result {
            Ok(mut users) => {
                if let Some(user) = users.remove(&key) {
                    snapshot.available = true;
                    snapshot.display_name = user.display_name;
                    snapshot.description = user.description.trim().to_string();
                    snapshot.profile_image_url = user
                        .profile_image_url
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    snapshot.banner_url =
                        user.offline_image_url.unwrap_or_default().trim().to_string();
                }
            }
            Err(error) => {
                tracing::warn!(%error, twitch_login = %login, "Twitch Profildaten nicht abrufbar");
            }
        }

        match streams_result {
            Ok(streams) => {
                if let Some(stream) = streams.into_iter().find(|stream| stream.user_id == key) {
                    snapshot.live = Some(TwitchLiveProfile {
                        title: stream.title,
                        game_name: stream.game_name,
                        thumbnail_url: stream
                            .thumbnail_url
                            .replace("{width}", "960")
                            .replace("{height}", "540"),
                    });
                }
            }
            Err(error) => {
                tracing::warn!(%error, twitch_login = %login, "Twitch Livebild nicht abrufbar");
            }
        }

        match schedule_result {
            Ok(segments) => {
                snapshot.schedule = segments
                    .into_iter()
                    .filter_map(|segment| {
                        let starts_at = DateTime::parse_from_rfc3339(&segment.start_time)
                            .ok()?
                            .with_timezone(&Utc);
                        let ends_at = DateTime::parse_from_rfc3339(&segment.end_time)
                            .ok()?
                            .with_timezone(&Utc);
                        (ends_at > now && starts_at < schedule_until && ends_at > starts_at).then_some(
                            TwitchScheduleProfile {
                                id: segment.id,
                                title: segment.title.trim().to_string(),
                                starts_at,
                                ends_at,
                                is_recurring: segment.is_recurring,
                            },
                        )
                    })
                    .collect();
                snapshot.schedule.sort_by_key(|segment| segment.starts_at);
            }
            Err(error) => {
                tracing::warn!(%error, twitch_login = %login, "Twitch Streamplan nicht abrufbar");
            }
        }

        match clips_result {
            Ok(clips) => {
                snapshot.clips = clips
                    .into_iter()
                    .filter(|clip| !clip.url.trim().is_empty())
                    .map(|clip| TwitchClipProfile {
                        id: clip.id,
                        title: clip.title.trim().to_string(),
                        url: clip.url.trim().to_string(),
                        thumbnail_url: clip.thumbnail_url.trim().to_string(),
                        view_count: clip.view_count.max(0),
                    })
                    .collect();
            }
            Err(error) => {
                tracing::warn!(%error, twitch_login = %login, "Twitch Clips nicht abrufbar");
            }
        }

        self.entries.lock().await.insert(
            key,
            CachedTwitchProfile {
                value: snapshot.clone(),
                expires_at: Instant::now() + self.ttl,
            },
        );
        snapshot
    }
}

fn no_store(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        "no-store, max-age=0".parse().unwrap(),
    );
    response
}
fn error(status: StatusCode, message: &str) -> Response {
    no_store((status, Json(json!({"error": message}))).into_response())
}
fn unavailable(err: sqlx::Error) -> Response {
    tracing::warn!(%err, "Partner profile query failed");
    error(
        StatusCode::SERVICE_UNAVAILABLE,
        "Das Profil ist gerade nicht erreichbar. Bitte erneut versuchen.",
    )
}
fn valid_login(raw: &str) -> Option<String> {
    let login = raw.trim().to_ascii_lowercase();
    (!login.is_empty()
        && login.len() <= 25
        && login
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_'))
    .then_some(login)
}
// Existing website pages own these single-segment paths.
const RESERVED_PROFILE_LOGINS: &[&str] = &[
    "commands",
    "help",
    "faq",
    "vergleich",
    "v1",
    "v2",
    "v3",
    "onboarding",
    "vertriebler",
    "assets",
    "fonts",
    "brand",
];
fn profile_login(raw: &str) -> Option<String> {
    valid_login(raw).filter(|login| !RESERVED_PROFILE_LOGINS.contains(&login.as_str()))
}
fn safe_url(raw: &str) -> bool {
    let Ok(url) = url::Url::parse(raw) else {
        return false;
    };
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && matches!(url.host(), Some(url::Host::Domain(host)) if host.contains('.') && !host.ends_with(".localhost") && !host.ends_with(".local") && !host.ends_with(".internal"))
        && !raw.chars().any(char::is_control)
}
fn text(value: &mut String, max: usize) -> bool {
    *value = value.trim().to_string();
    value.chars().count() <= max
        && !value
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
}
fn safe_twitch_image(raw: &str) -> bool {
    raw.is_empty()
        || (raw.len() <= 2048
            && safe_url(raw)
            && url::Url::parse(raw)
                .ok()
                .and_then(|url| url.host_str().map(str::to_string))
                .as_deref()
                == Some("static-cdn.jtvnw.net"))
}
const PLAYSTYLE_KEYS: &[&str] = &[
    "competitive",
    "tryhard",
    "chill",
    "community",
    "educational",
    "variety",
];
const PREFERRED_TIME_KEYS: &[&str] = &[
    "weekday_day",
    "weekday_evening",
    "weekday_late",
    "weekend_day",
    "weekend_evening",
    "spontaneous",
];

fn validate(update: &mut Update) -> Result<(), &'static str> {
    if update.revision < 0 || update.revision == i64::MAX {
        return Err("Ungültiger Profilstand.");
    }
    let p = &mut update.profile;
    if !text(&mut p.headline, 120) || !text(&mut p.about, 4000) {
        return Err("Überschrift: höchstens 120 Zeichen. Über mich: höchstens 4000 Zeichen.");
    }
    if p.socials.len() > 12
        || p.featured.len() > 6
        || p.main_heroes.len() > 3
        || p.playstyles.len() > 6
        || p.preferred_times.len() > 6
        || p.events.len() > 200
    {
        return Err("Zu viele Profilangaben. Bitte Auswahl und Termine reduzieren.");
    }
    p.avatar_url = p.avatar_url.trim().into();
    p.banner_url = p.banner_url.trim().into();
    if !safe_twitch_image(&p.avatar_url) || !safe_twitch_image(&p.banner_url) {
        return Err("Profilbild und Banner müssen gültige Twitch Bildadressen sein.");
    }
    if !text(&mut p.rank, 40) {
        return Err("Der Rang darf höchstens 40 Zeichen haben.");
    }
    let mut heroes = HashSet::new();
    for hero in &mut p.main_heroes {
        if !text(hero, 40) || hero.is_empty() || !heroes.insert(hero.to_lowercase()) {
            return Err("Bitte höchstens drei unterschiedliche Helden auswählen.");
        }
    }
    let mut playstyles = HashSet::new();
    for playstyle in &mut p.playstyles {
        *playstyle = playstyle.trim().to_ascii_lowercase();
        if !PLAYSTYLE_KEYS.contains(&playstyle.as_str()) || !playstyles.insert(playstyle.clone()) {
            return Err("Ungültiger Spielstil im Profil.");
        }
    }
    let mut preferred_times = HashSet::new();
    for preferred_time in &mut p.preferred_times {
        *preferred_time = preferred_time.trim().to_ascii_lowercase();
        if !PREFERRED_TIME_KEYS.contains(&preferred_time.as_str())
            || !preferred_times.insert(preferred_time.clone())
        {
            return Err("Ungültige bevorzugte Streamzeit im Profil.");
        }
    }
    for social in &mut p.socials {
        social.url = social.url.trim().into();
        if !text(&mut social.label, 40)
            || social.label.is_empty()
            || social.url.len() > 2048
            || !safe_url(&social.url)
        {
            return Err(
                "Jeder Social-Link braucht einen Namen und eine öffentliche HTTPS-Adresse.",
            );
        }
    }
    let mut logins = HashSet::new();
    for login in &mut p.featured {
        *login = valid_login(login.trim_start_matches('@'))
            .ok_or("Ungültiger Kanalname in den Partnerempfehlungen.")?;
        if !logins.insert(login.clone()) {
            return Err("Partner bitte nur einmal empfehlen.");
        }
    }
    let mut ids = HashSet::new();
    for event in &mut p.events {
        if !ids.insert(event.id) || event.id.is_nil() {
            return Err("Jeder Termin braucht eine eindeutige Kennung.");
        }
        if !text(&mut event.title, 120)
            || event.title.is_empty()
            || !text(&mut event.description, 1000)
        {
            return Err("Termine brauchen einen Titel bis 120 Zeichen und höchstens 1000 Zeichen Beschreibung.");
        }
        if event.ends_at <= event.starts_at
            || event.ends_at - event.starts_at > Duration::hours(48)
            || event.starts_at.year() < 2000
            || event.ends_at.year() > 2100
        {
            return Err("Termine müssen zwischen 2000 und 2100 liegen und länger als null, aber höchstens 48 Stunden dauern.");
        }
    }
    p.events.sort_by_key(|event| event.starts_at);
    Ok(())
}

async fn load(pool: &PgPool, login: &str) -> Result<Option<Record>, sqlx::Error> {
    sqlx::query_as::<_, Record>(&format!(
        r#"
        SELECT p.twitch_user_id, lower(p.twitch_login) AS login, ({ACTIVE}) AS active,
          COALESCE(d.published,false) AS published, COALESCE(d.revision,0) AS revision,
          COALESCE(d.content,'{{}}'::jsonb) AS content,
          COALESCE(l.is_live,0)::int AS is_live, l.last_seen_at, l.last_game
        FROM twitch_partners p
        LEFT JOIN twitch_partner_profiles d USING(twitch_user_id)
        LEFT JOIN twitch_live_state l USING(twitch_user_id)
        WHERE lower(p.twitch_login)=$1 LIMIT 1
    "#
    ))
    .bind(login)
    .fetch_optional(pool)
    .await
}
#[allow(clippy::result_large_err)]
fn owner(auth: &DashboardAuthLevel, params: &OwnerParams) -> Result<(String, String), Response> {
    // Reject cross-account requests, rather than silently saving to the owner.
    if let DashboardAuthLevel::Partner { twitch_login, .. } = auth {
        if params
            .streamer
            .as_deref()
            .is_some_and(|s| !s.trim().eq_ignore_ascii_case(twitch_login))
        {
            return Err(error(
                StatusCode::FORBIDDEN,
                "Du kannst nur dein eigenes Profil bearbeiten.",
            ));
        }
    }
    let (login, id) = resolve_settings_target(auth, &params.streamer)?;
    let login = valid_login(&login)
        .ok_or_else(|| error(StatusCode::BAD_REQUEST, "Ungültiger Twitch-Kanal."))?;
    Ok((login, id))
}
fn owner_response(record: Record, twitch: Option<TwitchProfileSnapshot>) -> Response {
    no_store(Json(json!({
        "login": record.login,
        "public_path": format!("/streamer/{}", record.login),
        "active": record.active,
        "published": record.published,
        "revision": record.revision,
        "profile": record.content.0,
        "twitch": twitch,
    })).into_response())
}

async fn twitch_snapshot(
    cache: Option<Extension<TwitchProfileCache>>,
    record: &Record,
    now: DateTime<Utc>,
    force_refresh: bool,
) -> TwitchProfileSnapshot {
    let Some(Extension(cache)) = cache else {
        return TwitchProfileSnapshot::default();
    };
    tokio::time::timeout(
        StdDuration::from_secs(4),
        cache.snapshot(&record.twitch_user_id, &record.login, now, force_refresh),
    )
    .await
    .unwrap_or_default()
}

pub async fn get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<OwnerParams>,
    cache: Option<Extension<TwitchProfileCache>>,
) -> Response {
    let (login, id) = match owner(&auth, &params) {
        Ok(v) => v,
        Err(r) => return no_store(r),
    };
    match load(&pool, &login).await {
        Ok(Some(record)) if id.is_empty() || record.twitch_user_id == id => {
            let twitch = twitch_snapshot(
                cache,
                &record,
                Utc::now(),
                params.refresh_twitch.unwrap_or(false),
            )
            .await;
            owner_response(record, Some(twitch))
        }
        Ok(_) => error(
            StatusCode::NOT_FOUND,
            "Kein Partnerprofil für diesen Kanal gefunden.",
        ),
        Err(err) => unavailable(err),
    }
}
pub async fn put_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<OwnerParams>,
    cache: Option<Extension<TwitchProfileCache>>,
    Json(mut update): Json<Update>,
) -> Response {
    let (login, id) = match owner(&auth, &params) {
        Ok(v) => v,
        Err(r) => return no_store(r),
    };
    if update.published && profile_login(&login).is_none() {
        return error(
            StatusCode::CONFLICT,
            "Diese Profiladresse ist bereits für eine Website-Seite reserviert.",
        );
    }
    if let Err(message) = validate(&mut update) {
        return error(StatusCode::BAD_REQUEST, message);
    }
    let record = match load(&pool, &login).await {
        Ok(Some(record)) if id.is_empty() || record.twitch_user_id == id => record,
        Ok(_) => {
            return error(
                StatusCode::NOT_FOUND,
                "Kein Partnerprofil für diesen Kanal gefunden.",
            )
        }
        Err(err) => return unavailable(err),
    };
    if !record.active {
        return error(StatusCode::FORBIDDEN, "Dein Profil bleibt gespeichert. Bearbeiten und Veröffentlichen ist möglich, sobald dein Bot wieder aktiv ist.");
    }
    if record.revision != update.revision {
        return error(StatusCode::CONFLICT, "Das Profil wurde inzwischen geändert. Bitte den aktuellen Stand laden, bevor du erneut speicherst.");
    }
    // One atomic compare-and-swap, including every calendar entry. No partial
    // saves, no last-write-wins race between tabs, no user-controlled SQL.
    let revision = sqlx::query_scalar::<_, i64>(&format!(
        r#"
        INSERT INTO twitch_partner_profiles (twitch_user_id,published,content)
        SELECT p.twitch_user_id,$2,$3 FROM twitch_partners p
        WHERE p.twitch_user_id=$1 AND {ACTIVE}
          AND ($4=0 OR EXISTS(SELECT 1 FROM twitch_partner_profiles WHERE twitch_user_id=$1))
        ON CONFLICT(twitch_user_id) DO UPDATE SET published=EXCLUDED.published,
          content=EXCLUDED.content, revision=twitch_partner_profiles.revision+1, updated_at=now()
        WHERE twitch_partner_profiles.revision=$4
        RETURNING revision
    "#
    ))
    .bind(&record.twitch_user_id)
    .bind(update.published)
    .bind(SqlJson(&update.profile))
    .bind(update.revision)
    .fetch_optional(&pool)
    .await;
    match revision {
        Ok(Some(revision)) => {
            let saved = Record {
                revision,
                published: update.published,
                content: SqlJson(update.profile),
                ..record
            };
            let twitch = twitch_snapshot(cache, &saved, Utc::now(), false).await;
            owner_response(saved, Some(twitch))
        },
        Ok(None) => error(
            StatusCode::CONFLICT,
            "Der Profil- oder Partnerstatus wurde geändert. Bitte neu laden.",
        ),
        Err(err) => unavailable(err),
    }
}
async fn directory(pool: &PgPool) -> Result<Vec<DirectoryEntry>, sqlx::Error> {
    sqlx::query_as::<_, DirectoryEntry>(&format!("SELECT lower(p.twitch_login) AS login, COALESCE(d.content->>'headline','') AS headline FROM twitch_partners p JOIN twitch_partner_profiles d USING(twitch_user_id) WHERE {ACTIVE} AND d.published AND NOT(lower(p.twitch_login)=ANY($1)) ORDER BY lower(p.twitch_login) LIMIT 500"))
        .bind(RESERVED_PROFILE_LOGINS).fetch_all(pool).await
}
pub async fn directory_handler(State(pool): State<PgPool>) -> Response {
    match directory(&pool).await {
        Ok(entries) => no_store(Json(json!({"profiles": entries})).into_response()),
        Err(err) => unavailable(err),
    }
}
fn month_range(
    raw: Option<&str>,
    now: DateTime<Utc>,
) -> Option<(NaiveDate, DateTime<Utc>, DateTime<Utc>)> {
    let date = match raw {
        Some(raw) if raw.len() == 7 => {
            NaiveDate::parse_from_str(&format!("{raw}-01"), "%Y-%m-%d").ok()?
        }
        Some(_) => return None,
        None => now.with_timezone(&Berlin).date_naive().with_day(1)?,
    };
    if !(2000..=2100).contains(&date.year()) {
        return None;
    }
    let next = date.checked_add_months(chrono::Months::new(1))?;
    Some((
        date,
        Berlin
            .from_local_datetime(&date.and_hms_opt(0, 0, 0)?)
            .single()?
            .with_timezone(&Utc),
        Berlin
            .from_local_datetime(&next.and_hms_opt(0, 0, 0)?)
            .single()?
            .with_timezone(&Utc),
    ))
}
async fn history(
    pool: &PgPool,
    user_id: &str,
    since: DateTime<Utc>,
    until: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<Vec<Session>, sqlx::Error> {
    sqlx::query_as::<_, Session>(r#"
        SELECT lower(streamer_login) AS streamer_login, started_at, ended_at, game_name, left(stream_title,500) AS stream_title
        FROM twitch_stream_sessions
        WHERE twitch_user_id=$1 AND ended_at>$2 AND started_at<$3
          AND ended_at<=$4 AND ended_at>started_at AND ended_at-started_at<=INTERVAL '48 hours'
        ORDER BY started_at DESC LIMIT 2001
    "#).bind(user_id).bind(since).bind(until).bind(now).fetch_all(pool).await
}
fn is_live(record: &Record, now: DateTime<Utc>) -> bool {
    record.is_live == 1
        && record
            .last_seen_at
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .is_some_and(|seen| {
                seen >= now - Duration::minutes(5) && seen <= now + Duration::seconds(5)
            })
}
/// One wildcard route owns both public profiles and existing website assets.
/// Axum cannot merge a catch-all with a sibling parameter route at this path.
pub async fn streamer_handler(
    State(pool): State<PgPool>,
    Path(path): Path<String>,
    OriginalUri(uri): OriginalUri,
    profile_cache: Option<Extension<TwitchProfileCache>>,
) -> Response {
    let handle = path.strip_suffix('/').unwrap_or(&path);
    if profile_login(handle).is_some() || handle.starts_with('@') {
        let params = match Query::<PageParams>::try_from_uri(&uri) {
            Ok(Query(params)) => params,
            Err(err) => return no_store(err.into_response()),
        };
        return page_handler_with_twitch(
            State(pool),
            Path(handle.to_string()),
            Query(params),
            profile_cache,
        )
        .await;
    }
    // Existing pages/assets keep their original query handling and traversal guard.
    super::website::streamer_asset_handler(Path(path)).await
}
pub async fn page_handler(
    state: State<PgPool>,
    path: Path<String>,
    query: Query<PageParams>,
) -> Response {
    page_handler_with_twitch(state, path, query, None).await
}
async fn page_handler_with_twitch(
    State(pool): State<PgPool>,
    Path(handle): Path<String>,
    Query(params): Query<PageParams>,
    profile_cache: Option<Extension<TwitchProfileCache>>,
) -> Response {
    let Some(login) = profile_login(&handle) else {
        return html::missing(StatusCode::NOT_FOUND);
    };
    let record = match load(&pool, &login).await {
        Ok(Some(record)) if record.active && record.published => record,
        Ok(_) => return html::missing(StatusCode::NOT_FOUND),
        Err(err) => {
            tracing::warn!(%err,"Public profile unavailable");
            return html::missing(StatusCode::SERVICE_UNAVAILABLE);
        }
    };
    let now = Utc::now();
    let twitch = twitch_snapshot(profile_cache, &record, now, false).await;
    let Some((month, start, end)) = month_range(params.month.as_deref(), now) else {
        return html::missing(StatusCode::BAD_REQUEST);
    };
    let recent_start = now - Duration::days(90);
    let mut recent = vec![];
    let mut monthly = vec![];
    if record.content.show_history {
        let result = tokio::try_join!(
            history(&pool, &record.twitch_user_id, recent_start, now, now),
            history(&pool, &record.twitch_user_id, start, end, now)
        );
        match result {
            Ok((a, b)) => {
                recent = a;
                monthly = b;
            }
            Err(err) => {
                tracing::warn!(%err,"Public profile history unavailable");
                return html::missing(StatusCode::SERVICE_UNAVAILABLE);
            }
        }
    }
    let entries = match directory(&pool).await {
        Ok(v) => v,
        Err(err) => {
            tracing::warn!(%err,"Profile directory unavailable");
            return html::missing(StatusCode::SERVICE_UNAVAILABLE);
        }
    };
    // Re-check visibility after the secondary reads. Never render a withdrawn
    // profile out of the loaded snapshot following a concurrent disconnect/save.
    match load(&pool, &login).await {
        Ok(Some(current))
            if current.active && current.published && current.revision == record.revision => {}
        Ok(_) => return html::missing(StatusCode::NOT_FOUND),
        Err(_) => return html::missing(StatusCode::SERVICE_UNAVAILABLE),
    }
    let observed = schedule(&recent, recent_start, now);
    html::page(
        &record,
        &twitch,
        &observed,
        &monthly,
        &entries,
        month,
        now,
        recent.len() > 2000 || monthly.len() > 2000,
    )
}
pub async fn css_handler() -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        include_str!("partner_profiles/profile.css"),
    )
        .into_response()
}
