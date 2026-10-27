use std::{
    process::{Child, Command, Stdio},
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

mod postmaster {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-support/postmaster.rs"
    ));
}

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
    _postmaster: postmaster::Postmaster,
    _directory: postmaster::RamDir,
    _slot: tokio::sync::OwnedSemaphorePermit,
}

impl TestPostgres {
    #[allow(dead_code)]
    pub async fn start() -> Self {
        Self::start_with_preload(false).await
    }

    #[allow(dead_code)]
    pub async fn start_with_timescaledb() -> Self {
        Self::start_with_preload(true).await
    }

    async fn start_with_preload(timescaledb: bool) -> Self {
        let slot = SLOTS
            .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2)))
            .clone()
            .acquire_owned()
            .await
            .expect("Testslot");
        let directory = postmaster::RamDir::new("tb");
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
        let postmaster = postmaster::Postmaster::start(
            &data,
            directory.path(),
            &[
                if timescaledb {
                    "shared_preload_libraries=timescaledb"
                } else {
                    "shared_preload_libraries="
                },
                "unix_socket_permissions=0700",
                "shared_buffers=16MB",
                "max_connections=32",
                "fsync=off",
            ],
            None,
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
            _postmaster: postmaster,
            _directory: directory,
            _slot: slot,
        }
    }
}
