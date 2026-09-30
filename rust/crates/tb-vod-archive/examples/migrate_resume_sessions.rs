//! Einmaliger Cutover nach Anhalten der alten Archiv-Writer.
//! Zugriff ausschließlich über den vorhandenen Secret-Launcher, keine Dateien.
use std::time::Duration;

async fn run() -> Result<u64, ()> {
    if std::env::args().skip(1).collect::<Vec<_>>() != ["--apply"] {
        return Err(());
    }
    let dsn = std::env::var("TWITCH_ANALYTICS_DSN").map_err(|_| ())?;
    let cipher = tb_crypto::FieldCipher::from_env().map_err(|_| ())?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&dsn)
        .await
        .map_err(|_| ())?;
    let result = tb_vod_archive::store::migrate_resume_sessions(&pool, &cipher)
        .await
        .map_err(|_| ());
    pool.close().await;
    result
}

#[tokio::main]
async fn main() {
    match run().await {
        Ok(count) => println!(
            "{count} Upload-Sitzungen umgestellt. Offsets und Videozuordnungen sind erhalten."
        ),
        Err(()) => {
            eprintln!("Migration nicht abgeschlossen. --apply, angehaltene Alt-Writer, Datenbank und Schlüssel prüfen. Keine Sitzungsadressen ausgegeben.");
            std::process::exit(1);
        }
    }
}
