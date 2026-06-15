//! Global admin-verwalteter Promo-Modus für Twitch-Chat-Promo-Announcements.
//!
//! Port von `bot/promo_mode.py`. Ein Singleton-Datensatz
//! (`twitch_global_promo_modes`, `config_key='global'`) steuert, ob die
//! Chat-Promos im Standard-Pool laufen (`standard`) oder global durch einen
//! Event-Text ersetzt werden (`custom_event`, optional zeitfenster-gebunden).
//!
//! Konsumenten:
//! - **Chat** (`tb-chat::promos`): liest via [`load_global_promo_mode`] +
//!   [`evaluate_global_promo_mode`] den aktiven Event-Text (Schritt 1 vor
//!   Streamer- und Pool-Promos).
//! - **Admin-API** (`tb-dashboard-api`): GET/POST zum Lesen/Setzen der Config
//!   via [`load_global_promo_mode`] / [`save_global_promo_mode`].

use chrono::{DateTime, NaiveDateTime, SecondsFormat, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::collections::HashSet;

pub const PROMO_MODE_STANDARD: &str = "standard";
pub const PROMO_MODE_CUSTOM_EVENT: &str = "custom_event";
pub const PROMO_MODE_SINGLETON_KEY: &str = "global";
pub const STREAMER_PROMO_MESSAGE_MAX_LENGTH: usize = 500;

fn is_allowed_mode(mode: &str) -> bool {
    mode == PROMO_MODE_STANDARD || mode == PROMO_MODE_CUSTOM_EVENT
}

fn is_allowed_placeholder(root: &str) -> bool {
    root == "invite"
}

// ── Typen ───────────────────────────────────────────────────────────────────

/// Normalisierte Promo-Modus-Config (Python `default_global_promo_mode_config`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromoModeConfig {
    pub mode: String,
    pub custom_message: String,
    pub starts_at: Option<String>,
    pub ends_at: Option<String>,
    pub is_enabled: bool,
    pub updated_at: Option<String>,
    pub updated_by: String,
}

impl PromoModeConfig {
    /// Default: Standard-Modus, nichts aktiv.
    pub fn default_config() -> Self {
        Self {
            mode: PROMO_MODE_STANDARD.to_string(),
            custom_message: String::new(),
            starts_at: None,
            ends_at: None,
            is_enabled: false,
            updated_at: None,
            updated_by: String::new(),
        }
    }

    /// JSON-Repräsentation für API-Antworten (camelCase-frei, 1:1 Python-Keys).
    pub fn to_json(&self) -> Value {
        json!({
            "mode": self.mode,
            "custom_message": self.custom_message,
            "starts_at": self.starts_at,
            "ends_at": self.ends_at,
            "is_enabled": self.is_enabled,
            "updated_at": self.updated_at,
            "updated_by": self.updated_by,
        })
    }
}

/// Ein Validierungs-Issue (Python `_issue`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    pub field: String,
    pub message: String,
    pub code: Option<String>,
}

impl ValidationIssue {
    fn new(field: &str, message: impl Into<String>, code: &str) -> Self {
        Self {
            field: field.to_string(),
            message: message.into(),
            code: if code.is_empty() { None } else { Some(code.to_string()) },
        }
    }

    pub fn to_json(&self) -> Value {
        match &self.code {
            Some(c) => json!({ "field": self.field, "message": self.message, "code": c }),
            None => json!({ "field": self.field, "message": self.message }),
        }
    }
}

/// Ergebnis von [`evaluate_global_promo_mode`].
#[derive(Debug, Clone)]
pub struct PromoModeEvaluation {
    pub config: PromoModeConfig,
    pub status: String,
    pub reason: String,
    pub is_active: bool,
    pub active_message: Option<String>,
    pub starts_at: Option<String>,
    pub ends_at: Option<String>,
    pub now: String,
}

impl PromoModeEvaluation {
    pub fn to_json(&self) -> Value {
        json!({
            "config": self.config.to_json(),
            "status": self.status,
            "reason": self.reason,
            "is_active": self.is_active,
            "active_message": self.active_message,
            "starts_at": self.starts_at,
            "ends_at": self.ends_at,
            "now": self.now,
        })
    }
}

// ── Datums-Helfer ────────────────────────────────────────────────────────────

/// Parst einen ISO/datetime-local-String nach UTC (Python `parse_utc_datetime`).
pub fn parse_utc_datetime(raw: Option<&str>) -> Option<DateTime<Utc>> {
    let text = raw?.trim();
    if text.is_empty() {
        return None;
    }
    let normalized = text.replace('Z', "+00:00");
    if let Ok(dt) = DateTime::parse_from_rfc3339(&normalized) {
        return Some(dt.with_timezone(&Utc));
    }
    // Naive Varianten (kein Offset) → als UTC interpretieren.
    for fmt in ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M"] {
        if let Ok(ndt) = NaiveDateTime::parse_from_str(text, fmt) {
            return Some(DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc));
        }
    }
    None
}

fn iso_seconds(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Secs, false)
}

/// `parse_utc_datetime` → ISO-UTC-String (Sekunden-genau). Python `to_iso_utc`.
pub fn to_iso_utc(raw: Option<&str>) -> Option<String> {
    parse_utc_datetime(raw).map(|dt| iso_seconds(&dt))
}

/// Coerce eines JSON-Werts zu bool (Python `_coerce_bool`).
fn coerce_bool(value: &Value) -> bool {
    match value {
        Value::String(s) => !matches!(s.trim().to_lowercase().as_str(), "" | "0" | "false" | "off" | "no"),
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::Null => false,
        _ => true,
    }
}

fn value_as_str(value: &Value) -> Option<String> {
    value.as_str().map(|s| s.to_string())
}

// ── Normalisierung ───────────────────────────────────────────────────────────

/// Normalisiert eine rohe Config (Python `normalize_global_promo_mode_config`).
pub fn normalize_global_promo_mode_config(raw: &Value) -> PromoModeConfig {
    let mut config = PromoModeConfig::default_config();
    let obj = match raw.as_object() {
        Some(o) => o,
        None => return config,
    };

    let raw_mode = obj.get("mode").and_then(Value::as_str).unwrap_or("").trim().to_lowercase();
    config.mode = if is_allowed_mode(&raw_mode) { raw_mode } else { PROMO_MODE_STANDARD.to_string() };
    config.custom_message = obj.get("custom_message").and_then(Value::as_str).unwrap_or("").trim().to_string();
    config.starts_at = to_iso_utc(obj.get("starts_at").and_then(value_as_str).as_deref());
    config.ends_at = to_iso_utc(obj.get("ends_at").and_then(value_as_str).as_deref());
    config.is_enabled = obj.get("is_enabled").map(coerce_bool).unwrap_or(false);
    config.updated_at = to_iso_utc(obj.get("updated_at").and_then(value_as_str).as_deref());
    config.updated_by = obj.get("updated_by").and_then(Value::as_str).unwrap_or("").trim().to_string();
    config
}

// ── Platzhalter-Validierung ──────────────────────────────────────────────────

/// Extrahiert die Root-Feldnamen aller `{…}`-Platzhalter (Port von
/// `string.Formatter().parse`). `Err(())` bei unbalancierten Klammern.
fn parse_placeholder_roots(text: &str) -> Result<Vec<String>, ()> {
    let chars: Vec<char> = text.chars().collect();
    let mut roots: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '{' => {
                if i + 1 < chars.len() && chars[i + 1] == '{' {
                    i += 2;
                    continue;
                }
                let mut j = i + 1;
                let mut field = String::new();
                let mut closed = false;
                while j < chars.len() {
                    if chars[j] == '}' {
                        closed = true;
                        break;
                    }
                    field.push(chars[j]);
                    j += 1;
                }
                if !closed {
                    return Err(());
                }
                // field_name vor `!conversion`/`:format_spec`, dann Root vor `.`/`[`.
                let root = field
                    .split([':', '!'])
                    .next()
                    .unwrap_or("")
                    .split('.')
                    .next()
                    .unwrap_or("")
                    .split('[')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                roots.push(root);
                i = j + 1;
            }
            '}' => {
                if i + 1 < chars.len() && chars[i + 1] == '}' {
                    i += 2;
                    continue;
                }
                return Err(());
            }
            _ => i += 1,
        }
    }
    Ok(roots)
}

fn validate_template_placeholders(
    text: &str,
    field: &str,
    invalid_message: &str,
    unsupported_prefix: &str,
) -> (Vec<ValidationIssue>, HashSet<String>) {
    let mut issues = Vec::new();
    let mut used = HashSet::new();
    let roots = match parse_placeholder_roots(text) {
        Ok(r) => r,
        Err(()) => return (vec![ValidationIssue::new(field, invalid_message, "invalid_placeholder")], HashSet::new()),
    };
    for root in roots {
        if root.is_empty() {
            issues.push(ValidationIssue::new(field, invalid_message, "invalid_placeholder"));
            continue;
        }
        used.insert(root.clone());
        if !is_allowed_placeholder(&root) {
            issues.push(ValidationIssue::new(
                field,
                format!("{unsupported_prefix} {{{root}}}. Erlaubt ist aktuell nur {{invite}}."),
                "invalid_placeholder",
            ));
        }
    }
    (issues, used)
}

/// Validiert den Event-Text (Python `validate_custom_promo_message`).
pub fn validate_custom_promo_message(message: &str) -> Vec<ValidationIssue> {
    let text = message.trim();
    if text.is_empty() {
        return vec![ValidationIssue::new("custom_message", "Bitte einen Event-Text hinterlegen.", "empty")];
    }
    let (issues, _used) = validate_template_placeholders(
        text,
        "custom_message",
        "Ungültiger Platzhalter im Event-Text.",
        "Nicht unterstützter Platzhalter",
    );
    issues
}

/// Validiert eine Streamer-Promo-Nachricht (Python `validate_streamer_promo_message`).
pub fn validate_streamer_promo_message(message: &str) -> Vec<ValidationIssue> {
    let text = message.trim();
    if text.is_empty() {
        return Vec::new();
    }
    let mut issues: Vec<ValidationIssue> = Vec::new();
    if text.chars().count() > STREAMER_PROMO_MESSAGE_MAX_LENGTH {
        issues.push(ValidationIssue::new(
            "promo_message",
            format!("Die Promo-Nachricht darf maximal {STREAMER_PROMO_MESSAGE_MAX_LENGTH} Zeichen lang sein."),
            "too_long",
        ));
    }
    let (placeholder_issues, used) = validate_template_placeholders(
        text,
        "promo_message",
        "Ungültiger Platzhalter in der Promo-Nachricht.",
        "Nicht unterstützter Platzhalter",
    );
    issues.extend(placeholder_issues);
    if !used.contains("invite") {
        issues.push(ValidationIssue::new(
            "promo_message",
            "Die Promo-Nachricht muss den Platzhalter {invite} enthalten.",
            "missing_invite",
        ));
    }
    issues
}

/// Validiert eine rohe global-Config (Python `validate_global_promo_mode_config`).
/// Gibt die normalisierte Config + alle Issues zurück.
pub fn validate_global_promo_mode_config(raw: &Value) -> (PromoModeConfig, Vec<ValidationIssue>) {
    let config = normalize_global_promo_mode_config(raw);
    let mut issues: Vec<ValidationIssue> = Vec::new();

    let raw_mode = raw
        .as_object()
        .and_then(|o| o.get("mode"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if !raw_mode.is_empty() && !is_allowed_mode(&raw_mode) {
        issues.push(ValidationIssue {
            field: "mode".to_string(),
            message: "Unbekannter Modus. Erlaubt sind standard und custom_event.".to_string(),
            code: None,
        });
    }

    let starts_raw = raw.as_object().and_then(|o| o.get("starts_at")).and_then(value_as_str);
    let ends_raw = raw.as_object().and_then(|o| o.get("ends_at")).and_then(value_as_str);
    if let Some(s) = starts_raw.as_deref().filter(|s| !s.is_empty()) {
        if parse_utc_datetime(Some(s)).is_none() {
            issues.push(ValidationIssue {
                field: "starts_at".to_string(),
                message: "Startzeit ist ungültig. Bitte UTC-ISO oder datetime-local senden.".to_string(),
                code: None,
            });
        }
    }
    if let Some(e) = ends_raw.as_deref().filter(|s| !s.is_empty()) {
        if parse_utc_datetime(Some(e)).is_none() {
            issues.push(ValidationIssue {
                field: "ends_at".to_string(),
                message: "Endzeit ist ungültig. Bitte UTC-ISO oder datetime-local senden.".to_string(),
                code: None,
            });
        }
    }

    let starts_at = parse_utc_datetime(config.starts_at.as_deref());
    let ends_at = parse_utc_datetime(config.ends_at.as_deref());
    if let (Some(s), Some(e)) = (starts_at, ends_at) {
        if e < s {
            issues.push(ValidationIssue {
                field: "ends_at".to_string(),
                message: "Endzeit muss nach der Startzeit liegen.".to_string(),
                code: None,
            });
        }
    }

    if config.mode == PROMO_MODE_CUSTOM_EVENT {
        issues.extend(validate_custom_promo_message(&config.custom_message));
    }

    (config, issues)
}

// ── Auswertung ───────────────────────────────────────────────────────────────

/// Wertet die Config gegen `now` aus (Python `evaluate_global_promo_mode`).
pub fn evaluate_global_promo_mode(raw: &Value, now: Option<DateTime<Utc>>) -> PromoModeEvaluation {
    let config = normalize_global_promo_mode_config(raw);
    let now_utc = now.unwrap_or_else(Utc::now);
    let starts_at = parse_utc_datetime(config.starts_at.as_deref());
    let ends_at = parse_utc_datetime(config.ends_at.as_deref());

    let (status, reason, is_active, active_message) = if config.mode != PROMO_MODE_CUSTOM_EVENT {
        ("standard", "standard_mode", false, None)
    } else if !config.is_enabled {
        ("disabled", "disabled", false, None)
    } else if starts_at.map(|s| now_utc < s).unwrap_or(false) {
        ("scheduled", "before_start", false, None)
    } else if ends_at.map(|e| now_utc > e).unwrap_or(false) {
        ("expired", "after_end", false, None)
    } else if !validate_custom_promo_message(&config.custom_message).is_empty() {
        ("invalid", "invalid_message", false, None)
    } else {
        ("active", "active_custom_event", true, Some(config.custom_message.trim().to_string()))
    };

    PromoModeEvaluation {
        config,
        status: status.to_string(),
        reason: reason.to_string(),
        is_active,
        active_message,
        starts_at: starts_at.map(|d| iso_seconds(&d)),
        ends_at: ends_at.map(|d| iso_seconds(&d)),
        now: iso_seconds(&now_utc),
    }
}

// ── Persistenz ───────────────────────────────────────────────────────────────

/// Legt die Tabelle idempotent an (Python `ensure_global_promo_mode_storage`).
pub async fn ensure_global_promo_mode_storage(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS twitch_global_promo_modes (
            config_key TEXT PRIMARY KEY,
            mode TEXT NOT NULL DEFAULT 'standard',
            custom_message TEXT,
            starts_at TEXT,
            ends_at TEXT,
            is_enabled INTEGER NOT NULL DEFAULT 0,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_by TEXT
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_twitch_global_promo_modes_updated_at \
         ON twitch_global_promo_modes(updated_at)",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Lädt den Singleton-Datensatz, normalisiert (Python `load_global_promo_mode`).
pub async fn load_global_promo_mode(pool: &PgPool) -> Result<PromoModeConfig, sqlx::Error> {
    ensure_global_promo_mode_storage(pool).await?;
    let row: Option<(String, Option<String>, Option<String>, Option<String>, i32, Option<String>, Option<String>)> =
        sqlx::query_as(
            "SELECT mode, custom_message, starts_at, ends_at, is_enabled, updated_at, updated_by \
             FROM twitch_global_promo_modes WHERE config_key = $1 LIMIT 1",
        )
        .bind(PROMO_MODE_SINGLETON_KEY)
        .fetch_optional(pool)
        .await?;

    let Some((mode, custom_message, starts_at, ends_at, is_enabled, updated_at, updated_by)) = row else {
        return Ok(PromoModeConfig::default_config());
    };
    let raw = json!({
        "mode": mode,
        "custom_message": custom_message,
        "starts_at": starts_at,
        "ends_at": ends_at,
        "is_enabled": is_enabled,
        "updated_at": updated_at,
        "updated_by": updated_by,
    });
    Ok(normalize_global_promo_mode_config(&raw))
}

/// Fehler beim Speichern: erstes Validierungs-Issue (Python `raise ValueError`).
#[derive(Debug)]
pub enum SavePromoModeError {
    Validation(String),
    Db(sqlx::Error),
}

impl std::fmt::Display for SavePromoModeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SavePromoModeError::Validation(m) => write!(f, "{m}"),
            SavePromoModeError::Db(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SavePromoModeError {}

impl From<sqlx::Error> for SavePromoModeError {
    fn from(e: sqlx::Error) -> Self {
        SavePromoModeError::Db(e)
    }
}

/// Validiert + speichert die Config (Python `save_global_promo_mode`).
pub async fn save_global_promo_mode(
    pool: &PgPool,
    raw_config: &Value,
    updated_by: &str,
) -> Result<PromoModeConfig, SavePromoModeError> {
    ensure_global_promo_mode_storage(pool).await?;
    let (normalized, issues) = validate_global_promo_mode_config(raw_config);
    if let Some(first) = issues.first() {
        return Err(SavePromoModeError::Validation(first.message.clone()));
    }

    let updated_at = iso_seconds(&Utc::now());
    let updated_by = updated_by.trim();
    let custom_message = if normalized.custom_message.is_empty() {
        None
    } else {
        Some(normalized.custom_message.as_str())
    };
    sqlx::query(
        r#"
        INSERT INTO twitch_global_promo_modes (
            config_key, mode, custom_message, starts_at, ends_at, is_enabled, updated_at, updated_by
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT (config_key) DO UPDATE SET
            mode = EXCLUDED.mode,
            custom_message = EXCLUDED.custom_message,
            starts_at = EXCLUDED.starts_at,
            ends_at = EXCLUDED.ends_at,
            is_enabled = EXCLUDED.is_enabled,
            updated_at = EXCLUDED.updated_at,
            updated_by = EXCLUDED.updated_by
        "#,
    )
    .bind(PROMO_MODE_SINGLETON_KEY)
    .bind(&normalized.mode)
    .bind(custom_message)
    .bind(&normalized.starts_at)
    .bind(&normalized.ends_at)
    .bind(if normalized.is_enabled { 1_i32 } else { 0_i32 })
    .bind(&updated_at)
    .bind(if updated_by.is_empty() { None } else { Some(updated_by) })
    .execute(pool)
    .await?;

    let raw = json!({
        "mode": normalized.mode,
        "custom_message": normalized.custom_message,
        "starts_at": normalized.starts_at,
        "ends_at": normalized.ends_at,
        "is_enabled": normalized.is_enabled,
        "updated_at": updated_at,
        "updated_by": updated_by,
    });
    Ok(normalize_global_promo_mode_config(&raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_ist_standard_inaktiv() {
        let c = PromoModeConfig::default_config();
        assert_eq!(c.mode, "standard");
        assert!(!c.is_enabled);
    }

    #[test]
    fn normalize_unbekannter_modus_wird_standard() {
        let c = normalize_global_promo_mode_config(&json!({ "mode": "BOGUS", "custom_message": "  hi  " }));
        assert_eq!(c.mode, "standard");
        assert_eq!(c.custom_message, "hi");
    }

    #[test]
    fn normalize_is_enabled_coercion() {
        assert!(normalize_global_promo_mode_config(&json!({ "is_enabled": 1 })).is_enabled);
        assert!(normalize_global_promo_mode_config(&json!({ "is_enabled": true })).is_enabled);
        assert!(!normalize_global_promo_mode_config(&json!({ "is_enabled": "off" })).is_enabled);
        assert!(!normalize_global_promo_mode_config(&json!({ "is_enabled": 0 })).is_enabled);
    }

    #[test]
    fn parse_utc_varianten() {
        assert!(parse_utc_datetime(Some("2026-06-15T12:00:00Z")).is_some());
        assert!(parse_utc_datetime(Some("2026-06-15T12:00:00+00:00")).is_some());
        assert!(parse_utc_datetime(Some("2026-06-15T12:00")).is_some()); // datetime-local
        assert!(parse_utc_datetime(Some("quatsch")).is_none());
        assert!(parse_utc_datetime(None).is_none());
        // Round-trip auf Sekunden-ISO.
        assert_eq!(to_iso_utc(Some("2026-06-15T12:00:00Z")).as_deref(), Some("2026-06-15T12:00:00+00:00"));
    }

    #[test]
    fn validate_custom_leer_und_platzhalter() {
        assert_eq!(validate_custom_promo_message("").len(), 1); // empty
        assert!(validate_custom_promo_message("Komm zu {invite}").is_empty()); // ok
        let bad = validate_custom_promo_message("Folge {streamer} bei {invite}");
        assert_eq!(bad.len(), 1); // {streamer} nicht erlaubt
        assert_eq!(bad[0].code.as_deref(), Some("invalid_placeholder"));
    }

    #[test]
    fn validate_streamer_braucht_invite() {
        let issues = validate_streamer_promo_message("Schau mal vorbei");
        assert!(issues.iter().any(|i| i.code.as_deref() == Some("missing_invite")));
        assert!(validate_streamer_promo_message("Schau bei {invite}").is_empty());
        assert!(validate_streamer_promo_message("").is_empty()); // leer = ok (kein Override)
    }

    #[test]
    fn evaluate_standard_kein_active_message() {
        let e = evaluate_global_promo_mode(&json!({ "mode": "standard" }), None);
        assert_eq!(e.status, "standard");
        assert!(!e.is_active);
        assert!(e.active_message.is_none());
    }

    #[test]
    fn evaluate_custom_event_aktiv() {
        let now = DateTime::parse_from_rfc3339("2026-06-15T12:00:00Z").unwrap().with_timezone(&Utc);
        let e = evaluate_global_promo_mode(
            &json!({ "mode": "custom_event", "is_enabled": true, "custom_message": "Event bei {invite}!" }),
            Some(now),
        );
        assert_eq!(e.status, "active");
        assert!(e.is_active);
        assert_eq!(e.active_message.as_deref(), Some("Event bei {invite}!"));
    }

    #[test]
    fn evaluate_custom_event_disabled_und_fenster() {
        let now = DateTime::parse_from_rfc3339("2026-06-15T12:00:00Z").unwrap().with_timezone(&Utc);
        // disabled
        let e = evaluate_global_promo_mode(&json!({ "mode": "custom_event", "is_enabled": false, "custom_message": "x bei {invite}" }), Some(now));
        assert_eq!(e.status, "disabled");
        // scheduled (Start in der Zukunft)
        let e = evaluate_global_promo_mode(&json!({ "mode": "custom_event", "is_enabled": true, "custom_message": "x bei {invite}", "starts_at": "2026-06-16T00:00:00Z" }), Some(now));
        assert_eq!(e.status, "scheduled");
        // expired (Ende in der Vergangenheit)
        let e = evaluate_global_promo_mode(&json!({ "mode": "custom_event", "is_enabled": true, "custom_message": "x bei {invite}", "ends_at": "2026-06-14T00:00:00Z" }), Some(now));
        assert_eq!(e.status, "expired");
        // invalid (Platzhalter nicht erlaubt)
        let e = evaluate_global_promo_mode(&json!({ "mode": "custom_event", "is_enabled": true, "custom_message": "x bei {streamer}" }), Some(now));
        assert_eq!(e.status, "invalid");
    }

    #[test]
    fn validate_global_endzeit_vor_startzeit() {
        let (_c, issues) = validate_global_promo_mode_config(&json!({
            "mode": "custom_event",
            "is_enabled": true,
            "custom_message": "x bei {invite}",
            "starts_at": "2026-06-15T12:00:00Z",
            "ends_at": "2026-06-15T10:00:00Z",
        }));
        assert!(issues.iter().any(|i| i.field == "ends_at"));
    }
}
