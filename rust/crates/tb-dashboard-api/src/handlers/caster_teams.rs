use super::caster_overlay::{self, SceneCoach, ScenePlayer, SceneTeam};
use crate::auth::level::DashboardAuthLevel;
use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{postgres::PgRow, PgPool, Row};
use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};
use tb_http_core::ApiError;
use tokio::sync::Mutex;
use uuid::Uuid;

const LIST_TEAMS: &str = "SELECT id::text AS id, name, short, logo_asset_id::text AS logo_asset_id, players, coach_name, coach_discord_id FROM twitch_caster_teams ORDER BY lower(name), id";
const MEMBER_CACHE_TTL: Duration = Duration::from_secs(120);
const MEMBER_SEARCH_LIMIT: usize = 20;
const MEMBER_QUERY_MAX: usize = 60;

fn db_error(error: sqlx::Error) -> ApiError {
    tracing::error!(%error, "Caster-Teambibliothek: Datenbankfehler");
    ApiError::internal()
}

fn no_store(body: Value) -> Response {
    ([(header::CACHE_CONTROL, "no-store")], Json(body)).into_response()
}

fn team_from_row(row: &PgRow) -> Result<SceneTeam, sqlx::Error> {
    let players: Value = row.try_get("players")?;
    let players: Vec<ScenePlayer> = serde_json::from_value(players).unwrap_or_default();
    let coach_name: Option<String> = row.try_get("coach_name")?;
    let coach_discord_id: Option<String> = row.try_get("coach_discord_id")?;
    Ok(SceneTeam {
        id: None,
        library_id: Some(row.try_get("id")?),
        name: row.try_get("name")?,
        short: row.try_get("short")?,
        logo_asset_id: row.try_get("logo_asset_id")?,
        players,
        coach: coach_name.map(|name| SceneCoach {
            name,
            discord_id: coach_discord_id,
        }),
    })
}

fn prepared_team(mut team: SceneTeam) -> Result<SceneTeam, ApiError> {
    caster_overlay::normalize_team(&mut team);
    if team.name.is_empty() || !caster_overlay::team_valid(&team) {
        return Err(ApiError::bad_request_with_body(json!({
            "message":"Ungültiges Team. Nötig ist ein Teamname bis 80 Zeichen; Kürzel bis acht Zeichen, höchstens zwölf Spieler und ein Coach-Name bis 80 Zeichen."
        })));
    }
    Ok(team)
}

fn players_json(team: &SceneTeam) -> Result<Value, ApiError> {
    serde_json::to_value(&team.players).map_err(|_| ApiError::internal())
}

async fn write_team(
    pool: &PgPool,
    id: &str,
    team: &SceneTeam,
    insert: bool,
) -> Result<Option<SceneTeam>, ApiError> {
    let sql = if insert {
        "INSERT INTO twitch_caster_teams (id, name, short, logo_asset_id, players, coach_name, coach_discord_id) \
         VALUES ($1::uuid, $2, $3, (SELECT a.id FROM twitch_caster_overlay_assets a WHERE a.id = $4::uuid AND a.kind = 'team_logo'), $5, $6, $7) \
         RETURNING id::text AS id, name, short, logo_asset_id::text AS logo_asset_id, players, coach_name, coach_discord_id"
    } else {
        "UPDATE twitch_caster_teams SET name = $2, short = $3, \
         logo_asset_id = (SELECT a.id FROM twitch_caster_overlay_assets a WHERE a.id = $4::uuid AND a.kind = 'team_logo'), \
         players = $5, coach_name = $6, coach_discord_id = $7, updated_at = now() WHERE id = $1::uuid \
         RETURNING id::text AS id, name, short, logo_asset_id::text AS logo_asset_id, players, coach_name, coach_discord_id"
    };
    let coach = team.coach.as_ref();
    let row = sqlx::query(sql)
        .bind(id)
        .bind(&team.name)
        .bind(&team.short)
        .bind(&team.logo_asset_id)
        .bind(players_json(team)?)
        .bind(coach.map(|coach| coach.name.clone()))
        .bind(coach.and_then(|coach| coach.discord_id.clone()))
        .fetch_optional(pool)
        .await
        .map_err(db_error)?;
    row.as_ref()
        .map(team_from_row)
        .transpose()
        .map_err(db_error)
}

pub async fn list_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Result<Response, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    let rows = sqlx::query(LIST_TEAMS)
        .fetch_all(&pool)
        .await
        .map_err(db_error)?;
    let items = rows
        .iter()
        .map(team_from_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;
    Ok(no_store(json!({ "items": items })))
}

pub async fn create_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Json(team): Json<SceneTeam>,
) -> Result<Response, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    let team = prepared_team(team)?;
    let id = Uuid::new_v4().to_string();
    let saved = write_team(&pool, &id, &team, true)
        .await?
        .ok_or_else(ApiError::internal)?;
    Ok(no_store(json!({ "team": saved })))
}

pub async fn update_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(id): Path<String>,
    Json(team): Json<SceneTeam>,
) -> Result<Response, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    if Uuid::parse_str(&id).is_err() {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    let team = prepared_team(team)?;
    match write_team(&pool, &id, &team, false).await? {
        Some(saved) => Ok(no_store(json!({ "team": saved }))),
        None => Ok(StatusCode::NOT_FOUND.into_response()),
    }
}

pub async fn delete_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    if Uuid::parse_str(&id).is_err() {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    let deleted = sqlx::query("DELETE FROM twitch_caster_teams WHERE id = $1::uuid")
        .bind(&id)
        .execute(&pool)
        .await
        .map_err(db_error)?
        .rows_affected();
    if deleted == 0 {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    Ok(no_store(json!({ "deleted": id })))
}

#[derive(Clone, Debug, Deserialize)]
struct GuildMember {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    global_name: Option<String>,
    #[serde(default)]
    nick: Option<String>,
}

impl GuildMember {
    fn display_name(&self) -> String {
        [&self.nick, &self.global_name, &self.name]
            .into_iter()
            .flatten()
            .map(|value| value.trim())
            .find(|value| !value.is_empty())
            .unwrap_or_default()
            .to_owned()
    }

    fn matches(&self, needle: &str) -> bool {
        [&self.nick, &self.global_name, &self.name]
            .into_iter()
            .flatten()
            .any(|value| value.to_lowercase().contains(needle))
    }
}

#[derive(Debug, Deserialize)]
struct MembersResponse {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    members: Vec<GuildMember>,
}

type MemberCache = Mutex<Option<(Instant, Vec<GuildMember>)>>;

fn member_cache() -> &'static MemberCache {
    static CACHE: OnceLock<MemberCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

async fn fetch_guild_members() -> Result<Vec<GuildMember>, &'static str> {
    let response = caster_overlay::broker_get("/internal/master/v1/discord/members", &[]).await?;
    let body: MembersResponse = response.json().await.map_err(|_| "broker_invalid")?;
    if !body.ok {
        return Err("broker_rejected");
    }
    Ok(body
        .members
        .into_iter()
        .filter(|member| caster_overlay::discord_id_valid(&member.id))
        .collect())
}

async fn guild_members() -> Result<Vec<GuildMember>, &'static str> {
    let mut cache = member_cache().lock().await;
    if let Some((loaded, members)) = cache.as_ref() {
        if loaded.elapsed() < MEMBER_CACHE_TTL {
            return Ok(members.clone());
        }
    }
    let members = fetch_guild_members().await?;
    *cache = Some((Instant::now(), members.clone()));
    Ok(members)
}

fn search_members(members: &[GuildMember], query: &str) -> Vec<Value> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut found = members
        .iter()
        .filter(|member| member.matches(&needle))
        .map(|member| (member.display_name(), member))
        .filter(|(display_name, _)| !display_name.is_empty())
        .collect::<Vec<_>>();
    found.sort_by_key(|(display_name, _)| {
        let lower = display_name.to_lowercase();
        (!lower.starts_with(&needle), lower)
    });
    found
        .into_iter()
        .take(MEMBER_SEARCH_LIMIT)
        .map(|(display_name, member)| {
            json!({
                "discordId": member.id,
                "displayName": display_name,
                "username": member.name,
            })
        })
        .collect()
}

#[derive(Deserialize)]
pub struct MemberSearchQuery {
    #[serde(default)]
    q: String,
}

pub async fn member_search_handler(
    auth: DashboardAuthLevel,
    Query(query): Query<MemberSearchQuery>,
) -> Result<Response, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    if query.q.chars().count() > MEMBER_QUERY_MAX {
        return Err(ApiError::bad_request("Suchbegriff ist zu lang."));
    }
    match guild_members().await {
        Ok(members) => Ok(no_store(
            json!({ "available": true, "items": search_members(&members, &query.q) }),
        )),
        Err(reason) => Ok(no_store(
            json!({ "available": false, "reason": reason, "items": [] }),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: &str, name: &str, global: Option<&str>, nick: Option<&str>) -> GuildMember {
        GuildMember {
            id: id.to_owned(),
            name: Some(name.to_owned()),
            global_name: global.map(str::to_owned),
            nick: nick.map(str::to_owned),
        }
    }

    #[test]
    fn mitgliedersuche_bevorzugt_praefix_und_nick() {
        let members = vec![
            member("100000000000000001", "zander", Some("Der Coach"), None),
            member("100000000000000002", "coachmax", None, Some("Max")),
            member("100000000000000003", "other", None, None),
        ];
        let found = search_members(&members, "COACH");
        let names = found
            .iter()
            .filter_map(|item| item["displayName"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["Der Coach", "Max"]);
        assert!(search_members(&members, "  ").is_empty());
    }
}
