use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tb_config::stt::SttConfig;
use tokio::{process::Command, time::timeout};
use tracing::warn;

const SERVICE: &str = "deadlock-stt-server.service";
#[cfg(test)]
const SYSTEMD_FIXTURE_SERVICE: &str = "deadlock-stt-recovery-fixture.service";
#[cfg(test)]
const SYSTEMD_FIXTURE_PORT: u16 = 39_481;
const RECOVERY_FLAG: &str = "--recovery-check";
const STATE_FLAG: &str = "--recovery-state";
const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(3);
const BASE_BACKOFF_SECONDS: u64 = 5 * 60;
const MAX_BACKOFF_SECONDS: u64 = 6 * 60 * 60;
const WARNING_INTERVAL_SECONDS: u64 = 24 * 60 * 60;
const WARNING_WINDOW_SECONDS: u64 = 7 * 24 * 60 * 60;
const MAX_STATE_BYTES: u64 = 16 * 1024;
const MAX_FAILED_ATTEMPTS: u32 = 64;
const STARTUP_GRACE_SECONDS: u64 = 15 * 60;

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RecoveryState {
    last_attempt: Option<u64>,
    failed_attempts: u32,
    #[serde(default)]
    unhealthy_generation: Option<u64>,
    #[serde(default)]
    unhealthy_since: Option<u64>,
    warnings: Vec<u64>,
    suppressed_warnings: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct UnitSnapshot {
    active_enter_monotonic_usec: u64,
}

pub fn parse_arguments(arguments: &[OsString]) -> Result<Option<PathBuf>, String> {
    match arguments {
        [] => Ok(None),
        [mode, flag, path] if mode == RECOVERY_FLAG && flag == STATE_FLAG => {
            let path = PathBuf::from(path);
            if !path.is_absolute() {
                return Err("Der Recovery-Statepfad muss absolut sein.".to_owned());
            }
            Ok(Some(path))
        }
        _ => Err(
            "Der STT-Start akzeptiert ausschließlich --config oder den festen Recovery-Modus."
                .to_owned(),
        ),
    }
}

pub async fn run(config: &SttConfig, state_path: &Path) -> Result<(), String> {
    run_for_unit(config, state_path, SERVICE, STARTUP_GRACE_SECONDS).await
}

async fn run_for_unit(
    config: &SttConfig,
    state_path: &Path,
    service: &str,
    startup_grace_seconds: u64,
) -> Result<(), String> {
    if health_ok(&config.local_origin()).await {
        let _ = reset_backoff(state_path);
        return Ok(());
    }

    let now = unix_time();
    let Ok(mut state) = load_state(state_path) else {
        return Ok(());
    };
    let Some(unit_before_probe) = unit_snapshot_if_recoverable(service).await else {
        let _ = clear_startup_observation(state_path, &mut state);
        return Ok(());
    };
    if observe_generation(&mut state, unit_before_probe, now) {
        let _ = persist_state(state_path, &state);
        return Ok(());
    }
    if !startup_grace_elapsed(state.unhealthy_since, now, startup_grace_seconds)
        || !backoff_elapsed(&state, now)
    {
        return Ok(());
    }
    if health_ok(&config.local_origin()).await {
        let _ = reset_backoff(state_path);
        return Ok(());
    }
    let Some(unit_before_recovery) = unit_snapshot_if_recoverable(service).await else {
        let _ = clear_startup_observation(state_path, &mut state);
        return Ok(());
    };
    if observe_generation(&mut state, unit_before_recovery, unix_time()) {
        let _ = persist_state(state_path, &state);
        return Ok(());
    }
    if !startup_grace_elapsed(state.unhealthy_since, unix_time(), startup_grace_seconds) {
        return Ok(());
    }
    if health_ok(&config.local_origin()).await {
        let _ = reset_backoff(state_path);
        return Ok(());
    }
    let Some(unit_at_action) = unit_snapshot_if_recoverable(service).await else {
        let _ = clear_startup_observation(state_path, &mut state);
        return Ok(());
    };
    if observe_generation(&mut state, unit_at_action, unix_time()) {
        let _ = persist_state(state_path, &state);
        return Ok(());
    }
    if !startup_grace_elapsed(state.unhealthy_since, unix_time(), startup_grace_seconds) {
        return Ok(());
    }

    record_attempt(&mut state, now);
    let warning = warning_due(&mut state, now);
    if persist_state(state_path, &state).is_err() {
        return Ok(());
    }
    if let Some(suppressed) = warning {
        warn!(
            suppressed_repeats = suppressed,
            "STT-Healthprüfung fehlgeschlagen; begrenzter Recoveryversuch für aktive Unit"
        );
    }

    let _ = run_systemctl(&["--no-block", "try-restart", service]).await;
    Ok(())
}

async fn health_ok(origin: &str) -> bool {
    let Ok(client) = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(1))
        .timeout(HEALTH_TIMEOUT)
        .build()
    else {
        return false;
    };
    let Ok(result) = timeout(HEALTH_TIMEOUT, async {
        let mut response = client.get(format!("{origin}/health")).send().await.ok()?;
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|length| length > 4096)
        {
            return None;
        }
        let mut body = Vec::with_capacity(512);
        while let Some(chunk) = response.chunk().await.ok()? {
            if body.len().saturating_add(chunk.len()) > 4096 {
                return None;
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice::<HealthStatus>(&body)
            .ok()
            .map(|health| health.status == "ok")
    })
    .await
    else {
        return false;
    };
    result.unwrap_or(false)
}

#[derive(Deserialize)]
struct HealthStatus {
    status: String,
}

async fn unit_snapshot_if_recoverable(service: &str) -> Option<UnitSnapshot> {
    let enabled = run_systemctl(&["is-enabled", service]).await.ok();
    if !unit_enabled_state_allows_recovery(service, enabled.as_deref()) {
        return None;
    }
    let properties = run_systemctl(&[
        "show",
        "--property=ActiveState,ActiveEnterTimestampMonotonic",
        service,
    ])
    .await
    .ok();
    let properties = properties?;
    parse_unit_snapshot(&properties)
}

fn unit_enabled_state_allows_recovery(service: &str, enabled: Option<&str>) -> bool {
    if enabled_state_allows_recovery(enabled) {
        return true;
    }
    #[cfg(test)]
    {
        // Only the hard-coded transient fixture may bypass enablement; the
        // subsequent ActiveState snapshot still has to prove it is active.
        service == SYSTEMD_FIXTURE_SERVICE
    }
    #[cfg(not(test))]
    {
        let _ = service;
        false
    }
}

fn enabled_state_allows_recovery(enabled: Option<&str>) -> bool {
    enabled == Some("enabled")
}

fn parse_unit_snapshot(properties: &str) -> Option<UnitSnapshot> {
    let mut active = None;
    let mut started = None;
    for line in properties.lines() {
        if let Some(value) = line.strip_prefix("ActiveState=") {
            active = Some(value);
        } else if let Some(value) = line.strip_prefix("ActiveEnterTimestampMonotonic=") {
            started = value.parse::<u64>().ok();
        }
    }
    if active != Some("active") {
        return None;
    }
    started
        .filter(|timestamp| *timestamp > 0)
        .map(|active_enter_monotonic_usec| UnitSnapshot {
            active_enter_monotonic_usec,
        })
}

fn startup_grace_elapsed(unhealthy_since: Option<u64>, now: u64, grace_seconds: u64) -> bool {
    unhealthy_since.is_some_and(|since| now.saturating_sub(since) >= grace_seconds)
}

fn clear_startup_observation(path: &Path, state: &mut RecoveryState) -> Result<(), String> {
    if state.unhealthy_generation.is_none() && state.unhealthy_since.is_none() {
        return Ok(());
    }
    state.unhealthy_generation = None;
    state.unhealthy_since = None;
    persist_state(path, state)
}

fn observe_generation(state: &mut RecoveryState, snapshot: UnitSnapshot, now: u64) -> bool {
    if state.unhealthy_generation == Some(snapshot.active_enter_monotonic_usec) {
        return false;
    }
    state.unhealthy_generation = Some(snapshot.active_enter_monotonic_usec);
    state.unhealthy_since = Some(now);
    true
}

fn record_attempt(state: &mut RecoveryState, now: u64) {
    state.last_attempt = Some(now);
    state.failed_attempts = state
        .failed_attempts
        .saturating_add(1)
        .min(MAX_FAILED_ATTEMPTS);
}

async fn run_systemctl(arguments: &[&str]) -> Result<String, ()> {
    let command = Command::new("/usr/bin/systemctl")
        .arg("--user")
        .arg("--no-pager")
        .args(arguments)
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output();
    let Ok(Ok(output)) = timeout(COMMAND_TIMEOUT, command).await else {
        return Err(());
    };
    if !output.status.success() {
        return Err(());
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_owned())
        .map_err(|_| ())
}

fn load_state(path: &Path) -> Result<RecoveryState, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && metadata.len() <= MAX_STATE_BYTES => {
            let file = OpenOptions::new().read(true).open(path).map_err(|_| {
                "Die Recovery-State-Datei ist nicht lesbar; Recovery bleibt gesperrt.".to_owned()
            })?;
            let mut bytes = Vec::with_capacity(metadata.len() as usize);
            file.take(MAX_STATE_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| {
                    "Die Recovery-State-Datei ist nicht lesbar; Recovery bleibt gesperrt."
                        .to_owned()
                })?;
            if bytes.len() as u64 > MAX_STATE_BYTES {
                return Err(
                    "Die Recovery-State-Datei ist ungültig; Recovery bleibt gesperrt.".to_owned(),
                );
            }
            let state: RecoveryState = serde_json::from_slice(&bytes).map_err(|_| {
                "Die Recovery-State-Datei ist ungültig; Recovery bleibt gesperrt.".to_owned()
            })?;
            if state.warnings.len() > 2 || state.failed_attempts > MAX_FAILED_ATTEMPTS {
                return Err(
                    "Die Recovery-State-Datei enthält ungültige Grenzen; Recovery bleibt gesperrt."
                        .to_owned(),
                );
            }
            Ok(state)
        }
        Ok(_) => Err("Die Recovery-State-Datei ist ungültig; Recovery bleibt gesperrt.".to_owned()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(RecoveryState::default()),
        Err(_) => {
            Err("Die Recovery-State-Datei ist nicht lesbar; Recovery bleibt gesperrt.".to_owned())
        }
    }
}

fn persist_state(path: &Path, state: &RecoveryState) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Der Recovery-Statepfad hat kein Verzeichnis.".to_owned())?;
    let bytes = serde_json::to_vec(state)
        .map_err(|_| "Der Recovery-Status konnte nicht gespeichert werden.".to_owned())?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = path.with_extension(format!("tmp-{}-{nonce}", std::process::id()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|_| "Der Recovery-Status konnte nicht atomar angelegt werden.".to_owned())?;
    let result = file
        .write_all(&bytes)
        .and_then(|()| file.sync_all())
        .and_then(|()| fs::rename(&temporary, path))
        .and_then(|()| OpenOptions::new().read(true).open(parent)?.sync_all());
    if result.is_err() {
        let _ = fs::remove_file(temporary);
        return Err("Der Recovery-Status konnte nicht sicher gespeichert werden.".to_owned());
    }
    Ok(())
}

fn reset_backoff(path: &Path) -> Result<(), String> {
    let mut state = match load_state(path) {
        Ok(state) => state,
        Err(_) => return Ok(()),
    };
    reset_backoff_state(&mut state);
    persist_state(path, &state)
}

fn reset_backoff_state(state: &mut RecoveryState) {
    state.unhealthy_generation = None;
    state.unhealthy_since = None;
    if state.last_attempt.is_none() && state.failed_attempts == 0 {
        return;
    }
    state.last_attempt = None;
    state.failed_attempts = 0;
}

fn backoff_elapsed(state: &RecoveryState, now: u64) -> bool {
    let Some(last_attempt) = state.last_attempt else {
        return true;
    };
    let exponent = state.failed_attempts.saturating_sub(1).min(7);
    let delay = BASE_BACKOFF_SECONDS
        .saturating_mul(1_u64 << exponent)
        .min(MAX_BACKOFF_SECONDS);
    now.saturating_sub(last_attempt) >= delay
}

fn warning_due(state: &mut RecoveryState, now: u64) -> Option<u64> {
    state
        .warnings
        .retain(|timestamp| now.saturating_sub(*timestamp) < WARNING_WINDOW_SECONDS);
    let allowed = state.warnings.len() < 2
        && state
            .warnings
            .last()
            .is_none_or(|timestamp| now.saturating_sub(*timestamp) >= WARNING_INTERVAL_SECONDS);
    if !allowed {
        state.suppressed_warnings = state.suppressed_warnings.saturating_add(1);
        return None;
    }
    state.warnings.push(now);
    Some(std::mem::take(&mut state.suppressed_warnings))
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::{
        backoff_elapsed, enabled_state_allows_recovery, health_ok, load_state, observe_generation,
        parse_arguments, parse_unit_snapshot, record_attempt, reset_backoff_state, run_for_unit,
        startup_grace_elapsed, warning_due, RecoveryState, UnitSnapshot, MAX_FAILED_ATTEMPTS,
        STARTUP_GRACE_SECONDS, SYSTEMD_FIXTURE_PORT, SYSTEMD_FIXTURE_SERVICE,
    };
    use axum::{
        extract::State,
        http::StatusCode,
        response::IntoResponse,
        routing::{get, post},
        Json, Router,
    };
    use serde::Serialize;
    use std::{
        ffi::OsString,
        fs,
        net::Ipv4Addr,
        path::PathBuf,
        process::Stdio,
        sync::atomic::{AtomicU64, Ordering},
        sync::Arc,
        time::{Duration, SystemTime},
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        process::Command,
        time::timeout,
    };

    const FIXTURE_STARTUP_DELAY: Duration = Duration::from_secs(3);
    const FIXTURE_SLOW_REQUEST: Duration = Duration::from_secs(4);

    struct FixtureUnitGuard;

    impl Drop for FixtureUnitGuard {
        fn drop(&mut self) {
            let _ = std::process::Command::new("/usr/bin/systemctl")
                .arg("--user")
                .arg("--no-pager")
                .args([
                    "kill",
                    "--signal=SIGCONT",
                    "--kill-whom=main",
                    SYSTEMD_FIXTURE_SERVICE,
                ])
                .stdin(Stdio::null())
                .output();
            let _ = std::process::Command::new("/usr/bin/systemctl")
                .arg("--user")
                .arg("--no-pager")
                .args(["stop", SYSTEMD_FIXTURE_SERVICE])
                .stdin(Stdio::null())
                .output();
            let _ = std::process::Command::new("/usr/bin/systemctl")
                .arg("--user")
                .arg("--no-pager")
                .args(["reset-failed", SYSTEMD_FIXTURE_SERVICE])
                .stdin(Stdio::null())
                .output();
        }
    }

    #[derive(Clone)]
    struct SystemdFixtureState {
        started: tokio::time::Instant,
        slow_requests: Arc<AtomicU64>,
    }

    #[derive(Serialize)]
    struct FixtureHealth {
        status: &'static str,
    }

    async fn fixture_health(State(state): State<SystemdFixtureState>) -> impl IntoResponse {
        if state.started.elapsed() < FIXTURE_STARTUP_DELAY {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(FixtureHealth { status: "starting" }),
            );
        }
        (StatusCode::OK, Json(FixtureHealth { status: "ok" }))
    }

    async fn fixture_slow_transcription(
        State(state): State<SystemdFixtureState>,
    ) -> impl IntoResponse {
        state.slow_requests.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(FIXTURE_SLOW_REQUEST).await;
        (StatusCode::OK, "fixture-complete")
    }

    // This ignored test is executed by systemd-run as the fixture unit's main
    // process. Keeping /health inside that process makes SIGSTOP exercise the
    // same endpoint the recovery checker probes.
    #[tokio::test]
    #[ignore = "systemd-run fixture process entry; called only by the isolated recovery test"]
    async fn systemd_fixture_http_server_entry() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, SYSTEMD_FIXTURE_PORT))
            .await
            .expect("isolated fixture port must be free");
        let state = SystemdFixtureState {
            started: tokio::time::Instant::now(),
            slow_requests: Arc::new(AtomicU64::new(0)),
        };
        let app = Router::new()
            .route("/health", get(fixture_health))
            .route("/v1/audio/transcriptions", post(fixture_slow_transcription))
            .with_state(state);
        axum::serve(listener, app).await.unwrap();
    }

    #[test]
    fn recovery_mode_requires_exact_arguments_and_absolute_state_path() {
        assert_eq!(parse_arguments(&[]).unwrap(), None);
        let valid = [
            OsString::from("--recovery-check"),
            OsString::from("--recovery-state"),
            OsString::from("/var/lib/deadlock-stt-recovery/state.json"),
        ];
        assert_eq!(
            parse_arguments(&valid).unwrap(),
            Some(PathBuf::from("/var/lib/deadlock-stt-recovery/state.json"))
        );
        let relative = [
            OsString::from("--recovery-check"),
            OsString::from("--recovery-state"),
            OsString::from("state.json"),
        ];
        assert!(parse_arguments(&relative).is_err());
        assert!(parse_arguments(&valid[..2]).is_err());
    }

    #[test]
    fn backoff_grows_and_refuses_clock_rollback_bypass() {
        let state = RecoveryState {
            last_attempt: Some(10_000),
            failed_attempts: 3,
            ..RecoveryState::default()
        };
        assert!(!backoff_elapsed(&state, 11_199));
        assert!(backoff_elapsed(&state, 11_200));
        assert!(!backoff_elapsed(&state, 9_999));
    }

    #[test]
    fn warning_budget_coalesces_and_preserves_repeat_count() {
        let mut state = RecoveryState::default();
        assert_eq!(warning_due(&mut state, 100), Some(0));
        assert_eq!(warning_due(&mut state, 200), None);
        assert_eq!(warning_due(&mut state, 86_500), Some(1));
        assert_eq!(warning_due(&mut state, 172_900), None);
        assert_eq!(warning_due(&mut state, 604_900), Some(1));
    }

    #[test]
    fn healthy_reset_clears_backoff_but_keeps_warning_history() {
        let mut state = RecoveryState {
            last_attempt: Some(123),
            failed_attempts: 4,
            unhealthy_generation: Some(789),
            unhealthy_since: Some(100),
            warnings: vec![100, 200],
            suppressed_warnings: 9,
        };
        reset_backoff_state(&mut state);
        assert_eq!(state.last_attempt, None);
        assert_eq!(state.failed_attempts, 0);
        assert_eq!(state.unhealthy_generation, None);
        assert_eq!(state.unhealthy_since, None);
        assert_eq!(state.warnings, [100, 200]);
        assert_eq!(state.suppressed_warnings, 9);
    }

    #[test]
    fn stopped_or_disabled_unit_never_passes_recovery_guard() {
        assert!(enabled_state_allows_recovery(Some("enabled")));
        assert!(!enabled_state_allows_recovery(Some("disabled")));
        assert!(!enabled_state_allows_recovery(Some("static")));
        assert!(!enabled_state_allows_recovery(None));
        assert!(
            parse_unit_snapshot("ActiveState=active\nActiveEnterTimestampMonotonic=12345\n")
                .is_some()
        );
        assert!(
            parse_unit_snapshot("ActiveState=inactive\nActiveEnterTimestampMonotonic=12345\n")
                .is_none()
        );
        assert!(
            parse_unit_snapshot("ActiveState=active\nActiveEnterTimestampMonotonic=bad\n")
                .is_none()
        );
    }

    #[test]
    fn enabled_state_bypass_is_limited_to_the_exact_test_fixture() {
        assert!(super::unit_enabled_state_allows_recovery(
            SYSTEMD_FIXTURE_SERVICE,
            None
        ));
        assert!(!super::unit_enabled_state_allows_recovery(
            "deadlock-stt-server.service",
            Some("static")
        ));
        assert!(!super::unit_enabled_state_allows_recovery(
            "other.service",
            Some("transient")
        ));
    }

    #[test]
    fn admin_restart_restarts_startup_grace_without_erasing_backoff_or_warning_budget() {
        let old_generation = UnitSnapshot {
            active_enter_monotonic_usec: 100,
        };
        let restarted_generation = UnitSnapshot {
            active_enter_monotonic_usec: 200,
        };
        let mut state = RecoveryState {
            last_attempt: Some(50),
            failed_attempts: 64,
            warnings: vec![10, 20],
            suppressed_warnings: 7,
            ..RecoveryState::default()
        };
        assert!(observe_generation(&mut state, old_generation, 1_000));
        assert_eq!(state.last_attempt, Some(50));
        assert_eq!(state.failed_attempts, 64);
        assert_eq!(state.unhealthy_since, Some(1_000));
        assert!(!startup_grace_elapsed(
            state.unhealthy_since,
            1_000 + STARTUP_GRACE_SECONDS - 1,
            STARTUP_GRACE_SECONDS
        ));
        assert!(startup_grace_elapsed(
            state.unhealthy_since,
            1_000 + STARTUP_GRACE_SECONDS,
            STARTUP_GRACE_SECONDS
        ));
        assert!(observe_generation(&mut state, restarted_generation, 1_100));
        assert_eq!(state.unhealthy_since, Some(1_100));
        assert_eq!(state.last_attempt, Some(50));
        assert_eq!(state.failed_attempts, 64);
        assert!(!startup_grace_elapsed(
            state.unhealthy_since,
            1_100 + STARTUP_GRACE_SECONDS - 1,
            STARTUP_GRACE_SECONDS
        ));
        assert_eq!(state.warnings, [10, 20]);
        assert_eq!(state.suppressed_warnings, 7);
    }

    #[test]
    fn recovery_backoff_grows_across_its_own_unhealthy_restart_generations() {
        let generations = [
            UnitSnapshot {
                active_enter_monotonic_usec: 10,
            },
            UnitSnapshot {
                active_enter_monotonic_usec: 20,
            },
            UnitSnapshot {
                active_enter_monotonic_usec: 30,
            },
        ];
        let mut state = RecoveryState::default();
        assert!(observe_generation(&mut state, generations[0], 100));
        record_attempt(&mut state, 200);
        assert_eq!(state.failed_attempts, 1);
        assert!(observe_generation(&mut state, generations[1], 300));
        assert_eq!(state.failed_attempts, 1);
        assert!(!backoff_elapsed(&state, 200 + 5 * 60 - 1));
        assert!(backoff_elapsed(&state, 200 + 5 * 60));
        record_attempt(&mut state, 600);
        assert_eq!(state.failed_attempts, 2);
        assert!(observe_generation(&mut state, generations[2], 700));
        assert_eq!(state.failed_attempts, 2);
        assert!(!backoff_elapsed(&state, 600 + 10 * 60 - 1));
        assert!(backoff_elapsed(&state, 600 + 10 * 60));
    }

    #[test]
    fn capped_attempt_counter_stays_loadable_after_the_sixty_fourth_attempt() {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("tb-stt-recovery-cap-{nonce}.json"));
        let mut state = RecoveryState {
            failed_attempts: MAX_FAILED_ATTEMPTS,
            last_attempt: Some(10_000),
            warnings: vec![1, 2],
            ..RecoveryState::default()
        };
        record_attempt(&mut state, 10_001);
        assert_eq!(state.failed_attempts, MAX_FAILED_ATTEMPTS);
        super::persist_state(&path, &state).unwrap();
        let loaded = load_state(&path).unwrap();
        assert_eq!(loaded.failed_attempts, MAX_FAILED_ATTEMPTS);
        assert!(backoff_elapsed(&loaded, 10_001 + 6 * 60 * 60));
        let mut reset = loaded;
        reset_backoff_state(&mut reset);
        assert_eq!(reset.failed_attempts, 0);
        assert_eq!(reset.warnings, [1, 2]);
        fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn health_probe_rejects_redirect_and_never_contacts_redirect_target() {
        let health = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let health_addr = health.local_addr().unwrap();
        let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_addr = target.local_addr().unwrap();
        let fixture = tokio::spawn(async move {
            let (mut stream, _) = health.accept().await.unwrap();
            let mut request = [0; 1024];
            let _ = stream.read(&mut request).await.unwrap();
            let response = format!(
                "HTTP/1.1 302 Found\r\nLocation: http://{target_addr}/health\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        assert!(!health_ok(&format!("http://{health_addr}")).await);
        assert!(timeout(Duration::from_millis(100), target.accept())
            .await
            .is_err());
        fixture.await.unwrap();
    }

    #[tokio::test]
    #[ignore = "uses an isolated transient systemd user unit and SIGSTOP; run only with an assigned process-test slot"]
    async fn systemd_fixture_proves_sigstop_recovery_and_stopped_unit_guard() {
        let executable = std::env::current_exe().unwrap();
        let config = tb_config::stt::SttConfig {
            port: SYSTEMD_FIXTURE_PORT,
            ..Default::default()
        };
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let state_path = std::env::temp_dir().join(format!(
            "tb-stt-systemd-fixture-{}-{nonce}.json",
            std::process::id()
        ));

        let _ = fixture_systemctl(&["stop", SYSTEMD_FIXTURE_SERVICE]).await;
        let _ = fixture_systemctl(&["reset-failed", SYSTEMD_FIXTURE_SERVICE]).await;
        let start = Command::new("/usr/bin/systemd-run")
            .arg("--user")
            .arg(format!("--unit={SYSTEMD_FIXTURE_SERVICE}"))
            .arg("--property=Type=simple")
            .arg("--property=Restart=no")
            .arg("--property=TimeoutStopSec=2s")
            .arg(executable)
            .args([
                "--exact",
                "recovery::tests::systemd_fixture_http_server_entry",
                "--ignored",
                "--nocapture",
            ])
            .stdin(Stdio::null())
            .output();
        let output = timeout(Duration::from_secs(5), start)
            .await
            .expect("systemd-run should return promptly")
            .expect("systemd-run should execute");
        assert!(
            output.status.success(),
            "systemd-run failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let _fixture_guard = FixtureUnitGuard;

        let original_pid = wait_for_main_pid().await;
        assert_ne!(
            original_pid, 0,
            "fixture should have a running Rust process"
        );

        // The same Rust process initially answers 503, then becomes healthy.
        // Observing its first unhealthy generation must not restart it.
        assert!(!health_ok(&config.local_origin()).await);
        run_for_unit(&config, &state_path, SYSTEMD_FIXTURE_SERVICE, 10)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert_eq!(
            main_pid().await,
            original_pid,
            "startup grace restarted the fixture"
        );
        wait_for_health(&config.local_origin()).await;

        // A slow transcription request must not make the cheap health endpoint
        // fail or cause a recovery action.
        let transcription = tokio::spawn(async move {
            reqwest::Client::new()
                .post(format!("{}/v1/audio/transcriptions", config.local_origin()))
                .send()
                .await
                .unwrap()
        });
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(health_ok(&format!("http://127.0.0.1:{SYSTEMD_FIXTURE_PORT}")).await);
        run_for_unit(
            &tb_config::stt::SttConfig {
                port: SYSTEMD_FIXTURE_PORT,
                ..Default::default()
            },
            &state_path,
            SYSTEMD_FIXTURE_SERVICE,
            10,
        )
        .await
        .unwrap();
        assert_eq!(
            main_pid().await,
            original_pid,
            "healthy slow-request fixture restarted"
        );
        assert!(transcription.await.unwrap().status().is_success());

        // SIGSTOP targets the actual HTTP server process. Its endpoint must
        // stop responding before the recovery checker asks systemd to restart it.
        fixture_systemctl(&[
            "kill",
            "--signal=SIGSTOP",
            "--kill-whom=main",
            SYSTEMD_FIXTURE_SERVICE,
        ])
        .await
        .expect("SIGSTOP should target the isolated fixture main process");
        assert!(!health_ok(&format!("http://127.0.0.1:{SYSTEMD_FIXTURE_PORT}")).await);
        run_for_unit(
            &tb_config::stt::SttConfig {
                port: SYSTEMD_FIXTURE_PORT,
                ..Default::default()
            },
            &state_path,
            SYSTEMD_FIXTURE_SERVICE,
            0,
        )
        .await
        .unwrap();
        run_for_unit(
            &tb_config::stt::SttConfig {
                port: SYSTEMD_FIXTURE_PORT,
                ..Default::default()
            },
            &state_path,
            SYSTEMD_FIXTURE_SERVICE,
            0,
        )
        .await
        .unwrap();

        let recovered_pid = wait_for_new_main_pid(original_pid).await;
        assert_ne!(
            recovered_pid, original_pid,
            "try-restart should create a new process"
        );
        wait_for_health(&format!("http://127.0.0.1:{SYSTEMD_FIXTURE_PORT}")).await;

        // Administrative stop is respected even though the HTTP endpoint is down.
        fixture_systemctl(&["stop", SYSTEMD_FIXTURE_SERVICE])
            .await
            .expect("administrative fixture stop should succeed");
        run_for_unit(
            &tb_config::stt::SttConfig {
                port: SYSTEMD_FIXTURE_PORT,
                ..Default::default()
            },
            &state_path,
            SYSTEMD_FIXTURE_SERVICE,
            0,
        )
        .await
        .unwrap();
        assert_eq!(
            main_pid().await,
            0,
            "checker must not start an administratively stopped unit"
        );

        let _ = fs::remove_file(state_path);
    }

    async fn fixture_systemctl(arguments: &[&str]) -> Result<String, String> {
        let output = timeout(
            Duration::from_secs(5),
            Command::new("/usr/bin/systemctl")
                .arg("--user")
                .arg("--no-pager")
                .args(arguments)
                .stdin(Stdio::null())
                .output(),
        )
        .await
        .map_err(|_| "systemctl fixture command timed out".to_owned())?
        .map_err(|_| "systemctl fixture command could not run".to_owned())?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
        }
    }

    async fn main_pid() -> u32 {
        fixture_systemctl(&[
            "show",
            "--property=MainPID",
            "--value",
            SYSTEMD_FIXTURE_SERVICE,
        ])
        .await
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
    }

    async fn wait_for_main_pid() -> u32 {
        timeout(Duration::from_secs(10), async {
            loop {
                let pid = main_pid().await;
                if pid != 0 {
                    return pid;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("fixture should start")
    }

    async fn wait_for_new_main_pid(previous_pid: u32) -> u32 {
        timeout(Duration::from_secs(10), async {
            loop {
                let pid = main_pid().await;
                if pid != 0 && pid != previous_pid {
                    return pid;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("recovery should start a new fixture process")
    }

    async fn wait_for_health(origin: &str) {
        timeout(Duration::from_secs(10), async {
            while !health_ok(origin).await {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .expect("fixture HTTP health should become OK");
    }

    #[test]
    fn missing_state_is_fresh_but_corrupt_state_fails_closed() {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("tb-stt-recovery-{nonce}.json"));
        assert!(load_state(&path).unwrap().warnings.is_empty());
        fs::write(&path, b"{broken").unwrap();
        assert!(load_state(&path).is_err());
        fs::remove_file(path).unwrap();
    }
}
