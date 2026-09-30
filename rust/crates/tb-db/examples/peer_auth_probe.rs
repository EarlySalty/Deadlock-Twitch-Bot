use std::{error::Error, time::Duration};

use tb_config::DbConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = DbConfig {
        // Deliberately omit a database username: PostgreSQL peer auth must use
        // the Unix account selected by SQLx's whoami-backed default.
        dsn: "postgresql:///twitch_analytics?host=/var/run/postgresql".to_owned(),
        pool_max: 1,
        acquire_timeout: Duration::from_secs(5),
        connect_timeout: Duration::from_secs(5),
    };
    let pool = tb_db::connect(&config).await?;
    let (user, database): (String, String) =
        sqlx::query_as("SELECT current_user, current_database()")
            .fetch_one(&pool)
            .await?;
    println!("peer probe connected: user={user} database={database}");
    pool.close().await;
    Ok(())
}
