//! Provision the pinned Brain dependency's real, empty PostgreSQL schema.
//! Baseline ownership: Deadlock-Bots/dl-central-db, not the Brain Rust crates.
//! No credentials, production access, copied DDL, data imports or offline fallback.
use std::{env, fs, path::Path, process::Command};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const BRAIN_REV: &str = "d8c34270868e129098e12243f53f5b52ee507b8b";
const SCHEMA_REV: &str = "2b62eee4bfca1ea185c2aae758fb1e8d99acaf77";
const BASE_MIGRATIONS: &[(&str, &str)] = &[
    (
        "0012_brain_knowledge_timeline.sql",
        "ff2ec4f6d4715918d96958b2022311335c09257f058f0cb366679a0c82a46fe9",
    ),
    (
        "0013_brain_insight_records.sql",
        "6e8601cb75e0296f7378305c6c399a2411a0d6c62f1b606074442c0c43f243eb",
    ),
    (
        "2026070410_brain_ingestion_tables.sql",
        "43c0b2bf974f74684c2f3e34a633449a2ccee261e5fae0054f6144ae261ad6f8",
    ),
];

fn check_brain_pin(lock: &str) -> Result<()> {
    let expected = format!("source = \"git+https://github.com/EarlySalty/Deadlock-Brain.git?rev={BRAIN_REV}#{BRAIN_REV}\"");
    let sources: Vec<_> = lock
        .lines()
        .filter(|line| line.starts_with("source = ") && line.contains("EarlySalty/Deadlock-Brain"))
        .collect();
    if sources.is_empty() || sources.iter().any(|line| *line != expected) {
        return Err(
            "Brain dependency changed or missing: review the schema pin before provisioning".into(),
        );
    }
    Ok(())
}

fn check_test_database(dsn: &str) -> Result<()> {
    // Deliberately a narrow allow-list, NOT a general PostgreSQL URI parser.
    // Query parameters (e.g. host=production), encoded hosts and service names
    // are rejected. psql also receives a fixed PGHOSTADDR below.
    let rest = dsn
        .strip_prefix("postgres://")
        .or_else(|| dsn.strip_prefix("postgresql://"))
        .ok_or("Only an explicit disposable PostgreSQL URI is accepted")?;
    if rest.contains(['?', '#', '%', '\\']) || rest.chars().any(char::is_whitespace) {
        return Err("PostgreSQL URI options and encoded connection targets are forbidden".into());
    }
    let (authority, database) = rest.split_once('/').ok_or("Missing test database name")?;
    if !matches!(database, "twitchbot_sqlx" | "sqlx_prepare") {
        return Err("Refusing a database other than twitchbot_sqlx or sqlx_prepare".into());
    }
    let host_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let (host, port) = host_port
        .split_once(':')
        .ok_or("An explicit test port is required")?;
    if !matches!(host, "127.0.0.1" | "localhost") || port.parse::<u16>().ok().is_none_or(|p| p == 0)
    {
        return Err("Only a loopback test database with a valid port is accepted".into());
    }
    Ok(())
}

fn download(repo: &str, rev: &str, relative: &str, checksum: &str, target: &Path) -> Result<()> {
    let url = format!("https://raw.githubusercontent.com/EarlySalty/{repo}/{rev}/{relative}");
    if !Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-time",
            "90",
            "--retry",
            "3",
            "--output",
        ])
        .arg(target)
        .arg(url)
        .status()?
        .success()
    {
        return Err(format!("Failed to download pinned schema file {relative}").into());
    }
    let output = Command::new("sha256sum").arg(target).output()?;
    let actual = String::from_utf8(output.stdout)?;
    if !output.status.success() || actual.split_whitespace().next() != Some(checksum) {
        return Err(format!("SHA-256 mismatch for {relative}; no SQL will be applied").into());
    }
    Ok(())
}

fn provision(lockfile: &Path, scratch: &Path, canonical_base: Option<&Path>) -> Result<()> {
    check_brain_pin(&fs::read_to_string(lockfile)?)?;
    let dsn = env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is required")?;
    check_test_database(&dsn)?;
    // create_dir (not create_dir_all) rejects pre-existing or symlink targets.
    let directory = scratch.join(format!("brain-schema-{}", std::process::id()));
    fs::create_dir(&directory)?;
    let mut paths = Vec::new();
    for (name, checksum) in BASE_MIGRATIONS {
        if let Some(base) = canonical_base {
            // Read the owner's original files locally without modifying or
            // publishing them. Only the exact pinned bytes are accepted.
            let path = base.join(name);
            let output = Command::new("sha256sum").arg(&path).output()?;
            let actual = String::from_utf8(output.stdout)?;
            if !output.status.success() || actual.split_whitespace().next() != Some(*checksum) {
                return Err(format!("Canonical migration checksum mismatch: {name}").into());
            }
            paths.push(path);
        } else {
            let path = directory.join(name);
            download(
                "Deadlock-Bots",
                SCHEMA_REV,
                &format!("rust/crates/dl-central-db/migrations/{name}"),
                checksum,
                &path,
            ).map_err(|_| "Canonical baseline is private. Brain must publish an approved schema export; see Brain PR #9 comment 5799482619. No token or offline fallback is permitted.")?;
            paths.push(path);
        }
    }
    for (name, checksum) in BRAIN_MIGRATIONS {
        let path = directory.join(name);
        download(
            "Deadlock-Brain",
            BRAIN_REV,
            &format!("scripts/migrations/{name}"),
            checksum,
            &path,
        )?;
        paths.push(path);
    }
    // All downloads are verified before the first statement. The original
    // migrations are applied whole and in order, atomically, without seed data.
    let mut psql = Command::new("psql");
    psql.arg("--dbname")
        .arg(dsn)
        .env("PGHOSTADDR", "127.0.0.1")
        .env("PGCONNECT_TIMEOUT", "5")
        .env_remove("PGSERVICE")
        .env_remove("PGSERVICEFILE")
        .env_remove("PGOPTIONS")
        .args([
            "--no-psqlrc",
            "--set=ON_ERROR_STOP=1",
            "--single-transaction",
        ]);
    for path in &paths {
        psql.arg("--file").arg(path);
    }
    // Database output stays in memory. Neither connection details nor rows
    // from an unexpected migration are forwarded into CI logs.
    let applied = psql.output()?;
    if !applied.status.success() {
        return Err(format!(
            "Canonical Brain schema provisioning failed (psql exit {:?})",
            applied.status.code()
        )
        .into());
    }
    println!("Canonical empty Brain schema: Brain {BRAIN_REV}, dl-central-db {SCHEMA_REV}");
    fs::remove_dir_all(directory)?;
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = env::args_os().collect();
    if !matches!(args.len(), 3 | 4) {
        return Err("Usage: brain-schema <rust/Cargo.lock> <existing scratch directory> [local canonical migration directory]".into());
    }
    provision(
        Path::new(&args[1]),
        Path::new(&args[2]),
        args.get(3).map(Path::new),
    )
}

// Digests are the original SQL at BRAIN_REV, not a locally reconstructed schema.
const BRAIN_MIGRATIONS: &[(&str, &str)] = &[
    (
        "2026-09-12-reasoner.sql",
        "937f79af9694c896ab96513311d822c522b0f9edbeeb5757611f8b9a7fb0c0e2",
    ),
    (
        "2026-09-16-population.sql",
        "3ca20e92fb2848f4bb4b01d8dc329df80c05703b7fe0d44d460d8242f59f4a82",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_lock_is_accepted() {
        let lock = format!("source = \"git+https://github.com/EarlySalty/Deadlock-Brain.git?rev={BRAIN_REV}#{BRAIN_REV}\"\n");
        assert!(check_brain_pin(&lock.repeat(3)).is_ok());
        assert!(check_brain_pin(&lock.replace(BRAIN_REV, "main")).is_err());
        assert!(check_brain_pin("").is_err());
        assert!(check_brain_pin(&(lock + "source = \"git+https://github.com/EarlySalty/Deadlock-Brain.git?branch=main#new\"\n")).is_err());
    }

    #[test]
    fn only_named_loopback_test_databases_are_accepted() {
        for dsn in [
            "postgres://postgres@127.0.0.1:5432/twitchbot_sqlx",
            "postgresql://postgres:sqlxprepare@localhost:32777/sqlx_prepare",
        ] {
            assert!(check_test_database(dsn).is_ok());
        }
    }

    #[test]
    fn production_and_connection_overrides_are_rejected() {
        for dsn in [
            "postgres://postgres@production:5432/twitchbot_sqlx",
            "postgres://postgres@127.0.0.1:5432/production",
            "postgres://postgres@127.0.0.1:5432/twitchbot_sqlx?host=production",
            "postgres://postgres@127.0.0.1:5432/twitchbot_sqlx#x",
            "postgres://postgres@localhost%2fremote:5432/sqlx_prepare",
            "postgres://postgres@localhost:0/sqlx_prepare",
            "postgres://postgres@localhost:65536/sqlx_prepare",
            "postgres://postgres@localhost/sqlx_prepare",
            "service=production",
            "",
        ] {
            assert!(check_test_database(dsn).is_err(), "accepted {dsn}");
        }
    }

    #[test]
    fn schema_sources_are_immutable_and_checksummed() {
        for rev in [BRAIN_REV, SCHEMA_REV] {
            assert_eq!(rev.len(), 40);
            assert!(rev.bytes().all(|b| b.is_ascii_hexdigit()));
        }
        for (name, digest) in BASE_MIGRATIONS.iter().chain(BRAIN_MIGRATIONS) {
            assert!(name.ends_with(".sql"));
            assert!(!name.contains('/'));
            assert_eq!(digest.len(), 64);
            assert!(digest.bytes().all(|b| b.is_ascii_hexdigit()));
        }
    }
}
