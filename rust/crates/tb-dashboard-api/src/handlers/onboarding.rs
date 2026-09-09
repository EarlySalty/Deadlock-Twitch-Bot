//! Gemeinsamer Rundgang. Fortschritt und echte Kontoverbindungen bleiben getrennt.
use crate::auth::level::DashboardAuthLevel;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;

const STEPS: &[&str] = &[
    "bookmark",
    "discord",
    "steam",
    "chat",
    "bot",
    "overlay",
    "advertising",
    "feedback",
];
const CONFIRMABLE: &[&str] = &[
    "bookmark",
    "chat",
    "bot",
    "overlay",
    "advertising",
    "feedback",
];

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OnboardingUpdate {
    // Kompatibilität für bereits ausgelieferte Clients. Alte Schritte nie zu neuen Häkchen machen.
    pub current_step: Option<i32>,
    pub completed: Option<bool>,
    pub active_step: Option<String>,
    pub complete_step: Option<String>,
    pub paused: Option<bool>,
}

impl OnboardingUpdate {
    fn validate(&self) -> Result<(), &'static str> {
        if self
            .current_step
            .is_some_and(|step| !(0..=4).contains(&step))
            || self
                .active_step
                .as_deref()
                .is_some_and(|step| !STEPS.contains(&step))
            || self
                .complete_step
                .as_deref()
                .is_some_and(|step| !CONFIRMABLE.contains(&step))
        {
            return Err("Dieser Einrichtungsschritt ist nicht bekannt.");
        }
        if self.completed == Some(false) {
            return Err("Wiederholen setzt abgeschlossene Schritte nicht zurück.");
        }
        Ok(())
    }
}

#[derive(Serialize, sqlx::FromRow)]
struct Progress {
    current_step: i32,
    completed: bool,
    active_step: String,
    completed_step_ids: Vec<String>,
    paused: bool,
}
impl Default for Progress {
    fn default() -> Self {
        Self {
            current_step: 0,
            completed: false,
            active_step: "bookmark".into(),
            completed_step_ids: vec![],
            paused: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LinkStatus {
    Connected,
    Missing,
    Error,
}

#[derive(Serialize)]
pub(crate) struct AccountLinks {
    pub discord_status: LinkStatus,
    pub steam_status: LinkStatus,
    #[serde(skip)]
    pub discord_id: Option<String>,
}

/// ID-basierter gemeinsamer Lesepfad. Mitgliedschaft ist keine Kontoverknüpfung;
/// ein fehlgeschlagener Steam-Aufruf ist kein Nachweis einer fehlenden Verbindung.
pub(crate) async fn account_links(pool: &PgPool, twitch_user_id: &str) -> AccountLinks {
    let discord = sqlx::query_scalar::<_, Option<String>>(
        "SELECT NULLIF(TRIM(discord_user_id), '') FROM twitch_streamer_identities WHERE twitch_user_id = $1"
    ).bind(twitch_user_id).fetch_optional(pool).await;
    match discord {
        Err(error) => {
            tracing::warn!(%error, "Kontoverknüpfungen konnten nicht geprüft werden");
            AccountLinks {
                discord_status: LinkStatus::Error,
                steam_status: LinkStatus::Error,
                discord_id: None,
            }
        }
        Ok(id) => match id.flatten() {
            None => AccountLinks {
                discord_status: LinkStatus::Missing,
                steam_status: LinkStatus::Missing,
                discord_id: None,
            },
            Some(id) => {
                let steam_status = steam_status(
                    tb_chat::stats::fetch_rank(&id, false)
                        .await
                        .map(|rank| rank.linked),
                );
                AccountLinks {
                    discord_status: LinkStatus::Connected,
                    steam_status,
                    discord_id: Some(id),
                }
            }
        },
    }
}
fn steam_status(linked: Option<bool>) -> LinkStatus {
    match linked {
        Some(true) => LinkStatus::Connected,
        Some(false) => LinkStatus::Missing,
        None => LinkStatus::Error,
    }
}

#[allow(clippy::result_large_err)]
fn resolve_partner(auth: &DashboardAuthLevel) -> Result<(&str, &str), Response> {
    match auth {
        DashboardAuthLevel::Partner {
            twitch_user_id,
            twitch_login,
            ..
        } if !twitch_user_id.trim().is_empty()
            && twitch_user_id.bytes().all(|c| c.is_ascii_digit())
            && !twitch_login.trim().is_empty() =>
        {
            Ok((twitch_user_id, twitch_login))
        }
        DashboardAuthLevel::Admin { .. } => Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error":"Bitte nutze deine persönliche Partner-Ansicht."})),
        )
            .into_response()),
        _ => Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"Bitte melde dich erneut mit Twitch an."})),
        )
            .into_response()),
    }
}

pub async fn get_status(auth: DashboardAuthLevel, State(pool): State<PgPool>) -> Response {
    let (id, _) = match resolve_partner(&auth) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let progress = sqlx::query_as::<_, Progress>(
        "SELECT current_step, completed, active_step, completed_step_ids, paused FROM streamer_onboarding WHERE twitch_user_id = $1"
    ).bind(id).fetch_optional(&pool).await;
    match progress {
        Ok(progress) => {
            let links = account_links(&pool, id).await;
            let mut value =
                serde_json::to_value(progress.unwrap_or_default()).expect("progress serialization");
            value["discord_linked"] = json!(links.discord_status == LinkStatus::Connected);
            value["steam_linked"] = json!(links.steam_status == LinkStatus::Connected);
            value["discord_status"] = json!(links.discord_status);
            value["steam_status"] = json!(links.steam_status);
            Json(value).into_response()
        }
        Err(error) => db_error(error),
    }
}

pub async fn post_status(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<OnboardingUpdate>,
) -> Response {
    let (id, login) = match resolve_partner(&auth) {
        Ok(v) => v,
        Err(r) => return r,
    };
    if let Err(error) = body.validate() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": error}))).into_response();
    }
    // Ein atomarer Upsert vereinigt Bestätigungen auch bei zwei Tabs/Geräten.
    // Pause/Navigation verändert weder Bestätigungen noch Verbindungen.
    let row = sqlx::query_as::<_, Progress>(r#"
        INSERT INTO streamer_onboarding (twitch_user_id, twitch_login, current_step, completed,
            completed_at, active_step, completed_step_ids, paused, updated_at)
        VALUES ($1, $2, COALESCE($3, 0), COALESCE($4, FALSE), CASE WHEN $4 THEN NOW() END,
            COALESCE($5, 'bookmark'), CASE WHEN $6::TEXT IS NULL THEN '{}'::TEXT[] ELSE ARRAY[$6] END,
            COALESCE($7, TRUE), NOW())
        ON CONFLICT (twitch_user_id) DO UPDATE SET
            twitch_login = EXCLUDED.twitch_login,
            current_step = COALESCE($3, streamer_onboarding.current_step),
            completed = streamer_onboarding.completed OR COALESCE($4, FALSE),
            completed_at = CASE WHEN $4 THEN COALESCE(streamer_onboarding.completed_at, NOW()) ELSE streamer_onboarding.completed_at END,
            active_step = COALESCE($5, streamer_onboarding.active_step),
            completed_step_ids = CASE WHEN $6::TEXT IS NULL OR $6 = ANY(streamer_onboarding.completed_step_ids)
                THEN streamer_onboarding.completed_step_ids ELSE array_append(streamer_onboarding.completed_step_ids, $6) END,
            paused = COALESCE($7, streamer_onboarding.paused), updated_at = NOW()
        RETURNING current_step, completed, active_step, completed_step_ids, paused
    "#).bind(id).bind(login).bind(body.current_step).bind(body.completed).bind(body.active_step)
        .bind(body.complete_step).bind(body.paused).fetch_one(&pool).await;
    match row {
        Ok(row) => Json(json!({"ok": true, "progress": row})).into_response(),
        Err(error) => db_error(error),
    }
}
fn db_error(error: sqlx::Error) -> Response {
    tracing::error!(%error, "Einrichtungsfortschritt: Datenbankfehler");
    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"Dein Fortschritt konnte gerade nicht gespeichert oder geladen werden. Bitte versuche es erneut."}))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn json_body(response: Response) -> serde_json::Value {
        assert!(response.status().is_success(), "{}", response.status());
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }
    fn partner(id: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_user_id: id.into(),
            twitch_login: format!("partner{id}"),
            display_name: String::new(),
        }
    }
    #[tokio::test]
    async fn fortschritt_persistiert_getrennt_vereint_parallele_tabs_und_ueberlebt_pause() {
        let db = crate::test_database::Database::new().await;
        sqlx::raw_sql(include_str!(
            "../../../../migrations/20260621080000_streamer_onboarding.sql"
        ))
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO streamer_onboarding (twitch_user_id,twitch_login,current_step,completed) VALUES ('90','alt',2,TRUE)").execute(&db.pool).await.unwrap();
        sqlx::raw_sql(include_str!(
            "../../../../migrations/20260909210000_dashboard_onboarding_steps.sql"
        ))
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query("CREATE TABLE twitch_streamer_identities (twitch_user_id TEXT PRIMARY KEY, discord_user_id TEXT)").execute(&db.pool).await.unwrap();
        let old = json_body(get_status(partner("90"), State(db.pool.clone())).await).await;
        assert_eq!(old["active_step"], "steam");
        assert_eq!(old["completed"], true);
        assert_eq!(old["paused"], true);
        assert_eq!(old["completed_step_ids"], json!([]));
        let (a, b) = tokio::join!(
            post_status(
                partner("42"),
                State(db.pool.clone()),
                Json(OnboardingUpdate {
                    complete_step: Some("bookmark".into()),
                    ..Default::default()
                })
            ),
            post_status(
                partner("42"),
                State(db.pool.clone()),
                Json(OnboardingUpdate {
                    complete_step: Some("chat".into()),
                    ..Default::default()
                })
            )
        );
        assert!(a.status().is_success());
        assert!(b.status().is_success());
        for paused in [true, false] {
            let response = post_status(
                partner("42"),
                State(db.pool.clone()),
                Json(OnboardingUpdate {
                    active_step: Some("bot".into()),
                    paused: Some(paused),
                    ..Default::default()
                }),
            )
            .await;
            assert!(response.status().is_success());
            let own = json_body(get_status(partner("42"), State(db.pool.clone())).await).await;
            assert_eq!(own["completed_step_ids"].as_array().unwrap().len(), 2);
            assert_eq!(own["paused"], paused);
            assert_eq!(own["active_step"], "bot");
            assert_eq!(own["discord_linked"], false);
        }
        let other = json_body(get_status(partner("43"), State(db.pool.clone())).await).await;
        assert_eq!(other["completed_step_ids"], json!([]));
        assert_eq!(other["paused"], true);
        assert_eq!(other["active_step"], "bookmark");
        db.close().await;
    }
    #[tokio::test]
    async fn konto_db_fehler_wird_als_fehler_gemeldet() {
        let db = crate::test_database::Database::new().await;
        let status = account_links(&db.pool, "42").await;
        assert_eq!(status.discord_status, LinkStatus::Error);
        assert_eq!(status.steam_status, LinkStatus::Error);
        db.close().await;
    }
    #[test]
    fn kontofehler_ist_keine_fehlende_verbindung() {
        assert_eq!(steam_status(None), LinkStatus::Error);
        assert_eq!(steam_status(Some(false)), LinkStatus::Missing);
        assert_eq!(steam_status(Some(true)), LinkStatus::Connected);
    }
    #[test]
    fn client_darf_verbindungen_nicht_bestaetigen_oder_fortschritt_loeschen() {
        for step in ["discord", "steam", "unknown"] {
            assert!(OnboardingUpdate {
                complete_step: Some(step.into()),
                ..Default::default()
            }
            .validate()
            .is_err());
        }
        assert!(OnboardingUpdate {
            completed: Some(false),
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(OnboardingUpdate {
            paused: Some(true),
            ..Default::default()
        }
        .validate()
        .is_ok());
        assert!(OnboardingUpdate {
            active_step: Some("bookmark".into()),
            paused: Some(false),
            ..Default::default()
        }
        .validate()
        .is_ok());
    }
    #[test]
    fn fortschritt_verwendet_ausschliesslich_session_identitaet() {
        let partner = |id: &str| DashboardAuthLevel::Partner {
            twitch_login: "name".into(),
            twitch_user_id: id.into(),
            display_name: String::new(),
        };
        assert!(resolve_partner(&partner("")).is_err());
        assert!(resolve_partner(&partner("name")).is_err());
        assert_eq!(resolve_partner(&partner("42")).unwrap(), ("42", "name"));
        assert!(resolve_partner(&DashboardAuthLevel::None).is_err());
        assert!(
            serde_json::from_value::<OnboardingUpdate>(json!({"twitch_user_id":"43"})).is_err()
        );
    }
}
