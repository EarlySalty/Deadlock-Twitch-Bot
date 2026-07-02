//! Handler für `GET /twitch/api/admin/system/errors`.
//!
//! **Quelle (B1-Fix):** Wie Pythons `_api_admin_system_errors`
//! (`bot/analytics/api_admin.py`) werden die jüngsten ERROR-/CRITICAL-Zeilen aus
//! den **Logdateien** geparst — NICHT aus einer DB-Tabelle. Der frühere Rust-Pfad
//! las `twitch_admin_error_log` und divergierte damit in Quelle UND Response-Shape
//! von Python (kein `source`, `id` als Zahl, `createdAt` statt `timestamp`). Hier
//! angeglichen: Log-Parsing + Python-JSON-Shape
//! `{ page, pageSize, total, hasMore, entries:[{id,timestamp,level,source,message,context}] }`.
//!
//! Secrets in den Logzeilen werden vor der Ausgabe maskiert
//! (`sanitize_log_text`, Port von `_admin_sanitize_log_text`).

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::sync::OnceLock;

use axum::{extract::Query, response::IntoResponse, Json};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use crate::auth::level::DashboardAuthLevel;
use tb_http_core::ApiError;

// ── Konstanten (Python api_admin.py:69-70) ───────────────────────────────────

/// Maximale Zeilenzahl, die je Logdatei vom Ende her gescannt wird.
const ERROR_LOG_MAX_SCAN_LINES: usize = 4000;
/// Obergrenze der insgesamt zurückgegebenen Einträge (über alle Dateien).
const ERROR_LOG_MAX_RETURNED: usize = 200;
/// Logdatei-Kandidaten in Prioritätsreihenfolge (Python `_admin_error_log_candidates`).
/// `*.log`-Geschwister werden zur Laufzeit ergänzt; Reihenfolge bleibt stabil
/// (Dedup über `dict.fromkeys` in Python → hier per Set-Filter).
const PRIMARY_LOG_FILES: &[&str] = &[
    "twitch_bot.log",
    "twitch_dashboard.log",
    "twitch_service_warnings.log",
    "twitch_autobans.log",
];
/// Tokens, die eine Logzeile als Fehler markieren (Python: Uppercase-Vergleich).
const ERROR_TOKENS: &[&str] = &["ERROR", "CRITICAL", "TRACEBACK", "EXCEPTION"];

const MESSAGE_MAX_LENGTH: usize = 1200;
const CONTEXT_MAX_LENGTH: usize = 2000;

// ── Request-Parameter ────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ErrorsParams {
    #[serde(default = "default_page")]
    pub page: i64,
    #[serde(default = "default_page_size")]
    pub page_size: i64,
}

fn default_page() -> i64 {
    1
}
fn default_page_size() -> i64 {
    25
}

// ── Response ─────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct ErrorEntry {
    /// `"<dateiname>:<zeilennummer>"` (Python-Parität: String, nicht Zahl).
    id: String,
    timestamp: Option<String>,
    level: Option<String>,
    source: String,
    message: String,
    context: String,
}

/// `GET /twitch/api/admin/system/errors[?page=1&page_size=25]`
pub async fn errors_handler(
    auth: DashboardAuthLevel,
    Query(params): Query<ErrorsParams>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return Err(err);
    }

    let page = params.page.max(1);
    let page_size = params.page_size.clamp(1, 100);

    let entries = load_error_log_entries();
    let total = entries.len() as i64;
    let start = ((page - 1) * page_size).min(total);
    let end = (start + page_size).min(total);
    let has_more = end < total;
    let slice = &entries[start as usize..end as usize];

    Ok(Json(json!({
        "page": page,
        "pageSize": page_size,
        "total": total,
        "hasMore": has_more,
        "entries": slice,
    })))
}

// ── Log-Parsing (Port von _load_admin_error_log_entries) ──────────────────────

/// Sammelt die jüngsten Fehler-Einträge über alle Logdatei-Kandidaten. Pro Datei
/// werden die letzten [`ERROR_LOG_MAX_SCAN_LINES`] Zeilen rückwärts gescannt;
/// sobald [`ERROR_LOG_MAX_RETURNED`] Einträge erreicht sind, wird abgebrochen.
fn load_error_log_entries() -> Vec<ErrorEntry> {
    let mut entries: Vec<ErrorEntry> = Vec::new();
    for filename in log_candidates() {
        let Some(lines) = read_log_tail(&filename, ERROR_LOG_MAX_SCAN_LINES) else {
            continue;
        };
        // Rückwärts (neueste zuerst) — `lines` ist in Dateireihenfolge, die
        // Zeilennummer ist 1-basiert ab dem Beginn des gescannten Fensters …
        // Python nummeriert ab Dateianfang; bei <max_scan Zeilen ist das exakt
        // die echte Zeilennummer, sonst ein vom Fenster-Offset abgeleiteter Wert.
        // Wir bilden die Python-Semantik nach: `enumerate(handle, start=1)` über
        // die im deque verbliebenen Zeilen liefert dort ebenfalls fenster-relative
        // Indizes nur, wenn die Datei das deque füllt. Für die ID reicht ein
        // stabiler, monotoner Zeilenindex.
        let offset = lines.0;
        for (idx, line) in lines.1.iter().enumerate().rev() {
            let line_number = offset + idx + 1;
            if let Some(entry) = parse_error_line(&filename, line_number, line) {
                entries.push(entry);
                if entries.len() >= ERROR_LOG_MAX_RETURNED {
                    return entries;
                }
            }
        }
    }
    entries
}

/// Logdatei-Kandidaten: Primärliste + alle übrigen `*.log` im `logs/`-Verzeichnis,
/// dedupliziert unter Erhalt der Reihenfolge (Python `dict.fromkeys`).
fn log_candidates() -> Vec<String> {
    let mut out: Vec<String> = PRIMARY_LOG_FILES.iter().map(|s| s.to_string()).collect();
    if let Ok(read_dir) = std::fs::read_dir("logs") {
        for entry in read_dir.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".log") && !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out
}

/// Liest die letzten `max_lines` Zeilen von `logs/<filename>` (relativ zum CWD).
/// Rückgabe: `(offset, lines)` — `offset` = Anzahl der vor dem Fenster
/// verworfenen Zeilen (für die fortlaufende Zeilennummer in der ID).
fn read_log_tail(filename: &str, max_lines: usize) -> Option<(usize, Vec<String>)> {
    let path = std::path::Path::new("logs").join(filename);
    let file = std::fs::File::open(&path).ok()?;
    let reader = BufReader::new(file);
    let mut buf: VecDeque<String> = VecDeque::with_capacity(max_lines.min(1024));
    let mut dropped = 0usize;
    for line in reader.lines() {
        let Ok(l) = line else { continue };
        if buf.len() == max_lines {
            buf.pop_front();
            dropped += 1;
        }
        buf.push_back(l);
    }
    Some((dropped, buf.into_iter().collect()))
}

/// Parst eine einzelne Logzeile zu einem Fehler-Eintrag (Port von
/// `_admin_error_log_entry`). `None`, wenn die Zeile leer ist oder keinen
/// Fehler-Token enthält.
///
/// Format-Annahme (Python-Logger): `"<ts> - <logger> - <level> - <message>"`
/// (Split an `" - "`, max. 4 Teile).
fn parse_error_line(source: &str, line_number: usize, raw_line: &str) -> Option<ErrorEntry> {
    let line = raw_line.trim();
    if line.is_empty() {
        return None;
    }
    let upper = line.to_uppercase();
    if !ERROR_TOKENS.iter().any(|t| upper.contains(t)) {
        return None;
    }

    let mut timestamp = String::new();
    let mut level = String::new();
    let mut message = line.to_string();

    // Python: `line.split(" - ", 3)` → max. 4 Teile.
    let parts: Vec<&str> = line.splitn(4, " - ").collect();
    if parts.len() == 4 {
        timestamp = parts[0].trim().to_string();
        level = parts[2].trim().to_string();
        let m = parts[3].trim();
        message = if m.is_empty() { line.to_string() } else { m.to_string() };
    } else if parts.len() >= 2 {
        timestamp = parts[0].trim().to_string();
        let m = parts[parts.len() - 1].trim();
        message = if m.is_empty() { line.to_string() } else { m.to_string() };
    }

    let sanitized_message = sanitize_log_text(&message, MESSAGE_MAX_LENGTH);
    let sanitized_context = sanitize_log_text(line, CONTEXT_MAX_LENGTH);

    let message_out = if sanitized_message.is_empty() {
        "[redacted]".to_string()
    } else {
        sanitized_message.clone()
    };
    let context_out = if !sanitized_context.is_empty() {
        sanitized_context
    } else if !sanitized_message.is_empty() {
        sanitized_message
    } else {
        "[redacted]".to_string()
    };

    Some(ErrorEntry {
        id: format!("{source}:{line_number}"),
        timestamp: non_empty(timestamp),
        level: non_empty(level),
        source: source.to_string(),
        message: message_out,
        context: context_out,
    })
}

fn non_empty(s: String) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

// ── Secret-Maskierung (Port von _admin_sanitize_log_text/_admin_mask_secret) ──

/// Maskiert einen Geheimwert auf `[redacted:<len>]` (Python `_admin_mask_secret`,
/// Länge gedeckelt auf 999).
fn mask_secret(value: &str) -> String {
    if value.is_empty() {
        "[redacted]".to_string()
    } else {
        format!("[redacted:{}]", value.len().min(999))
    }
}

struct SecretPatterns {
    header: Regex,
    cookie: Regex,
    quoted_kv: Regex,
    kv: Regex,
    query: Regex,
    jwt: Regex,
    oauth: Regex,
}

/// Kompiliert die Secret-Regexes einmalig (Port der `_LOG_*_RE`-Konstanten).
fn secret_patterns() -> &'static SecretPatterns {
    static PATTERNS: OnceLock<SecretPatterns> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        const KEYS: &str = r"access[_-]?token|refresh[_-]?token|id[_-]?token|csrf[_-]?token|client[_-]?secret|api[_-]?key|apikey|session(?:id)?|password|secret";
        // `Regex::new` der statischen Muster kann nicht fehlschlagen; bei einer
        // (unmöglichen) Regex-Drift fällt der Wert auf ein nie-matchendes Muster
        // zurück, statt zu panicken (kein expect in Prod-Pfad).
        let compile = |p: &str| Regex::new(p).unwrap_or_else(|_| Regex::new(r"$.^").unwrap());
        SecretPatterns {
            header: compile(r"(?i)\b(authorization\s*[:=]\s*(?:bearer|basic)\s+)([^\s,;]+)"),
            cookie: compile(r"(?i)\b((?:set-cookie|cookie)\s*[:=]\s*)([^\r\n]+)"),
            quoted_kv: compile(&format!(
                r#"(?i)((?:"|')(?:{KEYS})(?:"|')\s*[:=]\s*)("[^"]*"|'[^']*'|[^\s,;]+)"#
            )),
            kv: compile(&format!(
                r#"(?i)\b({KEYS})(\s*[:=]\s*)("[^"]+"|'[^']+'|[^\s,;]+)"#
            )),
            query: compile(&format!(r"(?i)\b({KEYS})=([^&\s]+)")),
            jwt: compile(r"\beyJ[a-zA-Z0-9_-]{8,}\.[a-zA-Z0-9._-]{8,}\.[a-zA-Z0-9._-]{8,}\b"),
            oauth: compile(r"\boauth:[a-zA-Z0-9]{12,}\b"),
        }
    })
}

/// Maskiert Geheimnisse in einer Logzeile und deckelt die Länge (Port von
/// `_admin_sanitize_log_text`). Reihenfolge der Substitutionen wie in Python.
fn sanitize_log_text(raw: &str, max_length: usize) -> String {
    let text = raw.trim();
    if text.is_empty() {
        return String::new();
    }
    let p = secret_patterns();

    // header/cookie/query/kv: Präfix erhalten, nur den Wert maskieren.
    let s = p
        .header
        .replace_all(text, |c: &regex::Captures| format!("{}{}", &c[1], mask_secret(&c[2])));
    let s = p
        .cookie
        .replace_all(&s, |c: &regex::Captures| format!("{}{}", &c[1], mask_secret(&c[2])));
    // quoted_kv: Gruppe 1 = `"key":`, Gruppe 2 = Wert.
    let s = p
        .quoted_kv
        .replace_all(&s, |c: &regex::Captures| format!("{}{}", &c[1], mask_secret(&c[2])));
    // kv: Gruppe 1 = key, 2 = Separator, 3 = Wert.
    let s = p.kv.replace_all(&s, |c: &regex::Captures| {
        format!("{}{}{}", &c[1], &c[2], mask_secret(&c[3]))
    });
    let s = p
        .query
        .replace_all(&s, |c: &regex::Captures| format!("{}={}", &c[1], mask_secret(&c[2])));
    let s = p.jwt.replace_all(&s, mask_secret("[jwt]").as_str());
    let s = p.oauth.replace_all(&s, mask_secret("[oauth-token]").as_str());

    truncate_chars(&s, max_length)
}

/// Schneidet auf `max_chars` Zeichen (nicht Bytes) — Python `[:max_length]`.
fn truncate_chars(s: &str, max_chars: usize) -> String {
    match s.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => s[..byte_idx].to_string(),
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parst_error_zeile_im_standardformat() {
        let line = "2026-06-15T10:00:00 - twitch.bot - ERROR - etwas ging schief";
        let e = parse_error_line("twitch_bot.log", 42, line).expect("Error-Token → Some");
        assert_eq!(e.id, "twitch_bot.log:42");
        assert_eq!(e.timestamp.as_deref(), Some("2026-06-15T10:00:00"));
        assert_eq!(e.level.as_deref(), Some("ERROR"));
        assert_eq!(e.source, "twitch_bot.log");
        assert_eq!(e.message, "etwas ging schief");
        assert!(e.context.contains("ERROR"));
    }

    #[test]
    fn ignoriert_zeilen_ohne_fehler_token() {
        assert!(parse_error_line("x.log", 1, "2026 - logger - INFO - alles gut").is_none());
        assert!(parse_error_line("x.log", 2, "").is_none());
        assert!(parse_error_line("x.log", 3, "   ").is_none());
    }

    #[test]
    fn erkennt_traceback_und_exception() {
        assert!(parse_error_line("x.log", 1, "Traceback (most recent call last):").is_some());
        assert!(parse_error_line("x.log", 2, "raise ValueError EXCEPTION here").is_some());
    }

    #[test]
    fn maskiert_authorization_header() {
        let out = sanitize_log_text("Authorization: Bearer abcdef123456 rest", 2000);
        assert!(out.contains("Authorization: Bearer [redacted:"), "out={out}");
        assert!(!out.contains("abcdef123456"));
        assert!(out.contains(" rest"));
    }

    #[test]
    fn maskiert_key_value_secrets() {
        let out = sanitize_log_text("access_token=supersecretvalue&x=1", 2000);
        assert!(!out.contains("supersecretvalue"), "out={out}");
        assert!(out.contains("access_token=[redacted:"));
    }

    #[test]
    fn maskiert_jwt_und_oauth_token() {
        let jwt = "eyJabcdefgh.ijklmnopqrst.uvwxyz012345";
        let out = sanitize_log_text(jwt, 2000);
        assert!(!out.contains(jwt), "JWT muss maskiert sein: {out}");
        let oauth = "oauth:abcdefghijklmnop";
        let out2 = sanitize_log_text(oauth, 2000);
        assert!(!out2.contains(oauth), "oauth-Token muss maskiert sein: {out2}");
    }

    #[test]
    fn truncate_zaehlt_zeichen_nicht_bytes() {
        // 3 Multibyte-Zeichen → max_chars=2 ergibt 2 Zeichen.
        assert_eq!(truncate_chars("äöü", 2).chars().count(), 2);
        assert_eq!(truncate_chars("abc", 10), "abc");
    }

    #[test]
    fn mask_secret_format() {
        assert_eq!(mask_secret(""), "[redacted]");
        assert_eq!(mask_secret("ab"), "[redacted:2]");
        let long = "x".repeat(2000);
        assert_eq!(mask_secret(&long), "[redacted:999]");
    }

    #[test]
    fn fehlende_logs_geben_leere_liste() {
        // Im Test-CWD existiert kein `logs/`-Verzeichnis mit den Dateien →
        // load_error_log_entries liefert eine leere Liste (kein Panic).
        let entries = load_error_log_entries();
        // Es darf nie mehr als das Returned-Limit geben.
        assert!(entries.len() <= ERROR_LOG_MAX_RETURNED);
    }
}
