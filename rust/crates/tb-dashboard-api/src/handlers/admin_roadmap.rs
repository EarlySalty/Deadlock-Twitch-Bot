//! Admin-Roadmap-Editor (dateibasiertes Text-Dokument).
//!
//! Port von `bot/dashboard/pages.py:load_roadmap_document` /
//! `save_roadmap_document` + `bot/analytics/api_admin.py:_api_admin_roadmap(_save)`.
//! Die Roadmap ist KEIN DB-Eintrag, sondern ein JSON-File unter
//! `<repo>/data/admin_dashboard/roadmap_body.json` (relativ zum
//! Service-WorkingDirectory = Repo-Root). Fehlt die Datei, liefert GET den
//! eingebetteten Default-Body.
//!
//! CSRF wird — wie im übrigen Rust-Dashboard etabliert — nicht geprüft; Admin
//! über `DashboardAuthLevel`. updated_by = "admin" (Pythons Fallback).

use std::path::{Path, PathBuf};

use crate::auth::level::DashboardAuthLevel;
use axum::{response::IntoResponse, Json};
use chrono::SecondsFormat;
use serde_json::{json, Value};
use tb_http_core::ApiError;

/// Eingebetteter Default-Body (Python `_default_roadmap_body`, `.strip()`).
const DEFAULT_BODY: &str = include_str!("../../templates/roadmap_default.html");
const ROADMAP_REL_PATH: &str = "data/admin_dashboard/roadmap_body.json";

fn default_roadmap_body() -> String {
    DEFAULT_BODY.trim().to_string()
}

/// Pfad zur Roadmap-Datei (relativ zum Prozess-cwd = Repo-Root, wie Python).
pub(crate) fn roadmap_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(ROADMAP_REL_PATH)
}

/// Liefert den aktuell gespeicherten Roadmap-Body (oder den Default), für die
/// öffentliche Anzeige-Seite (B1-ROADMAP-PAGE). Port von Pythons
/// `build_roadmap_body` (`pages.py`: `load_roadmap_document().get("body")`).
pub(crate) async fn load_roadmap_body() -> String {
    load_roadmap_document_at(&roadmap_path()).await.body
}

/// Geladenes/gespeichertes Roadmap-Dokument.
#[derive(Debug, Clone)]
struct RoadmapDoc {
    body: String,
    last_updated_at: Option<String>,
    last_updated_by: Option<String>,
}

fn text_document_payload(doc: &RoadmapDoc) -> Value {
    json!({
        "body": doc.body,
        "lastUpdatedAt": doc.last_updated_at,
        "lastUpdatedBy": doc.last_updated_by,
    })
}

fn nonempty_trim(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// Lädt das Dokument aus `path` (Python `load_roadmap_document`).
/// Fehlende/ungültige Datei → Default-Body.
async fn load_roadmap_document_at(path: &Path) -> RoadmapDoc {
    let mut doc = RoadmapDoc {
        body: default_roadmap_body(),
        last_updated_at: None,
        last_updated_by: None,
    };
    let Ok(raw) = tokio::fs::read_to_string(path).await else {
        return doc;
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&raw) else {
        return doc;
    };
    if let Some(obj) = parsed.as_object() {
        // body nur übernehmen, wenn nicht-leerer String (Python-Logik).
        if let Some(b) = obj.get("body").and_then(Value::as_str) {
            if !b.trim().is_empty() {
                doc.body = b.to_string();
            }
        }
        doc.last_updated_at = nonempty_trim(obj.get("lastUpdatedAt").and_then(Value::as_str));
        doc.last_updated_by = nonempty_trim(obj.get("lastUpdatedBy").and_then(Value::as_str));
    }
    doc
}

/// Speichert `body` als Dokument in `path` (Python `save_roadmap_document`).
async fn save_roadmap_document_at(
    path: &Path,
    body: &str,
    updated_by: &str,
) -> std::io::Result<RoadmapDoc> {
    let now = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Micros, false);
    let last_updated_by = {
        let t = updated_by.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    };
    let serialized = serde_json::to_string_pretty(&json!({
        "body": body,
        "lastUpdatedAt": now,
        "lastUpdatedBy": last_updated_by,
    }))
    .unwrap_or_default();

    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, serialized).await?;

    Ok(RoadmapDoc {
        body: body.to_string(),
        last_updated_at: Some(now),
        last_updated_by,
    })
}

/// `GET /twitch/api/admin/roadmap` — Roadmap-Dokument lesen (Admin).
pub async fn get_handler(auth: DashboardAuthLevel) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return Err(err);
    }
    let doc = load_roadmap_document_at(&roadmap_path()).await;
    Ok(Json(text_document_payload(&doc)))
}

/// `POST /twitch/api/admin/roadmap` — Roadmap-Dokument speichern (Admin).
/// Fehlt der `body`-Schlüssel → 400 `validation_failed`.
pub async fn save_handler(
    auth: DashboardAuthLevel,
    body: axum::body::Bytes,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return Err(err);
    }
    let payload: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let Some(body_val) = payload.as_object().and_then(|o| o.get("body")) else {
        return Err(ApiError::bad_request_with_body(json!({
            "error": "validation_failed",
            "message": "body ist erforderlich.",
        })));
    };
    let body_str = body_val.as_str().unwrap_or("").to_string();

    let doc = save_roadmap_document_at(&roadmap_path(), &body_str, "admin")
        .await
        .map_err(|e| {
            tracing::error!("admin_roadmap Schreibfehler: {e}");
            ApiError::internal()
        })?;
    Ok(Json(text_document_payload(&doc)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Bytes;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    fn temp_path(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "tb-roadmap-{}-{}-{}/roadmap.json",
            tag,
            std::process::id(),
            nanos
        ))
    }

    #[test]
    fn default_body_nicht_leer() {
        let b = default_roadmap_body();
        assert!(b.contains("Roadmap"));
        assert!(b.starts_with("<div class=\"hero\">"));
        assert!(b.ends_with("</script>"));
    }

    #[tokio::test]
    async fn load_fehlende_datei_gibt_default() {
        let doc = load_roadmap_document_at(&temp_path("missing")).await;
        assert_eq!(doc.body, default_roadmap_body());
        assert!(doc.last_updated_at.is_none());
        assert!(doc.last_updated_by.is_none());
    }

    #[tokio::test]
    async fn save_dann_load_roundtrip() {
        let path = temp_path("rt");
        let saved = save_roadmap_document_at(&path, "Meine Roadmap ä", "admin")
            .await
            .unwrap();
        assert_eq!(saved.body, "Meine Roadmap ä");
        assert_eq!(saved.last_updated_by.as_deref(), Some("admin"));
        assert!(saved.last_updated_at.is_some());

        let loaded = load_roadmap_document_at(&path).await;
        assert_eq!(loaded.body, "Meine Roadmap ä");
        assert_eq!(loaded.last_updated_by.as_deref(), Some("admin"));
        assert_eq!(loaded.last_updated_at, saved.last_updated_at);

        // leerer body beim Speichern → load fällt auf Default zurück (Python:
        // body nur uebernommen wenn nicht-leer).
        let _ = save_roadmap_document_at(&path, "   ", "admin")
            .await
            .unwrap();
        let loaded2 = load_roadmap_document_at(&path).await;
        assert_eq!(loaded2.body, default_roadmap_body());

        let _ = tokio::fs::remove_file(&path).await;
    }

    async fn status_of(r: Result<impl IntoResponse, ApiError>) -> StatusCode {
        r.into_response().status()
    }

    #[tokio::test]
    async fn unauth_auth_required_401() {
        assert_eq!(
            status_of(get_handler(DashboardAuthLevel::None).await).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status_of(save_handler(DashboardAuthLevel::None, Bytes::from(r#"{"body":"x"}"#)).await)
                .await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn save_ohne_body_key_400() {
        // Kein fs-Zugriff (Validierung vor dem Schreiben).
        let resp = save_handler(DashboardAuthLevel::admin(), Bytes::from(r#"{"foo":1}"#)).await;
        assert_eq!(status_of(resp).await, StatusCode::BAD_REQUEST);
    }
}
