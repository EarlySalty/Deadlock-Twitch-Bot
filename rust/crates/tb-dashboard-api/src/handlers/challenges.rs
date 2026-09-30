use crate::auth::level::DashboardAuthLevel;
use axum::{
    extract::{Extension, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Datelike, Duration as ChronoDuration, NaiveDate, TimeZone, Utc};
use chrono_tz::Europe::Berlin;
use futures_util::{stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    ConnectOptions, PgPool, Row,
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    str::FromStr,
    sync::{Arc, OnceLock},
    time::Duration,
};
use tb_transport_twitch::HelixClient;
use tokio::sync::OnceCell;

static CENTRAL_POOL: OnceCell<PgPool> = OnceCell::const_new();

#[derive(Clone)]
pub struct ChallengeHelix(pub Option<HelixClient>);

#[derive(Debug, Deserialize)]
struct ChallengeConfig {
    points: PointConfig,
    weekly_caps: WeeklyCaps,
    levels: LevelConfig,
    quests: QuestConfig,
    achievements: AchievementConfig,
}

#[derive(Debug, Deserialize)]
struct PointConfig {
    qualified_invite: i32,
    streamer_referral: i32,
    co_stream: i32,
    party_play: i32,
    clip_submitted: i32,
    clip_top3: [i32; 3],
    quest_done: i32,
    quest_all_three_bonus: i32,
}

#[derive(Debug, Deserialize)]
struct WeeklyCaps {
    party_play: i64,
    clip_submitted: i64,
}

#[derive(Debug, Deserialize)]
struct LevelConfig {
    thresholds: Vec<i64>,
}

#[derive(Debug, Deserialize)]
struct QuestConfig {
    stream_extra_minutes: i64,
}

#[derive(Debug, Deserialize)]
struct AchievementConfig {
    recruiter: Vec<i64>,
    team_player: Vec<i64>,
    duo: Vec<i64>,
    stamina: Vec<i64>,
    talent_scout: Vec<i64>,
    clip_hunter: Vec<i64>,
}

fn config() -> &'static ChallengeConfig {
    static CONFIG: OnceLock<ChallengeConfig> = OnceLock::new();
    CONFIG.get_or_init(|| {
        serde_json::from_str(include_str!("../../partner_challenges.cfg"))
            .expect("partner_challenges.cfg must be valid")
    })
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Params {
    pub streamer: Option<String>,
}

#[derive(Clone)]
struct Partner {
    twitch_user_id: String,
    login: String,
}

#[derive(Debug, Clone)]
struct EventInput {
    partner_twitch_user_id: String,
    partner_login: String,
    event_type: &'static str,
    source_id: String,
    occurred_at: DateTime<Utc>,
    viewer_twitch_user_id: Option<String>,
    metadata: Value,
}

#[derive(Debug, Serialize)]
struct QuestResponse {
    key: &'static str,
    text: String,
    progress: i64,
    goal: i64,
    completed: bool,
}

#[derive(Debug, Serialize)]
struct StreakResponse {
    current: i32,
    longest: i32,
}

#[derive(Debug, Serialize)]
struct LevelResponse {
    level: usize,
    total_points: i64,
    current_threshold: i64,
    next_threshold: Option<i64>,
}

#[derive(Debug, Serialize)]
struct NextGoalResponse {
    missing_points: i64,
    fastest_route: String,
}

#[derive(Debug, Serialize)]
struct AchievementTier {
    target: i64,
    unlocked: bool,
}

#[derive(Debug, Serialize)]
struct AchievementResponse {
    key: &'static str,
    name: &'static str,
    progress: i64,
    tiers: Vec<AchievementTier>,
}

#[derive(Debug, Serialize)]
struct WithUsResponse {
    people_brought_in_who_stayed: i64,
    community_hours: f64,
    received_raids: i64,
}

#[derive(Debug, Serialize)]
struct SeasonResponse {
    month: String,
    points: i64,
    rank: i64,
    active_partners: i64,
}

#[derive(Debug, Serialize)]
struct MeResponse {
    generated_at: DateTime<Utc>,
    timezone: &'static str,
    streamer: String,
    quests: Vec<QuestResponse>,
    streak: StreakResponse,
    level: LevelResponse,
    next_goal: NextGoalResponse,
    achievements: Vec<AchievementResponse>,
    with_us: WithUsResponse,
    season: SeasonResponse,
}

#[derive(Debug, Serialize)]
struct ViewerRecruiter {
    twitch_user_id: String,
    display_name: Option<String>,
    qualified_invites: i64,
}

#[derive(Debug, Serialize)]
struct ViewersResponse {
    generated_at: DateTime<Utc>,
    streamer: String,
    recruiters: Vec<ViewerRecruiter>,
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        [(header::CACHE_CONTROL, "private, no-store")],
        Json(json!({"error": code, "message": message})),
    )
        .into_response()
}

fn success(value: impl Serialize) -> Response {
    ([(header::CACHE_CONTROL, "private, no-store")], Json(value)).into_response()
}

fn active_partner_filter() -> &'static str {
    "status='active' AND departnered_at IS NULL AND admin_archived_at IS NULL AND COALESCE(manual_partner_opt_out,0)=0 AND COALESCE(trim(technical_pause_reason),'')=''"
}

async fn partner_by_login(pool: &PgPool, login: &str) -> Result<Option<Partner>, sqlx::Error> {
    let sql = format!(
        "SELECT twitch_user_id, lower(twitch_login) AS twitch_login FROM twitch_partners WHERE lower(twitch_login)=$1 AND {} LIMIT 1",
        active_partner_filter()
    );
    sqlx::query_as::<_, (String, String)>(&sql)
        .bind(login)
        .fetch_optional(pool)
        .await
        .map(|row| {
            row.map(|(twitch_user_id, login)| Partner {
                twitch_user_id,
                login,
            })
        })
}

async fn subject(
    auth: &DashboardAuthLevel,
    requested: Option<&str>,
    pool: &PgPool,
) -> Result<Partner, Response> {
    let requested = requested
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_lowercase);
    match auth {
        DashboardAuthLevel::None => Err(error(
            StatusCode::UNAUTHORIZED,
            "login_required",
            "Bitte mit Twitch anmelden.",
        )),
        DashboardAuthLevel::Partner {
            twitch_login,
            twitch_user_id,
            ..
        } => {
            if requested
                .as_deref()
                .is_some_and(|value| !value.eq_ignore_ascii_case(twitch_login))
            {
                return Err(error(
                    StatusCode::FORBIDDEN,
                    "self_only",
                    "Challenges sind nur für deinen eigenen Kanal verfügbar.",
                ));
            }
            Ok(Partner {
                twitch_user_id: twitch_user_id.clone(),
                login: twitch_login.to_lowercase(),
            })
        }
        DashboardAuthLevel::Admin { actor } => {
            let login = requested.or_else(|| actor.as_ref().map(|a| a.twitch_login.clone()));
            let Some(login) = login else {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "streamer_required",
                    "Bitte einen Streamer auswählen.",
                ));
            };
            match partner_by_login(pool, &login).await {
                Ok(Some(partner)) => Ok(partner),
                Ok(None) => Err(error(
                    StatusCode::NOT_FOUND,
                    "partner_not_found",
                    "Dieser Kanal ist derzeit kein aktiver Partner.",
                )),
                Err(err) => {
                    tracing::warn!(%err, "Challenge-Partner konnte nicht geladen werden");
                    Err(error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "database_unavailable",
                        "Challenge-Daten sind gerade nicht verfügbar.",
                    ))
                }
            }
        }
    }
}

fn berlin_week_start(at: DateTime<Utc>) -> NaiveDate {
    let local = at.with_timezone(&Berlin);
    local.date_naive() - ChronoDuration::days(i64::from(local.weekday().num_days_from_monday()))
}

fn berlin_week_bounds(at: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>, NaiveDate) {
    let date = berlin_week_start(at);
    let start = Berlin
        .from_local_datetime(&date.and_hms_opt(0, 0, 0).expect("midnight"))
        .earliest()
        .expect("Berlin midnight")
        .with_timezone(&Utc);
    let next_date = date + ChronoDuration::days(7);
    let end = Berlin
        .from_local_datetime(&next_date.and_hms_opt(0, 0, 0).expect("midnight"))
        .earliest()
        .expect("Berlin midnight")
        .with_timezone(&Utc);
    (start, end, date)
}

fn berlin_month_bounds(at: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>, String) {
    let local = at.with_timezone(&Berlin);
    let start_date = NaiveDate::from_ymd_opt(local.year(), local.month(), 1).expect("valid month");
    let (next_year, next_month) = if local.month() == 12 {
        (local.year() + 1, 1)
    } else {
        (local.year(), local.month() + 1)
    };
    let end_date = NaiveDate::from_ymd_opt(next_year, next_month, 1).expect("valid next month");
    let start = Berlin
        .from_local_datetime(&start_date.and_hms_opt(0, 0, 0).expect("midnight"))
        .earliest()
        .expect("Berlin midnight")
        .with_timezone(&Utc);
    let end = Berlin
        .from_local_datetime(&end_date.and_hms_opt(0, 0, 0).expect("midnight"))
        .earliest()
        .expect("Berlin midnight")
        .with_timezone(&Utc);
    (
        start,
        end,
        format!("{:04}-{:02}", local.year(), local.month()),
    )
}

fn points_for(event_type: &str, metadata: &Value) -> Option<i32> {
    let points = &config().points;
    match event_type {
        "qualified_invite" => Some(points.qualified_invite),
        "streamer_referral" => Some(points.streamer_referral),
        "co_stream" => Some(points.co_stream),
        "party_play" => Some(points.party_play),
        "clip_submitted" => Some(points.clip_submitted),
        "clip_top3" => metadata
            .get("rank")
            .and_then(Value::as_u64)
            .and_then(|rank| points.clip_top3.get(rank.saturating_sub(1) as usize))
            .copied(),
        "quest_done" if metadata.get("bonus").and_then(Value::as_str) == Some("all_three") => {
            Some(points.quest_all_three_bonus)
        }
        "quest_done" => Some(points.quest_done),
        _ => None,
    }
}

fn weekly_cap(event_type: &str) -> Option<i64> {
    match event_type {
        "party_play" => Some(config().weekly_caps.party_play),
        "clip_submitted" => Some(config().weekly_caps.clip_submitted),
        _ => None,
    }
}

async fn insert_event(pool: &PgPool, event: EventInput) -> Result<bool, sqlx::Error> {
    let Some(base_points) = points_for(event.event_type, &event.metadata) else {
        tracing::warn!(
            event_type = event.event_type,
            "Ungültiges Effort-Ereignis verworfen"
        );
        return Ok(false);
    };
    let mut tx = pool.begin().await?;
    let mut awarded = base_points;
    if let Some(cap) = weekly_cap(event.event_type) {
        let (week_start, week_end, week_date) = berlin_week_bounds(event.occurred_at);
        let lock_key = format!(
            "{}|{}|{}",
            event.partner_twitch_user_id, event.event_type, week_date
        );
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
            .bind(lock_key)
            .execute(&mut *tx)
            .await?;
        let existing: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM partner_effort_events
             WHERE partner_twitch_user_id=$1 AND event_type=$2
               AND occurred_at >= $3 AND occurred_at < $4 AND points > 0",
        )
        .bind(&event.partner_twitch_user_id)
        .bind(event.event_type)
        .bind(week_start)
        .bind(week_end)
        .fetch_one(&mut *tx)
        .await?;
        if existing >= cap {
            awarded = 0;
        }
    }
    let metadata = serde_json::to_string(&event.metadata).unwrap_or_else(|_| "{}".to_string());
    let inserted = sqlx::query(
        "INSERT INTO partner_effort_events
         (partner_twitch_user_id, partner_login, event_type, source_id, points, occurred_at, viewer_twitch_user_id, metadata)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8::text::jsonb)
         ON CONFLICT (partner_twitch_user_id,event_type,source_id) DO NOTHING",
    )
    .bind(&event.partner_twitch_user_id)
    .bind(&event.partner_login)
    .bind(event.event_type)
    .bind(&event.source_id)
    .bind(awarded)
    .bind(event.occurred_at)
    .bind(event.viewer_twitch_user_id.as_deref())
    .bind(metadata)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        == 1;
    tx.commit().await?;
    Ok(inserted)
}

async fn central_pool() -> Result<PgPool, String> {
    CENTRAL_POOL
        .get_or_try_init(|| async {
            let dsn = std::env::var("DEADLOCK_CENTRAL_DSN")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "central database is not configured".to_string())?;
            let options = PgConnectOptions::from_str(&dsn)
                .map_err(|_| "central database configuration is invalid".to_string())?
                .application_name("twitch-partner-effort")
                .options([
                    ("default_transaction_read_only", "on"),
                    ("statement_timeout", "10000"),
                ])
                .disable_statement_logging();
            PgPoolOptions::new()
                .max_connections(3)
                .acquire_timeout(Duration::from_secs(5))
                .idle_timeout(Duration::from_secs(60))
                .connect_with(options)
                .await
                .map_err(|_| "central database is unavailable".to_string())
        })
        .await
        .cloned()
}

#[derive(Deserialize)]
struct BrokerEnvelope {
    ok: bool,
    result: Option<QualifiedInviteResult>,
}

#[derive(Deserialize)]
struct QualifiedInviteResult {
    items: Vec<QualifiedInviteItem>,
}

#[derive(Deserialize)]
struct QualifiedInviteItem {
    streamer_login: String,
    inviter_twitch_user_id: Option<String>,
    joined_at: String,
    status: String,
    qualified_at: Option<String>,
}

fn broker_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .connect_timeout(Duration::from_secs(2))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("challenge broker client")
    })
}

async fn sync_qualified_invites(pool: &PgPool) -> Result<(), String> {
    let token = [
        "MASTER_BROKER_TOKEN",
        "MAIN_BOT_INTERNAL_TOKEN",
        "TWITCH_INTERNAL_API_TOKEN",
    ]
    .iter()
    .find_map(|key| {
        std::env::var(key)
            .ok()
            .filter(|value| !value.trim().is_empty())
    });
    let Some(token) = token else {
        return Ok(());
    };
    let Ok(runtime) = tb_config::runtime::settings() else {
        return Ok(());
    };
    let url = format!(
        "{}/internal/master/v1/twitch/qualified-invites",
        runtime.broker.base_url.trim_end_matches('/')
    );
    let response = broker_client()
        .get(url)
        .header("X-Internal-Token", token)
        .query(&[("since", "1970-01-01T00:00:00Z")])
        .send()
        .await
        .map_err(|_| "qualified invite broker request failed".to_string())?;
    if response.status() == reqwest::StatusCode::NOT_FOUND
        || response.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE
    {
        return Ok(());
    }
    let response = response
        .error_for_status()
        .map_err(|_| "qualified invite broker returned an error".to_string())?;
    let envelope = response
        .json::<BrokerEnvelope>()
        .await
        .map_err(|_| "qualified invite broker response was invalid".to_string())?;
    if !envelope.ok {
        return Ok(());
    }
    let Some(result) = envelope.result else {
        return Ok(());
    };
    let mut items = result
        .items
        .into_iter()
        .filter(|item| item.status == "qualified")
        .collect::<Vec<_>>();
    items.sort_by(|a, b| {
        a.qualified_at
            .cmp(&b.qualified_at)
            .then(a.joined_at.cmp(&b.joined_at))
    });
    for item in items {
        let login = item.streamer_login.trim().to_lowercase();
        let Some(partner) = sqlx::query_as::<_, (String, String)>(
            "SELECT twitch_user_id, lower(twitch_login) FROM twitch_partners WHERE lower(twitch_login)=$1 LIMIT 1",
        )
        .bind(&login)
        .fetch_optional(pool)
        .await
        .map_err(|_| "partner lookup failed".to_string())?
        .map(|(twitch_user_id, login)| Partner {
            twitch_user_id,
            login,
        }) else {
            continue;
        };
        let Some(occurred_at) = item
            .qualified_at
            .as_deref()
            .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
            .map(|value| value.with_timezone(&Utc))
        else {
            continue;
        };
        let source_material = format!(
            "{}|{}|{}",
            partner.login,
            item.inviter_twitch_user_id.as_deref().unwrap_or("channel"),
            item.joined_at
        );
        let source_id = format!(
            "qualified-invite:{:x}",
            Sha256::digest(source_material.as_bytes())
        );
        insert_event(
            pool,
            EventInput {
                partner_twitch_user_id: partner.twitch_user_id,
                partner_login: partner.login,
                event_type: "qualified_invite",
                source_id,
                occurred_at,
                viewer_twitch_user_id: item.inviter_twitch_user_id,
                metadata: json!({"joined_at": item.joined_at}),
            },
        )
        .await
        .map_err(|_| "qualified invite event insert failed".to_string())?;
    }
    Ok(())
}

async fn sync_clip_outbox(pool: &PgPool) -> Result<(), sqlx::Error> {
    let exists: Option<String> =
        sqlx::query_scalar("SELECT to_regclass('public.twitch_clip_contest_effort_outbox')::text")
            .fetch_one(pool)
            .await?;
    if exists.is_none() {
        return Ok(());
    }
    let rows = sqlx::query(
        "SELECT event_type, streamer_login, source_id, occurred_at, payload::text AS payload
         FROM public.twitch_clip_contest_effort_outbox
         ORDER BY occurred_at, id",
    )
    .fetch_all(pool)
    .await?;
    for row in rows {
        let event_type: String = row.try_get("event_type")?;
        let static_type = match event_type.as_str() {
            "clip_submitted" => "clip_submitted",
            "clip_top3" => "clip_top3",
            _ => continue,
        };
        let login: String = row.try_get::<String, _>("streamer_login")?.to_lowercase();
        let Some(partner) = sqlx::query_as::<_, (String, String)>(
            "SELECT twitch_user_id, lower(twitch_login) FROM twitch_partners WHERE lower(twitch_login)=$1 LIMIT 1",
        )
        .bind(&login)
        .fetch_optional(pool)
        .await?
        .map(|(twitch_user_id, login)| Partner {
            twitch_user_id,
            login,
        }) else {
            continue;
        };
        let payload_raw: String = row.try_get("payload")?;
        let payload = serde_json::from_str::<Value>(&payload_raw).unwrap_or_else(|_| json!({}));
        let source_id: String = row.try_get("source_id")?;
        let occurred_at: DateTime<Utc> = row.try_get("occurred_at")?;
        insert_event(
            pool,
            EventInput {
                partner_twitch_user_id: partner.twitch_user_id,
                partner_login: partner.login,
                event_type: static_type,
                source_id,
                occurred_at,
                viewer_twitch_user_id: None,
                metadata: payload,
            },
        )
        .await?;
    }
    Ok(())
}

async fn sync_referrals(pool: &PgPool) -> Result<(), sqlx::Error> {
    let rows = sqlx::query(
        "SELECT twitch_user_id,
                lower(twitch_login) AS login,
                added_by,
                CASE
                    WHEN NULLIF(trim(partnered_at),'') IS NULL THEN NULL
                    ELSE partnered_at::timestamptz
                END AS partnered_at
         FROM twitch_partners
         ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    let mut by_login = HashMap::new();
    let mut by_id = HashMap::new();
    let mut candidates = Vec::new();
    for row in rows {
        let twitch_user_id: String = row.try_get("twitch_user_id")?;
        let login: String = row.try_get("login")?;
        by_login.insert(login.clone(), (twitch_user_id.clone(), login.clone()));
        by_id.insert(
            twitch_user_id.clone(),
            (twitch_user_id.clone(), login.clone()),
        );
        candidates.push((
            twitch_user_id,
            login,
            row.try_get::<Option<String>, _>("added_by")?,
            row.try_get::<Option<DateTime<Utc>>, _>("partnered_at")?,
        ));
    }
    for (referred_id, referred_login, added_by, partnered_at) in candidates {
        let (Some(mut referrer_key), Some(occurred_at)) = (added_by, partnered_at) else {
            continue;
        };
        referrer_key = referrer_key.trim().to_lowercase();
        if let Some(value) = referrer_key.strip_prefix("twitch:") {
            referrer_key = value.to_string();
        }
        if let Some(value) = referrer_key.strip_prefix('@') {
            referrer_key = value.to_string();
        }
        let referrer = by_login
            .get(&referrer_key)
            .or_else(|| by_id.get(&referrer_key))
            .cloned();
        let Some((referrer_id, referrer_login)) = referrer else {
            continue;
        };
        if referrer_id == referred_id {
            continue;
        }
        insert_event(
            pool,
            EventInput {
                partner_twitch_user_id: referrer_id,
                partner_login: referrer_login,
                event_type: "streamer_referral",
                source_id: format!("partner:{referred_id}"),
                occurred_at,
                viewer_twitch_user_id: None,
                metadata: json!({
                    "referred_twitch_user_id": referred_id,
                    "referred_login": referred_login
                }),
            },
        )
        .await?;
    }
    Ok(())
}

async fn live_deadlock_partners(
    pool: &PgPool,
) -> Result<Vec<(Partner, i64, DateTime<Utc>)>, sqlx::Error> {
    let sql = format!(
        "SELECT p.twitch_user_id, lower(p.twitch_login) AS login,
                s.id::bigint, s.started_at, COALESCE(l.is_live,0)::int AS is_live,
                l.last_seen_at, l.last_game
         FROM twitch_partners p
         JOIN LATERAL (
             SELECT id, started_at
             FROM twitch_stream_sessions
             WHERE ended_at IS NULL
               AND (twitch_user_id=p.twitch_user_id OR lower(streamer_login)=lower(p.twitch_login))
             ORDER BY started_at DESC
             LIMIT 1
         ) s ON TRUE
         LEFT JOIN twitch_live_state l ON l.twitch_user_id=p.twitch_user_id
         WHERE {}",
        active_partner_filter()
    );
    let rows = sqlx::query(&sql).fetch_all(pool).await?;
    let now = Utc::now();
    let mut result = Vec::new();
    for row in rows {
        let is_live: i32 = row.try_get("is_live")?;
        let last_seen_at: Option<String> = row.try_get("last_seen_at")?;
        let last_game: Option<String> = row.try_get("last_game")?;
        let fresh = last_seen_at
            .as_deref()
            .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
            .map(|value| value.with_timezone(&Utc))
            .is_some_and(|seen| {
                seen <= now + ChronoDuration::seconds(5) && seen >= now - ChronoDuration::minutes(5)
            });
        if is_live != 1
            || !fresh
            || !last_game
                .as_deref()
                .is_some_and(|game| game.eq_ignore_ascii_case("Deadlock"))
        {
            continue;
        }
        result.push((
            Partner {
                twitch_user_id: row.try_get("twitch_user_id")?,
                login: row.try_get("login")?,
            },
            row.try_get("id")?,
            row.try_get("started_at")?,
        ));
    }
    Ok(result)
}

async fn collect_shared_chat(pool: &PgPool, helix: &HelixClient) -> Result<(), String> {
    let live = live_deadlock_partners(pool)
        .await
        .map_err(|_| "live partner query failed".to_string())?;
    let active_ids = live
        .iter()
        .map(|(partner, _, _)| partner.twitch_user_id.clone())
        .collect::<HashSet<_>>();
    let now = Utc::now();
    let mut pending = stream::iter(live.into_iter())
        .map(|(partner, session_id, started_at)| {
            let helix = helix.clone();
            async move {
                let users = helix
                    .get_shared_chat_users(&partner.twitch_user_id)
                    .await
                    .map_err(|_| "shared chat request failed".to_string())?;
                Ok::<_, String>((partner, session_id, started_at, users))
            }
        })
        .buffer_unordered(4);
    while let Some(item) = pending.next().await {
        let (partner, stream_session_id, stream_started_at, users) = item?;
        if now - stream_started_at < ChronoDuration::minutes(30) {
            continue;
        }
        for other in users
            .into_iter()
            .filter(|user| active_ids.contains(&user.id))
        {
            let first_seen: DateTime<Utc> = sqlx::query_scalar(
                "INSERT INTO partner_effort_shared_chat_observations
                 (partner_twitch_user_id,stream_session_id,other_partner_twitch_user_id,first_seen_at,last_seen_at)
                 VALUES ($1,$2,$3,$4,$4)
                 ON CONFLICT (partner_twitch_user_id,stream_session_id,other_partner_twitch_user_id)
                 DO UPDATE SET last_seen_at=EXCLUDED.last_seen_at
                 RETURNING first_seen_at",
            )
            .bind(&partner.twitch_user_id)
            .bind(stream_session_id)
            .bind(&other.id)
            .bind(now)
            .fetch_one(pool)
            .await
            .map_err(|_| "shared chat observation failed".to_string())?;
            if now - first_seen >= ChronoDuration::minutes(30) {
                insert_event(
                    pool,
                    EventInput {
                        partner_twitch_user_id: partner.twitch_user_id.clone(),
                        partner_login: partner.login.clone(),
                        event_type: "co_stream",
                        source_id: format!("stream:{stream_session_id}"),
                        occurred_at: now,
                        viewer_twitch_user_id: None,
                        metadata: json!({
                            "other_partner_twitch_user_id": other.id,
                            "stream_session_id": stream_session_id
                        }),
                    },
                )
                .await
                .map_err(|_| "co stream event insert failed".to_string())?;
            }
        }
    }
    Ok(())
}

async fn collect_party_play(pool: &PgPool) -> Result<(), String> {
    let live = live_deadlock_partners(pool)
        .await
        .map_err(|_| "live partner query failed".to_string())?;
    if live.is_empty() {
        return Ok(());
    }
    let identities = sqlx::query(
        "SELECT twitch_user_id, NULLIF(trim(discord_user_id),'') AS discord_id
         FROM twitch_streamer_identities
         WHERE NULLIF(trim(discord_user_id),'') IS NOT NULL",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| "community identity query failed".to_string())?;
    let mut twitch_by_discord: HashMap<i64, Vec<String>> = HashMap::new();
    for row in identities {
        let twitch_id: String = row
            .try_get("twitch_user_id")
            .map_err(|_| "community twitch id invalid".to_string())?;
        let discord_id: String = row
            .try_get("discord_id")
            .map_err(|_| "community discord id invalid".to_string())?;
        let Ok(discord_id) = discord_id.parse::<i64>() else {
            continue;
        };
        twitch_by_discord
            .entry(discord_id)
            .or_default()
            .push(twitch_id);
    }
    if twitch_by_discord.is_empty() {
        return Ok(());
    }
    let central = central_pool().await?;
    let discord_ids = twitch_by_discord.keys().copied().collect::<Vec<_>>();
    let link_rows = sqlx::query(
        "SELECT discord_id, steam_id
         FROM core.steam_links
         WHERE discord_id = ANY($1)
           AND steam_id <> ''
           AND verified = TRUE",
    )
    .bind(&discord_ids)
    .fetch_all(&central)
    .await
    .map_err(|_| "central steam link query failed".to_string())?;
    let mut steam_by_twitch: HashMap<String, Vec<String>> = HashMap::new();
    let mut all_steam = HashSet::new();
    for row in link_rows {
        let discord_id: i64 = row
            .try_get("discord_id")
            .map_err(|_| "central discord id invalid".to_string())?;
        let steam_id: String = row
            .try_get("steam_id")
            .map_err(|_| "central steam id invalid".to_string())?;
        let Some(twitch_ids) = twitch_by_discord.get(&discord_id) else {
            continue;
        };
        all_steam.insert(steam_id.clone());
        for twitch_id in twitch_ids {
            steam_by_twitch
                .entry(twitch_id.clone())
                .or_default()
                .push(steam_id.clone());
        }
    }
    if all_steam.len() < 2 {
        return Ok(());
    }
    let steam_ids = all_steam.iter().cloned().collect::<Vec<_>>();
    let fresh_after = Utc::now() - ChronoDuration::minutes(10);
    let rows = sqlx::query(
        "SELECT pm.party_id, pm.steam_id, pm.seen_at, COALESCE(lps.in_match_now_strict,FALSE) AS in_match
         FROM voice.deadlock_party_members pm
         LEFT JOIN activity.live_player_state lps ON lps.steam_id=pm.steam_id
         WHERE pm.steam_id = ANY($1) AND pm.seen_at >= $2",
    )
    .bind(&steam_ids)
    .bind(fresh_after)
    .fetch_all(&central)
    .await
    .map_err(|_| "central party query failed".to_string())?;
    let mut parties: HashMap<String, Vec<(String, bool)>> = HashMap::new();
    for row in rows {
        let party_id: String = row
            .try_get("party_id")
            .map_err(|_| "party id invalid".to_string())?;
        let steam_id: String = row
            .try_get("steam_id")
            .map_err(|_| "party steam id invalid".to_string())?;
        let in_match: bool = row
            .try_get("in_match")
            .map_err(|_| "party match state invalid".to_string())?;
        parties
            .entry(party_id)
            .or_default()
            .push((steam_id, in_match));
    }
    let now = Utc::now();
    for (partner, session_id, _) in live {
        let Some(partner_steam) = steam_by_twitch.get(&partner.twitch_user_id) else {
            continue;
        };
        for (party_id, members) in &parties {
            let partner_in_match = members
                .iter()
                .any(|(steam, in_match)| partner_steam.contains(steam) && *in_match);
            if !partner_in_match {
                continue;
            }
            let has_other = members
                .iter()
                .any(|(steam, _)| !partner_steam.contains(steam) && all_steam.contains(steam));
            if !has_other {
                continue;
            }
            insert_event(
                pool,
                EventInput {
                    partner_twitch_user_id: partner.twitch_user_id.clone(),
                    partner_login: partner.login.clone(),
                    event_type: "party_play",
                    source_id: format!("party:{party_id}:stream:{session_id}"),
                    occurred_at: now,
                    viewer_twitch_user_id: None,
                    metadata: json!({"party_id": party_id, "stream_session_id": session_id}),
                },
            )
            .await
            .map_err(|_| "party play event insert failed".to_string())?;
        }
    }
    Ok(())
}

async fn sync_sources(pool: &PgPool, helix: Option<&HelixClient>) -> Result<(), String> {
    sync_referrals(pool)
        .await
        .map_err(|_| "streamer referral database sync failed".to_string())?;
    sync_clip_outbox(pool)
        .await
        .map_err(|_| "clip effort database sync failed".to_string())?;
    sync_qualified_invites(pool).await?;
    if let Some(helix) = helix {
        collect_shared_chat(pool, helix).await?;
    }
    collect_party_play(pool).await?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuestKind {
    Party,
    CoStream,
    Invite,
    Clip,
    StreamExtra,
}

impl QuestKind {
    fn key(self) -> &'static str {
        match self {
            Self::Party => "community_match",
            Self::CoStream => "stream_together",
            Self::Invite => "active_discord_invite",
            Self::Clip => "submit_clip",
            Self::StreamExtra => "stream_above_average",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        match key {
            "community_match" => Some(Self::Party),
            "stream_together" => Some(Self::CoStream),
            "active_discord_invite" => Some(Self::Invite),
            "submit_clip" => Some(Self::Clip),
            "stream_above_average" => Some(Self::StreamExtra),
            _ => None,
        }
    }
}

fn deterministic_quests(
    partner_id: &str,
    week: NaiveDate,
    party_achievable: bool,
    co_stream_achievable: bool,
) -> Vec<QuestKind> {
    let candidates = [
        (QuestKind::Party, party_achievable),
        (QuestKind::CoStream, co_stream_achievable),
        (QuestKind::Invite, true),
        (QuestKind::Clip, true),
        (QuestKind::StreamExtra, true),
    ];
    let mut ranked = candidates
        .into_iter()
        .filter(|(_, enabled)| *enabled)
        .map(|(kind, _)| {
            let material = format!("{partner_id}|{week}|{}", kind.key());
            let digest = Sha256::digest(material.as_bytes());
            (digest.to_vec(), kind)
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|a, b| a.0.cmp(&b.0));
    ranked.into_iter().map(|(_, kind)| kind).take(3).collect()
}

async fn load_weekly_quests(
    pool: &PgPool,
    partner: &Partner,
    week_start: NaiveDate,
) -> Result<Vec<QuestKind>, sqlx::Error> {
    let rows = sqlx::query_scalar::<_, String>(
        "SELECT quest_key
         FROM partner_effort_weekly_quests
         WHERE partner_twitch_user_id=$1 AND week_start=$2
         ORDER BY position",
    )
    .bind(&partner.twitch_user_id)
    .bind(week_start)
    .fetch_all(pool)
    .await?;
    let quests = rows
        .into_iter()
        .filter_map(|key| QuestKind::from_key(&key))
        .collect::<Vec<_>>();
    if quests.len() != 3 {
        return Err(sqlx::Error::Protocol(
            "weekly quest assignment incomplete".to_string(),
        ));
    }
    Ok(quests)
}

async fn ensure_weekly_quests(
    pool: &PgPool,
    partner: &Partner,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    let (_, _, week_start) = berlin_week_bounds(now);
    let existing: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint
         FROM partner_effort_weekly_quests
         WHERE partner_twitch_user_id=$1 AND week_start=$2",
    )
    .bind(&partner.twitch_user_id)
    .bind(week_start)
    .fetch_one(pool)
    .await?;
    if existing == 3 {
        return Ok(());
    }
    let own_links: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM twitch_streamer_identities i
         JOIN twitch_player_steam_links l ON l.twitch_user_id=i.twitch_user_id
         WHERE i.twitch_user_id=$1 AND NULLIF(trim(i.discord_user_id),'') IS NOT NULL
           AND l.lookup_enabled=TRUE AND l.steam_id64 IS NOT NULL",
    )
    .bind(&partner.twitch_user_id)
    .fetch_one(pool)
    .await?;
    let linked_people: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT i.twitch_user_id)::bigint
         FROM twitch_streamer_identities i
         JOIN twitch_player_steam_links l ON l.twitch_user_id=i.twitch_user_id
         WHERE NULLIF(trim(i.discord_user_id),'') IS NOT NULL
           AND l.lookup_enabled=TRUE AND l.steam_id64 IS NOT NULL",
    )
    .fetch_one(pool)
    .await?;
    let active_sql = format!(
        "SELECT COUNT(*)::bigint FROM twitch_partners WHERE {}",
        active_partner_filter()
    );
    let active_count: i64 = sqlx::query_scalar(&active_sql).fetch_one(pool).await?;
    let selected = deterministic_quests(
        &partner.twitch_user_id,
        week_start,
        own_links > 0 && linked_people > 1,
        active_count > 1,
    );
    if selected.len() != 3 {
        return Err(sqlx::Error::Protocol(
            "weekly quest pool produced fewer than three quests".to_string(),
        ));
    }
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(format!(
            "weekly-quests|{}|{}",
            partner.twitch_user_id, week_start
        ))
        .execute(&mut *tx)
        .await?;
    let locked_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint
         FROM partner_effort_weekly_quests
         WHERE partner_twitch_user_id=$1 AND week_start=$2",
    )
    .bind(&partner.twitch_user_id)
    .bind(week_start)
    .fetch_one(&mut *tx)
    .await?;
    if locked_count == 3 {
        tx.commit().await?;
        return Ok(());
    }
    sqlx::query(
        "DELETE FROM partner_effort_weekly_quests
         WHERE partner_twitch_user_id=$1 AND week_start=$2",
    )
    .bind(&partner.twitch_user_id)
    .bind(week_start)
    .execute(&mut *tx)
    .await?;
    for (index, quest) in selected.into_iter().enumerate() {
        sqlx::query(
            "INSERT INTO partner_effort_weekly_quests
             (partner_twitch_user_id,week_start,position,quest_key,assigned_at)
             VALUES ($1,$2,$3,$4,$5)",
        )
        .bind(&partner.twitch_user_id)
        .bind(week_start)
        .bind((index + 1) as i16)
        .bind(quest.key())
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

async fn quest_context(
    pool: &PgPool,
    partner: &Partner,
    now: DateTime<Utc>,
) -> Result<(Vec<QuestResponse>, Vec<QuestKind>), sqlx::Error> {
    let (week_start, week_end, week_date) = berlin_week_bounds(now);
    let counts = sqlx::query(
        "SELECT event_type, COUNT(*)::bigint AS count
         FROM partner_effort_events
         WHERE partner_twitch_user_id=$1 AND occurred_at >= $2 AND occurred_at < $3
         GROUP BY event_type",
    )
    .bind(&partner.twitch_user_id)
    .bind(week_start)
    .bind(week_end)
    .fetch_all(pool)
    .await?;
    let mut event_counts = HashMap::new();
    for row in counts {
        event_counts.insert(
            row.try_get::<String, _>("event_type")?,
            row.try_get::<i64, _>("count")?,
        );
    }
    let selected = load_weekly_quests(pool, partner, week_date).await?;
    let history_start = week_start - ChronoDuration::days(28);
    let sessions = sqlx::query(
        "SELECT started_at, ended_at, duration_seconds
         FROM twitch_stream_sessions
         WHERE (twitch_user_id=$1 OR lower(streamer_login)=$2)
           AND started_at >= $3 AND started_at < $4
           AND (lower(COALESCE(game_name,''))='deadlock' OR COALESCE(had_deadlock_in_session,false))",
    )
    .bind(&partner.twitch_user_id)
    .bind(&partner.login)
    .bind(history_start)
    .bind(week_end)
    .fetch_all(pool)
    .await?;
    let mut previous_minutes = 0i64;
    let mut current_minutes = 0i64;
    for row in sessions {
        let started_at: DateTime<Utc> = row.try_get("started_at")?;
        let ended_at: Option<DateTime<Utc>> = row.try_get("ended_at")?;
        let stored_duration: Option<i32> = row.try_get("duration_seconds")?;
        let seconds = ended_at
            .map(|end| (end - started_at).num_seconds())
            .filter(|seconds| *seconds > 0)
            .or_else(|| {
                if ended_at.is_none() && now > started_at {
                    Some((now - started_at).num_seconds())
                } else {
                    stored_duration
                        .map(i64::from)
                        .filter(|seconds| *seconds > 0)
                }
            })
            .unwrap_or(0)
            .clamp(0, 48 * 3600);
        let minutes = seconds / 60;
        if started_at >= week_start {
            current_minutes += minutes;
        } else {
            previous_minutes += minutes;
        }
    }
    let average_minutes = previous_minutes / 4;
    let extra_progress = (current_minutes - average_minutes).max(0);
    let extra_goal = config().quests.stream_extra_minutes;
    let mut responses = Vec::with_capacity(selected.len());
    for kind in &selected {
        let (text, progress, goal) = match kind {
            QuestKind::Party => (
                "Spiele ein Match mit jemandem aus der Community".to_string(),
                *event_counts.get("party_play").unwrap_or(&0),
                1,
            ),
            QuestKind::CoStream => (
                "Streame mindestens 30 Minuten per Stream Together mit einem Partner".to_string(),
                *event_counts.get("co_stream").unwrap_or(&0),
                1,
            ),
            QuestKind::Invite => (
                "Bringe 1 neue aktive Person in den Discord".to_string(),
                *event_counts.get("qualified_invite").unwrap_or(&0),
                1,
            ),
            QuestKind::Clip => (
                "Reiche einen Clip ein".to_string(),
                *event_counts.get("clip_submitted").unwrap_or(&0),
                1,
            ),
            QuestKind::StreamExtra => (
                format!(
                    "Streame {} Minuten länger als dein 4-Wochen-Mittel",
                    extra_goal
                ),
                extra_progress,
                extra_goal,
            ),
        };
        responses.push(QuestResponse {
            key: kind.key(),
            text,
            progress,
            goal,
            completed: progress >= goal,
        });
    }
    Ok((responses, selected))
}

async fn settle_quests(
    pool: &PgPool,
    partner: &Partner,
    now: DateTime<Utc>,
) -> Result<Vec<QuestResponse>, sqlx::Error> {
    let (quests, selected) = quest_context(pool, partner, now).await?;
    let (_, _, week_date) = berlin_week_bounds(now);
    let mut complete = 0usize;
    for (quest, kind) in quests.iter().zip(selected) {
        if !quest.completed {
            continue;
        }
        complete += 1;
        insert_event(
            pool,
            EventInput {
                partner_twitch_user_id: partner.twitch_user_id.clone(),
                partner_login: partner.login.clone(),
                event_type: "quest_done",
                source_id: format!("quest:{week_date}:{}", kind.key()),
                occurred_at: now,
                viewer_twitch_user_id: None,
                metadata: json!({"quest": kind.key(), "week": week_date.to_string()}),
            },
        )
        .await?;
    }
    if complete == 3 && quests.len() == 3 {
        insert_event(
            pool,
            EventInput {
                partner_twitch_user_id: partner.twitch_user_id.clone(),
                partner_login: partner.login.clone(),
                event_type: "quest_done",
                source_id: format!("quest:{week_date}:all_three"),
                occurred_at: now,
                viewer_twitch_user_id: None,
                metadata: json!({"bonus":"all_three","week":week_date.to_string()}),
            },
        )
        .await?;
    }
    Ok(quests)
}

#[derive(Default, Clone, Copy)]
struct WeekState {
    streamed: bool,
    effort: bool,
}

fn streak_from_weeks(
    mut weeks: BTreeMap<NaiveDate, WeekState>,
    current_week: NaiveDate,
) -> (i32, i32, Option<NaiveDate>) {
    if weeks.is_empty() {
        return (0, 0, None);
    }
    let first = *weeks.keys().next().expect("nonempty");
    let mut cursor = first;
    let mut current = 0i32;
    let mut longest = 0i32;
    let mut freezes: HashSet<(i32, u32)> = HashSet::new();
    let mut last_qualified = None;
    while cursor < current_week {
        let qualified = weeks
            .remove(&cursor)
            .is_some_and(|state| state.streamed && state.effort);
        if qualified {
            current += 1;
            longest = longest.max(current);
            last_qualified = Some(cursor);
        } else if current > 0 && freezes.insert((cursor.year(), cursor.month())) {
        } else {
            current = 0;
        }
        cursor += ChronoDuration::days(7);
    }
    if weeks
        .get(&current_week)
        .is_some_and(|state| state.streamed && state.effort)
    {
        current += 1;
        longest = longest.max(current);
        last_qualified = Some(current_week);
    }
    (current, longest, last_qualified)
}

async fn refresh_streak(
    pool: &PgPool,
    partner: &Partner,
    now: DateTime<Utc>,
) -> Result<StreakResponse, sqlx::Error> {
    let sessions = sqlx::query(
        "SELECT started_at, ended_at, duration_seconds
         FROM twitch_stream_sessions
         WHERE (twitch_user_id=$1 OR lower(streamer_login)=$2)
           AND lower(COALESCE(game_name,''))='deadlock'
         ORDER BY started_at",
    )
    .bind(&partner.twitch_user_id)
    .bind(&partner.login)
    .fetch_all(pool)
    .await?;
    let snapshots = sqlx::query(
        "SELECT snapshot_at, sample_seconds
         FROM category_stream_snapshots
         WHERE user_id=$1
         ORDER BY snapshot_at",
    )
    .bind(&partner.twitch_user_id)
    .fetch_all(pool)
    .await?;
    let events = sqlx::query(
        "SELECT occurred_at FROM partner_effort_events
         WHERE partner_twitch_user_id=$1 ORDER BY occurred_at",
    )
    .bind(&partner.twitch_user_id)
    .fetch_all(pool)
    .await?;
    let mut weeks: BTreeMap<NaiveDate, WeekState> = BTreeMap::new();
    for row in sessions {
        let started_at: DateTime<Utc> = row.try_get("started_at")?;
        let ended_at: Option<DateTime<Utc>> = row.try_get("ended_at")?;
        let stored: Option<i32> = row.try_get("duration_seconds")?;
        let seconds = ended_at
            .map(|end| (end - started_at).num_seconds())
            .filter(|value| *value > 0)
            .or_else(|| {
                if ended_at.is_none() && now > started_at {
                    Some((now - started_at).num_seconds())
                } else {
                    stored.map(i64::from)
                }
            })
            .unwrap_or(0);
        if seconds >= 1800 {
            weeks
                .entry(berlin_week_start(started_at))
                .or_default()
                .streamed = true;
        }
    }
    let mut deadlock_seconds: HashMap<NaiveDate, f64> = HashMap::new();
    for row in snapshots {
        let snapshot_at: DateTime<Utc> = row.try_get("snapshot_at")?;
        let sample_seconds: f64 = row.try_get("sample_seconds")?;
        if sample_seconds.is_finite() && sample_seconds > 0.0 {
            *deadlock_seconds
                .entry(berlin_week_start(snapshot_at))
                .or_default() += sample_seconds.min(300.0);
        }
    }
    for (week, seconds) in deadlock_seconds {
        if seconds >= 1800.0 {
            weeks.entry(week).or_default().streamed = true;
        }
    }
    for row in events {
        let occurred_at: DateTime<Utc> = row.try_get("occurred_at")?;
        weeks
            .entry(berlin_week_start(occurred_at))
            .or_default()
            .effort = true;
    }
    let current_week = berlin_week_start(now);
    let (current, longest, last_qualified) = streak_from_weeks(weeks, current_week);
    sqlx::query(
        "INSERT INTO partner_effort_streaks
         (partner_twitch_user_id,current_streak,longest_streak,last_qualified_week,updated_at)
         VALUES ($1,$2,$3,$4,$5)
         ON CONFLICT (partner_twitch_user_id) DO UPDATE SET
           current_streak=EXCLUDED.current_streak,
           longest_streak=GREATEST(partner_effort_streaks.longest_streak,EXCLUDED.longest_streak),
           last_qualified_week=EXCLUDED.last_qualified_week,
           updated_at=EXCLUDED.updated_at",
    )
    .bind(&partner.twitch_user_id)
    .bind(current)
    .bind(longest)
    .bind(last_qualified)
    .bind(now)
    .execute(pool)
    .await?;
    let stored_longest: i32 = sqlx::query_scalar(
        "SELECT longest_streak FROM partner_effort_streaks WHERE partner_twitch_user_id=$1",
    )
    .bind(&partner.twitch_user_id)
    .fetch_one(pool)
    .await?;
    Ok(StreakResponse {
        current,
        longest: stored_longest,
    })
}

async fn load_streak(pool: &PgPool, partner: &Partner) -> Result<StreakResponse, sqlx::Error> {
    let row = sqlx::query_as::<_, (i32, i32)>(
        "SELECT current_streak, longest_streak
         FROM partner_effort_streaks
         WHERE partner_twitch_user_id=$1",
    )
    .bind(&partner.twitch_user_id)
    .fetch_optional(pool)
    .await?;
    let (current, longest) = row.unwrap_or((0, 0));
    Ok(StreakResponse { current, longest })
}

fn level_response(total_points: i64) -> (LevelResponse, NextGoalResponse) {
    let thresholds = &config().levels.thresholds;
    let mut level_index = 0usize;
    for (index, threshold) in thresholds.iter().enumerate() {
        if total_points >= *threshold {
            level_index = index;
        }
    }
    let current_threshold = thresholds.get(level_index).copied().unwrap_or(0);
    let next_threshold = thresholds.get(level_index + 1).copied();
    let next_goal = if let Some(next) = next_threshold {
        let missing = (next - total_points).max(0);
        let candidates = [
            (
                config().points.qualified_invite.max(1) as i64,
                "aktive Einladung",
                "aktive Einladungen",
            ),
            (
                config().points.co_stream.max(1) as i64,
                "Stream-Together-Session",
                "Stream-Together-Sessions",
            ),
            (
                config().points.party_play.max(1) as i64,
                "Community-Match",
                "Community-Matches",
            ),
            (
                config().points.clip_submitted.max(1) as i64,
                "Clip-Einreichung",
                "Clip-Einreichungen",
            ),
        ];
        let (actions, singular, plural) = candidates
            .into_iter()
            .map(|(points, singular, plural)| {
                let actions = (missing + points - 1) / points;
                (actions, singular, plural)
            })
            .min_by_key(|value| value.0)
            .expect("route candidates");
        NextGoalResponse {
            missing_points: missing,
            fastest_route: format!(
                "{} weitere {}",
                actions,
                if actions == 1 { singular } else { plural }
            ),
        }
    } else {
        NextGoalResponse {
            missing_points: 0,
            fastest_route: "Maximalstufe erreicht".to_string(),
        }
    };
    (
        LevelResponse {
            level: level_index + 1,
            total_points,
            current_threshold,
            next_threshold,
        },
        next_goal,
    )
}

fn achievement(
    key: &'static str,
    name: &'static str,
    progress: i64,
    targets: &[i64],
) -> AchievementResponse {
    AchievementResponse {
        key,
        name,
        progress,
        tiers: targets
            .iter()
            .copied()
            .map(|target| AchievementTier {
                target,
                unlocked: progress >= target,
            })
            .collect(),
    }
}

async fn achievements(
    pool: &PgPool,
    partner: &Partner,
    streak: &StreakResponse,
) -> Result<Vec<AchievementResponse>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT event_type, COUNT(*)::bigint AS count
         FROM partner_effort_events
         WHERE partner_twitch_user_id=$1
         GROUP BY event_type",
    )
    .bind(&partner.twitch_user_id)
    .fetch_all(pool)
    .await?;
    let mut counts = HashMap::new();
    for row in rows {
        counts.insert(
            row.try_get::<String, _>("event_type")?,
            row.try_get::<i64, _>("count")?,
        );
    }
    let cfg = &config().achievements;
    Ok(vec![
        achievement(
            "recruiter",
            "Recruiter",
            *counts.get("qualified_invite").unwrap_or(&0),
            &cfg.recruiter,
        ),
        achievement(
            "team_player",
            "Teamplayer",
            *counts.get("party_play").unwrap_or(&0),
            &cfg.team_player,
        ),
        achievement(
            "duo",
            "Duo",
            *counts.get("co_stream").unwrap_or(&0),
            &cfg.duo,
        ),
        achievement(
            "stamina",
            "Ausdauer",
            i64::from(streak.longest),
            &cfg.stamina,
        ),
        achievement(
            "talent_scout",
            "Talentscout",
            *counts.get("streamer_referral").unwrap_or(&0),
            &cfg.talent_scout,
        ),
        achievement(
            "clip_hunter",
            "Clipjäger",
            *counts.get("clip_top3").unwrap_or(&0),
            &cfg.clip_hunter,
        ),
    ])
}

async fn with_us(pool: &PgPool, partner: &Partner) -> Result<WithUsResponse, String> {
    let people: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM partner_effort_events
         WHERE partner_twitch_user_id=$1 AND event_type='qualified_invite'",
    )
    .bind(&partner.twitch_user_id)
    .fetch_one(pool)
    .await
    .map_err(|_| "effort database unavailable".to_string())?;
    let received_raids: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM twitch_raid_history
         WHERE COALESCE(success,TRUE)=TRUE
           AND (to_broadcaster_id=$1 OR lower(to_broadcaster_login)=$2)",
    )
    .bind(&partner.twitch_user_id)
    .bind(&partner.login)
    .fetch_one(pool)
    .await
    .map_err(|_| "raid database unavailable".to_string())?;
    let discord_id: Option<String> = sqlx::query_scalar(
        "SELECT NULLIF(trim(discord_user_id),'') FROM twitch_streamer_identities
         WHERE twitch_user_id=$1 LIMIT 1",
    )
    .bind(&partner.twitch_user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| "identity database unavailable".to_string())?
    .flatten();
    let community_seconds = if let Some(discord_id) = discord_id {
        let id = discord_id
            .parse::<i64>()
            .map_err(|_| "discord identity is invalid".to_string())?;
        let central = central_pool().await?;
        sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(SUM(duration_seconds),0)::bigint
             FROM activity.voice_session_log
             WHERE user_id=$1 AND duration_seconds > 0
               AND COALESCE(
                   jsonb_array_length(
                       CASE WHEN jsonb_typeof(co_player_ids)='array' THEN co_player_ids ELSE '[]'::jsonb END
                   ),
                   0
               ) > 0",
        )
        .bind(id)
        .fetch_one(&central)
        .await
        .map_err(|_| "community activity database unavailable".to_string())?
    } else {
        0
    };
    Ok(WithUsResponse {
        people_brought_in_who_stayed: people,
        community_hours: (community_seconds as f64 / 3600.0 * 10.0).round() / 10.0,
        received_raids,
    })
}

async fn season(
    pool: &PgPool,
    partner: &Partner,
    now: DateTime<Utc>,
) -> Result<SeasonResponse, sqlx::Error> {
    let (start, end, month) = berlin_month_bounds(now);
    let sql = format!(
        "WITH active AS (
            SELECT twitch_user_id FROM twitch_partners WHERE {}
         ),
         scores AS (
            SELECT a.twitch_user_id,
                   COALESCE(SUM(e.points),0)::bigint AS points,
                   COUNT(*) FILTER (WHERE e.event_type='qualified_invite')::bigint AS qualified_invites,
                   MAX(e.occurred_at) FILTER (WHERE e.points > 0) AS score_reached_at
            FROM active a
            LEFT JOIN partner_effort_events e
              ON e.partner_twitch_user_id=a.twitch_user_id
             AND e.occurred_at >= $1 AND e.occurred_at < $2
            GROUP BY a.twitch_user_id
         ),
         ranked AS (
            SELECT twitch_user_id, points,
                   ROW_NUMBER() OVER (
                       ORDER BY points DESC,
                                qualified_invites DESC,
                                score_reached_at ASC NULLS LAST,
                                twitch_user_id
                   ) AS rank,
                   COUNT(*) OVER () AS active_partners
            FROM scores
         )
         SELECT points, rank::bigint, active_partners::bigint
         FROM ranked WHERE twitch_user_id=$3",
        active_partner_filter()
    );
    let row = sqlx::query(&sql)
        .bind(start)
        .bind(end)
        .bind(&partner.twitch_user_id)
        .fetch_one(pool)
        .await?;
    Ok(SeasonResponse {
        month,
        points: row.try_get("points")?,
        rank: row.try_get("rank")?,
        active_partners: row.try_get("active_partners")?,
    })
}

async fn total_points(pool: &PgPool, partner: &Partner) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT COALESCE(SUM(points),0)::bigint FROM partner_effort_events
         WHERE partner_twitch_user_id=$1",
    )
    .bind(&partner.twitch_user_id)
    .fetch_one(pool)
    .await
}

pub async fn me_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<Params>,
) -> Response {
    let partner = match subject(&auth, params.streamer.as_deref(), &pool).await {
        Ok(partner) => partner,
        Err(response) => return response,
    };
    let now = Utc::now();
    let quests = match quest_context(&pool, &partner, now).await {
        Ok((value, _)) => value,
        Err(err) => {
            tracing::warn!(%err, "Challenge-Quests konnten nicht berechnet werden");
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "database_unavailable",
                "Challenge-Daten sind gerade nicht verfügbar.",
            );
        }
    };
    let streak = match load_streak(&pool, &partner).await {
        Ok(value) => value,
        Err(err) => {
            tracing::warn!(%err, "Challenge-Streak konnte nicht berechnet werden");
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "database_unavailable",
                "Challenge-Daten sind gerade nicht verfügbar.",
            );
        }
    };
    let total = match total_points(&pool, &partner).await {
        Ok(value) => value,
        Err(err) => {
            tracing::warn!(%err, "Challenge-Punkte konnten nicht geladen werden");
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "database_unavailable",
                "Challenge-Daten sind gerade nicht verfügbar.",
            );
        }
    };
    let achievement_data = match achievements(&pool, &partner, &streak).await {
        Ok(value) => value,
        Err(err) => {
            tracing::warn!(%err, "Achievements konnten nicht geladen werden");
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "database_unavailable",
                "Challenge-Daten sind gerade nicht verfügbar.",
            );
        }
    };
    let with_us_data = match with_us(&pool, &partner).await {
        Ok(value) => value,
        Err(err) => {
            tracing::warn!(%err, "Mit-uns-Statistik konnte nicht geladen werden");
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "source_unavailable",
                "Community-Daten sind gerade nicht vollständig verfügbar.",
            );
        }
    };
    let season_data = match season(&pool, &partner, now).await {
        Ok(value) => value,
        Err(err) => {
            tracing::warn!(%err, "Challenge-Saison konnte nicht geladen werden");
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "database_unavailable",
                "Challenge-Daten sind gerade nicht verfügbar.",
            );
        }
    };
    let (level, next_goal) = level_response(total);
    success(MeResponse {
        generated_at: now,
        timezone: "Europe/Berlin",
        streamer: partner.login,
        quests,
        streak,
        level,
        next_goal,
        achievements: achievement_data,
        with_us: with_us_data,
        season: season_data,
    })
}

pub async fn viewers_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Extension(ChallengeHelix(helix)): Extension<ChallengeHelix>,
    Query(params): Query<Params>,
) -> Response {
    let partner = match subject(&auth, params.streamer.as_deref(), &pool).await {
        Ok(partner) => partner,
        Err(response) => return response,
    };
    let rows = match sqlx::query(
        "SELECT viewer_twitch_user_id, COUNT(*)::bigint AS qualified_invites
         FROM partner_effort_events
         WHERE partner_twitch_user_id=$1
           AND event_type='qualified_invite'
           AND viewer_twitch_user_id IS NOT NULL
         GROUP BY viewer_twitch_user_id
         ORDER BY qualified_invites DESC, viewer_twitch_user_id
         LIMIT 50",
    )
    .bind(&partner.twitch_user_id)
    .fetch_all(&pool)
    .await
    {
        Ok(rows) => rows,
        Err(err) => {
            tracing::warn!(%err, "Viewer-Recruiter konnten nicht geladen werden");
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "database_unavailable",
                "Recruiter-Daten sind gerade nicht verfügbar.",
            );
        }
    };
    let mut recruiters = Vec::with_capacity(rows.len());
    for row in rows {
        let twitch_user_id = match row.try_get::<String, _>("viewer_twitch_user_id") {
            Ok(value) => value,
            Err(_) => {
                return error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "database_unavailable",
                    "Recruiter-Daten sind gerade nicht verfügbar.",
                )
            }
        };
        let qualified_invites = match row.try_get::<i64, _>("qualified_invites") {
            Ok(value) => value,
            Err(_) => {
                return error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "database_unavailable",
                    "Recruiter-Daten sind gerade nicht verfügbar.",
                )
            }
        };
        recruiters.push((twitch_user_id, qualified_invites));
    }
    if recruiters.is_empty() {
        return success(ViewersResponse {
            generated_at: Utc::now(),
            streamer: partner.login,
            recruiters: Vec::new(),
        });
    }
    let Some(helix) = helix else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "helix_unavailable",
            "Twitch-Anzeigenamen sind gerade nicht verfügbar.",
        );
    };
    let ids = recruiters
        .iter()
        .map(|(id, _)| id.as_str())
        .collect::<Vec<_>>();
    let users = match helix.get_users_by_id(&ids).await {
        Ok(users) => users,
        Err(err) => {
            tracing::warn!(%err, "Helix-Anzeigenamen für Recruiter konnten nicht geladen werden");
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "helix_unavailable",
                "Twitch-Anzeigenamen sind gerade nicht verfügbar.",
            );
        }
    };
    let recruiters = recruiters
        .into_iter()
        .map(|(twitch_user_id, qualified_invites)| ViewerRecruiter {
            display_name: users
                .get(&twitch_user_id)
                .map(|user| user.display_name.clone()),
            twitch_user_id,
            qualified_invites,
        })
        .collect();
    success(ViewersResponse {
        generated_at: Utc::now(),
        streamer: partner.login,
        recruiters,
    })
}

async fn active_partners(pool: &PgPool) -> Result<Vec<Partner>, sqlx::Error> {
    let sql = format!(
        "SELECT twitch_user_id, lower(twitch_login) AS login FROM twitch_partners WHERE {} ORDER BY twitch_user_id",
        active_partner_filter()
    );
    let rows = sqlx::query(&sql).fetch_all(pool).await?;
    rows.into_iter()
        .map(|row| {
            Ok(Partner {
                twitch_user_id: row.try_get("twitch_user_id")?,
                login: row.try_get("login")?,
            })
        })
        .collect()
}

async fn settle_all(pool: &PgPool) -> Result<(), sqlx::Error> {
    let now = Utc::now();
    for partner in active_partners(pool).await? {
        ensure_weekly_quests(pool, &partner, now).await?;
        settle_quests(pool, &partner, now).await?;
        refresh_streak(pool, &partner, now).await?;
    }
    Ok(())
}

pub fn spawn_collector(pool: PgPool, helix: Option<HelixClient>) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(300));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            if let Err(err) = sync_sources(&pool, helix.as_ref()).await {
                tracing::warn!(%err, "Challenge-Quellensync fehlgeschlagen");
                continue;
            }
            if let Err(err) = settle_all(&pool).await {
                tracing::warn!(%err, "Challenge-Settlement fehlgeschlagen");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monday(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    #[test]
    fn deterministic_quest_draw_is_stable_and_has_three_entries() {
        let week = monday(2026, 9, 21);
        let first = deterministic_quests("123", week, true, true);
        let second = deterministic_quests("123", week, true, true);
        assert_eq!(first, second);
        assert_eq!(first.len(), 3);
        assert_eq!(
            first.iter().map(|q| q.key()).collect::<HashSet<_>>().len(),
            3
        );
    }

    #[test]
    fn quest_draw_excludes_unachievable_party_and_costream() {
        let quests = deterministic_quests("123", monday(2026, 9, 21), false, false);
        assert_eq!(quests.len(), 3);
        assert!(!quests.contains(&QuestKind::Party));
        assert!(!quests.contains(&QuestKind::CoStream));
    }

    #[test]
    fn streak_bridges_one_missed_week_per_calendar_month() {
        let mut weeks = BTreeMap::new();
        for date in [monday(2026, 8, 24), monday(2026, 9, 7), monday(2026, 9, 14)] {
            weeks.insert(
                date,
                WeekState {
                    streamed: true,
                    effort: true,
                },
            );
        }
        weeks.insert(monday(2026, 8, 31), WeekState::default());
        let (current, longest, _) = streak_from_weeks(weeks, monday(2026, 9, 21));
        assert_eq!(current, 3);
        assert_eq!(longest, 3);
    }

    #[test]
    fn second_miss_in_same_month_breaks_streak() {
        let mut weeks = BTreeMap::new();
        weeks.insert(
            monday(2026, 9, 7),
            WeekState {
                streamed: true,
                effort: true,
            },
        );
        weeks.insert(monday(2026, 9, 14), WeekState::default());
        weeks.insert(monday(2026, 9, 21), WeekState::default());
        weeks.insert(
            monday(2026, 9, 28),
            WeekState {
                streamed: true,
                effort: true,
            },
        );
        let (current, longest, _) = streak_from_weeks(weeks, monday(2026, 10, 5));
        assert_eq!(current, 1);
        assert_eq!(longest, 1);
    }

    #[test]
    fn level_next_goal_uses_effort_not_reach() {
        let (level, goal) = level_response(132);
        assert_eq!(level.level, 2);
        assert_eq!(level.next_threshold, Some(150));
        assert_eq!(goal.missing_points, 18);
        assert_eq!(goal.fastest_route, "2 weitere aktive Einladungen");
    }

    #[test]
    fn weekly_boundaries_use_berlin_monday() {
        let sunday_utc = DateTime::parse_from_rfc3339("2026-10-25T23:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let (start, end, date) = berlin_week_bounds(sunday_utc);
        assert_eq!(date, monday(2026, 10, 26));
        assert!(end > start);
        assert_eq!((end - start).num_hours(), 168);
    }
}
