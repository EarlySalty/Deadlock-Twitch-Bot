//! Admin-Editor für die Rechtsseiten (Impressum/Datenschutz/AGB/Sicherheit).
//!
//! Port von `bot/dashboard/admin/legal_mixin.py:load/save_legal_page_document`
//! (+ `_default_legal_page_document`, `normalize_legal_page_slug`) +
//! `bot/analytics/api_admin.py:_api_admin_legal_page(_save)`. Wie die Roadmap ist
//! das ein **dateibasiertes** Dokument — eine JSON-Datei
//! `<repo>/data/admin_dashboard/legal_pages.json`, als Dict pro Slug. Fehlt ein
//! Eintrag, liefert GET den eingebetteten Default-Titel + -Body.
//!
//! CSRF wird — wie im übrigen Rust-Dashboard etabliert — nicht geprüft; Admin
//! über `AuthLevel::is_privileged`. updated_by = "admin" (Pythons Fallback).

use std::path::{Path, PathBuf};

use axum::{extract::Path as AxumPath, response::IntoResponse, Json};
use chrono::SecondsFormat;
use serde_json::{json, Value};
use tb_http_core::{ApiError, AuthLevel};

const SLUGS: [&str; 4] = ["impressum", "datenschutz", "agb", "sicherheit"];
const LEGAL_REL_PATH: &str = "data/admin_dashboard/legal_pages.json";

// Eingebettete Default-Bodies (1:1 aus _DEFAULT_LEGAL_PAGE_BODIES extrahiert).
const DEFAULT_IMPRESSUM: &str = include_str!("../../templates/legal_default_impressum.html");
const DEFAULT_DATENSCHUTZ: &str = include_str!("../../templates/legal_default_datenschutz.html");
const DEFAULT_AGB: &str = include_str!("../../templates/legal_default_agb.html");
const DEFAULT_SICHERHEIT: &str = include_str!("../../templates/legal_default_sicherheit.html");

fn default_title(slug: &str) -> &'static str {
    match slug {
        "impressum" => "Impressum",
        "datenschutz" => "Datenschutzerklärung",
        "agb" => "Allgemeine Geschäftsbedingungen",
        "sicherheit" => "Sicherheitskonzept",
        _ => "",
    }
}

fn default_body(slug: &str) -> &'static str {
    match slug {
        "impressum" => DEFAULT_IMPRESSUM,
        "datenschutz" => DEFAULT_DATENSCHUTZ,
        "agb" => DEFAULT_AGB,
        "sicherheit" => DEFAULT_SICHERHEIT,
        _ => "",
    }
}

/// Slug normalisieren (Python `normalize_legal_page_slug`): lowercase + muss
/// einer der vier bekannten Slugs sein, sonst `None`.
fn normalize_slug(raw: Option<&str>) -> Option<String> {
    let s = raw.unwrap_or("").trim().to_lowercase();
    if SLUGS.contains(&s.as_str()) {
        Some(s)
    } else {
        None
    }
}

fn legal_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(LEGAL_REL_PATH)
}

#[derive(Debug, Clone)]
struct LegalDoc {
    slug: String,
    title: String,
    body: String,
    last_updated_at: Option<String>,
    last_updated_by: Option<String>,
}

fn default_legal_page_document(slug: &str) -> LegalDoc {
    LegalDoc {
        slug: slug.to_string(),
        title: default_title(slug).to_string(),
        body: default_body(slug).to_string(),
        last_updated_at: None,
        last_updated_by: None,
    }
}

fn nonempty_trim(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

fn legal_payload(doc: &LegalDoc) -> Value {
    json!({
        "slug": doc.slug.trim(),
        "title": doc.title.trim(),
        "body": doc.body,
        "lastUpdatedAt": doc.last_updated_at,
        "lastUpdatedBy": doc.last_updated_by,
    })
}

/// Lädt ein Legal-Dokument aus der JSON-Datei (Python `load_legal_page_document`).
async fn load_legal_page_document_at(path: &Path, slug: &str) -> LegalDoc {
    let mut doc = default_legal_page_document(slug);
    let Ok(raw) = tokio::fs::read_to_string(path).await else {
        return doc;
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&raw) else {
        return doc;
    };
    let Some(entry) = parsed.as_object().and_then(|o| o.get(slug)).and_then(Value::as_object) else {
        return doc;
    };
    if let Some(t) = entry.get("title").and_then(Value::as_str) {
        if !t.trim().is_empty() {
            doc.title = t.trim().to_string();
        }
    }
    if let Some(b) = entry.get("body").and_then(Value::as_str) {
        if !b.trim().is_empty() {
            doc.body = b.to_string(); // roh, nicht getrimmt (Python-Logik)
        }
    }
    doc.last_updated_at = nonempty_trim(entry.get("lastUpdatedAt").and_then(Value::as_str));
    doc.last_updated_by = nonempty_trim(entry.get("lastUpdatedBy").and_then(Value::as_str));
    doc
}

/// Speichert ein Legal-Dokument (Python `save_legal_page_document`).
async fn save_legal_page_document_at(
    path: &Path,
    slug: &str,
    title: &str,
    body: &str,
    updated_by: &str,
) -> std::io::Result<LegalDoc> {
    let mut doc = default_legal_page_document(slug);
    let t = title.trim();
    if !t.is_empty() {
        doc.title = t.to_string();
    } // sonst Default-Titel
    doc.body = body.to_string();
    doc.last_updated_at = Some(chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Micros, false));
    doc.last_updated_by = nonempty_trim(Some(updated_by));

    // Bestehendes Dict lesen (Fehler → leeres Dict), Slug-Eintrag setzen, schreiben.
    let mut raw_payload = match tokio::fs::read_to_string(path).await {
        Ok(s) => serde_json::from_str::<Value>(&s).unwrap_or_else(|_| json!({})),
        Err(_) => json!({}),
    };
    if !raw_payload.is_object() {
        raw_payload = json!({});
    }
    raw_payload[slug] = json!({
        "slug": doc.slug,
        "title": doc.title,
        "body": doc.body,
        "lastUpdatedAt": doc.last_updated_at,
        "lastUpdatedBy": doc.last_updated_by,
    });

    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let serialized = serde_json::to_string_pretty(&raw_payload).unwrap_or_default();
    tokio::fs::write(path, serialized).await?;
    Ok(doc)
}

// ── Core (pfad-injiziert, testbar) ─────────────────────────────────────────────

async fn load_legal(path: &Path, slug_raw: &str) -> Result<Value, ApiError> {
    let Some(slug) = normalize_slug(Some(slug_raw)) else {
        return Err(ApiError::not_found());
    };
    let doc = load_legal_page_document_at(path, &slug).await;
    Ok(legal_payload(&doc))
}

async fn save_legal(path: &Path, slug_raw: &str, body_bytes: &[u8]) -> Result<Value, ApiError> {
    let Some(slug) = normalize_slug(Some(slug_raw)) else {
        return Err(ApiError::not_found());
    };
    let payload: Value = serde_json::from_slice(body_bytes).unwrap_or(Value::Null);
    let Some(obj) = payload.as_object() else {
        return Err(ApiError::bad_request_with_body(json!({
            "error": "validation_failed",
            "message": "body ist erforderlich.",
        })));
    };
    let Some(body_val) = obj.get("body") else {
        return Err(ApiError::bad_request_with_body(json!({
            "error": "validation_failed",
            "message": "body ist erforderlich.",
        })));
    };
    let body_str = body_val.as_str().unwrap_or("").to_string();

    // Titel: Default = aktueller Titel; wenn "title" im Payload → getrimmt, leer → 400.
    let current = load_legal_page_document_at(path, &slug).await;
    let next_title = match obj.get("title") {
        Some(tv) => {
            let t = tv.as_str().unwrap_or("").trim().to_string();
            if t.is_empty() {
                return Err(ApiError::bad_request_with_body(json!({
                    "error": "validation_failed",
                    "message": "title darf nicht leer sein.",
                })));
            }
            t
        }
        None => current.title.clone(),
    };

    let doc = save_legal_page_document_at(path, &slug, &next_title, &body_str, "admin")
        .await
        .map_err(|e| {
            tracing::error!("admin_legal Schreibfehler: {e}");
            ApiError::internal()
        })?;
    Ok(legal_payload(&doc))
}

// ── Handler ────────────────────────────────────────────────────────────────────

/// `GET /twitch/api/admin/legal/:slug` — Rechtsseite lesen (Admin).
pub async fn get_handler(
    auth: AuthLevel,
    AxumPath(slug): AxumPath<String>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    Ok(Json(load_legal(&legal_path(), &slug).await?))
}

/// `POST /twitch/api/admin/legal/:slug` — Rechtsseite speichern (Admin).
pub async fn save_handler(
    auth: AuthLevel,
    AxumPath(slug): AxumPath<String>,
    body: axum::body::Bytes,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    Ok(Json(save_legal(&legal_path(), &slug, &body).await?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    fn temp_path(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("tb-legal-{}-{}-{}/legal.json", tag, std::process::id(), nanos))
    }

    #[test]
    fn normalize_slug_varianten() {
        assert_eq!(normalize_slug(Some("AGB")).as_deref(), Some("agb"));
        assert_eq!(normalize_slug(Some("  impressum ")).as_deref(), Some("impressum"));
        assert_eq!(normalize_slug(Some("bogus")), None);
        assert_eq!(normalize_slug(None), None);
    }

    #[test]
    fn default_bodies_nicht_leer() {
        for slug in SLUGS {
            assert!(!default_body(slug).is_empty(), "Default-Body {slug} leer");
            assert!(!default_title(slug).is_empty(), "Default-Titel {slug} leer");
        }
    }

    #[tokio::test]
    async fn load_invalider_slug_404() {
        let err = load_legal(&temp_path("badslug"), "quatsch").await.unwrap_err();
        assert_eq!(err.into_response().status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn load_fehlende_datei_gibt_default() {
        let v = load_legal(&temp_path("missing"), "impressum").await.unwrap();
        assert_eq!(v["slug"], "impressum");
        assert_eq!(v["title"], "Impressum");
        assert!(v["body"].as_str().unwrap().len() > 10);
        assert!(v["lastUpdatedAt"].is_null());
    }

    #[tokio::test]
    async fn save_dann_load_roundtrip_und_default_titel() {
        let path = temp_path("rt");
        // ohne title → Default-Titel bleibt; body gesetzt.
        let saved = save_legal(&path, "agb", r#"{"body":"<p>Neue AGB ä</p>"}"#.as_bytes()).await.unwrap();
        assert_eq!(saved["slug"], "agb");
        assert_eq!(saved["title"], "Allgemeine Geschäftsbedingungen"); // Default
        assert_eq!(saved["body"], "<p>Neue AGB ä</p>");
        assert_eq!(saved["lastUpdatedBy"], "admin");
        assert!(saved["lastUpdatedAt"].is_string());

        let loaded = load_legal(&path, "agb").await.unwrap();
        assert_eq!(loaded["body"], "<p>Neue AGB ä</p>");
        assert_eq!(loaded["title"], "Allgemeine Geschäftsbedingungen");

        // anderer Slug bleibt unberührt = Default.
        let other = load_legal(&path, "impressum").await.unwrap();
        assert_eq!(other["title"], "Impressum");

        let _ = tokio::fs::remove_file(&path).await;
    }

    #[tokio::test]
    async fn save_eigener_titel_und_leerer_titel_400() {
        let path = temp_path("title");
        let saved = save_legal(&path, "impressum", br#"{"title":"Mein Impressum","body":"x"}"#).await.unwrap();
        assert_eq!(saved["title"], "Mein Impressum");

        // title vorhanden aber leer → 400.
        let err = save_legal(&path, "impressum", br#"{"title":"   ","body":"x"}"#).await.unwrap_err();
        assert_eq!(err.into_response().status(), StatusCode::BAD_REQUEST);

        let _ = tokio::fs::remove_file(&path).await;
    }

    #[tokio::test]
    async fn save_ohne_body_400_und_invalid_slug_404() {
        let path = temp_path("nobody");
        let err = save_legal(&path, "agb", br#"{"title":"x"}"#).await.unwrap_err();
        assert_eq!(err.into_response().status(), StatusCode::BAD_REQUEST);
        let err = save_legal(&path, "bogus", br#"{"body":"x"}"#).await.unwrap_err();
        assert_eq!(err.into_response().status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn handler_unauth_401() {
        let r = get_handler(AuthLevel::None, AxumPath("impressum".to_string())).await;
        assert_eq!(r.into_response().status(), StatusCode::UNAUTHORIZED);
        let r = save_handler(AuthLevel::None, AxumPath("impressum".to_string()), axum::body::Bytes::from(r#"{"body":"x"}"#)).await;
        assert_eq!(r.into_response().status(), StatusCode::UNAUTHORIZED);
    }
}
