//! Echte isolierte PostgreSQL-Instanz, ohne Secrets, ENV oder Produktionszugriff.
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool,
};
use std::{
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
pub struct Database {
    pub pool: PgPool,
    child: Child,
    directory: PathBuf,
}
impl Database {
    pub async fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = PathBuf::from(format!(
            "/tmp/tb-dashboard-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .unwrap();
        assert!(Command::new("/usr/lib/postgresql/16/bin/initdb")
            .args([
                "--auth=trust",
                "--username=dashboard_test",
                "--no-locale",
                "-D"
            ])
            .arg(directory.join("data"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success());
        let child = Command::new("/usr/lib/postgresql/16/bin/postgres")
            .arg("-D")
            .arg(directory.join("data"))
            .args(["-h", "", "-k"])
            .arg(&directory)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let options = PgConnectOptions::new()
            .host(directory.to_str().unwrap())
            .username("dashboard_test")
            .database("postgres");
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .acquire_timeout(Duration::from_secs(5))
            .connect_lazy_with(options);
        let db = Self {
            pool,
            child,
            directory,
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
    pub async fn close(mut self) {
        self.pool.close().await;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
