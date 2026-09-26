//! Read-only Diagnose-Endpoint: Twitch-Auth/Scope-Status zu einer Discord-User-ID.
//!
//! `GET /internal/twitch/v1/diagnose?discord_id=...`
//!
//! Liefert einen sanitisierten Status für den Self-Service-Support (FAQ-/Ticket-Bot):
//! ist ein Twitch-Streamer-Account verknüpft, ist OAuth verbunden, welche Pflicht-
//! Scopes fehlen, ist eine Neu-Autorisierung nötig, Partner-/Live-Status und ob die
//! Discord-Verknüpfung steht. **Nur lesend, keine Tokens/Secrets in der Antwort.**
//!
//! Die Status-Logik wird 1:1 aus `tb_analytics::admin_streamers` wiederverwendet
//! (`scope_snapshot`, `partner_status`, `streamer_detail`) — Einstieg ist hier die
//! Discord-ID statt des Logins.

use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Extension, Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};
use tb_analytics::admin_streamers;
use tb_http_core::{ApiError, AuthLevel};
use tb_transport_twitch::{streams::HelixStream, HelixClient};
use tokio::sync::{Mutex, Semaphore};

const LIVE_MAX_AGE: i64 = 180;
const HELIX_TIMEOUT: Duration = Duration::from_millis(1200);
const HELIX_CACHE_AGE: Duration = Duration::from_secs(30);
const HELIX_CACHE_LIMIT: usize = 256;
const HELIX_CALLS_PER_MINUTE: usize = 60;

#[derive(Clone)]
struct LiveEvidence {
    is_live: bool,
    last_seen_at: Option<String>,
    twitch_login: Option<String>,
}

struct CachedEvidence {
    evidence: LiveEvidence,
    cached_at: Instant,
}

/// Limits on-demand checks for linked role holders absent from the poller.
pub struct LiveProbeCache {
    entries: Mutex<HashMap<String, CachedEvidence>>,
    calls: Mutex<VecDeque<Instant>>,
    permits: Semaphore,
}

impl Default for LiveProbeCache {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            calls: Mutex::new(VecDeque::new()),
            permits: Semaphore::new(8),
        }
    }
}

impl LiveProbeCache {
    async fn cached_evidence(&self, id: &str) -> Option<LiveEvidence> {
        let entries = self.entries.lock().await;
        entries.get(id).and_then(|entry| {
            (entry.cached_at.elapsed() < HELIX_CACHE_AGE).then(|| entry.evidence.clone())
        })
    }

    async fn claim_helix_call(&self) -> bool {
        let mut calls = self.calls.lock().await;
        while calls
            .front()
            .is_some_and(|called| called.elapsed() >= Duration::from_secs(60))
        {
            calls.pop_front();
        }
        if calls.len() >= HELIX_CALLS_PER_MINUTE {
            return false;
        }
        calls.push_back(Instant::now());
        true
    }
}

fn fresh_poller_evidence(linked: &admin_streamers::DiscordLinkedLiveState) -> Option<LiveEvidence> {
    if linked.is_live == 0
        || linked
            .twitch_login
            .as_deref()
            .is_none_or(|name| name.trim().is_empty())
    {
        return None;
    }
    let seen = linked
        .last_seen_at
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())?;
    let age = Utc::now().signed_duration_since(seen);
    if age < chrono::Duration::zero() || age > chrono::Duration::seconds(LIVE_MAX_AGE) {
        return None;
    }
    Some(LiveEvidence {
        is_live: true,
        last_seen_at: linked.last_seen_at.clone(),
        twitch_login: linked.twitch_login.clone(),
    })
}

fn helix_evidence(id: &str, streams: Vec<HelixStream>) -> LiveEvidence {
    let stream = streams.into_iter().find(|stream| {
        stream.user_id == id && !stream.id.is_empty() && !stream.user_login.trim().is_empty()
    });
    match stream {
        Some(stream) => LiveEvidence {
            is_live: true,
            last_seen_at: Some(Utc::now().to_rfc3339()),
            twitch_login: Some(stream.user_login),
        },
        None => LiveEvidence {
            is_live: false,
            last_seen_at: None,
            twitch_login: None,
        },
    }
}

async fn live_evidence(
    linked: &admin_streamers::DiscordLinkedLiveState,
    helix: &Option<HelixClient>,
    cache: &LiveProbeCache,
) -> LiveEvidence {
    if let Some(evidence) = fresh_poller_evidence(linked) {
        return evidence;
    }
    let denied = || LiveEvidence {
        is_live: false,
        last_seen_at: None,
        twitch_login: None,
    };
    let id = linked.twitch_user_id.trim();
    if id.is_empty()
        || !id.bytes().all(|byte| byte.is_ascii_digit())
        || !id.parse::<u64>().is_ok_and(|value| value > 0)
    {
        return denied();
    }
    let Some(helix) = helix.as_ref() else {
        return denied();
    };
    if let Some(evidence) = cache.cached_evidence(id).await {
        return evidence;
    }
    let Ok(_permit) = cache.permits.try_acquire() else {
        return denied();
    };
    if !cache.claim_helix_call().await {
        return denied();
    }
    let response = tokio::time::timeout(
        HELIX_TIMEOUT,
        helix.get_streams_by_user_ids(&[id.to_string()], None),
    )
    .await;
    let evidence = match response {
        Ok(Ok(streams)) => helix_evidence(id, streams),
        _ => return denied(),
    };
    let mut entries = cache.entries.lock().await;
    entries.retain(|_, entry| entry.cached_at.elapsed() < HELIX_CACHE_AGE);
    if entries.len() >= HELIX_CACHE_LIMIT {
        entries.clear();
    }
    entries.insert(
        id.to_string(),
        CachedEvidence {
            evidence: evidence.clone(),
            cached_at: Instant::now(),
        },
    );
    evidence
}

#[derive(Deserialize)]
pub struct DiagnoseQuery {
    #[serde(default)]
    pub discord_id: Option<String>,
}

#[derive(Serialize)]
pub struct DiagnoseResponse {
    pub ok: bool,
    /// true, wenn zur Discord-ID eine Twitch-User-ID verknüpft ist
    pub found: bool,
    pub twitch_login: Option<String>,
    pub discord_linked: bool,
    pub oauth_connected: bool,
    pub needs_reauth: bool,
    /// "reauth" | "missing" | "partial" | "connected"
    pub oauth_status: String,
    pub missing_scopes: Vec<String>,
    pub granted_scope_count: usize,
    pub required_scope_count: usize,
    /// Zeitpunkt der letzten Autorisierung (kein Token, kein Ablaufdatum verfügbar).
    pub authorized_at: Option<String>,
    /// "active" | "departnered" | "archived" | "blocked" | "token_error" | "non_partner"
    pub partner_status: String,
    pub is_partner_active: bool,
    pub is_verified: bool,
    pub is_monitored_only: bool,
    pub is_live: bool,
    /// Letzte erfolgreiche Live-Beobachtung des Pollers, unabhängig vom Spiel.
    /// Berechtigungsprüfungen müssen fehlende oder veraltete Werte ablehnen.
    pub last_seen_at: Option<String>,
    pub raid_bot_enabled: bool,
    pub technical_pause_reason: Option<String>,
    pub operational_state: Option<String>,
}

/// Antwort, wenn kein verknüpfter/auswertbarer Streamer-Account existiert.
fn empty_response(twitch_login: Option<String>) -> DiagnoseResponse {
    DiagnoseResponse {
        ok: true,
        found: false,
        twitch_login,
        discord_linked: false,
        oauth_connected: false,
        needs_reauth: false,
        oauth_status: "missing".to_string(),
        missing_scopes: admin_streamers::REQUIRED_SCOPES
            .iter()
            .map(|s| s.to_string())
            .collect(),
        granted_scope_count: 0,
        required_scope_count: admin_streamers::REQUIRED_SCOPES.len(),
        authorized_at: None,
        partner_status: "non_partner".to_string(),
        is_partner_active: false,
        is_verified: false,
        is_monitored_only: false,
        is_live: false,
        last_seen_at: None,
        raid_bot_enabled: false,
        technical_pause_reason: None,
        operational_state: None,
    }
}

fn linked_response(linked: &admin_streamers::DiscordLinkedLiveState) -> DiagnoseResponse {
    let mut response = empty_response(linked.twitch_login.clone());
    response.found = true;
    response.discord_linked = true;
    response.is_live = linked.is_live != 0;
    response.last_seen_at = linked.last_seen_at.clone();
    response
}

pub async fn handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Query(q): Query<DiagnoseQuery>,
    Extension(helix): Extension<Arc<Option<HelixClient>>>,
    Extension(cache): Extension<Arc<LiveProbeCache>>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let discord_id = q
        .discord_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::bad_request("missing discord_id"))?;

    let linked = admin_streamers::linked_live_state_for_discord_user(&pool, &discord_id)
        .await
        .map_err(|e| {
            tracing::error!("diagnose: identity/live lookup failed: {e}");
            ApiError::internal()
        })?;

    let Some(linked) = linked else {
        // Keine Twitch-Streamer-Verknüpfung zu dieser Discord-ID.
        return Ok(Json(empty_response(None)));
    };

    let evidence = live_evidence(&linked, helix.as_ref(), &cache).await;
    let mut linked_response = linked_response(&linked);
    linked_response.is_live = evidence.is_live;
    linked_response.last_seen_at = evidence.last_seen_at.clone();
    if evidence.twitch_login.is_some() {
        linked_response.twitch_login = evidence.twitch_login.clone();
    }

    let Some(login) = linked.twitch_login.as_deref() else {
        return Ok(Json(linked_response));
    };

    let detail = admin_streamers::streamer_detail(&pool, login)
        .await
        .map_err(|e| {
            tracing::error!("diagnose: streamer_detail failed: {e}");
            ApiError::internal()
        })?;

    let Some(row) =
        detail.filter(|row| row.twitch_user_id.as_deref() == Some(linked.twitch_user_id.as_str()))
    else {
        // Kein Partner-State oder ein Login wurde inzwischen an ein anderes
        // Twitch-Konto vergeben. Live-Rechte bleiben an die verknüpfte ID gebunden.
        return Ok(Json(linked_response));
    };

    let snap =
        admin_streamers::scope_snapshot(row.scopes.as_deref(), row.needs_reauth.unwrap_or(false));
    let pstatus = admin_streamers::partner_status(
        row.status.as_deref(),
        row.archived_at.as_deref(),
        row.manual_partner_opt_out.unwrap_or(0),
        row.technical_pause_reason.as_deref(),
    );

    let resp = DiagnoseResponse {
        ok: true,
        found: true,
        twitch_login: evidence
            .twitch_login
            .or_else(|| Some(row.twitch_login.clone())),
        discord_linked: row.is_on_discord.unwrap_or(0) != 0,
        oauth_connected: snap.connected,
        needs_reauth: snap.needs_reauth,
        oauth_status: snap.status.to_string(),
        missing_scopes: snap.missing_scopes,
        granted_scope_count: snap.granted_scopes.len(),
        required_scope_count: admin_streamers::REQUIRED_SCOPES.len(),
        authorized_at: row.authorized_at.map(crate::security::datetime_to_iso),
        partner_status: pstatus.to_string(),
        is_partner_active: row.is_partner_active != 0,
        is_verified: row.is_verified != 0,
        is_monitored_only: row.is_monitored_only.unwrap_or(0) != 0,
        is_live: evidence.is_live,
        last_seen_at: evidence.last_seen_at,
        raid_bot_enabled: row.raid_bot_enabled.unwrap_or(0) != 0,
        technical_pause_reason: row.technical_pause_reason.clone(),
        operational_state: row.operational_state.clone(),
    };
    Ok(Json(resp))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linked(
        id: &str,
        is_live: i32,
        last_seen_at: Option<String>,
    ) -> admin_streamers::DiscordLinkedLiveState {
        admin_streamers::DiscordLinkedLiveState {
            twitch_user_id: id.to_string(),
            twitch_login: Some("streamer".to_string()),
            is_live,
            last_seen_at,
        }
    }

    #[test]
    fn poller_positiv_nur_mit_frischem_timestamp() {
        let now = Utc::now();
        assert!(fresh_poller_evidence(&linked("42", 1, Some(now.to_rfc3339()))).is_some());
        assert!(fresh_poller_evidence(&linked("42", 0, Some(now.to_rfc3339()))).is_none());
        assert!(fresh_poller_evidence(&linked(
            "42",
            1,
            Some((now - chrono::Duration::minutes(4)).to_rfc3339())
        ))
        .is_none());
        assert!(fresh_poller_evidence(&linked(
            "42",
            1,
            Some((now + chrono::Duration::minutes(1)).to_rfc3339())
        ))
        .is_none());
    }

    #[test]
    fn helix_live_nachweis_akzeptiert_jedes_spiel_nur_fuer_verknuepfte_id() {
        let stream = HelixStream {
            id: "live-1".to_string(),
            user_id: "42".to_string(),
            user_login: "umbenannt".to_string(),
            game_name: "Just Chatting".to_string(),
            ..Default::default()
        };
        let allowed = helix_evidence("42", vec![stream.clone()]);
        assert!(allowed.is_live);
        assert_eq!(allowed.twitch_login.as_deref(), Some("umbenannt"));
        assert!(allowed.last_seen_at.is_some());
        let denied = helix_evidence("43", vec![stream]);
        assert!(!denied.is_live);
        assert!(denied.last_seen_at.is_none());
    }

    #[tokio::test]
    async fn nicht_beobachteter_streamer_kann_id_gebundenen_cache_nutzen() {
        let cache = LiveProbeCache::default();
        cache.entries.lock().await.insert(
            "42".to_string(),
            CachedEvidence {
                evidence: LiveEvidence {
                    is_live: true,
                    last_seen_at: Some(Utc::now().to_rfc3339()),
                    twitch_login: Some("streamer".to_string()),
                },
                cached_at: Instant::now(),
            },
        );
        let evidence = cache.cached_evidence("42").await.expect("cached id");
        assert!(evidence.is_live);
        assert!(evidence.last_seen_at.is_some());
        assert!(cache.cached_evidence("43").await.is_none());
        let invalid = live_evidence(&linked("streamer", 0, None), &None, &cache).await;
        assert!(!invalid.is_live);
    }

    #[tokio::test]
    async fn helix_budget_stoppt_weitere_anfragen() {
        let cache = LiveProbeCache::default();
        for _ in 0..HELIX_CALLS_PER_MINUTE {
            assert!(cache.claim_helix_call().await);
        }
        assert!(!cache.claim_helix_call().await);
    }

    #[test]
    fn diagnose_live_freshness_is_missing_without_observation() {
        let value = serde_json::to_value(empty_response(None)).unwrap();
        assert_eq!(value["is_live"], false);
        assert!(value["last_seen_at"].is_null());
    }

    #[test]
    fn diagnose_live_freshness_serializes_original_timestamp() {
        let mut response = empty_response(Some("streamer".to_string()));
        response.found = true;
        response.is_live = true;
        response.last_seen_at = Some("2026-09-24T19:59:30+00:00".to_string());
        let value = serde_json::to_value(response).unwrap();
        assert_eq!(value["last_seen_at"], "2026-09-24T19:59:30+00:00");
        assert_eq!(value["is_live"], true);
    }

    #[test]
    fn verknuepfter_nichtpartner_bekommt_den_id_gebundenen_live_nachweis() {
        let linked = admin_streamers::DiscordLinkedLiveState {
            twitch_user_id: "42".to_string(),
            twitch_login: Some("streamer".to_string()),
            is_live: 1,
            last_seen_at: Some("2026-09-24T19:59:30+00:00".to_string()),
        };
        let value = serde_json::to_value(linked_response(&linked)).unwrap();
        assert_eq!(value["found"], true);
        assert_eq!(value["discord_linked"], true);
        assert_eq!(value["partner_status"], "non_partner");
        assert_eq!(value["is_partner_active"], false);
        assert_eq!(value["is_live"], true);
        assert_eq!(value["last_seen_at"], "2026-09-24T19:59:30+00:00");
    }

    #[test]
    fn empty_response_form_ohne_login() {
        let resp = empty_response(None);
        assert!(resp.ok);
        assert!(!resp.found);
        assert_eq!(resp.twitch_login, None);
        assert_eq!(resp.oauth_status, "missing");
        assert!(!resp.oauth_connected);
        assert!(!resp.needs_reauth);
        assert_eq!(resp.granted_scope_count, 0);
        assert_eq!(
            resp.required_scope_count,
            admin_streamers::REQUIRED_SCOPES.len()
        );
        // missing_scopes spiegelt vollständig die Pflicht-Scopes wider
        assert_eq!(
            resp.missing_scopes.len(),
            admin_streamers::REQUIRED_SCOPES.len()
        );
        assert_eq!(resp.partner_status, "non_partner");
    }

    #[test]
    fn empty_response_uebernimmt_login() {
        let resp = empty_response(Some("dragscope".to_string()));
        assert!(!resp.found);
        assert_eq!(resp.twitch_login.as_deref(), Some("dragscope"));
    }

    #[test]
    fn diagnose_response_serialisiert_zu_json() {
        let resp = empty_response(None);
        let v = serde_json::to_value(&resp).expect("serde-Serialisierung muss klappen");
        assert_eq!(v["ok"], true);
        assert_eq!(v["found"], false);
        assert_eq!(v["oauth_status"], "missing");
        assert_eq!(v["granted_scope_count"], 0);
        assert_eq!(
            v["required_scope_count"],
            admin_streamers::REQUIRED_SCOPES.len()
        );
        assert!(v["twitch_login"].is_null());
        assert!(v["missing_scopes"].is_array());
    }
}
