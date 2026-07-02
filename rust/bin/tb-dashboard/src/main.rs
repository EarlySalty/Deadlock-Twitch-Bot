//! Minimaler HTTP-Server für das Analytics-Dashboard.
//!
//! Port: `DASHBOARD_PORT`/`TWITCH_DASHBOARD_PORT` Env-Variable, Default 8765.
//! DSN:  `TWITCH_ANALYTICS_DSN` (via tb-config).
//!
//! **Nicht automatisch starten** — Start ist user-gated (erfordert echtes DSN).

use std::fs::{self, File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::net::SocketAddr;
use std::path::PathBuf;

use tb_config::Settings;
use tb_dashboard_api::build_router;

const DASHBOARD_SERVICE_PORT: u16 = 8765;
const MASTER_API_RESERVED_PORT: u16 = 8766;
const ROLE_DASHBOARD: &str = "dashboard";
const ROLE_TWITCH_WORKER: &str = "twitch_worker";

#[cfg(unix)]
const LOCK_EX: i32 = 2;
#[cfg(unix)]
const LOCK_NB: i32 = 4;
#[cfg(unix)]
const LOCK_UN: i32 = 8;

#[cfg(unix)]
extern "C" {
    fn flock(fd: std::os::raw::c_int, operation: std::os::raw::c_int) -> std::os::raw::c_int;
}

fn optional_env_bool(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(value) => {
            let raw = value.trim().to_lowercase();
            match raw.as_str() {
                "" => default,
                "1" | "true" | "yes" | "on" => true,
                "0" | "false" | "no" | "off" => false,
                _ => {
                    tracing::warn!(
                        setting = name,
                        value = %value,
                        default,
                        "Ungültiger optionaler Bool-Env-Wert; Default wird verwendet"
                    );
                    default
                }
            }
        }
        Err(_) => default,
    }
}

fn optional_env_u16(name: &str, default: u16) -> Option<u16> {
    match std::env::var(name) {
        Ok(value) if value.trim().is_empty() => None,
        Ok(value) => match value.trim().parse::<u16>() {
            Ok(parsed) if parsed > 0 => Some(parsed),
            _ => {
                tracing::warn!(
                    setting = name,
                    value = %value,
                    default,
                    "Ungültiger optionaler Port-Env-Wert; Default wird verwendet"
                );
                Some(default)
            }
        },
        Err(_) => None,
    }
}

fn dashboard_port_from_env() -> u16 {
    optional_env_u16("DASHBOARD_PORT", DASHBOARD_SERVICE_PORT)
        .or_else(|| optional_env_u16("TWITCH_DASHBOARD_PORT", DASHBOARD_SERVICE_PORT))
        .unwrap_or(DASHBOARD_SERVICE_PORT)
}

fn split_runtime_enforced() -> bool {
    if std::env::var("TWITCH_RUNTIME_ENFORCE")
        .ok()
        .is_some_and(|v| !v.trim().is_empty())
    {
        return optional_env_bool("TWITCH_RUNTIME_ENFORCE", true);
    }
    optional_env_bool("TWITCH_SPLIT_RUNTIME_ENFORCE", true)
}

fn resolve_runtime_role(raw: &str) -> String {
    match raw.trim().to_lowercase().replace('-', "_").as_str() {
        "bot" | "worker" | "twitch_worker" => ROLE_TWITCH_WORKER.to_string(),
        other => other.to_string(),
    }
}

fn runtime_role_from_env() -> String {
    let raw = std::env::var("TWITCH_RUNTIME_ROLE")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| {
            std::env::var("TWITCH_SPLIT_RUNTIME_ROLE")
                .ok()
                .filter(|v| !v.trim().is_empty())
        })
        .unwrap_or_default();
    resolve_runtime_role(&raw)
}

fn enforce_dashboard_runtime(role: Option<&str>, port: u16) -> Result<String, String> {
    let role = match role {
        Some(value) => resolve_runtime_role(value),
        None => runtime_role_from_env(),
    };
    if !split_runtime_enforced() {
        return Ok(role);
    }
    if role != ROLE_DASHBOARD {
        return Err(role_error_message(&role));
    }
    if port == MASTER_API_RESERVED_PORT {
        return Err(port_error_message(port));
    }
    Ok(role)
}

fn role_error_message(got_role: &str) -> String {
    const ALLOWED: [&str; 3] = ["master", "twitch_worker", "dashboard"];
    if got_role.is_empty() {
        return "Runtime hardening violation for dashboard_service: runtime role is missing. Set TWITCH_RUNTIME_ROLE=dashboard (or TWITCH_SPLIT_RUNTIME_ROLE=dashboard).".to_string();
    }
    if !ALLOWED.contains(&got_role) {
        return format!(
            "Runtime hardening violation for dashboard_service: unsupported runtime role '{got_role}'. Allowed roles: master, twitch_worker, dashboard."
        );
    }
    format!(
        "Runtime hardening violation for dashboard_service: expected role 'dashboard', got '{got_role}'."
    )
}

fn port_error_message(got_port: u16) -> String {
    debug_assert_eq!(got_port, MASTER_API_RESERVED_PORT);
    format!(
        "Runtime hardening violation for dashboard_service: port {got_port} is reserved for the master API service."
    )
}

struct RuntimePidLock {
    handle: File,
}

impl RuntimePidLock {
    fn acquire(service_name: &str, port: u16) -> Result<Self, String> {
        let lock_dir = runtime_lock_dir();
        fs::create_dir_all(&lock_dir)
            .map_err(|error| format!("Runtime-Lock-Verzeichnis nicht erstellbar: {error}"))?;
        let lock_path = lock_dir.join(format!("{service_name}-{port}.lock"));
        let pid_path = lock_dir.join(format!("{service_name}-{port}.pidlock"));
        let mut handle = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|error| {
                format!("Runtime-Lock-Datei nicht oeffenbar ({lock_path:?}): {error}")
            })?;
        acquire_file_lock(&handle).map_err(|error| {
            let owner = fs::read_to_string(&pid_path).unwrap_or_else(|_| String::new());
            if owner.trim().is_empty() {
                format!("dashboard_service runtime lock already held: path={lock_path:?}: {error}")
            } else {
                format!(
                    "dashboard_service runtime lock already held: path={lock_path:?}, owner={owner:?}: {error}"
                )
            }
        })?;
        let metadata = format!(
            "pid={}\nservice={service_name}\nport={port}\n",
            std::process::id()
        );
        if let Err(error) = handle.set_len(0) {
            release_file_lock(&handle);
            return Err(format!("Runtime-Lock-Metadaten nicht kuerzbar: {error}"));
        }
        if let Err(error) = handle.seek(SeekFrom::Start(0)) {
            release_file_lock(&handle);
            return Err(format!(
                "Runtime-Lock-Metadaten nicht positionierbar: {error}"
            ));
        }
        if let Err(error) = handle.write_all(metadata.as_bytes()) {
            release_file_lock(&handle);
            return Err(format!("Runtime-Lock-Metadaten nicht schreibbar: {error}"));
        }
        if let Err(error) = fs::write(&pid_path, metadata) {
            release_file_lock(&handle);
            return Err(format!("Runtime-PID-Metadaten nicht schreibbar: {error}"));
        }
        Ok(Self { handle })
    }
}

impl Drop for RuntimePidLock {
    fn drop(&mut self) {
        release_file_lock(&self.handle);
    }
}

fn runtime_lock_dir() -> PathBuf {
    std::env::var("TWITCH_RUNTIME_PID_LOCK_DIR")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/runtime/locks"))
}

#[cfg(unix)]
fn acquire_file_lock(handle: &File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;

    let rc = unsafe { flock(handle.as_raw_fd(), LOCK_EX | LOCK_NB) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(unix))]
fn acquire_file_lock(_handle: &File) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn release_file_lock(handle: &File) {
    use std::os::fd::AsRawFd;

    let rc = unsafe { flock(handle.as_raw_fd(), LOCK_UN) };
    if rc != 0 {
        tracing::warn!(error = %std::io::Error::last_os_error(), "Runtime-Lock konnte nicht geloest werden");
    }
}

#[cfg(not(unix))]
fn release_file_lock(_handle: &File) {}

fn spawn_affiliate_gutschrift_loop(pool: sqlx::PgPool) {
    tokio::spawn(async move {
        const INITIAL_DELAY: std::time::Duration = std::time::Duration::from_secs(20);
        const INTERVAL: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);
        let start = tokio::time::Instant::now() + INITIAL_DELAY;
        let mut tick = tokio::time::interval_at(start, INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            match tb_dashboard_api::handlers::admin_affiliate::run_pending_gutschriften_for_background(
                &pool,
            )
            .await
            {
                Ok(results) if !results.is_empty() => tracing::info!(
                    count = results.len(),
                    "affiliate.gutschrift.loop: Lauf abgeschlossen"
                ),
                Ok(_) => {}
                Err(error) => {
                    tracing::error!(%error, "affiliate.gutschrift.loop: Lauf fehlgeschlagen")
                }
            }
        }
    });
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let settings = Settings::from_env().unwrap_or_else(|e| {
        tracing::error!("Konfigurationsfehler: {e}");
        std::process::exit(1);
    });

    let pool = tb_db::connect(&settings.db).await.unwrap_or_else(|e| {
        tracing::error!("DB-Verbindungsfehler: {e}");
        std::process::exit(1);
    });

    // Native sqlx-Migrationen anwenden. Schema-/Migrationsfehler sind fatal:
    // mit kaputtem oder halb migriertem Schema darf das Dashboard nicht starten.
    if optional_env_bool("TB_DB_MIGRATE", true) {
        match tb_db::run_migrations(&pool).await {
            Ok(()) => tracing::info!("DB-Migrationen angewendet (oder bereits aktuell)"),
            Err(e) => {
                tracing::error!("DB-Migrationen fehlgeschlagen; Startup wird abgebrochen: {e}");
                std::process::exit(1);
            }
        }
    } else {
        tracing::warn!("DB-Migrationen deaktiviert (TB_DB_MIGRATE=0)");
    }

    // Startzeit-Timestamp so früh wie möglich setzen
    let _ = tb_dashboard_api::process_info::uptime_secs();

    let port: u16 = dashboard_port_from_env();

    match enforce_dashboard_runtime(None, port) {
        Ok(role) => {
            tracing::info!(runtime_role = %role, port, "Dashboard Runtime-Härtung bestanden");
        }
        Err(error) => {
            tracing::error!("Dashboard Runtime-Härtung verletzt: {error}");
            std::process::exit(1);
        }
    }
    let _dashboard_runtime_lock = RuntimePidLock::acquire("dashboard_service", port)
        .unwrap_or_else(|error| {
            tracing::error!("Dashboard Runtime-PID-Lock verletzt: {error}");
            std::process::exit(1);
        });

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let token = settings.internal_api.token.clone();
    let readiness_fingerprint = tb_dashboard_api::analytics_db_fingerprint_startup_check().await;
    spawn_affiliate_gutschrift_loop(pool.clone());
    let mut app = build_router(pool.clone(), token);
    app = app.layer(axum::Extension(readiness_fingerprint));

    // Welle D: Strangler-Fallback-Proxy → Python (8765) für noch nicht
    // portierte Dashboard-Routen. Ohne konfigurierte URL bleibt der Proxy
    // aus und unbekannte Pfade antworten wie bisher mit 404.
    let fallback_url = std::env::var("TB_DASHBOARD_LEGACY_FALLBACK_URL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let proxy_ext = match &fallback_url {
        Some(url) => {
            app = app.fallback(tb_dashboard_api::proxy::dashboard_fallback_handler);
            tracing::info!("Strangler-Fallback-Proxy aktiv → {url}");
            tb_dashboard_api::proxy::DashboardProxyExt(Some(std::sync::Arc::new(
                tb_dashboard_api::proxy::DashboardLegacyProxy::new(url.clone()),
            )))
        }
        None => tb_dashboard_api::proxy::DashboardProxyExt(None),
    };
    app = app.layer(axum::Extension(proxy_ext));

    // Welle D: Session-Auth (Fernet) — Partner-/Admin-Level für native
    // v2-Routen. Ohne Key bleibt der Extractor fail-closed (Localhost/None).
    match tb_dashboard_api::DashboardAuthState::fernet_key_from_env() {
        Some(key) => {
            let auth_state = tb_dashboard_api::DashboardAuthState::new(pool.clone(), key);
            app = app.layer(axum::Extension(auth_state));
            tracing::info!("Dashboard-Session-Auth aktiv (Fernet-Key geladen)");
        }
        None => {
            tracing::warn!(
                "SESSIONS_ENCRYPTION_KEY fehlt — Partner-/Admin-Session-Auth deaktiviert"
            );
        }
    }

    // P0 (B3-2): Nativer Twitch-OAuth-Login. Ohne TWITCH_CLIENT_ID/SECRET +
    // TWITCH_DASHBOARD_AUTH_REDIRECT_URI bleibt er aus → /twitch/auth/* liefert
    // 503 (statt in den toten Python-Proxy zu fallen). Secrets aus Env (Infisical),
    // nie geloggt.
    match tb_dashboard_api::oauth_login_config_from_env() {
        Some(config) => {
            app = app.layer(axum::Extension(config));
            tracing::info!("Nativer Twitch-OAuth-Login aktiv");
        }
        None => {
            tracing::warn!(
                "Twitch-OAuth-Login-Config fehlt (TWITCH_CLIENT_ID/SECRET/REDIRECT_URI) — nativer Login deaktiviert"
            );
        }
    }

    // Native Discord-Admin-OAuth-Ausstellung für master_dash_session. Der eigentliche
    // Discord-Code-Tausch läuft wie in Python über den lokalen Broker; Secret-Werte
    // werden nur aus Env gelesen und nie geloggt.
    match tb_dashboard_api::discord_admin_login_config_from_env() {
        Some(config) => {
            app = app.layer(axum::Extension(config));
            tracing::info!("Nativer Discord-Admin-Login aktiv");
        }
        None => {
            tracing::warn!(
                "Discord-Admin-Login-Config fehlt (interner Broker-Token/Base) — nativer Admin-Login deaktiviert"
            );
        }
    }

    // P0 (B2): Nativer Stripe-Webhook (Quelle der Wahrheit fürs Bezahlt-Sein).
    // Ohne STRIPE_WEBHOOK_SECRET bleibt er aus → der Webhook-Pfad liefert 503
    // (statt in den toten Python-Proxy zu fallen). Secret aus Env (Infisical),
    // nie geloggt.
    match tb_dashboard_api::stripe_webhook_config_from_env() {
        Some(config) => {
            app = app.layer(axum::Extension(config));
            tracing::info!("Nativer Stripe-Webhook aktiv");
        }
        None => {
            tracing::warn!(
                "STRIPE_WEBHOOK_SECRET fehlt — nativer Stripe-Webhook deaktiviert (503)"
            );
        }
    }

    // P0 (B2-2A): Nativer Abo-/Billing-Bezahlpfad (Checkout/Cancel/Katalog).
    // Ohne STRIPE_SECRET_KEY bleibt der Stripe-Client aus → Checkout/Cancel
    // leiten auf die Pricing-Seite mit reason=... um (kein 500), Katalog/
    // Readiness melden checkout_ready=false. Secret aus Env (Infisical), nie geloggt.
    match tb_dashboard_api::billing_page_config_from_env() {
        Some(config) => {
            app = app.layer(axum::Extension(config));
            tracing::info!("Nativer Abo-/Billing-Bezahlpfad aktiv");
        }
        None => {
            tracing::warn!(
                "STRIPE_SECRET_KEY fehlt — nativer Checkout/Cancel deaktiviert (Redirect mit reason)"
            );
        }
    }

    tracing::info!("tb-dashboard lauscht auf {addr}");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| {
            tracing::error!("Bind-Fehler auf {addr}: {e}");
            std::process::exit(1);
        });
    if let Err(error) = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    {
        tracing::error!(%error, "Dashboard-Server beendet");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvGuard {
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn capture(names: &[&'static str]) -> Self {
            Self {
                saved: names
                    .iter()
                    .map(|name| (*name, std::env::var(name).ok()))
                    .collect(),
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (name, value) in &self.saved {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    #[test]
    fn dashboard_runtime_enforcement_contract() {
        let _guard = EnvGuard::capture(&[
            "TWITCH_RUNTIME_ENFORCE",
            "TWITCH_SPLIT_RUNTIME_ENFORCE",
            "TWITCH_RUNTIME_ROLE",
            "TWITCH_SPLIT_RUNTIME_ROLE",
        ]);
        std::env::set_var("TWITCH_RUNTIME_ENFORCE", "1");
        std::env::remove_var("TWITCH_SPLIT_RUNTIME_ENFORCE");
        std::env::remove_var("TWITCH_RUNTIME_ROLE");
        std::env::remove_var("TWITCH_SPLIT_RUNTIME_ROLE");

        assert_eq!(
            enforce_dashboard_runtime(Some(ROLE_DASHBOARD), 8769).as_deref(),
            Ok(ROLE_DASHBOARD)
        );
        assert_eq!(
            enforce_dashboard_runtime(Some(ROLE_DASHBOARD), DASHBOARD_SERVICE_PORT).as_deref(),
            Ok(ROLE_DASHBOARD)
        );

        let reserved_error =
            enforce_dashboard_runtime(Some(ROLE_DASHBOARD), MASTER_API_RESERVED_PORT).unwrap_err();
        assert!(reserved_error.contains("reserved for the master API service"));

        assert!(enforce_dashboard_runtime(Some("master"), DASHBOARD_SERVICE_PORT).is_err());
        assert!(enforce_dashboard_runtime(Some(""), DASHBOARD_SERVICE_PORT).is_err());

        std::env::set_var("TWITCH_RUNTIME_ROLE", ROLE_DASHBOARD);
        assert_eq!(
            enforce_dashboard_runtime(None, 8769).as_deref(),
            Ok(ROLE_DASHBOARD)
        );

        std::env::set_var("TWITCH_RUNTIME_ENFORCE", "0");
        assert_eq!(
            enforce_dashboard_runtime(Some("bot"), MASTER_API_RESERVED_PORT).as_deref(),
            Ok(ROLE_TWITCH_WORKER)
        );
    }
}
