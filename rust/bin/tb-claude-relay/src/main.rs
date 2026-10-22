mod cli;
mod protocol;

use std::{
    os::unix::fs::{FileTypeExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::Semaphore,
};

use cli::Cli;
use protocol::{Failure, FailureKind, Request};

struct Args {
    socket: PathBuf,
    socket_group: String,
    claude_bin: PathBuf,
    work_dir: PathBuf,
    max_concurrency: usize,
    max_request_bytes: usize,
    max_timeout_ms: u64,
}

const FLAGS: [&str; 7] = [
    "--socket",
    "--socket-group",
    "--claude-bin",
    "--work-dir",
    "--max-concurrency",
    "--max-request-bytes",
    "--max-timeout-ms",
];

fn parse_args(raw: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut values: [Option<String>; 7] = Default::default();
    let mut raw = raw;
    while let Some(flag) = raw.next() {
        let index = FLAGS
            .iter()
            .position(|known| *known == flag)
            .ok_or_else(|| format!("unknown argument {flag}"))?;
        let value = raw.next().ok_or_else(|| format!("{flag} needs a value"))?;
        if values[index].replace(value).is_some() {
            return Err(format!("{flag} given twice"));
        }
    }
    let mut take = |index: usize| {
        values[index]
            .take()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("{} is required", FLAGS[index]))
    };
    let socket = PathBuf::from(take(0)?);
    let socket_group = take(1)?;
    let claude_bin = PathBuf::from(take(2)?);
    let work_dir = PathBuf::from(take(3)?);
    let positive = |name: &str, value: String| {
        value
            .parse::<u64>()
            .ok()
            .filter(|number| *number > 0)
            .ok_or_else(|| format!("{name} must be a positive integer"))
    };
    let max_concurrency = positive(FLAGS[4], take(4)?)?;
    let max_request_bytes = positive(FLAGS[5], take(5)?)?;
    let max_timeout_ms = positive(FLAGS[6], take(6)?)?;
    Ok(Args {
        socket,
        socket_group,
        claude_bin,
        work_dir,
        max_concurrency: usize::try_from(max_concurrency).map_err(|e| e.to_string())?,
        max_request_bytes: usize::try_from(max_request_bytes).map_err(|e| e.to_string())?,
        max_timeout_ms,
    })
}

fn bind(socket: &Path, group: &str) -> Result<UnixListener, String> {
    match std::fs::symlink_metadata(socket) {
        Ok(meta) if meta.file_type().is_socket() => {
            std::fs::remove_file(socket).map_err(|e| format!("remove stale socket: {e}"))?;
        }
        Ok(_) => return Err("socket path exists and is not a socket".to_owned()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("inspect socket: {error}")),
    }
    let listener = UnixListener::bind(socket).map_err(|e| format!("bind: {e}"))?;
    let gid = nix::unistd::Group::from_name(group)
        .map_err(|e| format!("lookup group: {e}"))?
        .ok_or_else(|| format!("unknown group {group}"))?
        .gid;
    nix::unistd::chown(socket, None, Some(gid)).map_err(|e| format!("chown: {e}"))?;
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o660))
        .map_err(|e| format!("chmod: {e}"))?;
    Ok(listener)
}

async fn read_request(
    stream: &mut BufReader<&mut UnixStream>,
    limit: usize,
) -> Result<Request, Failure> {
    let invalid = |message| Failure::new(FailureKind::InvalidRequest, message);
    let mut line = Vec::new();
    stream
        .take(limit as u64 + 1)
        .read_until(b'\n', &mut line)
        .await
        .map_err(|_| invalid("read_failed"))?;
    if line.len() > limit {
        return Err(invalid("too_large"));
    }
    if line.last() != Some(&b'\n') {
        return Err(invalid("unterminated"));
    }
    let request: Request = serde_json::from_slice(&line).map_err(|_| invalid("malformed"))?;
    if request.valid() {
        Ok(request)
    } else {
        Err(invalid("rejected"))
    }
}

struct Relay {
    cli: Cli,
    permits: Arc<Semaphore>,
    max_request_bytes: usize,
    max_timeout: Duration,
}

impl Relay {
    async fn respond(&self, stream: &mut UnixStream) -> serde_json::Value {
        let started = Instant::now();
        let mut reader = BufReader::new(stream);
        let request = match tokio::time::timeout(
            self.max_timeout,
            read_request(&mut reader, self.max_request_bytes),
        )
        .await
        {
            Ok(Ok(request)) => request,
            Ok(Err(failure)) => return failure.to_json(),
            Err(_) => return Failure::new(FailureKind::Timeout, "read_timeout").to_json(),
        };
        let Ok(_permit) = Arc::clone(&self.permits).try_acquire_owned() else {
            return Failure::new(FailureKind::Busy, "busy").to_json();
        };
        let limit = Duration::from_millis(request.timeout_ms).min(self.max_timeout);
        match tokio::time::timeout(limit, self.cli.run(&request)).await {
            Ok(Ok(success)) => {
                let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                success.to_json(elapsed)
            }
            Ok(Err(failure)) => failure.to_json(),
            Err(_) => Failure::new(FailureKind::Timeout, "cli_timeout").to_json(),
        }
    }

    async fn serve(&self, mut stream: UnixStream) {
        let reply = self.respond(&mut stream).await;
        let ok = reply.get("ok").and_then(serde_json::Value::as_bool) == Some(true);
        if ok {
            tracing::info!(kind = "ok", "relay request served");
        } else {
            tracing::warn!(
                kind = reply
                    .get("kind")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(""),
                reason = reply
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(""),
                "relay request failed"
            );
        }
        let Ok(mut line) = serde_json::to_vec(&reply) else {
            return;
        };
        line.push(b'\n');
        if stream.write_all(&line).await.is_err() || stream.shutdown().await.is_err() {
            tracing::warn!(kind = "write", reason = "client_gone", "relay reply lost");
        }
    }
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(error) => {
            tracing::error!(reason = %error, "invalid arguments");
            return std::process::ExitCode::from(2);
        }
    };
    let listener = match bind(&args.socket, &args.socket_group) {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(reason = %error, "socket setup failed");
            return std::process::ExitCode::FAILURE;
        }
    };
    let relay = Arc::new(Relay {
        cli: Cli::new(args.claude_bin, args.work_dir, args.max_request_bytes),
        permits: Arc::new(Semaphore::new(args.max_concurrency)),
        max_request_bytes: args.max_request_bytes,
        max_timeout: Duration::from_millis(args.max_timeout_ms),
    });
    tracing::info!(concurrency = args.max_concurrency, "relay listening");
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let relay = Arc::clone(&relay);
                tokio::spawn(async move { relay.serve(stream).await });
            }
            Err(_) => {
                tracing::warn!(kind = "accept", reason = "accept_failed", "accept failed");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Result<Args, String> {
        parse_args(list.iter().map(|s| (*s).to_owned()))
    }

    #[test]
    fn arguments_are_required_unique_and_positive() {
        let full = [
            "--socket",
            "/run/x.sock",
            "--socket-group",
            "g",
            "--claude-bin",
            "/bin/claude",
            "--work-dir",
            "/tmp",
            "--max-concurrency",
            "2",
            "--max-request-bytes",
            "1024",
            "--max-timeout-ms",
            "1000",
        ];
        let parsed = args(&full).expect("valid");
        assert_eq!(parsed.max_concurrency, 2);
        assert!(args(&full[..12]).is_err());
        let mut zero = full;
        zero[9] = "0";
        assert!(args(&zero).is_err());
        let mut twice = full.to_vec();
        twice.extend(["--socket", "/run/y.sock"]);
        assert!(args(&twice).is_err());
        let mut unknown = full.to_vec();
        unknown.extend(["--verbose", "1"]);
        assert!(args(&unknown).is_err());
    }
}
