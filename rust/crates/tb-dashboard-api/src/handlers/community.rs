//! Authenticated co-stream/co-play recommendations and permission-scoped Discord lobbies.
//! Read only: no invitations, voice moves, joins, role changes or unsolicited messages.
mod matching;
mod sources;
#[cfg(test)]
mod integration_tests;

use crate::{auth::level::DashboardAuthLevel, query_int::parse_bounded_query_int};
use axum::{extract::{Query, State}, http::{header, StatusCode}, response::{IntoResponse, Response}, Json};
use chrono::{DateTime, Duration, Utc};
use futures_util::{stream, StreamExt};
use matching::{match_score, schedule, title_mode, MatchScore, PlayerProfile, Schedule, Session};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use std::collections::HashMap;

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Params { pub streamer: Option<String>, pub days: Option<String> }

#[derive(Clone, sqlx::FromRow)]
struct Member {
    twitch_user_id: String, login: String, discord_id: Option<String>,
    is_live: i32, last_seen_at: Option<String>, last_game: Option<String>, last_title: Option<String>,
}

#[derive(Serialize)]
struct Recommendation {
    login: String, live_state: &'static str, current_game: Option<String>, current_title: Option<String>,
    both_live_same_game: bool, profile: PlayerProfile, schedule: Schedule,
    #[serde(flatten)] matching: MatchScore,
}
#[derive(Serialize)]
struct CommunityResponse {
    generated_at: DateTime<Utc>, days: i64, timezone: &'static str,
    streamer: String, own_profile: PlayerProfile, own_schedule: Schedule,
    recommendations: Vec<Recommendation>, discord: sources::LobbyResult,
    candidate_count: usize, evaluated_count: usize,
}

fn error(status: StatusCode, text: &str) -> Response { (status,Json(json!({"error":text}))).into_response() }

#[allow(clippy::result_large_err)]
fn subject(auth: &DashboardAuthLevel, requested: Option<&str>) -> Result<String,Response> {
    let requested = requested.map(str::trim).filter(|s| !s.is_empty()).map(str::to_lowercase);
    let login = match auth {
        DashboardAuthLevel::None => return Err(error(StatusCode::UNAUTHORIZED,"Bitte mit Twitch anmelden.")),
        DashboardAuthLevel::Partner {twitch_login,..} => {
            if requested.as_deref().is_some_and(|s| !s.eq_ignore_ascii_case(twitch_login)) {
                return Err(error(StatusCode::FORBIDDEN,"Vorschläge sind nur für deinen eigenen Kanal verfügbar."));
            }
            twitch_login.to_lowercase()
        }
        DashboardAuthLevel::Admin {actor} => requested.or_else(|| actor.as_ref().map(|a| a.twitch_login.to_lowercase()))
            .ok_or_else(|| error(StatusCode::BAD_REQUEST,"Bitte einen Streamer auswählen."))?,
    };
    if login.is_empty() || login.len()>25 || !login.bytes().all(|c| c.is_ascii_alphanumeric() || c==b'_') {
        return Err(error(StatusCode::BAD_REQUEST,"Ungültiger Twitch-Kanal."));
    }
    Ok(login)
}

/// Admins can inspect a selected channel's schedule, but may NOT impersonate
/// that channel's Discord account when checking private voice permissions.
fn viewer_twitch_id(auth: &DashboardAuthLevel) -> Option<&str> {
    match auth {
        DashboardAuthLevel::Partner {twitch_user_id,..} => Some(twitch_user_id),
        DashboardAuthLevel::Admin {actor:Some(actor)} => Some(&actor.twitch_user_id),
        _ => None,
    }
}

fn live_state(member: &Member, now: DateTime<Utc>) -> &'static str {
    if member.is_live != 1 { return "offline"; }
    let seen = member.last_seen_at.as_deref().and_then(|raw| DateTime::parse_from_rfc3339(raw).ok()).map(|v| v.with_timezone(&Utc));
    if seen.is_some_and(|seen| seen <= now+Duration::seconds(5) && seen >= now-Duration::minutes(5)) { "live" } else { "unknown" }
}

fn apply_title(mut profile: PlayerProfile, member: &Member, sessions: &[Session], now: DateTime<Utc>) -> PlayerProfile {
    // Current explicit intent can differ from the usual history (e.g. Brawl today).
    let live_title = (live_state(member,now)=="live" && member.last_game.as_deref().is_some_and(|g| g.eq_ignore_ascii_case("Deadlock")))
        .then_some(member.last_title.as_deref()).flatten();
    if let Some(mode) = live_title.and_then(title_mode) {
        profile.mode=Some(mode); profile.mode_source=Some("current_title".into()); profile.mode_samples=1;
    } else if profile.mode.is_none() {
        let title=sessions.iter().filter(|s| s.started_at>=now-Duration::days(14) && s.ended_at<=now && s.game_name.as_deref().is_some_and(|g| g.eq_ignore_ascii_case("Deadlock")))
            .max_by_key(|s| s.started_at).and_then(|s| s.stream_title.as_deref());
        if let Some(mode)=title.and_then(title_mode) { profile.mode=Some(mode);profile.mode_source=Some("historical_title".into());profile.mode_samples=1; }
    }
    profile
}

async fn load_members(pool: &PgPool) -> Result<Vec<Member>,sqlx::Error> {
    sqlx::query_as::<_,Member>(r#"
        SELECT p.twitch_user_id, lower(p.twitch_login) AS login,
               NULLIF(trim(i.discord_user_id),'') AS discord_id,
               COALESCE(l.is_live,0)::int AS is_live, l.last_seen_at,
               l.last_game, left(l.last_title,500) AS last_title
        FROM twitch_partners p
        LEFT JOIN twitch_streamer_identities i ON i.twitch_user_id=p.twitch_user_id
        LEFT JOIN twitch_live_state l ON l.twitch_user_id=p.twitch_user_id
        WHERE p.status='active' AND p.departnered_at IS NULL AND p.admin_archived_at IS NULL
          AND COALESCE(p.manual_partner_opt_out,0)=0
        ORDER BY lower(p.twitch_login) LIMIT 500
    "#).fetch_all(pool).await
}

async fn load_sessions(pool: &PgPool, logins: &[String], since: DateTime<Utc>, now: DateTime<Utc>) -> Result<Vec<Session>,sqlx::Error> {
    sqlx::query_as::<_,Session>(r#"
        SELECT streamer_login, started_at, ended_at, game_name, stream_title FROM (
            SELECT lower(streamer_login) AS streamer_login, started_at, ended_at,
                   game_name, left(stream_title,500) AS stream_title,
                   row_number() OVER (PARTITION BY lower(streamer_login) ORDER BY started_at DESC) AS n
            FROM twitch_stream_sessions
            WHERE lower(streamer_login)=ANY($1) AND ended_at>$2 AND ended_at<=$3
              AND started_at<$3 AND ended_at>started_at AND ended_at-started_at<=INTERVAL '48 hours'
        ) recent WHERE n<=180
    "#).bind(logins).bind(since).bind(now).fetch_all(pool).await
}

async fn enrich(members: &[Member]) -> HashMap<String,PlayerProfile> {
    let requests: Vec<(String, String)> = members.iter().filter_map(|m| m.discord_id.as_ref().map(|id| (m.login.clone(),id.clone()))).collect();
    let mut pending=stream::iter(requests).map(|(login,id)| async move { (login,sources::player_profile(&id).await) }).buffer_unordered(4);
    let deadline=tokio::time::Instant::now()+std::time::Duration::from_secs(6);
    let mut results=HashMap::new();
    while let Ok(Some((login,profile)))=tokio::time::timeout_at(deadline,pending.next()).await { results.insert(login,profile); }
    // Unfinished work is dropped, not left running as a background fan-out.
    results
}

pub async fn get_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>, Query(params): Query<Params>) -> Response {
    let login=match subject(&auth,params.streamer.as_deref()) { Ok(login)=>login,Err(resp)=>return resp };
    let days=match parse_bounded_query_int(params.days.as_deref(),"days",56,7,90) { Ok(days)=>days,Err(resp)=>return resp.into_response() };
    let now=Utc::now(); let since=now-Duration::days(days);
    let loaded=tokio::time::timeout(std::time::Duration::from_secs(5),async {
        let members=load_members(&pool).await?;
        let logins=members.iter().map(|m|m.login.clone()).collect::<Vec<_>>();
        let sessions=load_sessions(&pool,&logins,since,now).await?;
        Ok::<_,sqlx::Error>((members,sessions))
    }).await;
    let (members,sessions)=match loaded {
        Ok(Ok(data))=>data,
        Ok(Err(err))=>{tracing::warn!(%err,"Community history unavailable");return error(StatusCode::SERVICE_UNAVAILABLE,"Die Stream-Historie ist gerade nicht verfügbar.");},
        Err(_)=>return error(StatusCode::SERVICE_UNAVAILABLE,"Die Stream-Historie antwortet gerade zu langsam."),
    };
    let Some(own)=members.iter().find(|m|m.login==login).cloned() else {return error(StatusCode::NOT_FOUND,"Dieser Kanal ist derzeit nicht im aktiven Partner-Netzwerk.");};
    let mut histories:HashMap<String,Vec<Session>>=HashMap::new();
    for session in sessions { histories.entry(session.streamer_login.clone()).or_default().push(session); }
    let schedules:HashMap<_,_>=members.iter().map(|m|(m.login.clone(),schedule(histories.get(&m.login).map(Vec::as_slice).unwrap_or(&[]),since,now))).collect();
    let own_schedule=schedules[&login].clone();
    let empty=PlayerProfile::default();
    let mut candidates=members.iter().filter(|m|m.twitch_user_id!=own.twitch_user_id && m.login!=own.login).cloned().collect::<Vec<_>>();
    candidates.sort_by(|a,b| {
        let sa=match_score(&own_schedule,&schedules[&a.login],&empty,&empty);
        let sb=match_score(&own_schedule,&schedules[&b.login],&empty,&empty);
        sb.score.cmp(&sa.score).then(sb.observed_overlap_minutes.cmp(&sa.observed_overlap_minutes)).then(a.login.cmp(&b.login))
    });
    let candidate_count=candidates.len(); candidates.truncate(16);
    let mut selected=vec![own.clone()]; selected.extend(candidates.iter().cloned());
    let viewer_id=viewer_twitch_id(&auth);
    // Viewer identity lookup is independent of selected/recommended profiles.
    let (viewer_discord, identity_unavailable) = if let Some(id) = viewer_id {
        let lookup = sqlx::query_scalar::<_, Option<String>>(
            "SELECT NULLIF(trim(discord_user_id),'') FROM twitch_streamer_identities WHERE twitch_user_id=$1"
        ).bind(id).fetch_optional(&pool);
        match tokio::time::timeout(std::time::Duration::from_secs(2), lookup).await {
            Ok(Ok(id)) => (id.flatten(), false),
            _ => (None, true),
        }
    } else { (None, false) };
    let directory = async {
        if identity_unavailable {
            sources::LobbyResult { status: "unavailable".into(), captured_at: None, lobbies: vec![] }
        } else { sources::lobbies(viewer_discord.as_deref()).await }
    };
    let (mut profiles,discord)=tokio::join!(enrich(&selected),directory);
    let profile_for=|member:&Member,profiles:&mut HashMap<String,PlayerProfile>| {
        let profile=profiles.remove(&member.login).unwrap_or_else(||PlayerProfile {source_status:if member.discord_id.is_some(){"not_checked"}else{"not_linked"}.into(),..Default::default()});
        apply_title(profile,member,histories.get(&member.login).map(Vec::as_slice).unwrap_or(&[]),now)
    };
    let own_profile=profile_for(&own,&mut profiles);
    let mut recommendations=candidates.into_iter().map(|m| {
        let profile=profile_for(&m,&mut profiles); let candidate_schedule=schedules[&m.login].clone();
        let matching=match_score(&own_schedule,&candidate_schedule,&own_profile,&profile);
        let state=live_state(&m,now);
        let both_live_same_game=state=="live" && live_state(&own,now)=="live" && own.last_game.as_deref().zip(m.last_game.as_deref()).is_some_and(|(a,b)|!a.trim().is_empty()&&a.eq_ignore_ascii_case(b));
        Recommendation {login:m.login.clone(),live_state:state,current_game:(state=="live").then_some(m.last_game).flatten(),current_title:(state=="live").then_some(m.last_title).flatten(),both_live_same_game,profile,schedule:candidate_schedule,matching}
    }).collect::<Vec<_>>();
    recommendations.sort_by(|a,b| b.matching.compatible.cmp(&a.matching.compatible).then(b.matching.score.cmp(&a.matching.score)).then(b.both_live_same_game.cmp(&a.both_live_same_game)).then(a.login.cmp(&b.login)));
    let evaluated_count=recommendations.len();
    ([(header::CACHE_CONTROL,"private, no-store")],Json(CommunityResponse {generated_at:Utc::now(),days,timezone:"Europe/Berlin",streamer:login,own_profile,own_schedule,recommendations,discord,candidate_count,evaluated_count})).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn auth_scope_is_never_chosen_by_browser() {
        let auth=DashboardAuthLevel::Partner {twitch_login:"alice".into(),twitch_user_id:"1".into(),display_name:"Alice".into()};
        assert_eq!(subject(&auth,None).unwrap(),"alice");
        assert_eq!(subject(&auth,Some("ALICE")).unwrap(),"alice");
        assert_eq!(subject(&auth,Some("bob")).unwrap_err().status(),StatusCode::FORBIDDEN);
        assert_eq!(subject(&DashboardAuthLevel::None,Some("alice")).unwrap_err().status(),StatusCode::UNAUTHORIZED);
        assert_eq!(viewer_twitch_id(&DashboardAuthLevel::admin()),None);
    }
    #[test] fn stale_live_flags_do_not_advertise_live() {
        let now=Utc::now();
        let mut member=Member {twitch_user_id:"1".into(),login:"test".into(),discord_id:None,is_live:1,last_seen_at:Some((now-Duration::hours(1)).to_rfc3339()),last_game:Some("Deadlock".into()),last_title:None};
        assert_eq!(live_state(&member,now),"unknown");
        member.last_seen_at=Some(now.to_rfc3339());assert_eq!(live_state(&member,now),"live");
        member.last_seen_at=Some("invalid".into());assert_eq!(live_state(&member,now),"unknown");
    }
    #[tokio::test] async fn anonymous_request_does_not_touch_database() {
        let pool=sqlx::postgres::PgPoolOptions::new().connect_lazy("postgres://localhost/unused").unwrap();
        let response=get_handler(DashboardAuthLevel::None,State(pool),Query(Params::default())).await;
        assert_eq!(response.status(),StatusCode::UNAUTHORIZED);
    }
}
