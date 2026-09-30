use serde::Deserialize;
use serde_json::json;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};

const BROKER: &str = "http://127.0.0.1:8770/internal/master/v1/discord/send-message";
const WINDOW_SECS: u64 = 900;

#[derive(Debug, Deserialize)]
struct Credential {
    user_id: i64,
    token: String,
}

fn credential_path() -> PathBuf {
    let directory = env::var("CREDENTIALS_DIRECTORY")
        .unwrap_or_else(|_| "/run/credentials/tb-category-collector-failure.service".into());
    PathBuf::from(directory).join("category-notify")
}

fn load_credential(path: &Path) -> Result<Credential, String> {
    let text = fs::read_to_string(path).map_err(|_| "notification credential missing".to_string())?;
    let data: Credential =
        serde_json::from_str(&text).map_err(|_| "invalid notification credential".to_string())?;
    if data.user_id <= 0 || data.token.is_empty() || data.token.contains('\n') || data.token.contains('\r')
    {
        return Err("invalid notification credential".into());
    }
    Ok(data)
}

fn incident_key(now: u64) -> String {
    format!("category-collector-failed-{}", now / WINDOW_SECS)
}

fn already_sent(state_dir: &Path, key: &str) -> bool {
    fs::read_to_string(state_dir.join("last-key"))
        .ok()
        .is_some_and(|saved| saved.trim() == key)
}

fn remember_sent(state_dir: &Path, key: &str) -> Result<(), String> {
    fs::create_dir_all(state_dir).map_err(|_| "notify state unavailable".to_string())?;
    let path = state_dir.join("last-key");
    fs::write(&path, key).map_err(|_| "notify state unavailable".to_string())
}

fn hostname() -> String {
    fs::read_to_string("/etc/hostname")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown-host".into())
}

fn send_message(credential: &Credential, key: &str, broker: &str) -> Result<(), String> {
    let content = format!(
        "Der anonyme Deadlock-Kategoriesammler auf {} ist ausgefallen. Bitte tb-category-collector.service prüfen. Kategorie- und Chatdaten können Lücken enthalten; der Collector hat keine Twitch-Sendefunktion.",
        hostname()
    );
    let payload = json!({
        "user_id": credential.user_id,
        "content": content,
        "idempotency_key": key,
    });
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "notification broker unavailable".to_string())?;
    let response = client
        .post(broker)
        .header("Content-Type", "application/json")
        .header("X-Internal-Token", credential.token.trim())
        .json(&payload)
        .send()
        .map_err(|_| "notification broker unavailable".to_string())?;
    if !response.status().is_success() {
        return Err("notification broker rejected request".into());
    }
    Ok(())
}

fn run(args: &[String]) -> Result<i32, String> {
    let credential = load_credential(&credential_path())?;
    if args == ["--check"] {
        println!("Category notification credential validated; no message sent.");
        return Ok(0);
    }
    if !args.is_empty() {
        return Err("unexpected arguments".into());
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "clock unavailable".to_string())?
        .as_secs();
    let key = incident_key(now);
    let state_dir = env::var("STATE_DIRECTORY")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp/tb-category-notify"));
    if already_sent(&state_dir, &key) {
        println!("Category collector failure already reported for this incident.");
        return Ok(0);
    }
    send_message(&credential, &key, BROKER)?;
    remember_sent(&state_dir, &key)?;
    println!("Category collector failure notification delivered to configured operator.");
    Ok(0)
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match run(&args) {
        Ok(0) => ExitCode::SUCCESS,
        Ok(_) => ExitCode::from(1),
        Err(message) => {
            eprintln!("Category failure notification unavailable; inspect service journal. {message}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    #[test]
    fn incident_key_is_stable_inside_the_window() {
        assert_eq!(incident_key(900), incident_key(1799));
        assert_ne!(incident_key(899), incident_key(900));
    }

    #[test]
    fn duplicate_key_is_suppressed() {
        let dir = tempfile::tempdir().unwrap();
        remember_sent(dir.path(), "category-collector-failed-1").unwrap();
        assert!(already_sent(dir.path(), "category-collector-failed-1"));
        assert!(!already_sent(dir.path(), "category-collector-failed-2"));
    }

    #[test]
    fn send_message_posts_json_to_loopback() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 512];
            loop {
                let read = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let text = String::from_utf8_lossy(&request);
            let lower = text.to_ascii_lowercase();
            assert!(lower.contains("post /internal/master/v1/discord/send-message"));
            assert!(lower.contains("x-internal-token: secret"));
            write!(
                stream,
                "HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            text.into_owned()
        });
        let credential = Credential {
            user_id: 42,
            token: "secret".into(),
        };
        send_message(
            &credential,
            "category-collector-failed-1",
            &format!("http://{addr}/internal/master/v1/discord/send-message"),
        )
        .unwrap();
        let body = server.join().unwrap();
        assert!(body.contains("idempotency_key"));
    }
}
