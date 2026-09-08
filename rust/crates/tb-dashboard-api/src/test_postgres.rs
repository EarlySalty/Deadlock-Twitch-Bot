//! Tatsächliche isolierte PostgreSQL-Tests ohne ENV, Zugangsdaten oder Live-DB.
use std::{
    process::{Child, Command, Stdio},
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

static SLOTS: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub(crate) struct TestPostgres {
    pub pool: sqlx::PgPool,
    _process: Process,
    _directory: tempfile::TempDir,
    _slot: tokio::sync::OwnedSemaphorePermit,
}

impl TestPostgres {
    pub async fn start() -> Self {
        let slot = SLOTS
            .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2)))
            .clone()
            .acquire_owned()
            .await
            .expect("Testslot");
        let directory = tempfile::Builder::new()
            .prefix("uplink-api-pg-")
            .tempdir()
            .expect("Privates Testverzeichnis");
        let data = directory.path().join("data");
        let mut init = Process(
            Command::new("/usr/lib/postgresql/16/bin/initdb")
                .args(["--no-locale", "-A", "trust", "-U", "uplink_test", "-D"])
                .arg(&data)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("PostgreSQL16 initdb muss für diese Tests verfügbar sein"),
        );
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = init.0.try_wait().expect("initdb Status") {
                assert!(status.success(), "Isoliertes initdb fehlgeschlagen");
                break;
            }
            assert!(Instant::now() < deadline, "initdb überschreitet Startfrist");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let process = Process(
            Command::new("/usr/lib/postgresql/16/bin/postgres")
                .arg("-D")
                .arg(&data)
                .args(["-h", "", "-k"])
                .arg(directory.path())
                .args([
                    "-c",
                    "unix_socket_permissions=0700",
                    "-c",
                    "shared_buffers=16MB",
                    "-c",
                    "max_connections=12",
                    "-c",
                    "fsync=off",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("Isolierter PostgreSQL-Start"),
        );
        let options = sqlx::postgres::PgConnectOptions::new()
            .host(directory.path().to_str().unwrap())
            .username("uplink_test")
            .database("postgres");
        let deadline = Instant::now() + Duration::from_secs(10);
        let pool = loop {
            if let Ok(pool) = sqlx::postgres::PgPoolOptions::new()
                .max_connections(3)
                .acquire_timeout(Duration::from_secs(1))
                .connect_with(options.clone())
                .await
            {
                break pool;
            }
            assert!(
                Instant::now() < deadline,
                "Isolierter PostgreSQL nicht bereit"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        Self {
            pool,
            _process: process,
            _directory: directory,
            _slot: slot,
        }
    }
}
