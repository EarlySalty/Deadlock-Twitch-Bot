//! Wert-Normalisierung für Observability-Felder.
//!
//! Parität zu Pythons `RaidObservabilityService.normalize_value` /
//! `_analytics_observability_value` (`bot/raid/observability.py:84-108`,
//! `bot/analytics/mixin.py:425-447`): konvertiert beliebige JSON-Werte in einen
//! einzeiligen, längenbegrenzten String. Strings werden von CR/LF befreit und
//! getrimmt; alles andere wird als sortiertes, kompaktes JSON serialisiert.

use serde_json::Value;

/// Default-Längenlimit (Python `limit=240`).
pub const DEFAULT_VALUE_LIMIT: usize = 240;

/// Normalisiert einen Wert zu einem einzeiligen String mit Längenlimit.
///
/// - String-Werte: `\r`/`\n` → Space, dann `trim`.
/// - Alles andere: kompaktes, schlüssel-sortiertes JSON.
/// - Überschreitet das Ergebnis `limit` (in Zeichen), wird auf `limit` Zeichen
///   gekürzt und `...` angehängt (wie Python `text[:limit]`).
pub fn normalize_value(value: &Value, limit: usize) -> String {
    let text = match value {
        Value::String(s) => s.replace(['\r', '\n'], " ").trim().to_string(),
        other => compact_sorted_json(other),
    };
    truncate_with_ellipsis(&text, limit)
}

/// Kürzt `text` auf `limit` Zeichen (nicht Bytes) und hängt `...` an, wenn gekürzt
/// wurde. Parität zu Pythons `text[:limit]` (Zeichenbasiert).
fn truncate_with_ellipsis(text: &str, limit: usize) -> String {
    if text.chars().count() > limit {
        let head: String = text.chars().take(limit).collect();
        format!("{head}...")
    } else {
        text.to_string()
    }
}

/// Serialisiert einen JSON-Wert kompakt mit sortierten Objektschlüsseln und
/// ASCII-escaping (Parität zu Pythons `json.dumps(..., sort_keys=True,
/// ensure_ascii=True, separators=(",", ":"))`).
fn compact_sorted_json(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => json_ascii_string(s),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(compact_sorted_json).collect();
            format!("[{}]", parts.join(","))
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .into_iter()
                .map(|k| format!("{}:{}", json_ascii_string(k), compact_sorted_json(&map[k])))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
    }
}

/// JSON-String-Escaping mit ASCII-Garantie (Nicht-ASCII → `\uXXXX`).
fn json_ascii_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c if c.is_ascii() => out.push(c),
            c => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
    out
}

/// Formatiert ein sortiertes `key=value`-Feldset (Parität zu Pythons
/// `format_fields` / `_format_analytics_observability_fields`). `None`-Felder
/// (JSON `null`) werden ausgelassen; Schlüssel werden alphabetisch sortiert.
pub fn format_fields(fields: &[(&str, Value)]) -> String {
    let mut sorted: Vec<&(&str, Value)> = fields.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    sorted
        .into_iter()
        .filter(|(_, v)| !v.is_null())
        .map(|(k, v)| format!("{}={}", k.trim(), normalize_value(v, DEFAULT_VALUE_LIMIT)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Säubert einen Text für die Persistenz: CR/LF → Space, trim, Längenlimit
/// (Zeichenbasiert). Leerer Text → `None`. Parität zu Pythons
/// `_safe_observability_text` (`bot/storage/pg.py:208-212`).
pub fn safe_observability_text(value: &str, limit: usize) -> Option<String> {
    let text = value.replace(['\r', '\n'], " ");
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some(text.chars().take(limit).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalizes_string_strips_newlines_and_trims() {
        // \r und \n werden jeweils einzeln zu Space (Python-Parität) → zwei Spaces.
        assert_eq!(
            normalize_value(&json!("  a\r\nb  "), DEFAULT_VALUE_LIMIT),
            "a  b"
        );
    }

    #[test]
    fn normalizes_object_sorted_compact_json() {
        assert_eq!(
            normalize_value(&json!({"b": 1, "a": 2}), DEFAULT_VALUE_LIMIT),
            "{\"a\":2,\"b\":1}"
        );
    }

    #[test]
    fn truncates_by_chars_with_ellipsis() {
        let long = "x".repeat(300);
        let out = normalize_value(&json!(long), 240);
        assert_eq!(out.chars().count(), 243); // 240 + "..."
        assert!(out.ends_with("..."));
    }

    #[test]
    fn truncation_is_char_based_for_umlauts() {
        let long = "ä".repeat(300);
        let out = normalize_value(&json!(long), 240);
        assert!(out.ends_with("..."));
        assert_eq!(out.chars().count(), 243);
    }

    #[test]
    fn ascii_escapes_non_ascii_in_json() {
        // Umlaut innerhalb eines Objektwerts → \uXXXX
        let out = normalize_value(&json!({"k": "ä"}), DEFAULT_VALUE_LIMIT);
        assert_eq!(out, "{\"k\":\"\\u00e4\"}");
    }

    #[test]
    fn format_fields_sorts_and_skips_nulls() {
        let out = format_fields(&[
            ("zebra", json!("z")),
            ("alpha", json!(1)),
            ("skip", Value::Null),
        ]);
        assert_eq!(out, "alpha=1 zebra=z");
    }

    #[test]
    fn safe_text_trims_and_limits_and_nullifies_empty() {
        assert_eq!(safe_observability_text("  hi \n", 80).as_deref(), Some("hi"));
        assert_eq!(safe_observability_text("   ", 80), None);
        assert_eq!(
            safe_observability_text(&"a".repeat(100), 40).unwrap().len(),
            40
        );
    }
}
