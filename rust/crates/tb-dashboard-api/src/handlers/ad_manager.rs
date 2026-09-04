//! Session-gebundene API des Twitch-Werbemanagers.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgPool, Row};
use tb_analytics::ad_manager::{
    AdManagerStore, EnqueueOutcome, Settings, COMMERCIAL_SCOPE, READ_SCOPE, SNOOZE_SCOPE,
};

use crate::auth::level::DashboardAuthLevel;

const RECONNECT_URL: &str = "/twitch/raid/auth?scope_profile=dashboard_reauth";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdentityError {
    TwitchRequired,
    Unauthorized,
}

impl IdentityError {
    fn into_response(self) -> Response {
        match self {
            Self::TwitchRequired => (
                StatusCode::FORBIDDEN,
                Json(json!({"error":"Eine Twitch-Identität ist erforderlich."})),
            )
                .into_response(),
            Self::Unauthorized => crate::auth::unauthorized_v2_json().into_response(),
        }
    }
}

fn identity(auth: DashboardAuthLevel) -> Result<(String, String), IdentityError> {
    match auth {
        DashboardAuthLevel::Partner {
            twitch_user_id,
            twitch_login,
            ..
        } => Ok((twitch_user_id, twitch_login)),
        DashboardAuthLevel::Admin { actor: Some(actor) } => {
            Ok((actor.twitch_user_id, actor.twitch_login))
        }
        DashboardAuthLevel::Admin { actor: None } => Err(IdentityError::TwitchRequired),
        DashboardAuthLevel::None => Err(IdentityError::Unauthorized),
    }
}

pub(crate) async fn scopes(pool: &PgPool, uid: &str) -> Result<Vec<String>, sqlx::Error> {
    let raw: Option<String> =
        sqlx::query_scalar("SELECT scopes FROM twitch_raid_auth WHERE twitch_user_id=$1")
            .bind(uid)
            .fetch_optional(pool)
            .await?
            .flatten();
    Ok(raw
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_owned)
        .collect())
}

fn has(scopes: &[String], needle: &str) -> bool {
    scopes.iter().any(|s| s == needle)
}

fn missing(scopes: &[String], required: &[&str]) -> Vec<String> {
    required
        .iter()
        .filter(|scope| !has(scopes, scope))
        .map(|s| (*s).to_owned())
        .collect()
}

fn reauth(missing_scopes: Vec<String>) -> Response {
    (StatusCode::CONFLICT,Json(json!({"error":"reauth_required","missingScopes":missing_scopes,"reconnectUrl":RECONNECT_URL}))).into_response()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsResponse {
    #[serde(flatten)]
    value: Settings,
    updated_at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScopeStatus {
    read: bool,
    snooze: bool,
    commercial: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LastAction {
    kind: String,
    outcome: String,
    detail: Option<String>,
    at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusResponse {
    is_live: bool,
    next_ad_at: Option<String>,
    last_ad_at: Option<String>,
    duration_seconds: Option<i32>,
    preroll_free_seconds: Option<i32>,
    snooze_count: Option<i32>,
    snooze_refresh_at: Option<String>,
    observed_at: Option<String>,
    worker_healthy: bool,
    worker_heartbeat_at: Option<String>,
    last_action: Option<LastAction>,
    scopes: ScopeStatus,
}

fn iso(value: Option<DateTime<Utc>>) -> Option<String> {
    value.map(|v| v.to_rfc3339())
}

fn worker_is_healthy(heartbeat: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    heartbeat
        .map(|at| at >= now - chrono::Duration::seconds(120))
        .unwrap_or(false)
}

fn apply_saved_settings(
    body: &mut serde_json::Value,
    settings: Settings,
    updated_at: DateTime<Utc>,
) {
    body["settings"] = json!(SettingsResponse {
        value: settings,
        updated_at: Some(updated_at.to_rfc3339()),
    });
}

async fn response(pool: &PgPool, uid: &str) -> Result<serde_json::Value, sqlx::Error> {
    let store = AdManagerStore::new(pool.clone());
    let (settings, updated) = store
        .load_settings(uid)
        .await?
        .map(|(s, t)| (s, Some(t.to_rfc3339())))
        .unwrap_or((Settings::default(), None));
    let granted = scopes(pool, uid).await?;
    let row=sqlx::query("SELECT is_live,next_ad_at,last_ad_at,duration_seconds,preroll_free_seconds,snooze_count,snooze_refresh_at,observed_at,worker_heartbeat_at,last_action_kind,last_action_outcome,last_action_detail,last_action_at FROM twitch_ad_manager_state WHERE twitch_user_id=$1").bind(uid).fetch_optional(pool).await?;
    let status = if let Some(r) = row {
        let last_at: Option<DateTime<Utc>> = r.try_get("last_action_at")?;
        let heartbeat: Option<DateTime<Utc>> = r.try_get("worker_heartbeat_at")?;
        StatusResponse {
            is_live: r.try_get("is_live")?,
            next_ad_at: iso(r.try_get("next_ad_at")?),
            last_ad_at: iso(r.try_get("last_ad_at")?),
            duration_seconds: r.try_get("duration_seconds")?,
            preroll_free_seconds: r.try_get("preroll_free_seconds")?,
            snooze_count: r.try_get("snooze_count")?,
            snooze_refresh_at: iso(r.try_get("snooze_refresh_at")?),
            observed_at: iso(r.try_get("observed_at")?),
            worker_healthy: worker_is_healthy(heartbeat, Utc::now()),
            worker_heartbeat_at: iso(heartbeat),
            last_action: match (
                r.try_get::<Option<String>, _>("last_action_kind")?,
                r.try_get::<Option<String>, _>("last_action_outcome")?,
                last_at,
            ) {
                (Some(kind), Some(outcome), Some(at)) => Some(LastAction {
                    kind,
                    outcome,
                    detail: r.try_get("last_action_detail")?,
                    at: at.to_rfc3339(),
                }),
                _ => None,
            },
            scopes: ScopeStatus {
                read: has(&granted, READ_SCOPE),
                snooze: has(&granted, SNOOZE_SCOPE),
                commercial: has(&granted, COMMERCIAL_SCOPE),
            },
        }
    } else {
        StatusResponse {
            is_live: false,
            next_ad_at: None,
            last_ad_at: None,
            duration_seconds: None,
            preroll_free_seconds: None,
            snooze_count: None,
            snooze_refresh_at: None,
            observed_at: None,
            worker_healthy: false,
            worker_heartbeat_at: None,
            last_action: None,
            scopes: ScopeStatus {
                read: has(&granted, READ_SCOPE),
                snooze: has(&granted, SNOOZE_SCOPE),
                commercial: has(&granted, COMMERCIAL_SCOPE),
            },
        }
    };
    Ok(json!({"settings":SettingsResponse{value:settings,updated_at:updated},"status":status}))
}

pub async fn get_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>) -> Response {
    let (uid, _) = match identity(auth) {
        Ok(v) => v,
        Err(error) => return error.into_response(),
    };
    match response(&pool, &uid).await {
        Ok(body) => Json(body).into_response(),
        Err(error) => {
            tracing::error!(%error,"Werbemanager konnte nicht gelesen werden");
            crate::auth::analytics_request_failed_json().into_response()
        }
    }
}

pub async fn save_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Json(settings): Json<Settings>,
) -> Response {
    let (uid, login) = match identity(auth) {
        Ok(v) => v,
        Err(error) => return error.into_response(),
    };
    if let Err(message) = settings.validate() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"validation_error","message":message})),
        )
            .into_response();
    }
    let granted = match scopes(&pool, &uid).await {
        Ok(v) => v,
        Err(error) => {
            tracing::error!(%error,"Werbemanager-Scopes konnten nicht gelesen werden");
            return crate::auth::analytics_request_failed_json().into_response();
        }
    };
    if settings.enabled {
        let absent = missing(&granted, settings.strategy.required_scopes());
        if !absent.is_empty() {
            return reauth(absent);
        }
    }
    let mut body = match response(&pool, &uid).await {
        Ok(body) => body,
        Err(error) => {
            tracing::error!(%error,"Werbemanager-Status konnte vor dem Speichern nicht gelesen werden");
            return crate::auth::analytics_request_failed_json().into_response();
        }
    };
    let store = AdManagerStore::new(pool.clone());
    let updated_at = match store.save_settings(&uid, &login, &settings).await {
        Ok(updated_at) => updated_at,
        Err(error) => {
            tracing::error!(%error,"Werbemanager konnte nicht gespeichert werden");
            return crate::auth::analytics_request_failed_json().into_response();
        }
    };
    apply_saved_settings(&mut body, settings, updated_at);
    Json(body).into_response()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActionInput {
    action: String,
    duration_seconds: Option<i32>,
    idempotency_key: String,
}

fn validate_action(
    input: &ActionInput,
) -> Result<(Option<i32>, &'static [&'static str]), &'static str> {
    if uuid::Uuid::parse_str(input.idempotency_key.trim()).is_err() {
        return Err("idempotencyKey muss eine gültige UUID sein");
    }
    match input.action.as_str() {
        "snooze" if input.duration_seconds.is_none() => Ok((None, &[READ_SCOPE, SNOOZE_SCOPE])),
        "commercial"
            if input
                .duration_seconds
                .map(|duration| [30, 60, 90, 120, 150, 180].contains(&duration))
                .unwrap_or(false) =>
        {
            Ok((input.duration_seconds, &[READ_SCOPE, COMMERCIAL_SCOPE]))
        }
        _ => Err("Ungültige Werbeaktion"),
    }
}

fn enqueue_response(outcome: EnqueueOutcome) -> Response {
    match outcome {
        EnqueueOutcome::Queued => {
            (StatusCode::ACCEPTED, Json(json!({"queued":true}))).into_response()
        }
        EnqueueOutcome::AlreadyAccepted => (
            StatusCode::ACCEPTED,
            Json(json!({"queued":true,"idempotentReplay":true})),
        )
            .into_response(),
        EnqueueOutcome::Conflict => (
            StatusCode::CONFLICT,
            Json(json!({"queued":false,"error":"action_already_pending"})),
        )
            .into_response(),
        EnqueueOutcome::RateLimited => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"queued":false,"error":"rate_limited","message":"Zu viele manuelle Werbeaktionen. Bitte versuche es später erneut."})),
        ).into_response(),
    }
}

pub async fn action_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Json(input): Json<ActionInput>,
) -> Response {
    let (uid, login) = match identity(auth) {
        Ok(v) => v,
        Err(error) => return error.into_response(),
    };
    let (duration, required) = match validate_action(&input) {
        Ok(value) => value,
        Err(message) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":"validation_error","message":message})),
            )
                .into_response()
        }
    };
    let granted = match scopes(&pool, &uid).await {
        Ok(v) => v,
        Err(error) => {
            tracing::error!(%error,"Werbemanager-Scopes konnten vor der Aktion nicht gelesen werden");
            return crate::auth::analytics_request_failed_json().into_response();
        }
    };
    let absent = missing(&granted, required);
    if !absent.is_empty() {
        return reauth(absent);
    }
    let store = AdManagerStore::new(pool.clone());
    match store.load_settings(&uid).await {
        Ok(None) => {
            if let Err(error) = store
                .save_settings(&uid, &login, &Settings::default())
                .await
            {
                tracing::error!(%error,"Standardeinstellungen konnten nicht angelegt werden");
                return crate::auth::analytics_request_failed_json().into_response();
            }
        }
        Err(error) => {
            tracing::error!(%error,"Werbemanager-Einstellungen konnten nicht geprüft werden");
            return crate::auth::analytics_request_failed_json().into_response();
        }
        Ok(Some(_)) => {}
    }
    let idempotency = format!(
        "manual:{uid}:{}:{}",
        input.action,
        input.idempotency_key.trim().to_ascii_lowercase()
    );
    match store
        .enqueue(&uid, &login, &input.action, duration, &uid, &idempotency)
        .await
    {
        Ok(outcome) => enqueue_response(outcome),
        Err(error) => {
            tracing::error!(%error,"Werbeaktion konnte nicht eingereiht werden");
            crate::auth::analytics_request_failed_json().into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::level::AdminActor;

    #[test]
    fn partner_und_twitch_admin_sind_stets_auf_die_session_id_begrenzt() {
        let partner = DashboardAuthLevel::Partner {
            twitch_login: "nani".into(),
            twitch_user_id: "42".into(),
            display_name: "Nani".into(),
        };
        assert_eq!(identity(partner).ok(), Some(("42".into(), "nani".into())));

        let admin = DashboardAuthLevel::Admin {
            actor: Some(AdminActor {
                twitch_user_id: "77".into(),
                twitch_login: "admin".into(),
            }),
        };
        assert_eq!(identity(admin).ok(), Some(("77".into(), "admin".into())));
        assert_eq!(
            identity(DashboardAuthLevel::admin()).unwrap_err(),
            IdentityError::TwitchRequired
        );
        assert_eq!(
            identity(DashboardAuthLevel::None).unwrap_err(),
            IdentityError::Unauthorized
        );
        assert_eq!(
            IdentityError::TwitchRequired.into_response().status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            IdentityError::Unauthorized.into_response().status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[test]
    fn actionsvertrag_erzwingt_form_dauer_und_scopes() {
        let snooze = ActionInput {
            action: "snooze".into(),
            duration_seconds: None,
            idempotency_key: "018f0f65-7c2e-7df1-9f64-4a8f5b711234".into(),
        };
        assert_eq!(
            validate_action(&snooze).unwrap(),
            (None, &[READ_SCOPE, SNOOZE_SCOPE][..])
        );
        let commercial = ActionInput {
            action: "commercial".into(),
            duration_seconds: Some(180),
            idempotency_key: "018f0f65-7c2e-7df1-9f64-4a8f5b711235".into(),
        };
        assert_eq!(
            validate_action(&commercial).unwrap(),
            (Some(180), &[READ_SCOPE, COMMERCIAL_SCOPE][..])
        );
        for invalid in [
            ActionInput {
                action: "snooze".into(),
                duration_seconds: Some(30),
                idempotency_key: "018f0f65-7c2e-7df1-9f64-4a8f5b711236".into(),
            },
            ActionInput {
                action: "commercial".into(),
                duration_seconds: None,
                idempotency_key: "018f0f65-7c2e-7df1-9f64-4a8f5b711237".into(),
            },
            ActionInput {
                action: "commercial".into(),
                duration_seconds: Some(45),
                idempotency_key: "018f0f65-7c2e-7df1-9f64-4a8f5b711238".into(),
            },
            ActionInput {
                action: "pause".into(),
                duration_seconds: None,
                idempotency_key: "018f0f65-7c2e-7df1-9f64-4a8f5b711239".into(),
            },
            ActionInput {
                action: "snooze".into(),
                duration_seconds: None,
                idempotency_key: "keine-uuid".into(),
            },
        ] {
            assert!(validate_action(&invalid).is_err());
        }
    }

    #[test]
    fn reauth_ist_409_und_scope_pruefung_ist_exakt() {
        let granted = vec![READ_SCOPE.to_string()];
        assert_eq!(
            missing(&granted, &[READ_SCOPE, SNOOZE_SCOPE]),
            vec![SNOOZE_SCOPE]
        );
        assert_eq!(
            reauth(vec![SNOOZE_SCOPE.into()]).status(),
            StatusCode::CONFLICT
        );
    }

    #[test]
    fn worker_health_ist_bei_fehlendem_oder_altem_heartbeat_false() {
        let now = Utc::now();
        assert!(!worker_is_healthy(None, now));
        assert!(worker_is_healthy(
            Some(now - chrono::Duration::seconds(120)),
            now
        ));
        assert!(!worker_is_healthy(
            Some(now - chrono::Duration::seconds(121)),
            now
        ));
    }

    #[tokio::test]
    async fn idempotente_wiederholung_bleibt_accepted_und_queued() {
        use axum::body::to_bytes;

        let response = enqueue_response(EnqueueOutcome::AlreadyAccepted);
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["queued"], true);
        assert_eq!(json["idempotentReplay"], true);
    }

    #[test]
    fn save_ersetzt_nur_settings_im_vorher_geladenen_status() {
        let updated = Utc::now();
        let mut body = json!({"settings":{},"status":{"workerHealthy":true}});
        let settings = Settings {
            enabled: true,
            ..Settings::default()
        };
        apply_saved_settings(&mut body, settings, updated);
        assert_eq!(body["settings"]["enabled"], true);
        assert_eq!(body["settings"]["updatedAt"], updated.to_rfc3339());
        assert_eq!(body["status"]["workerHealthy"], true);
    }
}
