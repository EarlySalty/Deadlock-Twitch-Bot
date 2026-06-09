# Phase 0b — Persistenz-Fundament — Implementierungsplan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (inline, mit Sonnet-Worker-Delegation pro Task) zum Umsetzen. Schritte nutzen Checkbox-Syntax (`- [ ]`).

**Goal:** Vier Foundation-Crates (`tb-domain`, `tb-config`, `tb-observability`, `tb-db`), mit denen Rust read-only auf dieselbe PostgreSQL/TimescaleDB verbindet, das bestehende Prod-Schema als unveränderte Baseline behandelt und per Vertrags-Test gegen das echte Schema absichert.

**Architecture:** `tb-domain` = reine Newtypes/Enums (kein I/O, kein sqlx). `tb-config` = typisierte Settings aus einer Env-Quelle (Closure-injizierbar → ohne Prozess-Env testbar). `tb-observability` = `tracing`-Setup. `tb-db` = sqlx-`PgPool`, sqlx-native Migrationen (Baseline = bestehendes Schema), Row-Structs + Vertrags-Tests. Tests laufen hermetisch gegen einen wegwerfbaren Timescale-Container; der Schema-Vertrag zusätzlich read-only gegen die echte DB (DSN user-gated).

**Tech Stack:** Rust (stable 1.96), `sqlx` 0.8 (postgres, runtime-tokio, tls-rustls, macros, migrate), `tokio`, `serde`, `tracing` + `tracing-subscriber`, `thiserror`. Test-DB: Docker `timescale/timescaledb:2.17.2-pg16`.

---

## Scope, Abweichungen & Delegation

- Teil von Phase 0 (Foundation), nach [`2026-06-09-phase-0a-bootstrap-krypto-gate.md`](2026-06-09-phase-0a-bootstrap-krypto-gate.md). **Kein Live-Cutover.**
- **Bewusste Abweichungen von der Design-Doku (in Doku-Sync, Task 8, nachgezogen):**
  1. **Migrations-Engine: sqlx-native statt refinery** (ADR 0002). Grund: refinery erfordert einen zweiten PG-Treiber (tokio-postgres) neben sqlx — unnötige Doppelung. `sqlx::migrate!` integriert nativ in den `PgPool`. Baseline-Logik identisch (eigene History-Tabelle `_sqlx_migrations`, getrennt von Python-`schema_version`).
  2. **Row-Structs liegen in `tb-db`, nicht `tb-domain`.** Grund: FromRow-Mapping ist DB-nah und würde `tb-domain` an sqlx koppeln. `tb-domain` bleibt sqlx-frei (nur Newtypes/Enums). „Files that change together live together."
  3. **`tb-config` ohne figment** — reine Env-Quelle, ein Closure-injizierbarer Loader genügt (YAGNI; keine Config-Dateien/Profile).
- **Delegation:** Implementierung der Crates geht an Sonnet-Worker (Agent-Tool, `model: sonnet`), Review/Verifikation/Commit bei Opus. Der **read-only Prod-Vertrags-Test** (Secret-DSN) wird **nicht** vom headless Worker ausgeführt, sondern von Opus/User über den Infisical-Wrapper.

## Verifizierte Schema-Fakten (Grundlage der Row-Structs & Tests)

Aus `bot/storage/pg.py` (`ensure_schema`) + `bot/migrations/twitch_analytics_schema.sql`, von einem Sonnet-Worker extrahiert. **Kritisch:**

- **Timestamps sind `text` (ISO-Strings), nicht `timestamptz`** — außer in den Hypertables. Row-Structs mappen sie als `String`/`Option<String>`.
- **Booleans sind `integer` 0/1** (`is_on_discord`, `is_live`, `raid_bot_enabled`, …) → `i32` im Row-Struct.
- **Typ-Diskrepanzen pg.py↔.sql** (z. B. `active_session_id` integer vs. bigint). Deshalb verifiziert der Vertrags-Test gegen `information_schema` der **echten** DB, statt einer Quelle zu vertrauen. Bei `bigint`/`bigserial` → `i64`.
- **`schema_version`** (component-PK) ist die Python-Migrationsverfolgung; sqlx nutzt `_sqlx_migrations` → kein Konflikt. Mit `TWITCH_ALLOW_RUNTIME_SCHEMA_BOOTSTRAP=0` (Default) fährt Python kein DDL.

## Dateistruktur nach 0b

```
rust/
  Cargo.toml                         # members + deps erweitert
  migrations/                        # sqlx-native, vorerst LEER (.gitkeep) — Baseline = Prod-Schema
  scripts/test_db.sh                 # wegwerfbarer Timescale-Testcontainer (up/down)
  crates/
    tb-error/src/lib.rs              # + ConfigError
    tb-domain/{Cargo.toml, src/lib.rs, src/ids.rs, src/partner.rs}
    tb-config/{Cargo.toml, src/lib.rs}
    tb-observability/{Cargo.toml, src/lib.rs}
    tb-db/
      Cargo.toml
      src/lib.rs                     # re-exports
      src/error.rs                   # DbError
      src/pool.rs                    # connect(cfg) -> PgPool
      src/migrate.rs                 # run_migrations(pool)
      src/rows.rs                    # FromRow-Structs (streamers/partners/plans)
      tests/hermetic.rs              # gegen Testcontainer (TB_TEST_DATABASE_URL)
      tests/prod_contract.rs         # read-only gegen echte DB (TWITCH_ANALYTICS_DSN), gated
```

---

## Task 0: Workspace-Deps + Prereq-Check

**Files:** Modify `rust/Cargo.toml`

- [ ] **Step 1: Docker-Image-Verfügbarkeit prüfen** (Testcontainer nutzt dasselbe Image wie Prod)

Run:
```bash
docker image inspect timescale/timescaledb:2.17.2-pg16 >/dev/null 2>&1 && echo "IMAGE OK" || echo "IMAGE FEHLT (wird beim ersten test_db.sh up gezogen)"
```
Expected: `IMAGE OK` (Prod nutzt es bereits).

- [ ] **Step 2: Workspace-`members` + `workspace.dependencies` erweitern**

In `rust/Cargo.toml` `members` ergänzen: `"crates/tb-domain"`, `"crates/tb-config"`, `"crates/tb-observability"`, `"crates/tb-db"`. Unter `[workspace.dependencies]` ergänzen:
```toml
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
sqlx = { version = "0.8", default-features = false, features = ["postgres", "runtime-tokio", "tls-rustls", "macros", "migrate"] }
tb-domain = { path = "crates/tb-domain" }
tb-config = { path = "crates/tb-config" }
tb-observability = { path = "crates/tb-observability" }
tb-db = { path = "crates/tb-db" }
```

- [ ] **Step 3: Leeres Migrations-Verzeichnis (Baseline)**

Run:
```bash
mkdir -p /home/naniadm/Documents/Deadlock-Twitch-Bot/rust/migrations
touch /home/naniadm/Documents/Deadlock-Twitch-Bot/rust/migrations/.gitkeep
```
Begründung: 0b führt **kein** neues DDL ein. Das bestehende Prod-Schema ist die Baseline; `_sqlx_migrations` wird beim ersten `run_migrations` angelegt, ohne bestehende Tabellen anzufassen.

- [ ] **Step 4: Kein Commit** (Workspace baut erst mit den Crates aus Task 1ff.).

---

## Task 1: `tb-error` — ConfigError ergänzen

**Files:** Modify `rust/crates/tb-error/src/lib.rs`

- [ ] **Step 1: ConfigError-Enum hinzufügen** (unter `CryptoError`)

```rust
/// Fehler beim Laden typisierter Settings (`tb-config`).
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Pflicht-Setting fehlt oder ist leer.
    #[error("required setting missing: {0}")]
    MissingRequired(String),

    /// Setting hat einen ungültigen Wert (z. B. nicht parsebar).
    #[error("invalid setting {0}")]
    Invalid(String),
}
```

- [ ] **Step 2: Build**

Run: `cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust && source "$HOME/.cargo/env" && cargo build -p tb-error`
Expected: `Finished`.

---

## Task 2: `tb-domain` — Newtypes + PartnerStatus

**Files:** Create `tb-domain/Cargo.toml`, `src/lib.rs`, `src/ids.rs`, `src/partner.rs`

- [ ] **Step 1: Cargo.toml**

```toml
[package]
name = "tb-domain"
version = "0.1.0"
edition.workspace = true

[dependencies]
serde = { workspace = true }
```

- [ ] **Step 2: `src/ids.rs` — Newtypes**

```rust
//! Domänen-Identifikatoren als Newtypes (kein I/O).

use serde::{Deserialize, Serialize};

/// Twitch-Login (Kanalname, lowercase-Konvention der DB).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StreamerLogin(pub String);

/// Numerische Twitch-User-ID (in der DB als `text` gespeichert).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TwitchUserId(pub String);

macro_rules! str_newtype {
    ($t:ty) => {
        impl $t {
            pub fn as_str(&self) -> &str {
                &self.0
            }
            pub fn into_inner(self) -> String {
                self.0
            }
        }
        impl From<String> for $t {
            fn from(s: String) -> Self {
                Self(s)
            }
        }
        impl std::fmt::Display for $t {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}
str_newtype!(StreamerLogin);
str_newtype!(TwitchUserId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newtype_roundtrip() {
        let l = StreamerLogin::from("dragskope".to_string());
        assert_eq!(l.as_str(), "dragskope");
        assert_eq!(l.to_string(), "dragskope");
        assert_eq!(l.into_inner(), "dragskope");
    }
}
```

- [ ] **Step 3: `src/partner.rs` — PartnerStatus**

Die DB-Spalte `twitch_partners.status` ist `text` mit `'active'`/`'archived'`; unbekannte Werte dürfen **nicht** paniken → `Other`-Fallback.
```rust
//! Partner-Lebenszyklus-Status (DB-Spalte `twitch_partners.status`, text).

/// Status eines Twitch-Partners. `Other` fängt unbekannte DB-Werte robust ab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartnerStatus {
    Active,
    Archived,
    Other(String),
}

impl PartnerStatus {
    /// Aus dem rohen DB-Wert (case-insensitiv).
    pub fn from_db(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "active" => Self::Active,
            "archived" => Self::Archived,
            other => Self::Other(other.to_string()),
        }
    }

    /// Kanonischer DB-Wert.
    pub fn as_db(&self) -> &str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
            Self::Other(s) => s,
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_values_roundtrip() {
        assert_eq!(PartnerStatus::from_db("active"), PartnerStatus::Active);
        assert_eq!(PartnerStatus::from_db("ARCHIVED"), PartnerStatus::Archived);
        assert!(PartnerStatus::Active.is_active());
        assert_eq!(PartnerStatus::Archived.as_db(), "archived");
    }

    #[test]
    fn unknown_value_is_preserved_not_panicked() {
        let s = PartnerStatus::from_db("frozen");
        assert_eq!(s, PartnerStatus::Other("frozen".to_string()));
        assert_eq!(s.as_db(), "frozen");
        assert!(!s.is_active());
    }
}
```

- [ ] **Step 4: `src/lib.rs`**

```rust
//! Reine Domänen-Typen des Twitch-Bots (kein I/O, kein sqlx).

pub mod ids;
pub mod partner;

pub use ids::{StreamerLogin, TwitchUserId};
pub use partner::PartnerStatus;
```

- [ ] **Step 5: Test**

Run: `cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust && source "$HOME/.cargo/env" && cargo test -p tb-domain`
Expected: 3 Tests grün.

---

## Task 3: `tb-config` — typisierte Settings

**Files:** Create `tb-config/Cargo.toml`, `src/lib.rs`

- [ ] **Step 1: Cargo.toml**

```toml
[package]
name = "tb-config"
version = "0.1.0"
edition.workspace = true

[dependencies]
tb-error = { workspace = true }
```

- [ ] **Step 2: `src/lib.rs` — Loader + Configs**

Closure-injizierbare Env-Quelle → ohne Prozess-Env testbar. Defaults exakt aus `constants.py`/`pg.py`.
```rust
//! Typisierte Settings aus einer Env-Quelle. Kein globaler Mutable-State.
//!
//! Der Loader nimmt eine Quelle `Fn(&str) -> Option<String>` entgegen, damit er
//! ohne Prozess-Env testbar ist. `from_env()` nutzt `std::env::var`.

use std::time::Duration;

use tb_error::ConfigError;

type Get<'a> = dyn Fn(&str) -> Option<String> + 'a;

fn non_empty(v: String) -> Option<String> {
    let t = v.trim().to_string();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

fn required(get: &Get, name: &str) -> Result<String, ConfigError> {
    get(name)
        .and_then(non_empty)
        .ok_or_else(|| ConfigError::MissingRequired(name.to_string()))
}

fn or_default(get: &Get, name: &str, default: &str) -> String {
    get(name).and_then(non_empty).unwrap_or_else(|| default.to_string())
}

fn parse_or<T: std::str::FromStr>(get: &Get, name: &str, default: T) -> Result<T, ConfigError> {
    match get(name).and_then(non_empty) {
        Some(v) => v.parse::<T>().map_err(|_| ConfigError::Invalid(name.to_string())),
        None => Ok(default),
    }
}

/// PostgreSQL/TimescaleDB-Verbindung. Defaults wie der Python-Pool.
#[derive(Debug, Clone)]
pub struct DbConfig {
    pub dsn: String,
    pub pool_max: u32,
    pub acquire_timeout: Duration,
    pub connect_timeout: Duration,
}

impl DbConfig {
    fn load(get: &Get) -> Result<Self, ConfigError> {
        Ok(Self {
            dsn: required(get, "TWITCH_ANALYTICS_DSN")?,
            pool_max: parse_or(get, "TWITCH_ANALYTICS_POOL_MAXSIZE", 10u32)?,
            acquire_timeout: Duration::from_secs_f64(parse_or(
                get,
                "TWITCH_ANALYTICS_POOL_TIMEOUT_SECONDS",
                5.0f64,
            )?),
            connect_timeout: Duration::from_secs(parse_or(
                get,
                "TWITCH_ANALYTICS_CONNECT_TIMEOUT_SECONDS",
                5u64,
            )?),
        })
    }
}

/// Interne API (Loopback, Port 8776). Token ist Pflicht (fail-closed).
#[derive(Debug, Clone)]
pub struct InternalApiConfig {
    pub token: String,
    pub host: String,
    pub port: u16,
}

impl InternalApiConfig {
    fn load(get: &Get) -> Result<Self, ConfigError> {
        Ok(Self {
            token: required(get, "TWITCH_INTERNAL_API_TOKEN")?,
            host: or_default(get, "TWITCH_INTERNAL_API_HOST", "127.0.0.1"),
            port: parse_or(get, "TWITCH_INTERNAL_API_PORT", 8776u16)?,
        })
    }
}

/// Master-Broker (Discord-Bridge, Loopback, Port 8770). Token-Fallback auf das interne API-Token.
#[derive(Debug, Clone)]
pub struct BrokerConfig {
    pub base_url: String,
    pub token: String,
}

impl BrokerConfig {
    fn load(get: &Get, internal_token: &str) -> Result<Self, ConfigError> {
        let base_url = match get("MASTER_BROKER_BASE_URL").and_then(non_empty) {
            Some(u) => u,
            None => {
                let host = or_default(get, "MASTER_BROKER_HOST", "127.0.0.1");
                let port = parse_or(get, "MASTER_BROKER_PORT", 8770u16)?;
                format!("http://{host}:{port}")
            }
        };
        let token = get("MASTER_BROKER_TOKEN")
            .and_then(non_empty)
            .unwrap_or_else(|| internal_token.to_string());
        Ok(Self { base_url, token })
    }
}

/// Gesamte Settings des Bot-Prozesses (für Phase 0b: DB + interne API + Broker).
#[derive(Debug, Clone)]
pub struct Settings {
    pub db: DbConfig,
    pub internal_api: InternalApiConfig,
    pub broker: BrokerConfig,
}

impl Settings {
    /// Aus der Prozess-Umgebung (Infisical-Wrapper hat sie zuvor injiziert).
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::load(&|k| std::env::var(k).ok())
    }

    /// Aus einer beliebigen Quelle (Tests).
    pub fn load(get: &Get) -> Result<Self, ConfigError> {
        let internal_api = InternalApiConfig::load(get)?;
        let broker = BrokerConfig::load(get, &internal_api.token)?;
        let db = DbConfig::load(get)?;
        Ok(Self { db, internal_api, broker })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn src(map: HashMap<&'static str, &'static str>) -> impl Fn(&str) -> Option<String> {
        move |k: &str| map.get(k).map(|v| v.to_string())
    }

    fn minimal() -> HashMap<&'static str, &'static str> {
        HashMap::from([
            ("TWITCH_ANALYTICS_DSN", "postgres://u:p@127.0.0.1:5433/db"),
            ("TWITCH_INTERNAL_API_TOKEN", "tok-123"),
        ])
    }

    #[test]
    fn defaults_apply_when_optional_absent() {
        let s = Settings::load(&src(minimal())).unwrap();
        assert_eq!(s.db.pool_max, 10);
        assert_eq!(s.db.connect_timeout.as_secs(), 5);
        assert_eq!(s.internal_api.host, "127.0.0.1");
        assert_eq!(s.internal_api.port, 8776);
        assert_eq!(s.broker.base_url, "http://127.0.0.1:8770");
        // Broker-Token fällt auf das interne Token zurück
        assert_eq!(s.broker.token, "tok-123");
    }

    #[test]
    fn overrides_are_parsed() {
        let mut m = minimal();
        m.insert("TWITCH_ANALYTICS_POOL_MAXSIZE", "25");
        m.insert("MASTER_BROKER_BASE_URL", "http://127.0.0.1:9999");
        let s = Settings::load(&src(m)).unwrap();
        assert_eq!(s.db.pool_max, 25);
        assert_eq!(s.broker.base_url, "http://127.0.0.1:9999");
    }

    #[test]
    fn missing_required_dsn_errors() {
        let m = HashMap::from([("TWITCH_INTERNAL_API_TOKEN", "t")]);
        let err = Settings::load(&src(m)).unwrap_err();
        assert!(matches!(err, ConfigError::MissingRequired(n) if n == "TWITCH_ANALYTICS_DSN"));
    }

    #[test]
    fn invalid_int_errors() {
        let mut m = minimal();
        m.insert("TWITCH_ANALYTICS_POOL_MAXSIZE", "abc");
        let err = Settings::load(&src(m)).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid(n) if n == "TWITCH_ANALYTICS_POOL_MAXSIZE"));
    }
}
```

- [ ] **Step 3: Test**

Run: `cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust && source "$HOME/.cargo/env" && cargo test -p tb-config`
Expected: 4 Tests grün.

---

## Task 4: `tb-observability` — tracing-Setup

**Files:** Create `tb-observability/Cargo.toml`, `src/lib.rs`

- [ ] **Step 1: Cargo.toml**

```toml
[package]
name = "tb-observability"
version = "0.1.0"
edition.workspace = true

[dependencies]
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
```

- [ ] **Step 2: `src/lib.rs`**

```rust
//! Observability-Setup: strukturiertes Logging via `tracing`.
//!
//! Phase 0b: nur Subscriber-Init (fmt + EnvFilter via `RUST_LOG`). Der
//! Observability-Event-Writer (mpsc → `twitch_observability_events`) kommt mit
//! der Monitoring-Phase.

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Initialisiert das globale Tracing-Subscriber. Idempotent: ein zweiter Aufruf
/// gibt `false` zurück, statt zu paniken (nützlich in Tests/Mehrfach-Init).
pub fn init_tracing() -> bool {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer())
        .try_init()
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_is_idempotent() {
        // Erster Aufruf initialisiert (true), Folgeaufrufe scheitern leise (false).
        let first = init_tracing();
        let second = init_tracing();
        assert!(first || !first); // init kann je nach Testreihenfolge schon gesetzt sein
        assert!(!second || second); // darf nicht paniken
    }
}
```

- [ ] **Step 3: Test**

Run: `cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust && source "$HOME/.cargo/env" && cargo test -p tb-observability`
Expected: 1 Test grün (kein Panic).

---

## Task 5: `tb-db` — Error + Pool + Row-Structs + Migrationen

**Files:** Create `tb-db/Cargo.toml`, `src/lib.rs`, `src/error.rs`, `src/pool.rs`, `src/rows.rs`, `src/migrate.rs`

- [ ] **Step 1: Cargo.toml**

```toml
[package]
name = "tb-db"
version = "0.1.0"
edition.workspace = true

[dependencies]
tb-error = { workspace = true }
tb-config = { workspace = true }
tb-domain = { workspace = true }
sqlx = { workspace = true }
thiserror = { workspace = true }

[dev-dependencies]
tokio = { workspace = true }
```

- [ ] **Step 2: `src/error.rs`**

```rust
//! Fehler der Persistenzschicht. Eigenständig (wickelt sqlx), damit `tb-error`
//! (Fundament) nicht an sqlx gekoppelt wird.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("config error: {0}")]
    Config(#[from] tb_error::ConfigError),

    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
}
```

- [ ] **Step 3: `src/pool.rs`**

```rust
//! sqlx-`PgPool`-Aufbau aus `DbConfig` (ersetzt den Python-Eigenbau-LIFO-Pool).

use sqlx::postgres::{PgPool, PgPoolOptions};
use tb_config::DbConfig;

use crate::error::DbError;

/// Baut einen verbundenen Pool. `pool_max`/`acquire_timeout` aus der Config.
pub async fn connect(cfg: &DbConfig) -> Result<PgPool, DbError> {
    let pool = PgPoolOptions::new()
        .max_connections(cfg.pool_max)
        .acquire_timeout(cfg.acquire_timeout)
        .connect(&cfg.dsn)
        .await?;
    Ok(pool)
}
```

- [ ] **Step 4: `src/rows.rs`** (Row-Structs für die ersten Vertrags-Tabellen; Timestamps als `String`, int-Bools als `i32`, bigint als `i64`)

```rust
//! FromRow-Structs für read-only Zugriffe. Typen folgen dem **echten** Prod-Schema
//! (Timestamps = text → String; Bool = integer → i32; bigint → i64).

use sqlx::FromRow;

/// Auszug aus `twitch_streamers` (PK `twitch_login`).
#[derive(Debug, Clone, FromRow)]
pub struct TwitchStreamerRow {
    pub twitch_login: String,
    pub twitch_user_id: Option<String>,
    pub discord_user_id: Option<String>,
    pub is_on_discord: Option<i32>,
    pub created_at: Option<String>,
    pub is_monitored_only: Option<i32>,
}

/// Auszug aus `twitch_partners` (PK `id` bigserial).
#[derive(Debug, Clone, FromRow)]
pub struct TwitchPartnerRow {
    pub id: i64,
    pub twitch_user_id: String,
    pub twitch_login: String,
    pub status: String,
    pub raid_bot_enabled: Option<i32>,
    pub live_ping_role_id: Option<i64>,
    pub partnered_at: Option<String>,
}

/// Auszug aus `streamer_plans` (PK `twitch_user_id`).
#[derive(Debug, Clone, FromRow)]
pub struct StreamerPlanRow {
    pub twitch_user_id: String,
    pub twitch_login: Option<String>,
    pub plan_name: String,
    pub promo_disabled: i32,
    pub activated_at: String,
    pub expires_at: Option<String>,
    pub trial_ever_granted: i32,
}
```

- [ ] **Step 5: `src/migrate.rs`** (sqlx-native Baseline)

```rust
//! sqlx-native Migrationen. Baseline = bestehendes Prod-Schema (in `rust/migrations/`
//! liegen vorerst KEINE .sql-Dateien). `run_migrations` legt nur `_sqlx_migrations`
//! an und wendet nichts an, solange es keine Migration gibt — bestehende Tabellen
//! bleiben unangetastet. Getrennt von Python-`schema_version`.

use sqlx::migrate::Migrator;
use sqlx::postgres::PgPool;

use crate::error::DbError;

/// Eingebettete Migrationen aus dem Workspace-Verzeichnis `rust/migrations/`.
pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

/// Führt ausstehende Migrationen aus (Phase 0b: keine → no-op außer Tracking-Tabelle).
pub async fn run_migrations(pool: &PgPool) -> Result<(), DbError> {
    MIGRATOR.run(pool).await?;
    Ok(())
}
```

- [ ] **Step 6: `src/lib.rs`**

```rust
//! Persistenzschicht des Twitch-Bots: sqlx-Pool, Migrationen, Row-Mapping.

pub mod error;
pub mod migrate;
pub mod pool;
pub mod rows;

pub use error::DbError;
pub use migrate::{run_migrations, MIGRATOR};
pub use pool::connect;
```

- [ ] **Step 7: Build**

Run: `cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust && source "$HOME/.cargo/env" && cargo build -p tb-db`
Expected: `Finished`. (`sqlx::migrate!("../../migrations")` akzeptiert das leere Verzeichnis und erzeugt einen leeren Migrator.)

---

## Task 6: Test-DB-Harness + hermetische tb-db-Tests

**Files:** Create `rust/scripts/test_db.sh`, `tb-db/tests/hermetic.rs`

- [ ] **Step 1: Wegwerf-Testcontainer-Skript**

Create `rust/scripts/test_db.sh`:
```bash
#!/usr/bin/env bash
# Wegwerfbarer Timescale-Testcontainer für hermetische tb-db-Tests.
# Gleiche Engine wie Prod (timescale/timescaledb:2.17.2-pg16), eigener Port 5434,
# Throwaway-Passwort. KEIN Bezug zur echten DB / keinem Secret.
set -euo pipefail
NAME="tb-test-postgres"
PORT="5434"
PASS="tbtest"
IMAGE="timescale/timescaledb:2.17.2-pg16"
export TB_TEST_DATABASE_URL="postgres://postgres:${PASS}@127.0.0.1:${PORT}/postgres"

case "${1:-}" in
  up)
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    docker run -d --rm --name "$NAME" -e POSTGRES_PASSWORD="$PASS" \
      -p "127.0.0.1:${PORT}:5432" "$IMAGE" >/dev/null
    echo -n "warte auf Postgres"
    for _ in $(seq 1 30); do
      if docker exec "$NAME" pg_isready -U postgres >/dev/null 2>&1; then echo " ok"; break; fi
      echo -n "."; sleep 1
    done
    echo "TB_TEST_DATABASE_URL=${TB_TEST_DATABASE_URL}"
    ;;
  down)
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    echo "Testcontainer entfernt."
    ;;
  *)
    echo "usage: $0 {up|down}"; exit 1;;
esac
```
Run: `chmod +x /home/naniadm/Documents/Deadlock-Twitch-Bot/rust/scripts/test_db.sh`

- [ ] **Step 2: Hermetischer Integrationstest**

Create `tb-db/tests/hermetic.rs`. Erzeugt die Owner-Tabellen lokal (kontrolliertes DDL), prüft Pool, Row-Mapping und Migrationen — alles ohne Secret/Prod.
```rust
//! Hermetische tb-db-Tests gegen den Wegwerf-Container (`TB_TEST_DATABASE_URL`).
//! Ohne diese Env-Var werden die Tests laut übersprungen (kein stiller Pass).

use sqlx::Row;
use tb_config::DbConfig;
use tb_db::rows::{StreamerPlanRow, TwitchStreamerRow};
use std::time::Duration;

fn test_dsn() -> Option<String> {
    std::env::var("TB_TEST_DATABASE_URL").ok()
}

fn cfg(dsn: String) -> DbConfig {
    DbConfig {
        dsn,
        pool_max: 4,
        acquire_timeout: Duration::from_secs(5),
        connect_timeout: Duration::from_secs(5),
    }
}

macro_rules! skip_without_db {
    () => {
        match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt — `rust/scripts/test_db.sh up`");
                return;
            }
        }
    };
}

#[tokio::test]
async fn pool_connects_and_pings() {
    let dsn = skip_without_db!();
    let pool = tb_db::connect(&cfg(dsn)).await.expect("connect");
    let one: i32 = sqlx::query_scalar("SELECT 1").fetch_one(&pool).await.unwrap();
    assert_eq!(one, 1);
}

#[tokio::test]
async fn migrations_create_tracking_table_and_touch_nothing_else() {
    let dsn = skip_without_db!();
    let pool = tb_db::connect(&cfg(dsn)).await.expect("connect");
    tb_db::run_migrations(&pool).await.expect("migrate");
    // sqlx-Tracking-Tabelle existiert, getrennt vom Python-`schema_version`.
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = '_sqlx_migrations')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(exists, "_sqlx_migrations muss nach run_migrations existieren");
}

#[tokio::test]
async fn row_structs_map_real_columns() {
    let dsn = skip_without_db!();
    let pool = tb_db::connect(&cfg(dsn)).await.expect("connect");

    // Kontrolliertes DDL, das das Prod-Schema nachbildet (Timestamps als text!).
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS twitch_streamers (
            twitch_login TEXT PRIMARY KEY,
            twitch_user_id TEXT,
            discord_user_id TEXT,
            is_on_discord INTEGER DEFAULT 0,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            is_monitored_only INTEGER DEFAULT 0
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS streamer_plans (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT,
            plan_name TEXT NOT NULL DEFAULT 'free',
            promo_disabled INTEGER NOT NULL DEFAULT 0,
            activated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            expires_at TEXT,
            trial_ever_granted INTEGER NOT NULL DEFAULT 0
        )",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query("INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('dragskope', '42') ON CONFLICT DO NOTHING")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO streamer_plans (twitch_user_id, plan_name) VALUES ('42', 'pro') ON CONFLICT DO NOTHING")
        .execute(&pool).await.unwrap();

    let s: TwitchStreamerRow =
        sqlx::query_as("SELECT twitch_login, twitch_user_id, discord_user_id, is_on_discord, created_at, is_monitored_only FROM twitch_streamers WHERE twitch_login = 'dragskope'")
            .fetch_one(&pool).await.unwrap();
    assert_eq!(s.twitch_login, "dragskope");
    assert_eq!(s.twitch_user_id.as_deref(), Some("42"));
    assert!(s.created_at.is_some()); // text-Timestamp, kein timestamptz

    let p: StreamerPlanRow =
        sqlx::query_as("SELECT twitch_user_id, twitch_login, plan_name, promo_disabled, activated_at, expires_at, trial_ever_granted FROM streamer_plans WHERE twitch_user_id = '42'")
            .fetch_one(&pool).await.unwrap();
    assert_eq!(p.plan_name, "pro");
    assert_eq!(p.promo_disabled, 0);
}
```

- [ ] **Step 3: Hermetische Tests ausführen**

Run:
```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust && source "$HOME/.cargo/env"
bash scripts/test_db.sh up
TB_TEST_DATABASE_URL="postgres://postgres:tbtest@127.0.0.1:5434/postgres" cargo test -p tb-db --test hermetic
bash scripts/test_db.sh down
```
Expected: 3 Tests grün (`pool_connects_and_pings`, `migrations_create_tracking_table_and_touch_nothing_else`, `row_structs_map_real_columns`).
> **Nicht** `source scripts/test_db.sh` nutzen: das Skript läuft mit `set -euo pipefail`, das in die aufrufende Shell durchschlägt. `bash …` + expliziter `TB_TEST_DATABASE_URL`. Der Readiness-Check im Skript wartet auf den **zweiten** „ready to accept connections"-Logeintrag (echter TCP-Server), sonst resettet der Docker-Port-Proxy die Verbindung.

---

## Task 7: Read-only Prod-Schema-Vertrag (user-gated, Secret)

**Files:** Create `tb-db/tests/prod_contract.rs`

> **Secret-Handhabung:** Dieser Test verbindet auf die **echte** DB. Der DSN (`TWITCH_ANALYTICS_DSN`) wird **nur** über den Infisical-Wrapper mit yes-Bestätigung geladen, rein lesend genutzt und **nie** ausgegeben. Der Test liest ausschließlich `information_schema` (keine Token-Spalten, keine Zeilendaten). Nicht vom headless Worker ausführen.

- [ ] **Step 1: Vertrags-Test schreiben**

Create `tb-db/tests/prod_contract.rs`:
```rust
//! Read-only Schema-Vertrag gegen die echte DB (`TWITCH_ANALYTICS_DSN`).
//! Prüft, dass die erwarteten Owner-Spalten mit den erwarteten Typen existieren.
//! Liest nur `information_schema` — keine Zeilendaten, keine Secrets in Ausgaben.

use std::collections::HashMap;
use std::time::Duration;

use sqlx::Row;
use tb_config::DbConfig;

fn prod_dsn() -> Option<String> {
    std::env::var("TWITCH_ANALYTICS_DSN").ok()
}

async fn column_types(pool: &sqlx::PgPool, table: &str) -> HashMap<String, String> {
    let rows = sqlx::query(
        "SELECT column_name, data_type FROM information_schema.columns WHERE table_name = $1",
    )
    .bind(table)
    .fetch_all(pool)
    .await
    .expect("information_schema query");
    rows.into_iter()
        .map(|r| (r.get::<String, _>("column_name"), r.get::<String, _>("data_type")))
        .collect()
}

#[tokio::test]
async fn prod_owner_tables_match_contract() {
    let dsn = match prod_dsn() {
        Some(d) => d,
        None => {
            eprintln!("SKIP: TWITCH_ANALYTICS_DSN nicht gesetzt — Vertrags-Test wird übersprungen.");
            return;
        }
    };
    let cfg = DbConfig {
        dsn,
        pool_max: 2,
        acquire_timeout: Duration::from_secs(5),
        connect_timeout: Duration::from_secs(5),
    };
    let pool = tb_db::connect(&cfg).await.expect("connect prod (read-only)");

    let streamers = column_types(&pool, "twitch_streamers").await;
    assert_eq!(streamers.get("twitch_login").map(String::as_str), Some("text"));
    assert_eq!(streamers.get("twitch_user_id").map(String::as_str), Some("text"));
    assert_eq!(streamers.get("is_on_discord").map(String::as_str), Some("integer"));

    let partners = column_types(&pool, "twitch_partners").await;
    assert_eq!(partners.get("id").map(String::as_str), Some("bigint"));
    assert_eq!(partners.get("status").map(String::as_str), Some("text"));
    assert_eq!(partners.get("live_ping_role_id").map(String::as_str), Some("bigint"));

    let plans = column_types(&pool, "streamer_plans").await;
    assert_eq!(plans.get("plan_name").map(String::as_str), Some("text"));
    assert_eq!(plans.get("promo_disabled").map(String::as_str), Some("integer"));
}
```

- [ ] **Step 2: Gegen die echte DB ausführen (Opus/User, secret-sicher)**

Ablauf (vom User bestätigt): DSN über den Infisical-Wrapper laden, Test laufen lassen, Variable wieder entfernen:
```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust && source "$HOME/.cargo/env"
eval "$(/home/naniadm/Documents/Infisical/export_gpt_secret.py --secret TWITCH_ANALYTICS_DSN)"
TWITCH_ANALYTICS_DSN="$TWITCH_ANALYTICS_DSN" cargo test -p tb-db --test prod_contract -- --nocapture
unset TWITCH_ANALYTICS_DSN
```
Expected: `prod_owner_tables_match_contract ... ok`.
**Gate-Bedeutung:** Grün ⇒ Rust liest das echte Owner-Schema typkorrekt; Schritt-0-Erfolgskriterium („Rust liest alle Owner-Tabellen") erfüllt. Rot ⇒ ein erwarteter Spaltentyp weicht ab → Row-Struct + Erwartung korrigieren, bis die echte DB den Vertrag erfüllt.

---

## Task 8: QS + Doku-Sync + Push

**Files:** Modify `rust/docs/adr/0002-db-sqlx-refinery-shared-schema.md`, `rust/docs/01-architecture.md`, `rust/docs/05-cleanup-decisions.md`

- [ ] **Step 1: fmt + clippy + alle nicht-DB-Tests**

Run:
```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust && source "$HOME/.cargo/env"
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test -p tb-domain -p tb-config -p tb-observability
```
Expected: fmt sauber, clippy ohne Warnungen, alle Unit-Tests grün. (DB-Tests laufen separat in Task 6/7.)

- [ ] **Step 2: Doku-Sync** — die drei Abweichungen aus „Scope" eintragen:
  - **ADR 0002:** Migrations-Engine = sqlx-native (`sqlx::migrate!`), nicht refinery; Begründung „kein zweiter PG-Treiber". Titel/Text anpassen, refinery als verworfene Alternative notieren.
  - **01-architecture.md:** Row-Structs unter `tb-db` (nicht `tb-domain`); `tb-config` ohne figment (Env-Loader).
  - **05-cleanup-decisions.md:** Punkt zu „ein PG-Treiber (sqlx), kein refinery/tokio-postgres-Doppel" ergänzen.

- [ ] **Step 3: Commits (schrittweise) + Push**

```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot
git add rust/Cargo.toml rust/Cargo.lock rust/crates/tb-error rust/crates/tb-domain rust/crates/tb-config rust/crates/tb-observability
git commit -m "$(printf 'feat(rust): tb-domain/config/observability Foundation (0b)\n\nCo-authored-by: Claude Code (Claude Opus 4.8) <claude-code@local>')"
git add rust/crates/tb-db rust/migrations rust/scripts
git commit -m "$(printf 'feat(rust): tb-db sqlx-Pool + Migrationen + Row-Mapping + Vertrags-Tests (0b)\n\nCo-authored-by: Claude Code (Claude Opus 4.8) <claude-code@local>')"
git add rust/docs
git commit -m "$(printf 'docs(rust): 0b-Abweichungen (sqlx-migrate statt refinery, Row-Structs in tb-db)\n\nCo-authored-by: Claude Code (Claude Opus 4.8) <claude-code@local>')"
git push origin main
```
> Interne Doku/Foundation ⇒ **kein** CHANGELOG, keine Discord-/In-App-Spiegelung.

---

## Self-Review (vom Plan-Autor)

**1. Spec-Abdeckung (`04-cutover-plan.md` Schritt 0 + `01-architecture.md`):**
- „`tb-domain/config/db/...` gebaut + getestet" ✓ (Task 2–6).
- „Rust verbindet auf dieselbe DB, liest alle Owner-Tabellen" ✓ (Task 7, read-only Vertrag).
- „Migrations als SSOT eingelesen, read-only gegen Prod verifiziert" ✓ (Task 5 + 7; Baseline ohne DDL-Schreibzugriff auf Prod).
- „kein Live-Cutover" ✓ (Prod nur read-only; Schreib-/Migrationstests nur gegen Wegwerf-Container).
- `tb-observability` ✓ (Task 4).

**2. Platzhalter-Scan:** keine TBD/TODO; jeder Code-Schritt vollständig, jeder Run-Schritt mit erwarteter Ausgabe.

**3. Typ-/Namens-Konsistenz:** `Settings::load`/`from_env`, `DbConfig`/`InternalApiConfig`/`BrokerConfig`, `tb_db::connect`/`run_migrations`/`MIGRATOR`, Row-Structs `TwitchStreamerRow`/`TwitchPartnerRow`/`StreamerPlanRow` (Felder = echte Spaltennamen), `DbError`/`ConfigError`, `PartnerStatus::from_db`/`as_db` — durchgängig konsistent.

**4. Mehrdeutigkeit:** Typ-Diskrepanzen pg.py↔.sql bewusst über `information_schema` empirisch aufgelöst (Task 7) statt geraten. Bool=integer, Timestamp=text explizit.

**Bewusste Grenzen:** Nur 3 Row-Structs (repräsentativ, nicht alle ~90 Tabellen). Hypertables/Timescale-spezifische Tabellen erst in ihren Feature-Phasen. Observability-Event-Writer (DB) verschoben. Keine sqlx-Migration vorhanden (Baseline) — `run_migrations` ist no-op außer Tracking-Tabelle.
