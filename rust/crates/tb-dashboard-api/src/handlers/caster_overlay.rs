//! Persistente Caster-Szene. Admin-Schreibroute hängt hinter Auth und CSRF.
use crate::auth::level::DashboardAuthLevel;
use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{Html, IntoResponse},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use tb_http_core::ApiError;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Caster {
    pub id: String,
    pub name: String,
    pub handle: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Scene {
    pub roster: Vec<Caster>,
    pub slots: [Option<String>; 2],
}
#[derive(Deserialize)]
pub struct SaveRequest {
    pub revision: i64,
    pub scene: Scene,
}

fn validate(scene: &mut Scene) -> Result<(), ApiError> {
    let invalid = || {
        ApiError::bad_request_with_body(
            json!({"message":"Bitte höchstens 100 Personen mit eindeutiger ID, Name (1–60 Zeichen) und Handle (höchstens 60 Zeichen) verwenden."}),
        )
    };
    if scene.roster.len() > 100 {
        return Err(invalid());
    }
    let mut ids = std::collections::HashSet::new();
    for caster in &mut scene.roster {
        caster.name = caster.name.trim().to_owned();
        caster.handle = caster.handle.trim().trim_start_matches('@').to_owned();
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
            || caster.name.chars().any(char::is_control)
            || caster.handle.chars().any(char::is_control)
        {
            return Err(invalid());
        }
    }
    if scene.slots.iter().flatten().any(|id| !ids.contains(id)) {
        return Err(invalid());
    }
    Ok(())
}
fn db_error(error: sqlx::Error) -> ApiError {
    tracing::error!(%error, "Caster-Szene konnte nicht gespeichert oder geladen werden");
    ApiError::internal()
}
async fn load(pool: &PgPool) -> Result<(i64, Scene), ApiError> {
    let (revision, value): (i64, serde_json::Value) =
        sqlx::query_as("SELECT revision, scene FROM twitch_caster_overlay WHERE id = true")
            .fetch_one(pool)
            .await
            .map_err(db_error)?;
    let scene = serde_json::from_value(value).map_err(|error| {
        tracing::error!(%error, "Ungültige gespeicherte Caster-Szene");
        ApiError::internal()
    })?;
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
) -> Result<axum::response::Response, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    validate(&mut request.scene)?;
    let scene = serde_json::to_value(&request.scene).map_err(|_| ApiError::internal())?;
    let revision: Option<i64> = sqlx::query_scalar("UPDATE twitch_caster_overlay SET scene = $1, revision = revision + 1 WHERE id = true AND revision = $2 RETURNING revision").bind(scene).bind(request.revision).fetch_optional(&pool).await.map_err(db_error)?;
    let Some(revision) = revision else {
        return Ok((StatusCode::CONFLICT, Json(json!({"message":"Die Szene wurde inzwischen in einem anderen Fenster geändert. Bitte neu laden und deine Änderung erneut vornehmen."}))).into_response());
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
                .map(|caster| json!({"name":caster.name,"handle":caster.handle}))
        })
        .collect();
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({"revision":revision,"slots":slots})),
    ))
}
pub async fn html_handler() -> impl IntoResponse {
    ([(header::CACHE_CONTROL, "no-cache"), (header::X_FRAME_OPTIONS, "SAMEORIGIN"), (header::CONTENT_SECURITY_POLICY, "default-src 'none'; img-src 'self'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self'; base-uri 'none'; frame-ancestors 'self' https://admin.deutsche-deadlock-community.de")], Html(include_str!("caster_overlay.html")))
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

#[cfg(test)]
mod tests {
    use super::*;
    fn scene() -> Scene {
        Scene {
            roster: vec![Caster {
                id: "caster-1".into(),
                name: " ZeRo ".into(),
                handle: "@zro_dl".into(),
            }],
            slots: [Some("caster-1".into()), None],
        }
    }
    #[test]
    fn validates_and_normalizes_names() {
        let mut value = scene();
        validate(&mut value).unwrap();
        assert_eq!(value.roster[0].name, "ZeRo");
        assert_eq!(value.roster[0].handle, "zro_dl");
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
}
