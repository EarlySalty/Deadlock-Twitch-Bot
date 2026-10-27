use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool,
};
use std::{
    process::{Command, Stdio},
    time::Duration,
};
mod postmaster {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-support/postmaster.rs"
    ));
}
pub struct Database {
    pub pool: PgPool,
    _postmaster: postmaster::Postmaster,
    _directory: postmaster::RamDir,
}
impl Database {
    pub async fn new() -> Self {
        let directory = postmaster::RamDir::new("tb-dashboard");
        assert!(Command::new("/usr/lib/postgresql/16/bin/initdb")
            .args([
                "--auth=trust",
                "--username=dashboard_test",
                "--no-locale",
                "-D"
            ])
            .arg(directory.path().join("data"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success());
        let postmaster = postmaster::Postmaster::start(
            &directory.path().join("data"),
            directory.path(),
            &[],
            None,
        );
        let options = PgConnectOptions::new()
            .host(directory.path().to_str().unwrap())
            .username("dashboard_test")
            .database("postgres");
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .acquire_timeout(Duration::from_secs(5))
            .connect_lazy_with(options);
        let db = Self {
            pool,
            _postmaster: postmaster,
            _directory: directory,
        };
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if sqlx::query("SELECT 1").execute(&db.pool).await.is_ok() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        db
    }
    pub async fn close(self) {
        self.pool.close().await;
    }
}
