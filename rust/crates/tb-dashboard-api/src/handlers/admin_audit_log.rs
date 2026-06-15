//! Admin-Audit-Log — aggregiert Änderungs-Events aus mehreren Quellen.
//!
//! Port von `bot/analytics/audit_log.py:load_admin_audit_log` +
//! `bot/analytics/api_admin.py:_api_admin_audit_log`. `GET /twitch/api/admin/
//! audit-log?since=&limit=&source=` sammelt Audit-Einträge aus DB- und
//! Datei-Quellen, filtert nach `since`/`source`, sortiert nach Zeit absteigend
//! und limitiert. Jede Quelle ist einzeln fehlertolerant (fehlendes Schema →
//! Quelle übersprungen), 1:1 zu Pythons try/except je Loader.
//!
//! **Quellen-Status:** promo + roadmap + legal portiert (dieser Commit);
//! streamer_history + manual_plan + billing folgen als Teil 2/3.
//!
//! CSRF irrelevant (GET); Admin über `AuthLevel::is_privileged`.

use std::collections::BTreeSet;
use std::path::PathBuf;

use axum::{extract::RawQuery, extract::State, response::IntoResponse, Json};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;
use tb_http_core::{ApiError, AuthLevel};

use tb_analytics::promo_mode::{load_global_promo_mode, parse_utc_datetime};

const DEFAULT_LIMIT: i64 = 100;
const MAX_LIMIT: i64 = 500;

// ── Helfer ──────────────────────────────────────────────────────────────────

/// Parst einen Timestamp und formatiert ihn als ISO (Python `_coerce_iso_datetime`).
fn coerce_iso(value: Option<&str>) -> Option<String> {
    parse_utc_datetime(value).map(|dt| dt.to_rfc3339_opts(SecondsFormat::AutoSi, false))
}

fn trim_or_none(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

/// Baut einen Audit-Eintrag (Python `_make_entry`). `None`, wenn der Timestamp
/// nicht parsebar ist.
#[allow(clippy::too_many_arguments)]
fn make_entry(
    entry_id: String,
    source: &str,
    action: &str,
    actor: Option<&str>,
    target: Option<&str>,
    timestamp: Option<&str>,
    description: String,
    metadata: Option<Value>,
) -> Option<Value> {
    let iso = coerce_iso(timestamp)?;
    Some(json!({
        "id": entry_id,
        "source": source,
        "action": action,
        "actor": trim_or_none(actor),
        "target": trim_or_none(target),
        "timestamp": iso,
        "description": description,
        "metadata": metadata,
    }))
}

/// `since`-Filter (Python `_matches_since`).
fn matches_since(timestamp: Option<&str>, since: Option<DateTime<Utc>>) -> bool {
    match since {
        None => true,
        Some(s) => parse_utc_datetime(timestamp).map(|t| t >= s).unwrap_or(false),
    }
}

/// Source-Filter normalisieren: je Wert an Kommas splitten, lowercase, trim
/// (Python `_normalize_source_filters`).
fn normalize_source_filters(sources: &[String]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for raw in sources {
        for part in raw.split(',') {
            let v = part.trim().to_lowercase();
            if !v.is_empty() {
                out.insert(v);
            }
        }
    }
    out
}

/// Limit clampen (Python `_clamp_limit`): `None`/ungültig → Default, sonst [1, MAX].
fn clamp_limit(limit: Option<i64>) -> usize {
    match limit {
        Some(n) => n.clamp(1, MAX_LIMIT) as usize,
        None => DEFAULT_LIMIT as usize,
    }
}

fn data_path(name: &str) -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("data/admin_dashboard")
        .join(name)
}

// ── Quellen-Loader (Teil 1: promo + roadmap + legal) ──────────────────────────

/// Promo-/Announcements-Änderung (Python `_load_promo_entries`).
async fn promo_entries(pool: &PgPool) -> Vec<Value> {
    let Ok(config) = load_global_promo_mode(pool).await else {
        return Vec::new();
    };
    let Some(updated_at) = config.updated_at.as_deref().filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    let description = if !config.custom_message.is_empty() {
        "Announcements-Konfiguration aktualisiert."
    } else {
        "Promo-Modus-Konfiguration aktualisiert."
    };
    let metadata = json!({
        "mode": config.mode,
        "isEnabled": config.is_enabled,
        "startsAt": coerce_iso(config.starts_at.as_deref()),
        "endsAt": coerce_iso(config.ends_at.as_deref()),
    });
    make_entry(
        format!("promo:global:{updated_at}"),
        "promo",
        "announcement_update",
        config.updated_by.as_str().into(),
        Some("global"),
        Some(updated_at),
        description.to_string(),
        Some(metadata),
    )
    .into_iter()
    .collect()
}

/// Roadmap-Inhaltsänderung (Python `_load_roadmap_entries`).
async fn roadmap_entries() -> Vec<Value> {
    let Ok(raw) = tokio::fs::read_to_string(data_path("roadmap_body.json")).await else {
        return Vec::new();
    };
    let Ok(doc) = serde_json::from_str::<Value>(&raw) else {
        return Vec::new();
    };
    let updated_at = doc.get("lastUpdatedAt").and_then(Value::as_str).filter(|s| !s.is_empty());
    let Some(updated_at) = updated_at else {
        return Vec::new();
    };
    let updated_by = doc.get("lastUpdatedBy").and_then(Value::as_str);
    make_entry(
        format!("roadmap:main:{updated_at}"),
        "roadmap",
        "content_edit",
        updated_by,
        Some("roadmap"),
        Some(updated_at),
        "Roadmap-Inhalt aktualisiert.".to_string(),
        None,
    )
    .into_iter()
    .collect()
}

/// Legal-Seiten-Änderungen (Python `_load_legal_entries`, sorted Slugs).
async fn legal_entries() -> Vec<Value> {
    let Ok(raw) = tokio::fs::read_to_string(data_path("legal_pages.json")).await else {
        return Vec::new();
    };
    let Ok(doc) = serde_json::from_str::<Value>(&raw) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    // sorted(LEGAL_PAGE_SLUGS) = agb, datenschutz, impressum, sicherheit.
    for slug in ["agb", "datenschutz", "impressum", "sicherheit"] {
        let Some(entry) = doc.get(slug).and_then(Value::as_object) else {
            continue;
        };
        let updated_at = entry.get("lastUpdatedAt").and_then(Value::as_str).filter(|s| !s.is_empty());
        let Some(updated_at) = updated_at else {
            continue;
        };
        let updated_by = entry.get("lastUpdatedBy").and_then(Value::as_str);
        let title = trim_or_none(entry.get("title").and_then(Value::as_str));
        let metadata = json!({ "slug": slug, "title": title });
        if let Some(e) = make_entry(
            format!("legal:{slug}:{updated_at}"),
            "legal",
            "content_edit",
            updated_by,
            Some(slug),
            Some(updated_at),
            format!("Legal-Seite {slug} aktualisiert."),
            Some(metadata),
        ) {
            out.push(e);
        }
    }
    out
}

// ── Aggregation ───────────────────────────────────────────────────────────────

fn combine_and_filter(
    entries: Vec<Value>,
    since: Option<DateTime<Utc>>,
    source_filters: &BTreeSet<String>,
    limit: Option<i64>,
) -> Value {
    // since-Filter.
    let mut since_filtered: Vec<Value> = entries
        .into_iter()
        .filter(|e| matches_since(e.get("timestamp").and_then(Value::as_str), since))
        .collect();
    // Sortierung nach Timestamp absteigend (String-Vergleich wie Python).
    since_filtered.sort_by(|a, b| {
        let ta = a.get("timestamp").and_then(Value::as_str).unwrap_or("");
        let tb = b.get("timestamp").and_then(Value::as_str).unwrap_or("");
        tb.cmp(ta)
    });
    // all_sources aus since_filtered (vor Source-Filter, wie Python).
    let all_sources: BTreeSet<String> = since_filtered
        .iter()
        .filter_map(|e| e.get("source").and_then(Value::as_str))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let filtered: Vec<Value> = if source_filters.is_empty() {
        since_filtered
    } else {
        since_filtered
            .into_iter()
            .filter(|e| {
                let s = e.get("source").and_then(Value::as_str).unwrap_or("").trim().to_lowercase();
                source_filters.contains(&s)
            })
            .collect()
    };

    let resolved_limit = clamp_limit(limit);
    let total = filtered.len();
    let limited: Vec<Value> = filtered.into_iter().take(resolved_limit).collect();

    json!({
        "entries": limited,
        "sources": all_sources.into_iter().collect::<Vec<_>>(),
        "totalCount": total,
        "hasMore": total > resolved_limit,
    })
}

// ── Handler ────────────────────────────────────────────────────────────────────

/// `GET /twitch/api/admin/audit-log` — aggregiertes Audit-Log (Admin).
pub async fn handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    RawQuery(query): RawQuery,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let mut since_raw = String::new();
    let mut limit_raw = String::new();
    let mut sources: Vec<String> = Vec::new();
    for (key, value) in url::form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
        match key.as_ref() {
            "since" => since_raw = value.trim().to_string(),
            "limit" => limit_raw = value.trim().to_string(),
            "source" => {
                let s = value.trim().to_string();
                if !s.is_empty() {
                    sources.push(s);
                }
            }
            _ => {}
        }
    }

    let since = if since_raw.is_empty() {
        None
    } else {
        match parse_utc_datetime(Some(&since_raw)) {
            Some(dt) => Some(dt),
            None => {
                return Err(ApiError::bad_request_with_body(json!({
                    "error": "invalid_since",
                    "message": "since muss ein gueltiges ISO-Datum oder ISO-Timestamp sein.",
                })))
            }
        }
    };

    let limit: Option<i64> = if limit_raw.is_empty() {
        None
    } else {
        match limit_raw.parse::<i64>() {
            Ok(n) => Some(n),
            Err(_) => {
                return Err(ApiError::bad_request_with_body(json!({
                    "error": "invalid_limit",
                    "message": "limit muss eine ganze Zahl sein.",
                })))
            }
        }
    };

    let source_filters = normalize_source_filters(&sources);

    let mut entries: Vec<Value> = Vec::new();
    entries.extend(promo_entries(&pool).await);
    entries.extend(roadmap_entries().await);
    entries.extend(legal_entries().await);

    Ok(Json(combine_and_filter(entries, since, &source_filters, limit)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(ts: &str, source: &str) -> Value {
        make_entry(
            format!("{source}:{ts}"),
            source,
            "x",
            Some("admin"),
            Some("t"),
            Some(ts),
            "d".to_string(),
            None,
        )
        .unwrap()
    }

    #[test]
    fn make_entry_ungueltiger_timestamp_none() {
        assert!(make_entry("id".into(), "s", "a", None, None, Some("quatsch"), "d".into(), None).is_none());
        assert!(make_entry("id".into(), "s", "a", None, None, None, "d".into(), None).is_none());
        // actor leer → null.
        let e = make_entry("id".into(), "s", "a", Some("  "), None, Some("2026-06-15T12:00:00Z"), "d".into(), None).unwrap();
        assert!(e["actor"].is_null());
    }

    #[test]
    fn normalize_source_und_clamp() {
        let f = normalize_source_filters(&["Promo, legal".to_string(), "ROADMAP".to_string()]);
        assert!(f.contains("promo") && f.contains("legal") && f.contains("roadmap"));
        assert_eq!(clamp_limit(None), 100);
        assert_eq!(clamp_limit(Some(0)), 1);
        assert_eq!(clamp_limit(Some(9999)), 500);
        assert_eq!(clamp_limit(Some(50)), 50);
    }

    #[test]
    fn combine_sortiert_filtert_limitiert() {
        let entries = vec![
            entry("2026-06-01T00:00:00Z", "promo"),
            entry("2026-06-03T00:00:00Z", "legal"),
            entry("2026-06-02T00:00:00Z", "roadmap"),
        ];
        let v = combine_and_filter(entries.clone(), None, &BTreeSet::new(), None);
        // Sortierung absteigend → legal (3.) zuerst.
        assert_eq!(v["entries"][0]["source"], "legal");
        assert_eq!(v["totalCount"], 3);
        assert_eq!(v["hasMore"], false);
        // sources sortiert.
        assert_eq!(v["sources"], json!(["legal", "promo", "roadmap"]));

        // Source-Filter.
        let only = normalize_source_filters(&["promo".to_string()]);
        let v = combine_and_filter(entries.clone(), None, &only, None);
        assert_eq!(v["totalCount"], 1);
        assert_eq!(v["entries"][0]["source"], "promo");
        // sources bleibt die volle Liste (vor Source-Filter).
        assert_eq!(v["sources"].as_array().unwrap().len(), 3);

        // since-Filter (nur >= 2026-06-02).
        let since = parse_utc_datetime(Some("2026-06-02T00:00:00Z"));
        let v = combine_and_filter(entries.clone(), since, &BTreeSet::new(), None);
        assert_eq!(v["totalCount"], 2);

        // limit + hasMore.
        let v = combine_and_filter(entries, None, &BTreeSet::new(), Some(2));
        assert_eq!(v["entries"].as_array().unwrap().len(), 2);
        assert_eq!(v["hasMore"], true);
        assert_eq!(v["totalCount"], 3);
    }
}
