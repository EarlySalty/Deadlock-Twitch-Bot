#![allow(dead_code)]
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool,
};
use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
pub struct Database {
    pub pool: PgPool,
    child: Child,
    directory: PathBuf,
}
impl Database {
    pub async fn new() -> Self {
        let db = Self::empty().await;
        db.schema().await;
        db
    }
    pub async fn empty() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = PathBuf::from(format!(
            "/tmp/uat-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .unwrap();
        let mut init = Command::new("/usr/lib/postgresql/16/bin/initdb")
            .args(["--auth=trust", "--username=auth_test", "--no-locale", "-D"])
            .arg(directory.join("data"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = init.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            if Instant::now() >= deadline {
                let _ = init.kill();
                let _ = init.wait();
                panic!("Isolierte Testdatenbank konnte nicht initialisiert werden");
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
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
            .username("auth_test")
            .database("postgres");
        let pool = PgPoolOptions::new()
            .max_connections(6)
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
    async fn schema(&self) {
        sqlx::raw_sql("CREATE TABLE twitch_raid_auth (
            twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT,
            access_token TEXT, refresh_token TEXT,
            token_expires_at TIMESTAMPTZ, scopes TEXT, authorized_at TIMESTAMPTZ,
            raid_enabled BOOLEAN DEFAULT TRUE, needs_reauth BOOLEAN DEFAULT FALSE,
            reauth_notified_at TIMESTAMPTZ, last_refreshed_at TIMESTAMPTZ,
            access_token_enc BYTEA, refresh_token_enc BYTEA, enc_version INTEGER, enc_kid TEXT
        ); CREATE TABLE twitch_token_blacklist (twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT, error_message TEXT, error_count INTEGER DEFAULT 1, first_error_at TEXT, last_error_at TEXT, grace_expires_at TEXT, notified INTEGER DEFAULT 0); CREATE TABLE oauth_state_tokens (state_token TEXT PRIMARY KEY, platform TEXT, streamer_login TEXT, redirect_uri TEXT, pkce_verifier TEXT, expires_at TIMESTAMPTZ, created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP);
        CREATE TABLE twitch_partners (twitch_user_id TEXT, technical_pause_reason TEXT,
            manual_partner_opt_out INTEGER DEFAULT 0, raid_bot_enabled INTEGER DEFAULT 0);")
            .execute(&self.pool).await.unwrap();
        sqlx::raw_sql(include_str!(
            "../../../../migrations/20260908210000_twitch_uplink_intent.sql"
        ))
        .execute(&self.pool)
        .await
        .unwrap();
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
        let _ = fs::remove_dir_all(&self.directory);
    }
}
