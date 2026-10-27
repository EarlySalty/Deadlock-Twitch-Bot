use crate::auth::level::DashboardAuthLevel;
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use tb_http_core::ApiError;
use uuid::Uuid;

pub const MAX_ASSET_BYTES: usize = 2 * 1024 * 1024;
pub const UPLOAD_BODY_LIMIT: usize = MAX_ASSET_BYTES + 64 * 1024;
const MAX_ASSET_NAME: usize = 120;
const MAX_LISTED_ASSETS: i64 = 300;
const ASSET_CACHE: &str = "public, max-age=31536000, immutable";
const BUILTIN_CACHE: &str = "public, max-age=86400";
const SVG_CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; sandbox";
const SVG_FORBIDDEN: [&str; 9] = [
    "<script",
    "javascript:",
    "<foreignobject",
    "<iframe",
    "<embed",
    "<object",
    "<!entity",
    "<!doctype",
    "@import",
];

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    TeamLogo,
    EventLogo,
    Brand,
}

impl AssetKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::TeamLogo => "team_logo",
            Self::EventLogo => "event_logo",
            Self::Brand => "brand",
        }
    }
}

#[derive(Deserialize)]
pub struct UploadQuery {
    pub kind: AssetKind,
    pub name: String,
}

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub kind: Option<AssetKind>,
}

#[derive(sqlx::FromRow)]
struct AssetRow {
    id: String,
    kind: String,
    name: String,
    mime: String,
    size: i32,
    created_at: DateTime<Utc>,
}

pub fn asset_url(id: &str) -> String {
    format!("/twitch/caster-overlay/assets/{id}")
}

pub fn is_asset_id(value: &str) -> bool {
    Uuid::parse_str(value).is_ok()
}

fn asset_json(row: &AssetRow) -> serde_json::Value {
    json!({
        "id": row.id,
        "kind": row.kind,
        "name": row.name,
        "mime": row.mime,
        "size": row.size,
        "createdAt": row.created_at,
        "url": asset_url(&row.id),
    })
}

fn invalid(message: &str) -> ApiError {
    ApiError::bad_request_with_body(json!({ "message": message }))
}

fn db_error(error: sqlx::Error) -> ApiError {
    tracing::error!(%error, "Caster-Overlay-Bild konnte nicht gelesen oder gespeichert werden");
    ApiError::internal()
}

fn is_webp(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP"
}

fn looks_like_svg(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let head = text.trim_start_matches('\u{feff}').trim_start();
    (head.starts_with("<svg") || head.starts_with("<?xml")) && head.contains("<svg")
}

pub fn detect_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if is_webp(bytes) {
        Some("image/webp")
    } else if looks_like_svg(bytes) {
        Some("image/svg+xml")
    } else {
        None
    }
}

fn has_event_handler(lower: &str) -> bool {
    let bytes = lower.as_bytes();
    bytes.windows(3).enumerate().any(|(index, window)| {
        window[0].is_ascii_whitespace()
            && window[1] == b'o'
            && window[2] == b'n'
            && bytes[index + 3..]
                .iter()
                .skip_while(|b| b.is_ascii_alphabetic())
                .find(|b| !b.is_ascii_whitespace())
                == Some(&b'=')
    })
}

fn has_external_reference(lower: &str) -> bool {
    lower.match_indices("href").any(|(index, _)| {
        let rest = lower[index + 4..].trim_start();
        let Some(value) = rest.strip_prefix('=') else {
            return false;
        };
        let value = value.trim_start().trim_start_matches(['"', '\'']);
        !value.starts_with('#')
    }) || lower.contains("url(http")
        || lower.contains("url(//")
}

pub fn svg_is_safe(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let lower = text.to_ascii_lowercase();
    !SVG_FORBIDDEN.iter().any(|needle| lower.contains(needle))
        && !has_event_handler(&lower)
        && !has_external_reference(&lower)
}

fn validated_name(name: &str) -> Result<String, ApiError> {
    let name = name.trim();
    if name.is_empty()
        || name.chars().count() > MAX_ASSET_NAME
        || name.chars().any(char::is_control)
    {
        return Err(invalid(
            "Bitte gib dem Bild einen Namen mit höchstens 120 Zeichen.",
        ));
    }
    Ok(name.to_owned())
}

fn validated_mime(bytes: &[u8]) -> Result<&'static str, ApiError> {
    if bytes.is_empty() || bytes.len() > MAX_ASSET_BYTES {
        return Err(invalid("Das Bild darf höchstens 2 MB groß sein."));
    }
    let mime =
        detect_mime(bytes).ok_or_else(|| invalid("Erlaubt sind nur PNG, WebP, JPEG oder SVG."))?;
    if mime == "image/svg+xml" && !svg_is_safe(bytes) {
        return Err(invalid(
            "Diese SVG enthält Skripte oder externe Verweise und wird deshalb nicht angenommen.",
        ));
    }
    Ok(mime)
}

pub async fn upload_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<UploadQuery>,
    body: Bytes,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    let name = validated_name(&query.name)?;
    let mime = validated_mime(&body)?;
    let row: AssetRow = sqlx::query_as(
        "INSERT INTO twitch_caster_overlay_assets (id, kind, name, mime, bytes) VALUES ($1::uuid, $2, $3, $4, $5) \
         RETURNING id::text AS id, kind, name, mime, octet_length(bytes) AS size, created_at",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(query.kind.as_str())
    .bind(name)
    .bind(mime)
    .bind(body.as_ref())
    .fetch_one(&pool)
    .await
    .map_err(db_error)?;
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(asset_json(&row)),
    ))
}

pub async fn list_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<ListQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    let rows: Vec<AssetRow> = sqlx::query_as(
        "SELECT id::text AS id, kind, name, mime, octet_length(bytes) AS size, created_at \
         FROM twitch_caster_overlay_assets WHERE ($1::text IS NULL OR kind = $1) \
         ORDER BY created_at DESC LIMIT $2",
    )
    .bind(query.kind.map(AssetKind::as_str))
    .bind(MAX_LISTED_ASSETS)
    .fetch_all(&pool)
    .await
    .map_err(db_error)?;
    let assets: Vec<_> = rows.iter().map(asset_json).collect();
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({ "assets": assets })),
    ))
}

pub async fn delete_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    if !is_asset_id(&id) {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    let deleted = sqlx::query("DELETE FROM twitch_caster_overlay_assets WHERE id = $1::uuid")
        .bind(&id)
        .execute(&pool)
        .await
        .map_err(db_error)?
        .rows_affected();
    if deleted == 0 {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({ "deleted": id })),
    )
        .into_response())
}

fn image_response(mime: &'static str, bytes: Vec<u8>, cache: &'static str) -> Response {
    let mut response = (
        [
            (header::CONTENT_TYPE, mime),
            (header::CACHE_CONTROL, cache),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        bytes,
    )
        .into_response();
    if mime == "image/svg+xml" {
        response.headers_mut().insert(
            header::CONTENT_SECURITY_POLICY,
            header::HeaderValue::from_static(SVG_CSP),
        );
    }
    response
}

fn static_mime(mime: &str) -> Option<&'static str> {
    ["image/png", "image/webp", "image/jpeg", "image/svg+xml"]
        .into_iter()
        .find(|known| *known == mime)
}

pub async fn public_handler(
    State(pool): State<PgPool>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    if !is_asset_id(&id) {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    let row: Option<(String, Vec<u8>)> =
        sqlx::query_as("SELECT mime, bytes FROM twitch_caster_overlay_assets WHERE id = $1::uuid")
            .bind(&id)
            .fetch_optional(&pool)
            .await
            .map_err(db_error)?;
    let Some((mime, bytes)) = row else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let Some(mime) = static_mime(&mime) else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    Ok(image_response(mime, bytes, ASSET_CACHE))
}

fn builtin_asset(name: &str) -> Option<(&'static str, &'static [u8])> {
    match name {
        "dachlock.png" => Some((
            "image/png",
            include_bytes!("caster_assets/dachlock.png").as_slice(),
        )),
        "wochenende.webp" => Some((
            "image/webp",
            include_bytes!("caster_assets/wochenende.webp").as_slice(),
        )),
        "community.webp" => Some((
            "image/webp",
            include_bytes!("caster_assets/community.webp").as_slice(),
        )),
        "fun-turnier.webp" => Some((
            "image/webp",
            include_bytes!("caster_assets/fun_turnier.webp").as_slice(),
        )),
        _ => None,
    }
}

pub async fn builtin_handler(Path(name): Path<String>) -> Response {
    match builtin_asset(&name) {
        Some((mime, bytes)) => image_response(mime, bytes.to_vec(), BUILTIN_CACHE),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erkennt_bildformate_an_magic_bytes() {
        assert_eq!(detect_mime(b"\x89PNG\r\n\x1a\nrest"), Some("image/png"));
        assert_eq!(detect_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(detect_mime(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(
            detect_mime(b"<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"/>"),
            Some("image/svg+xml")
        );
        assert_eq!(detect_mime(b"GIF89a"), None);
        assert_eq!(detect_mime(b"<html><svg/></html>"), None);
    }

    #[test]
    fn lehnt_unsichere_svg_ab() {
        let safe =
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"><use href=\"#a\"/><circle r=\"4\"/></svg>";
        assert!(svg_is_safe(safe));
        for unsafe_svg in [
            "<svg><script>alert(1)</script></svg>",
            "<svg onload=\"alert(1)\"></svg>",
            "<svg><a href=\"javascript:alert(1)\"/></svg>",
            "<svg><image href=\"https://evil.example/x.png\"/></svg>",
            "<svg><foreignObject></foreignObject></svg>",
            "<svg><style>@import url(https://x)</style></svg>",
        ] {
            assert!(!svg_is_safe(unsafe_svg.as_bytes()), "{unsafe_svg}");
        }
    }

    #[test]
    fn eingebaute_bilder_sind_vorhanden() {
        for name in [
            "dachlock.png",
            "wochenende.webp",
            "community.webp",
            "fun-turnier.webp",
        ] {
            let (mime, bytes) = builtin_asset(name).expect("eingebautes Bild fehlt");
            assert_eq!(detect_mime(bytes), Some(mime));
        }
        assert!(builtin_asset("../caster_overlay.rs").is_none());
    }
}
