//! Hilfsfunktionen für die KI-Analyse-Endpunkte (`/twitch/api/v2/ai/*`).
//!
//! Port der reinen Parser aus `bot/analytics/api_ai.py`. Die LLM-Antworten sind
//! oft „schmutzig" (Markdown-Fences, Präambeln, abgeschnitten) — diese Funktionen
//! bergen das strukturierte JSON-Array robust. Der eigentliche Anthropic-/MiniMax-
//! Call + In-Memory-State + Persistenz folgen in späteren Slices.

use serde_json::Value;
use sqlx::PgPool;

/// KI-Modell-Kennungen (Python `AI_MODEL_OPUS`/`AI_MODEL_MINIMAX`).
pub const AI_MODEL_OPUS: &str = "opus";
pub const AI_MODEL_MINIMAX: &str = "minimax";

/// Echte Modellnamen (Python `CLAUDE_MODEL`/`MINIMAX_MODEL`), wie sie in
/// `ai_analyses.model` persistiert werden.
pub const CLAUDE_MODEL: &str = "claude-opus-4-6";
pub const MINIMAX_MODEL: &str = "MiniMax-M3";

/// Modellname für die Persistenz: `opus` → Claude, sonst MiniMax (1:1 Python
/// `CLAUDE_MODEL if ai_model == AI_MODEL_OPUS else MINIMAX_MODEL`).
pub fn model_name_for(ai_model: &str) -> &'static str {
    if ai_model == AI_MODEL_OPUS {
        CLAUDE_MODEL
    } else {
        MINIMAX_MODEL
    }
}

/// Reine Modellwahl aus Entitlements (Python `_plan_ai_model`-Logik):
/// `analytics.ai_full` → Opus, sonst `analytics.ai_mini` → MiniMax, sonst keins.
pub fn model_for_entitlements(entitlements: &[&str]) -> Option<&'static str> {
    if entitlements.contains(&"analytics.ai_full") {
        Some(AI_MODEL_OPUS)
    } else if entitlements.contains(&"analytics.ai_mini") {
        Some(AI_MODEL_MINIMAX)
    } else {
        None
    }
}

/// Plan-abhängiges KI-Modell eines Streamers (Python `_plan_ai_model`):
/// Plan-Snapshot (login-only) → Entitlements → Modellwahl.
pub async fn plan_ai_model(pool: &PgPool, streamer: &str) -> Result<Option<&'static str>, sqlx::Error> {
    let snapshot = crate::plan::resolve_plan_snapshot(pool, streamer, "").await?;
    Ok(model_for_entitlements(&snapshot.entitlements))
}

/// Extrahiert Text aus einer LLM-Antwort (Port von `_extract_text_response`).
/// String → getrimmt; Array von Content-Blocks → deren Text-Felder mit `\n`
/// verbunden + getrimmt (Claude `messages.content`); sonst best-effort.
pub fn extract_text_response(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.trim().to_string(),
        Value::Array(items) => {
            let mut parts: Vec<String> = Vec::new();
            for item in items {
                match item {
                    Value::String(s) => parts.push(s.clone()),
                    // dict-Block: type==text & text, sonst content (1:1 Python).
                    Value::Object(o) => {
                        let text = o.get("text").and_then(Value::as_str).filter(|s| !s.is_empty());
                        if o.get("type").and_then(Value::as_str) == Some("text") {
                            if let Some(t) = text {
                                parts.push(t.to_string());
                                continue;
                            }
                        }
                        if let Some(c) = o.get("content").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                            parts.push(c.to_string());
                        }
                    }
                    _ => {}
                }
            }
            parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join("\n").trim().to_string()
        }
        Value::Object(o) => match o.get("text").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            Some(t) => t.trim().to_string(),
            None => value.to_string().trim().to_string(),
        },
        other => other.to_string().trim().to_string(),
    }
}

/// Erstes vollständiges JSON-Array aus `text` (string-aware: `]` innerhalb von
/// Strings wird übersprungen). `None`, wenn das Array nicht terminiert ist
/// (abgeschnittene Antwort). Port von `_extract_json_array`.
pub fn extract_json_array(text: &str) -> Option<String> {
    let start = text.find('[')?;
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escape_next = false;
    for (i, ch) in text[start..].char_indices() {
        let byte = start + i;
        if escape_next {
            escape_next = false;
            continue;
        }
        if ch == '\\' && in_string {
            escape_next = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        if ch == '[' {
            depth += 1;
        } else if ch == ']' {
            depth -= 1;
            if depth == 0 {
                return Some(text[start..=byte].to_string());
            }
        }
    }
    None // abgeschnitten – kein passendes ]
}

/// Parst + birgt das strukturierte JSON-Array einer Modell-Antwort (Port von
/// `_parse_ai_analysis_points`). Drei Stufen: (1) Direktparse, (2) saubere
/// Bracket-Extraktion (Präambel/Trailing), (3) Truncation-Salvage (komplette
/// Objekte einsammeln). Liefert `[]`, wenn nichts Brauchbares gefunden wird.
pub fn parse_ai_analysis_points(raw: &str) -> Vec<Value> {
    let mut raw = raw.trim().to_string();

    // Markdown-Code-Fences entfernen.
    if raw.starts_with("```") {
        let kept: Vec<&str> = raw.lines().filter(|ln| !ln.trim().starts_with("```")).collect();
        raw = kept.join("\n").trim().to_string();
    }

    // 1) Direktparse – perfektes JSON.
    if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(&raw) {
        return arr;
    }

    // 2) Bracket-Extraktion – Präambel/Trailing + `]` in Strings.
    if let Some(extracted) = extract_json_array(&raw) {
        if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(&extracted) {
            return arr;
        }
    }

    // 3) Truncation-Salvage – komplette depth-1-Objekte einsammeln.
    if let Some(array_start) = raw.find('[') {
        let mut depth: i32 = 0;
        let mut in_string = false;
        let mut escape_next = false;
        let mut obj_start: Option<usize> = None;
        let mut salvaged: Vec<String> = Vec::new();
        for (i, ch) in raw[array_start..].char_indices() {
            let byte = array_start + i;
            if escape_next {
                escape_next = false;
                continue;
            }
            if ch == '\\' && in_string {
                escape_next = true;
                continue;
            }
            if ch == '"' {
                in_string = !in_string;
                continue;
            }
            if in_string {
                continue;
            }
            if ch == '{' {
                if depth == 0 {
                    obj_start = Some(byte);
                }
                depth += 1;
            } else if ch == '}' {
                depth -= 1;
                if depth == 0 {
                    if let Some(os) = obj_start.take() {
                        salvaged.push(raw[os..=byte].to_string());
                    }
                }
            } else if ch == ']' && depth == 0 {
                break;
            }
        }
        if !salvaged.is_empty() {
            let candidate = format!("[{}]", salvaged.join(","));
            if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(&candidate) {
                if !arr.is_empty() {
                    return arr;
                }
            }
        }
    }

    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_json_array_grundfaelle() {
        assert_eq!(extract_json_array("[1, 2, 3]").as_deref(), Some("[1, 2, 3]"));
        // Verschachtelt.
        assert_eq!(extract_json_array("pre [a, [b]] post").as_deref(), Some("[a, [b]]"));
        // `]` im String wird übersprungen.
        assert_eq!(extract_json_array(r#"["has ] bracket"]"#).as_deref(), Some(r#"["has ] bracket"]"#));
        // Abgeschnitten / keins.
        assert_eq!(extract_json_array("[1, 2"), None);
        assert_eq!(extract_json_array("kein array"), None);
    }

    #[test]
    fn parse_points_direkt_und_fence() {
        assert_eq!(parse_ai_analysis_points(r#"[{"a":1}]"#), vec![json!({"a": 1})]);
        // Markdown-Fence.
        assert_eq!(
            parse_ai_analysis_points("```json\n[{\"a\":1}]\n```"),
            vec![json!({"a": 1})]
        );
        // Präambel/Trailing → Stufe 2.
        assert_eq!(parse_ai_analysis_points(r#"Hier: [{"a":1}] fertig"#), vec![json!({"a": 1})]);
    }

    #[test]
    fn parse_points_nicht_array_und_salvage() {
        // Objekt statt Array → [].
        assert_eq!(parse_ai_analysis_points(r#"{"a":1}"#), Vec::<Value>::new());
        // Abgeschnitten → Salvage kompletter Objekte.
        assert_eq!(
            parse_ai_analysis_points(r#"[{"a":1}, {"b":2}, {"c":"#),
            vec![json!({"a": 1}), json!({"b": 2})]
        );
        // Gar nichts.
        assert_eq!(parse_ai_analysis_points("kaputt"), Vec::<Value>::new());
    }

    #[test]
    fn modellwahl_und_name() {
        // ai_full hat Vorrang vor ai_mini.
        assert_eq!(model_for_entitlements(&["analytics.ai_full"]), Some("opus"));
        assert_eq!(model_for_entitlements(&["analytics.ai_mini"]), Some("minimax"));
        assert_eq!(
            model_for_entitlements(&["analytics.ai_full", "analytics.ai_mini"]),
            Some("opus")
        );
        assert_eq!(model_for_entitlements(&["analytics.basic"]), None);
        assert_eq!(model_for_entitlements(&[]), None);
        // Persistenz-Modellname.
        assert_eq!(model_name_for("opus"), "claude-opus-4-6");
        assert_eq!(model_name_for("minimax"), "MiniMax-M3");
    }

    #[test]
    fn extract_text_response_faelle() {
        // String → getrimmt (MiniMax-Content).
        assert_eq!(extract_text_response(&json!("  hallo  ")), "hallo");
        // Claude content-Blocks → text-Felder mit \n.
        assert_eq!(
            extract_text_response(&json!([
                {"type": "text", "text": "Zeile 1"},
                {"type": "text", "text": "Zeile 2"}
            ])),
            "Zeile 1\nZeile 2"
        );
        // content-Fallback bei Nicht-text-Block.
        assert_eq!(
            extract_text_response(&json!([{"type": "tool", "content": "X"}])),
            "X"
        );
        // Null → leer.
        assert_eq!(extract_text_response(&Value::Null), "");
    }
}
