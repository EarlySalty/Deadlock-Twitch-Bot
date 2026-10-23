use std::{ffi::OsString, path::PathBuf, process::Stdio};

use serde_json::{json, Map, Value};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command,
};

use crate::protocol::{Failure, FailureKind, Request, Success};

const PASSED_ENV: [&str; 5] = ["HOME", "USER", "PATH", "LANG", "XDG_RUNTIME_DIR"];

pub struct Cli {
    pub binary: PathBuf,
    pub work_dir: PathBuf,
    pub line_limit: usize,
    env: Vec<(&'static str, OsString)>,
}

impl Cli {
    pub fn new(binary: PathBuf, work_dir: PathBuf, line_limit: usize) -> Self {
        let env = PASSED_ENV
            .iter()
            .filter_map(|name| std::env::var_os(name).map(|value| (*name, value)))
            .collect();
        Self {
            binary,
            work_dir,
            line_limit,
            env,
        }
    }

    fn command(&self, request: &Request, schema: &str) -> Command {
        let mut command = Command::new(&self.binary);
        command
            .args([
                "-p",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--verbose",
                "--json-schema",
                schema,
                "--model",
                &request.model,
                "--system-prompt",
                &request.system,
                "--tools",
                "",
                "--strict-mcp-config",
                "--setting-sources",
                "project",
                "--disable-slash-commands",
                "--no-session-persistence",
            ])
            .current_dir(&self.work_dir)
            .env_clear()
            .envs(self.env.iter().map(|(name, value)| (*name, value)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        command
    }

    pub async fn run(&self, request: &Request) -> Result<Success, Failure> {
        let cli_error = |message| Failure::new(FailureKind::CliError, message);
        let schema = serde_json::to_string(&request.json_schema)
            .map_err(|_| Failure::new(FailureKind::InvalidRequest, "schema"))?;
        let mut input = serde_json::to_vec(&json!({
            "type": "user",
            "message": {"role": "user", "content": request.content},
        }))
        .map_err(|_| Failure::new(FailureKind::InvalidRequest, "content"))?;
        input.push(b'\n');
        let mut child = self
            .command(request, &schema)
            .spawn()
            .map_err(|_| cli_error("spawn_failed"))?;
        let mut stdin = child.stdin.take().ok_or_else(|| cli_error("no_stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| cli_error("no_stdout"))?;
        stdin
            .write_all(&input)
            .await
            .map_err(|_| cli_error("stdin_failed"))?;
        drop(stdin);
        let outcome = last_result(BufReader::new(stdout), self.line_limit, &request.model).await;
        let status = child.wait().await.map_err(|_| cli_error("wait_failed"))?;
        match outcome? {
            Some(result) => result,
            None if status.success() => Err(cli_error("no_result")),
            None => Err(cli_error("exit_failure")),
        }
    }
}

type ParsedResult = Result<Success, Failure>;

async fn last_result<R: AsyncBufRead + Unpin>(
    mut reader: R,
    limit: usize,
    model: &str,
) -> Result<Option<ParsedResult>, Failure> {
    let mut last = None;
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = (&mut reader)
            .take(limit as u64 + 1)
            .read_until(b'\n', &mut line)
            .await
            .map_err(|_| Failure::new(FailureKind::CliError, "stdout_failed"))?;
        if read == 0 {
            return Ok(last);
        }
        if line.len() > limit {
            return Err(Failure::new(FailureKind::CliError, "oversized_output"));
        }
        if let Some(result) = serde_json::from_slice::<Value>(&line)
            .ok()
            .and_then(|value| parse_result(&value, model))
        {
            last = Some(result);
        }
    }
}

fn mentions_limit(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    ["usage limit", "rate limit", "rate_limit", "limit reached"]
        .iter()
        .any(|needle| text.contains(needle))
}

fn served_model(usage: Option<&Map<String, Value>>, requested: &str) -> Option<String> {
    let usage = usage?;
    if usage.contains_key(requested) {
        return Some(requested.to_owned());
    }
    let mut keys = usage.keys();
    match (keys.next(), keys.next()) {
        (Some(only), None) => Some(only.clone()),
        _ => None,
    }
}

fn usage(result: &Value) -> Value {
    let usage = result.get("usage");
    let field = |name: &str| {
        usage
            .and_then(|usage| usage.get(name))
            .and_then(Value::as_u64)
            .map_or(Value::Null, Value::from)
    };
    json!({
        "input_tokens": field("input_tokens"),
        "output_tokens": field("output_tokens"),
        "cache_creation_input_tokens": field("cache_creation_input_tokens"),
        "cache_read_input_tokens": field("cache_read_input_tokens"),
    })
}

pub fn parse_result(line: &Value, requested_model: &str) -> Option<ParsedResult> {
    if line.get("type").and_then(Value::as_str) != Some("result") {
        return None;
    }
    let is_error = line.get("is_error").and_then(Value::as_bool) != Some(false);
    if is_error {
        let limited = line.get("api_error_status").and_then(Value::as_u64) == Some(429)
            || line
                .get("result")
                .and_then(Value::as_str)
                .is_some_and(mentions_limit);
        return Some(Err(if limited {
            Failure::new(FailureKind::Limit, "usage_limit")
        } else {
            Failure::new(FailureKind::CliError, "result_error")
        }));
    }
    if line.get("subtype").and_then(Value::as_str) != Some("success") {
        return Some(Err(Failure::new(
            FailureKind::CliError,
            "unexpected_subtype",
        )));
    }
    let Some(structured_output) = line.get("structured_output").filter(|v| !v.is_null()) else {
        return Some(Err(Failure::new(
            FailureKind::CliError,
            "missing_structured_output",
        )));
    };
    let Some(model) = served_model(
        line.get("modelUsage").and_then(Value::as_object),
        requested_model,
    ) else {
        return Some(Err(Failure::new(FailureKind::CliError, "unknown_model")));
    };
    Some(Ok(Success {
        model,
        structured_output: structured_output.clone(),
        usage: usage(line),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODEL: &str = "claude-opus-5-5";

    #[test]
    fn success_result_carries_structured_output_model_and_usage() {
        let line = json!({
            "type": "result", "subtype": "success", "is_error": false,
            "structured_output": {"title": "clip"},
            "result": "",
            "usage": {"input_tokens": 12, "output_tokens": 7, "cache_creation_input_tokens": 3, "cache_read_input_tokens": 0},
            "modelUsage": {MODEL: {"inputTokens": 12}}
        });
        let success = parse_result(&line, MODEL)
            .expect("result line")
            .expect("success");
        assert_eq!(success.model, MODEL);
        assert_eq!(success.structured_output, json!({"title": "clip"}));
        assert_eq!(
            success.usage,
            json!({"input_tokens": 12, "output_tokens": 7, "cache_creation_input_tokens": 3, "cache_read_input_tokens": 0})
        );
    }

    #[test]
    fn usage_limits_are_reported_as_limit() {
        for line in [
            json!({"type": "result", "subtype": "success", "is_error": true, "api_error_status": 429, "result": "API Error"}),
            json!({"type": "result", "subtype": "success", "is_error": true, "result": "Claude AI usage limit reached|1760000000"}),
        ] {
            assert_eq!(
                parse_result(&line, MODEL).expect("result line").err(),
                Some(Failure::new(FailureKind::Limit, "usage_limit"))
            );
        }
    }

    #[test]
    fn other_errors_and_incomplete_results_are_cli_errors() {
        for (line, message) in [
            (
                json!({"type": "result", "subtype": "error_during_execution", "is_error": true, "api_error_status": 500, "result": "boom"}),
                "result_error",
            ),
            (
                json!({"type": "result", "subtype": "error_max_structured_output_retries", "is_error": false}),
                "unexpected_subtype",
            ),
            (
                json!({"type": "result", "subtype": "success", "is_error": false, "modelUsage": {MODEL: {}}}),
                "missing_structured_output",
            ),
            (
                json!({"type": "result", "subtype": "success", "is_error": false, "structured_output": {}, "modelUsage": {"a": {}, "b": {}}}),
                "unknown_model",
            ),
        ] {
            assert_eq!(
                parse_result(&line, MODEL).expect("result line").err(),
                Some(Failure::new(FailureKind::CliError, message))
            );
        }
        assert!(parse_result(&json!({"type": "assistant"}), MODEL).is_none());
    }

    #[tokio::test]
    async fn the_last_result_line_wins_and_oversized_lines_fail() {
        let output = concat!(
            "{\"type\":\"system\",\"subtype\":\"init\"}\n",
            "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":true,\"result\":\"x\"}\n",
            "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"structured_output\":{\"a\":1},\"modelUsage\":{\"claude-opus-5-5\":{}}}\n"
        );
        let last = last_result(output.as_bytes(), 4096, MODEL)
            .await
            .expect("readable")
            .expect("result");
        assert_eq!(last.expect("success").structured_output, json!({"a": 1}));
        assert_eq!(
            last_result(output.as_bytes(), 16, MODEL).await.err(),
            Some(Failure::new(FailureKind::CliError, "oversized_output"))
        );
    }
}
