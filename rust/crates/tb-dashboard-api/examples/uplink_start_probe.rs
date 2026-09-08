//! FD9-Nachweis mit dem echten Launcher und dem Dashboard-Argumentvertrag.
//! Ohne Argumente ausschließlich synthetischer Infisical-Server auf Loopback.
use std::{
    io::{Seek, Write},
    os::fd::AsRawFd,
    process::Stdio,
    time::Duration,
};

use tb_dashboard_api::uplink_config;
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};

#[tokio::main]
async fn main() -> Result<(), &'static str> {
    if std::env::args_os().len() > 1 {
        let runtime = uplink_config::load_arguments(std::env::args_os().skip(1))
            .await?
            .ok_or("Uplink-Konfiguration fehlt.")?;
        uplink_config::install(runtime)?;
        return Ok(());
    }
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v4/secrets/"))
        .and(header("Authorization", "Bearer synthetic-bootstrap"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"secrets":[
                {"secretKey":"RS_RELAY_API_SECRET","secretValue":"synthetic-api"},
                {"secretKey":"RS_RELAY_ADMIN_SECRET","secretValue":"synthetic-admin"}
            ]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let descriptor = nix::sys::memfd::memfd_create(
        c"uplink-public-probe",
        nix::sys::memfd::MemFdCreateFlag::MFD_CLOEXEC,
    )
    .map_err(|_| "Test-memfd fehlt.")?;
    let mut credential = std::fs::File::from(descriptor);
    credential
        .write_all(b"synthetic-bootstrap\n")
        .map_err(|_| "Test-memfd nicht schreibbar.")?;
    credential
        .rewind()
        .map_err(|_| "Test-memfd nicht lesbar.")?;
    let directory = tempfile::tempdir().map_err(|_| "Testkonfigurationsordner fehlt.")?;
    let config = directory.path().join("uplink.json");
    std::fs::write(&config, serde_json::to_vec(&serde_json::json!({
        "relay_base_url":server.uri(),"infisical_base_url":server.uri(),"project_id":"public-probe",
        "environment":"test","secret_path":"/uplink","credential_fd":9
    })).map_err(|_| "Testkonfiguration ungültig.")?).map_err(|_| "Testkonfiguration nicht schreibbar.")?;

    let source = include_str!("../../../scripts/run_tb_dashboard_service.sh");
    let function = source
        .split("start_dashboard_with_uplink() {")
        .nth(1)
        .and_then(|body| body.split("\n}\n").next())
        .ok_or("Launcherfunktion fehlt.")?;
    let command = format!(
        "start_dashboard_with_uplink() {{{function}\n}}\nstart_dashboard_with_uplink \"$@\""
    );
    let mut child = tokio::process::Command::new("/bin/bash")
        .env_clear()
        .args(["-c", &command, "uplink-public-probe"])
        .arg(std::env::current_exe().map_err(|_| "Probeprogramm fehlt.")?)
        .arg(format!(
            "/proc/{}/fd/{}",
            std::process::id(),
            credential.as_raw_fd()
        ))
        .arg(config)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "Probeprozess startet nicht.")?;
    let status = match tokio::time::timeout(Duration::from_secs(15), child.wait()).await {
        Ok(status) => status.map_err(|_| "Probeprozess nicht abgewartet.")?,
        Err(_) => {
            child
                .kill()
                .await
                .map_err(|_| "Probeprozess konnte nicht beendet werden.")?;
            child
                .wait()
                .await
                .map_err(|_| "Probeprozess konnte nicht abgeholt werden.")?;
            return Err("Probe überschreitet ihre Frist.");
        }
    };
    if !status.success() {
        return Err("Rust-Argumentvertrag oder FD9-Anschluss fehlgeschlagen.");
    }
    server.verify().await;
    println!("PASS: bestehender Dashboard-Launcher → FD9 → echter Rust-Argumentvertrag → synthetisches Infisical; kein Secretoutput und keine Secretdatei.");
    Ok(())
}
