include!(concat!(env!("OUT_DIR"), "/build_revision.rs"));

use chrono::NaiveDate;
use sqlx::postgres::PgPoolOptions;
use tb_analytics::category::storage::{self, ArchiveConfig, Error};
use uuid::Uuid;

fn day(args: &[String], at: usize) -> Result<NaiveDate, Error> {
    Ok(args.get(at).ok_or("UTC-Tag fehlt")?.parse()?)
}

#[tokio::main]
async fn main() {
    if print_build_revision() {
        return;
    }
    if let Err(error) = execute().await {
        eprintln!("Kategorie-Speicherlauf fehlgeschlagen: {error}");
        std::process::exit(1);
    }
}

async fn execute() -> Result<(), Error> {
    let input: Vec<String> = std::env::args().skip(1).collect();
    let testing = input.first().map(String::as_str) == Some("--test-database");
    let operator = input.first().map(String::as_str) == Some("--postgres-operator");
    let (pool, config, args) = if testing {
        let database = input
            .get(1)
            .ok_or("Name der synthetischen Datenbank fehlt")?;
        if !(database.starts_with("category_storage_") || database.starts_with("tb_storage_"))
            || !database
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err("Ungültiger synthetischer Datenbankname".into());
        }
        let options = sqlx::postgres::PgConnectOptions::new()
            .host("127.0.0.1")
            .port(33100)
            .username("postgres")
            .password("tbtest")
            .database(database);
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .map_err(|_| "Isolierte Testdatenbank nicht erreichbar")?;
        (pool, ArchiveConfig::default(), input[2..].to_vec())
    } else if operator {
        let account = nix::unistd::User::from_uid(nix::unistd::Uid::effective())?
            .ok_or("Betriebskonto fehlt")?;
        if account.name != "postgres" {
            return Err("Dieser Übergangspfad verlangt das Betriebskonto postgres".into());
        }
        let options = sqlx::postgres::PgConnectOptions::new()
            .host("/var/run/postgresql")
            .username("postgres")
            .database("twitch_analytics");
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .map_err(|_| "Betreiberdatenbank nicht erreichbar")?;
        (pool, ArchiveConfig::default(), input[1..].to_vec())
    } else {
        let (snapshot, args) = tb_config::runtime::start(std::env::args_os().skip(1))?;
        let settings = snapshot.runtime_settings(&|key| std::env::var(key).ok())?;
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&settings.db.dsn)
            .await
            .map_err(|_| "Datenbank nicht erreichbar")?;
        (
            pool,
            snapshot.settings().category_archive.clone(),
            args.iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect(),
        )
    };
    let command = args
        .first()
        .map(String::as_str)
        .ok_or("Unterbefehl fehlt: dry-run, backfill, finalize, archive, remove oder restore")?;
    let result = match command {
        "dry-run" if args.len() == 3 => {
            storage::dry_run(&pool, day(&args, 1)?, day(&args, 2)?).await?
        }
        "backfill" if args.len() == 3 => {
            let mut current = day(&args, 1)?;
            let end = day(&args, 2)?;
            if current > end {
                return Err("Ungültiger UTC-Zeitraum".into());
            }
            let mut result = Vec::new();
            while current <= end {
                result.push(storage::backfill(&pool, current).await?);
                storage::backfill_chat_metrics(&pool, current).await?;
                current = current.succ_opt().ok_or("Datumsüberlauf")?;
            }
            serde_json::json!(result)
        }
        "finalize" if args.get(1).map(String::as_str) == Some("--apply") && args.len() == 2 => {
            storage::finalize_chat_metrics(&pool).await?;
            serde_json::json!({"normalization":storage::finalize_normalization(&pool).await?})
        }
        "archive" | "remove" | "restore" if !testing && !operator => {
            let cipher = storage::archive_cipher(&|key| std::env::var(key).ok())?;
            let drive = storage::Drive::new(config.clone());
            match command {
                "archive" if args.len() == 4 && args[3] == "--apply" => serde_json::to_value(
                    storage::archive_day(&pool, day(&args, 1)?, &args[2], &config, &cipher, &drive)
                        .await?,
                )?,
                "remove" if args.len() == 3 && args[2] == "--apply" => {
                    serde_json::json!({"removed":storage::remove_local(&pool,args[1].parse::<Uuid>()?,&config,&cipher,&drive).await?})
                }
                "restore" if args.len() == 3 && args[2] == "--apply" => {
                    serde_json::json!({"processed":storage::restore(&pool,args[1].parse::<Uuid>()?,&config,&cipher,&drive).await?})
                }
                _ => {
                    return Err(
                        "Schreibender Archivlauf verlangt passende Argumente und --apply".into(),
                    )
                }
            }
        }
        _ => return Err("Ungültiger oder im Testmodus verbotener Aufruf".into()),
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    pool.close().await;
    Ok(())
}
