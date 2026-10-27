use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub model: String,
    pub system: String,
    pub json_schema: Value,
    pub timeout_ms: u64,
    pub content: Vec<Value>,
}

impl Request {
    pub fn valid(&self) -> bool {
        !self.model.is_empty()
            && self.model.len() <= 80
            && self
                .model
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
            && !self.system.trim().is_empty()
            && self.json_schema.is_object()
            && self.timeout_ms > 0
            && !self.content.is_empty()
            && self.content.iter().all(block_valid)
    }
}

fn block_valid(block: &Value) -> bool {
    let Some(object) = block.as_object() else {
        return false;
    };
    match object.get("type").and_then(Value::as_str) {
        Some("text") => object.len() == 2 && object.get("text").is_some_and(Value::is_string),
        Some("image") => {
            object.len() == 2
                && object.get("source").is_some_and(|source| {
                    source.get("type").and_then(Value::as_str) == Some("base64")
                        && matches!(
                            source.get("media_type").and_then(Value::as_str),
                            Some("image/png" | "image/jpeg")
                        )
                        && source.get("data").is_some_and(Value::is_string)
                })
        }
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    Limit,
    Timeout,
    CliError,
    InvalidRequest,
    Busy,
}

impl FailureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Limit => "limit",
            Self::Timeout => "timeout",
            Self::CliError => "cli_error",
            Self::InvalidRequest => "invalid_request",
            Self::Busy => "busy",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Failure {
    pub kind: FailureKind,
    pub message: &'static str,
}

impl Failure {
    pub fn new(kind: FailureKind, message: &'static str) -> Self {
        Self { kind, message }
    }

    pub fn to_json(self) -> Value {
        json!({"ok": false, "kind": self.kind.as_str(), "message": self.message})
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Success {
    pub model: String,
    pub structured_output: Value,
    pub usage: Value,
}

impl Success {
    pub fn to_json(&self, duration_ms: u64) -> Value {
        json!({
            "ok": true,
            "model": self.model,
            "structured_output": self.structured_output,
            "usage": self.usage,
            "duration_ms": duration_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(content: Value) -> Request {
        serde_json::from_value(json!({
            "model": "claude-opus-5-5",
            "system": "system",
            "json_schema": {"type": "object"},
            "timeout_ms": 1000,
            "content": content,
        }))
        .expect("request")
    }

    #[test]
    fn only_api_style_text_and_base64_image_blocks_are_accepted() {
        assert!(request(json!([
            {"type": "text", "text": "Frame bei 1 ms"},
            {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "AA=="}}
        ]))
        .valid());
        for content in [
            json!([]),
            json!([{"type": "image", "source": {"type": "url", "url": "https://example.com"}}]),
            json!([{"type": "tool_use", "id": "x"}]),
            json!([{"type": "text", "text": "a", "cache_control": {}}]),
        ] {
            assert!(!request(content).valid());
        }
    }
}
