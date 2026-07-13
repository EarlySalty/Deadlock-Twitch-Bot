//! Admin-Audit-Log — aggregiert Änderungs-Events aus mehreren Quellen.
//!
//! Port von `bot/analytics/audit_log.py:load_admin_audit_log` +
//! `bot/analytics/api_admin.py:_api_admin_audit_log`. `GET /twitch/api/admin/
//! audit-log?since=&limit=&source=` sammelt Audit-Einträge aus DB- und
//! Datei-Quellen, filtert nach `since`/`source`, sortiert nach Zeit absteigend
//! und limitiert. Jede Quelle ist einzeln fehlertolerant (fehlendes Schema →
//! Quelle übersprungen), 1:1 zu Pythons try/except je Loader.
//!
//! **Quellen:** admin_request, promo, roadmap, legal, streamer_history,
//! manual_plan, billing (Webhook-Events mit Abo-Tabelle als Fallback).
//!
//! CSRF irrelevant (GET); Admin über `DashboardAuthLevel`.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::auth::level::DashboardAuthLevel;
use axum::{extract::RawQuery, extract::State, response::IntoResponse, Json};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;
use tb_http_core::ApiError;

use tb_analytics::promo_mode::{load_global_promo_mode, parse_utc_datetime};

const DEFAULT_LIMIT: i64 = 100;
const MAX_LIMIT: i64 = 500;

// ── Helfer ──────────────────────────────────────────────────────────────────

/// Parst einen Timestamp und formatiert ihn als ISO (Python `_coerce_iso_datetime`).
fn coerce_iso(value: Option<&str>) -> Option<String> {
    parse_utc_datetime(value).map(|dt| dt.to_rfc3339_opts(SecondsFormat::AutoSi, false))
}

fn trim_or_none(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
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
        Some(s) => parse_utc_datetime(timestamp)
            .map(|t| t >= s)
            .unwrap_or(false),
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
    let updated_at = doc
        .get("lastUpdatedAt")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
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
        let updated_at = entry
            .get("lastUpdatedAt")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty());
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

/// `Value::Null` für leere Strings, sonst der String (Python `x or None`).
fn opt(s: &str) -> Value {
    if s.is_empty() {
        Value::Null
    } else {
        json!(s)
    }
}

/// `true` wenn der DB-Fehler auf fehlendes Schema deutet (Python
/// `_is_missing_schema_error`) → Quelle wird übersprungen statt 500.
fn is_missing_schema_error(e: &sqlx::Error) -> bool {
    let s = e.to_string().to_lowercase();
    [
        "does not exist",
        "no such table",
        "undefined table",
        "no such column",
        "undefined column",
    ]
    .iter()
    .any(|m| s.contains(m))
}

/// Partner-Lifecycle-Events (Python `_load_streamer_history_entries`). Erzeugt je
/// Zeile bis zu drei Einträge (added/restore, archive, remove) anhand der
/// gesetzten Zeitstempel. `prior_inactive` (chronologisch über ORDER BY befüllt)
/// unterscheidet erstes Hinzufügen von Re-Aktivierung.
async fn streamer_history_entries(pool: &PgPool) -> Result<Vec<Value>, sqlx::Error> {
    let rows = sqlx::query!(
        "SELECT p.id::text AS \"id?\", p.twitch_user_id AS \"twitch_user_id?\", \
                p.twitch_login AS \"twitch_login?\", p.added_by AS \"added_by?\", \
                p.partnered_at AS \"partnered_at?\", p.admin_archived_at AS \"admin_archived_at?\", \
                p.departnered_at AS \"departnered_at?\", p.status AS \"status?\", \
                p.technical_pause_reason AS \"technical_pause_reason?\" \
         FROM twitch_partners p \
         ORDER BY COALESCE(p.partnered_at, p.departnered_at, p.admin_archived_at, '') ASC, p.id ASC",
    )
    .fetch_all(pool)
    .await?;

    let mut prior_inactive: BTreeSet<String> = BTreeSet::new();
    let mut entries = Vec::new();
    for row in rows {
        let row_id = row.id.unwrap_or_default().trim().to_string();
        let twitch_user_id = row.twitch_user_id.unwrap_or_default().trim().to_string();
        let twitch_login = row.twitch_login.unwrap_or_default().trim().to_string();
        let identity = if !twitch_user_id.is_empty() {
            twitch_user_id.clone()
        } else {
            twitch_login.to_lowercase()
        };
        let added_by = row
            .added_by
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let status = row.status.unwrap_or_default().trim().to_lowercase();
        let pause_reason = row
            .technical_pause_reason
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        let target: Option<String> = if !twitch_login.is_empty() {
            Some(twitch_login.clone())
        } else if !twitch_user_id.is_empty() {
            Some(twitch_user_id.clone())
        } else {
            None
        };
        let label = if !twitch_login.is_empty() {
            twitch_login.clone()
        } else {
            twitch_user_id.clone()
        };

        if let Some(ts) = row.partnered_at.as_deref().filter(|s| !s.is_empty()) {
            let is_restore = !identity.is_empty() && prior_inactive.contains(&identity);
            let action = if is_restore { "restore" } else { "added" };
            let description = if is_restore {
                format!("Streamer {label} wurde wieder aktiviert.")
            } else {
                format!("Streamer {label} wurde hinzugefuegt.")
            };
            let metadata = json!({ "partnerId": row_id, "status": opt(&status), "twitchUserId": opt(&twitch_user_id) });
            if let Some(e) = make_entry(
                format!("streamer_history:{row_id}:{action}"),
                "streamer_history",
                action,
                added_by.as_deref(),
                target.as_deref(),
                Some(ts),
                description,
                Some(metadata),
            ) {
                entries.push(e);
            }
        }
        if let Some(ts) = row.admin_archived_at.as_deref().filter(|s| !s.is_empty()) {
            let metadata = json!({ "partnerId": row_id, "twitchUserId": opt(&twitch_user_id), "status": opt(&status) });
            if let Some(e) = make_entry(
                format!("streamer_history:{row_id}:archive"),
                "streamer_history",
                "archive",
                None,
                target.as_deref(),
                Some(ts),
                format!("Streamer {label} wurde archiviert."),
                Some(metadata),
            ) {
                entries.push(e);
            }
        }
        if let Some(ts) = row.departnered_at.as_deref().filter(|s| !s.is_empty()) {
            if status == "departnered" {
                let metadata = json!({ "partnerId": row_id, "twitchUserId": opt(&twitch_user_id) });
                if let Some(e) = make_entry(
                    format!("streamer_history:{row_id}:remove"),
                    "streamer_history",
                    "remove",
                    None,
                    target.as_deref(),
                    Some(ts),
                    format!("Streamer {label} wurde entfernt oder departnert."),
                    Some(metadata),
                ) {
                    entries.push(e);
                }
            }
        }

        if !identity.is_empty() && (status != "active" || !pause_reason.is_empty()) {
            prior_inactive.insert(identity);
        }
    }
    Ok(entries)
}

/// Manuelle Plan-Overrides (Python `_load_manual_plan_entries`).
async fn manual_plan_entries(pool: &PgPool) -> Result<Vec<Value>, sqlx::Error> {
    let rows = sqlx::query!(
        "SELECT twitch_user_id AS \"twitch_user_id?\", twitch_login AS \"twitch_login?\", \
                manual_plan_id AS \"manual_plan_id?\", manual_plan_expires_at AS \"manual_plan_expires_at?\", \
                manual_plan_notes AS \"manual_plan_notes?\", manual_plan_updated_at AS \"manual_plan_updated_at?\" \
         FROM streamer_plans WHERE manual_plan_updated_at IS NOT NULL ORDER BY manual_plan_updated_at DESC",
    )
    .fetch_all(pool)
    .await?;

    let mut entries = Vec::new();
    for row in rows {
        let twitch_user_id = row.twitch_user_id.unwrap_or_default().trim().to_string();
        let twitch_login = row.twitch_login.unwrap_or_default().trim().to_string();
        let manual_plan_id = row.manual_plan_id.unwrap_or_default().trim().to_string();
        let notes = row.manual_plan_notes.unwrap_or_default().trim().to_string();
        let updated_raw = row.manual_plan_updated_at.unwrap_or_default();
        let label = if !twitch_login.is_empty() {
            twitch_login.clone()
        } else {
            twitch_user_id.clone()
        };
        let target: Option<String> = if !twitch_login.is_empty() {
            Some(twitch_login.clone())
        } else if !twitch_user_id.is_empty() {
            Some(twitch_user_id.clone())
        } else {
            None
        };
        let id_key = if !twitch_user_id.is_empty() {
            twitch_user_id.clone()
        } else {
            twitch_login.clone()
        };

        let (action, description) = if !manual_plan_id.is_empty() {
            (
                "plan_override",
                format!("Manueller Plan fuer {label} auf {manual_plan_id} gesetzt."),
            )
        } else {
            (
                "plan_override_cleared",
                format!("Manueller Plan-Override fuer {label} entfernt."),
            )
        };
        let metadata = json!({
            "planId": opt(&manual_plan_id),
            "expiresAt": coerce_iso(row.manual_plan_expires_at.as_deref()),
            "notes": opt(&notes),
            "twitchUserId": opt(&twitch_user_id),
        });
        if let Some(e) = make_entry(
            format!("manual_plan:{id_key}:{updated_raw}"),
            "manual_plan",
            action,
            None,
            target.as_deref(),
            Some(updated_raw.as_str()),
            description,
            Some(metadata),
        ) {
            entries.push(e);
        }
    }
    Ok(entries)
}

/// Mappt einen Stripe-Event-Typ auf (action, description) (Python `_map_billing_action`).
fn map_billing_action(event_type: &str) -> (String, String) {
    let n = event_type.trim().to_lowercase();
    let known = match n.as_str() {
        "checkout.session.completed" => Some((
            "checkout_completed",
            "Stripe-Checkout fuer ein Abo abgeschlossen.",
        )),
        "customer.subscription.created" => Some(("subscription_created", "Stripe-Abo erstellt.")),
        "customer.subscription.updated" => {
            Some(("subscription_updated", "Stripe-Abo aktualisiert."))
        }
        "customer.subscription.deleted" => Some(("subscription_canceled", "Stripe-Abo beendet.")),
        "invoice.payment_succeeded" => Some(("invoice_paid", "Abo-Zahlung erfolgreich verbucht.")),
        "invoice.payment_failed" => Some(("invoice_failed", "Abo-Zahlung fehlgeschlagen.")),
        _ => None,
    };
    match known {
        Some((a, d)) => (a.to_string(), d.to_string()),
        None => {
            let fallback = if n.is_empty() {
                "billing_event".to_string()
            } else {
                n.replace('.', "_")
            };
            let label = if n.is_empty() { "unknown" } else { n.as_str() };
            (fallback, format!("Billing-Event {label} verarbeitet."))
        }
    }
}

/// Extrahiert Ziel + Detail-Metadaten aus dem Stripe-Event-Payload
/// (Python `_extract_billing_target`).
fn extract_billing_target(event_payload: &Value, object_id: &str) -> (Option<String>, Value) {
    let object_record = event_payload
        .get("data")
        .and_then(|d| d.get("object"))
        .filter(|o| o.is_object());
    let get_str = |key: &str| -> String {
        object_record
            .and_then(|o| o.get(key))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let metadata = object_record
        .and_then(|o| o.get("metadata"))
        .filter(|m| m.is_object());
    let meta_str = |key: &str| -> String {
        metadata
            .and_then(|m| m.get(key))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };

    let customer_reference = {
        let cr = meta_str("customer_reference");
        if !cr.is_empty() {
            cr
        } else {
            let crid = get_str("client_reference_id");
            if !crid.is_empty() {
                crid
            } else {
                get_str("customer_email")
            }
        }
    };
    let subscription_id = {
        let sub = get_str("subscription");
        if !sub.is_empty() {
            sub
        } else {
            let id = get_str("id");
            if !id.is_empty() {
                id
            } else {
                object_id.trim().to_string()
            }
        }
    };
    let plan_id = meta_str("plan_id");
    let status = get_str("status").to_lowercase();

    let details = json!({
        "customerReference": opt(&customer_reference),
        "subscriptionId": opt(&subscription_id),
        "planId": opt(&plan_id),
        "status": opt(&status),
    });
    let target = if !customer_reference.is_empty() {
        Some(customer_reference)
    } else if !subscription_id.is_empty() {
        Some(subscription_id)
    } else if !object_id.trim().is_empty() {
        Some(object_id.trim().to_string())
    } else {
        None
    };
    (target, details)
}

/// Stripe-Webhook-Events (Python `_load_billing_event_entries`).
async fn billing_event_entries(pool: &PgPool) -> Result<Vec<Value>, sqlx::Error> {
    let rows = sqlx::query!(
        "SELECT stripe_event_id AS \"stripe_event_id?\", event_type AS \"event_type?\", \
                object_id AS \"object_id?\", received_at AS \"received_at?\", \
                livemode AS \"livemode?\", payload AS \"payload?\" \
         FROM twitch_billing_events ORDER BY received_at DESC",
    )
    .fetch_all(pool)
    .await?;

    let mut entries = Vec::new();
    for row in rows {
        let event_id = row.stripe_event_id.unwrap_or_default().trim().to_string();
        let event_type = row.event_type.unwrap_or_default().trim().to_string();
        let object_id = row.object_id.unwrap_or_default().trim().to_string();
        let livemode = row.livemode.unwrap_or(0) != 0;
        let payload_text = row.payload.unwrap_or_default();
        let event_payload: Value =
            serde_json::from_str(payload_text.trim()).unwrap_or_else(|_| json!({}));

        let (target, details) = extract_billing_target(&event_payload, &object_id);
        let (action, description) = map_billing_action(&event_type);
        let mut metadata = json!({ "eventType": opt(&event_type), "objectId": opt(&object_id), "livemode": livemode });
        if let (Some(m), Some(d)) = (metadata.as_object_mut(), details.as_object()) {
            for (k, v) in d {
                m.insert(k.clone(), v.clone());
            }
        }
        let id = if !event_id.is_empty() {
            event_id
        } else {
            object_id.clone()
        };
        if let Some(e) = make_entry(
            format!("billing:{id}"),
            "billing",
            &action,
            None,
            target.as_deref(),
            row.received_at.as_deref(),
            description,
            Some(metadata),
        ) {
            entries.push(e);
        }
    }
    Ok(entries)
}

/// Abo-Statusänderungen als Fallback, wenn keine Webhook-Events vorliegen
/// (Python `_load_billing_subscription_entries`).
async fn billing_subscription_entries(pool: &PgPool) -> Result<Vec<Value>, sqlx::Error> {
    let rows = sqlx::query!(
        "SELECT stripe_subscription_id AS \"stripe_subscription_id?\", \
                customer_reference AS \"customer_reference?\", status AS \"status?\", \
                plan_id AS \"plan_id?\", current_period_end AS \"current_period_end?\", \
                canceled_at AS \"canceled_at?\", ended_at AS \"ended_at?\", updated_at AS \"updated_at?\" \
         FROM twitch_billing_subscriptions WHERE updated_at IS NOT NULL ORDER BY updated_at DESC",
    )
    .fetch_all(pool)
    .await?;

    let mut entries = Vec::new();
    for row in rows {
        let subscription_id = row
            .stripe_subscription_id
            .unwrap_or_default()
            .trim()
            .to_string();
        let customer_reference = row
            .customer_reference
            .unwrap_or_default()
            .trim()
            .to_string();
        let status = row.status.unwrap_or_default().trim().to_lowercase();
        let plan_id = row.plan_id.unwrap_or_default().trim().to_string();
        let updated_raw = row.updated_at.unwrap_or_default();
        let label = if !customer_reference.is_empty() {
            customer_reference.clone()
        } else {
            subscription_id.clone()
        };

        let is_canceled = row.ended_at.as_deref().is_some_and(|s| !s.is_empty())
            || row.canceled_at.as_deref().is_some_and(|s| !s.is_empty())
            || matches!(
                status.as_str(),
                "canceled" | "cancelled" | "incomplete_expired"
            );
        let (action, description): (&str, String) = if is_canceled {
            (
                "subscription_canceled",
                format!("Abo fuer {label} beendet oder gekuendigt."),
            )
        } else {
            let status_label = if status.is_empty() {
                "unknown"
            } else {
                status.as_str()
            };
            (
                "subscription_updated",
                format!("Abo-Status fuer {label} auf {status_label} aktualisiert."),
            )
        };
        let target = if !customer_reference.is_empty() {
            Some(customer_reference.clone())
        } else if !subscription_id.is_empty() {
            Some(subscription_id.clone())
        } else {
            None
        };
        let metadata = json!({
            "subscriptionId": opt(&subscription_id),
            "customerReference": opt(&customer_reference),
            "status": opt(&status),
            "planId": opt(&plan_id),
            "currentPeriodEnd": coerce_iso(row.current_period_end.as_deref()),
            "canceledAt": coerce_iso(row.canceled_at.as_deref()),
            "endedAt": coerce_iso(row.ended_at.as_deref()),
        });
        if let Some(e) = make_entry(
            format!("billing:{subscription_id}:{updated_raw}"),
            "billing",
            action,
            None,
            target.as_deref(),
            Some(updated_raw.as_str()),
            description,
            Some(metadata),
        ) {
            entries.push(e);
        }
    }
    Ok(entries)
}

#[derive(sqlx::FromRow)]
struct AdminRequestAuditRow {
    id: i64,
    occurred_at: DateTime<Utc>,
    actor: String,
    method: String,
    path: String,
    status_code: i32,
}

/// Serverseitig persistierte, erfolgreiche Admin-Mutationen.
async fn admin_request_entries(pool: &PgPool) -> Result<Vec<Value>, sqlx::Error> {
    let rows = sqlx::query_as::<_, AdminRequestAuditRow>(
        r#"SELECT id, occurred_at, actor, method, path, status_code
           FROM dashboard_admin_audit_events
           ORDER BY occurred_at DESC, id DESC
           LIMIT 5000"#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let timestamp = row.occurred_at.to_rfc3339();
            let method = row.method.trim().to_uppercase();
            let path = row.path.trim();
            make_entry(
                format!("admin_request:{}", row.id),
                "admin_request",
                &method.to_lowercase(),
                Some(&row.actor),
                Some(path),
                Some(&timestamp),
                format!("Admin-Aktion {method} {path} abgeschlossen."),
                Some(json!({ "statusCode": row.status_code })),
            )
        })
        .collect())
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
                let s = e
                    .get("source")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_lowercase();
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
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    RawQuery(query): RawQuery,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return Err(err);
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
    // DB-Quellen: fehlendes Schema → Quelle überspringen, sonst 500 (Python try/except).
    for result in [
        admin_request_entries(&pool).await,
        streamer_history_entries(&pool).await,
        manual_plan_entries(&pool).await,
    ] {
        match result {
            Ok(es) => entries.extend(es),
            Err(e) if is_missing_schema_error(&e) => {}
            Err(e) => {
                tracing::error!("audit-log DB-Quelle fehlgeschlagen: {e}");
                return Err(ApiError::internal());
            }
        }
    }
    // billing: Webhook-Events haben Vorrang; nur ohne sie die Abo-Tabelle (Fallback).
    let billing_events = match billing_event_entries(&pool).await {
        Ok(es) => es,
        Err(e) if is_missing_schema_error(&e) => Vec::new(),
        Err(e) => {
            tracing::error!("audit-log billing-events fehlgeschlagen: {e}");
            return Err(ApiError::internal());
        }
    };
    if !billing_events.is_empty() {
        entries.extend(billing_events);
    } else {
        match billing_subscription_entries(&pool).await {
            Ok(es) => entries.extend(es),
            Err(e) if is_missing_schema_error(&e) => {}
            Err(e) => {
                tracing::error!("audit-log billing-subscriptions fehlgeschlagen: {e}");
                return Err(ApiError::internal());
            }
        }
    }
    // promo (DB, tabellen-erzeugend) + Datei-Quellen.
    entries.extend(promo_entries(&pool).await);
    entries.extend(roadmap_entries().await);
    entries.extend(legal_entries().await);

    Ok(Json(combine_and_filter(
        entries,
        since,
        &source_filters,
        limit,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE twitch_partners (id BIGSERIAL PRIMARY KEY, twitch_user_id TEXT, twitch_login TEXT, added_by TEXT, partnered_at TEXT, admin_archived_at TEXT, departnered_at TEXT, status TEXT, technical_pause_reason TEXT)",
            "CREATE TABLE streamer_plans (twitch_login TEXT PRIMARY KEY, twitch_user_id TEXT, manual_plan_id TEXT, manual_plan_expires_at TEXT, manual_plan_notes TEXT, manual_plan_updated_at TEXT)",
            "CREATE TABLE twitch_billing_events (stripe_event_id TEXT PRIMARY KEY, event_type TEXT, object_id TEXT, received_at TEXT, livemode INTEGER, payload TEXT)",
            "CREATE TABLE twitch_billing_subscriptions (stripe_subscription_id TEXT PRIMARY KEY, customer_reference TEXT, status TEXT, plan_id TEXT, current_period_end TEXT, canceled_at TEXT, ended_at TEXT, updated_at TEXT)",
            "CREATE TABLE dashboard_admin_audit_events (id BIGSERIAL PRIMARY KEY, occurred_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), actor TEXT NOT NULL, method TEXT NOT NULL, path TEXT NOT NULL, status_code INTEGER NOT NULL)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn streamer_history_added() {
        let Some(pool) = make_pool("t_audit_sh").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_partners (twitch_user_id, twitch_login, added_by, partnered_at, status) VALUES ('100','nani','admin','2026-02-01T00:00:00Z','active')")
            .execute(&pool).await.unwrap();
        let entries = streamer_history_entries(&pool).await.unwrap();
        let actions: Vec<&str> = entries
            .iter()
            .map(|e| e["action"].as_str().unwrap())
            .collect();
        assert!(actions.contains(&"added"));
        let added = entries.iter().find(|e| e["action"] == "added").unwrap();
        assert_eq!(added["target"], "nani");
        assert_eq!(added["actor"], "admin");
        assert_eq!(added["metadata"]["twitchUserId"], "100");
    }

    #[tokio::test]
    async fn streamer_history_restore_und_remove() {
        let Some(pool) = make_pool("t_audit_restore").await else {
            return;
        };
        // Gleiche Identität: erst departnered (älter) → remove, dann re-partnered (neuer) → restore.
        sqlx::query("INSERT INTO twitch_partners (twitch_user_id, twitch_login, departnered_at, status) VALUES ('200','foo','2026-01-01T00:00:00Z','departnered')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_partners (twitch_user_id, twitch_login, partnered_at, status) VALUES ('200','foo','2026-02-01T00:00:00Z','active')")
            .execute(&pool).await.unwrap();
        let entries = streamer_history_entries(&pool).await.unwrap();
        let actions: Vec<&str> = entries
            .iter()
            .map(|e| e["action"].as_str().unwrap())
            .collect();
        assert!(actions.contains(&"remove"));
        assert!(
            actions.contains(&"restore"),
            "prior_inactive → restore statt added"
        );
        assert!(!actions.contains(&"added"));
    }

    #[tokio::test]
    async fn manual_plan_set_und_cleared() {
        let Some(pool) = make_pool("t_audit_mp").await else {
            return;
        };
        sqlx::query("INSERT INTO streamer_plans (twitch_login, twitch_user_id, manual_plan_id, manual_plan_updated_at, manual_plan_notes) VALUES ('nani','100','raid_extended','2026-03-01T00:00:00Z','VIP')")
            .execute(&pool).await.unwrap();
        // Ohne manual_plan_updated_at → ignoriert (WHERE NOT NULL).
        sqlx::query("INSERT INTO streamer_plans (twitch_login, twitch_user_id, manual_plan_id) VALUES ('bar','101','x')")
            .execute(&pool).await.unwrap();
        let entries = manual_plan_entries(&pool).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["action"], "plan_override");
        assert_eq!(entries[0]["metadata"]["planId"], "raid_extended");
        assert_eq!(entries[0]["metadata"]["notes"], "VIP");
        assert_eq!(entries[0]["target"], "nani");
    }

    #[test]
    fn map_billing_action_bekannt_und_fallback() {
        assert_eq!(
            map_billing_action("customer.subscription.deleted").0,
            "subscription_canceled"
        );
        assert_eq!(
            map_billing_action("invoice.payment_succeeded").0,
            "invoice_paid"
        );
        // unbekannt → fallback mit Punkt→Unterstrich.
        let (a, d) = map_billing_action("custom.weird.event");
        assert_eq!(a, "custom_weird_event");
        assert!(d.contains("custom.weird.event"));
        // leer → billing_event / unknown.
        let (a, d) = map_billing_action("");
        assert_eq!(a, "billing_event");
        assert!(d.contains("unknown"));
    }

    #[test]
    fn extract_billing_target_aus_payload() {
        let payload = json!({
            "data": { "object": {
                "id": "sub_123",
                "status": "ACTIVE",
                "metadata": { "customer_reference": "nani", "plan_id": "raid_plus" }
            }}
        });
        let (target, details) = extract_billing_target(&payload, "evt_obj");
        assert_eq!(target.as_deref(), Some("nani")); // customer_reference hat Vorrang
        assert_eq!(details["subscriptionId"], "sub_123");
        assert_eq!(details["planId"], "raid_plus");
        assert_eq!(details["status"], "active"); // lowercased
                                                 // ohne data.object → target fällt auf object_id.
        let (target, _d) = extract_billing_target(&json!({}), "evt_obj");
        assert_eq!(target.as_deref(), Some("evt_obj"));
    }

    #[tokio::test]
    async fn billing_event_entry_aus_db() {
        let Some(pool) = make_pool("t_audit_be").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_billing_events (stripe_event_id, event_type, object_id, received_at, livemode, payload) VALUES ('evt_1','customer.subscription.created','obj_1','2026-04-01T00:00:00Z',1,'{\"data\":{\"object\":{\"id\":\"sub_9\",\"metadata\":{\"customer_reference\":\"nani\"}}}}')")
            .execute(&pool).await.unwrap();
        let entries = billing_event_entries(&pool).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["action"], "subscription_created");
        assert_eq!(entries[0]["target"], "nani");
        assert_eq!(entries[0]["metadata"]["livemode"], true);
        assert_eq!(
            entries[0]["metadata"]["eventType"],
            "customer.subscription.created"
        );
        assert_eq!(entries[0]["metadata"]["subscriptionId"], "sub_9");
    }

    #[tokio::test]
    async fn billing_subscription_canceled_vs_updated() {
        let Some(pool) = make_pool("t_audit_bs").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_billing_subscriptions (stripe_subscription_id, customer_reference, status, updated_at) VALUES ('sub_a','nani','active','2026-04-02T00:00:00Z')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_billing_subscriptions (stripe_subscription_id, customer_reference, status, ended_at, updated_at) VALUES ('sub_b','foo','canceled','2026-04-01T00:00:00Z','2026-04-03T00:00:00Z')")
            .execute(&pool).await.unwrap();
        let entries = billing_subscription_entries(&pool).await.unwrap();
        assert_eq!(entries.len(), 2);
        let a = entries
            .iter()
            .find(|e| e["metadata"]["subscriptionId"] == "sub_a")
            .unwrap();
        assert_eq!(a["action"], "subscription_updated");
        let b = entries
            .iter()
            .find(|e| e["metadata"]["subscriptionId"] == "sub_b")
            .unwrap();
        assert_eq!(b["action"], "subscription_canceled");
    }

    #[tokio::test]
    async fn admin_request_entry_aus_persistenter_audit_tabelle() {
        let Some(pool) = make_pool("t_audit_requests").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO dashboard_admin_audit_events \
             (occurred_at, actor, method, path, status_code) \
             VALUES ('2026-07-13T10:00:00Z', 'discord:123', 'POST', \
                     '/twitch/api/admin/config/chat', 200)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let entries = admin_request_entries(&pool).await.unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["source"], "admin_request");
        assert_eq!(entries[0]["action"], "post");
        assert_eq!(entries[0]["actor"], "discord:123");
        assert_eq!(entries[0]["target"], "/twitch/api/admin/config/chat");
        assert_eq!(entries[0]["metadata"]["statusCode"], 200);
    }

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
        assert!(make_entry(
            "id".into(),
            "s",
            "a",
            None,
            None,
            Some("quatsch"),
            "d".into(),
            None
        )
        .is_none());
        assert!(make_entry("id".into(), "s", "a", None, None, None, "d".into(), None).is_none());
        // actor leer → null.
        let e = make_entry(
            "id".into(),
            "s",
            "a",
            Some("  "),
            None,
            Some("2026-06-15T12:00:00Z"),
            "d".into(),
            None,
        )
        .unwrap();
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
