# Schritt 1a — Erster Strangler-Cutover: Public Analytics-GETs — Implementierungsplan

> **Für agentic Workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (inline, mit Sonnet-Worker-Delegation pro Task) zum Umsetzen. Schritte nutzen Checkbox-Syntax (`- [ ]`).

**Goal:** Zwei neue Feature-Crates (`tb-analytics`, `tb-dashboard-api`) + eine minimale Binary (`tb-dashboard`, Port 8767) implementieren die drei public read-only GET-Endpoints des Python-Dashboards 1:1 nach — exaktes JSON-Feld-für-Feld, CORS `*`, kein Auth, keine Proxy-Umschaltung (die ist user-gated).

**Architecture:** `tb-analytics` hält Query-Funktionen gegen `&PgPool` und gibt typisierte Row-Structs zurück. `tb-dashboard-api` baut den axum-Router mit CORS-Layer und drei Handlern; der öffentliche API-Punkt ist `build_public_router(pool: PgPool) -> axum::Router`. `tb-dashboard` ist ein minimaler Binary, der den Pool baut und den Router bindet. Kein Auth, keine Loopback-Middleware — diese Routen sind bewusst public (`CORS: *`). Schritt 1b (Auth-Layer + streamer-scoped Endpoints) ist ein separater Plan.

**Tech Stack:** Rust stable, axum 0.7, tower-http 0.6 (`cors`-Feature ergänzen), sqlx 0.8, serde/serde_json, tokio. `tb-db`, `tb-config` (Workspace). Kein chrono im Workspace → Timestamps als `Option<String>` via `<spalte>::text` in SQL selektieren (Begründung: Python nutzt `.isoformat()` — Format-Drift durch chrono→RFC3339 vermeiden; `::text`-Cast gibt Postgres-Textdarstellung, die näher an Python-Output liegt; Shadow-Diff dokumentiert verbleibende Toleranzen).

**Test-DB:** Hermetisch via `rust/scripts/test_db.sh up` (Timescale-Container Port 5434, DSN `postgres://postgres:tbtest@127.0.0.1:5434/postgres`). Tests legen DDL an, INSERTen Fixtures, rufen Handler via `axum`-oneshot (`tower::ServiceExt::oneshot`) und prüfen exakte JSON-Form.

---

## Scope, Abweichungen & Offene Verifikationspunkte

### Was in 1a enthalten ist
- Drei public GET-Endpoints ohne Auth/CORS-Restriktion: `recent-bans`, `recent-raids`, `network`.
- Crates `tb-analytics`, `tb-dashboard-api`; Binary `bin/tb-dashboard` (baut, startet nicht automatisch).
- Workspace-Erweiterung: members + tower-http `cors`-Feature + neue Workspace-Paths.

### Was 1a explizit **nicht** enthält (→ Slice 1b)
- Auth-Layer, Session-Cookies, Loopback-Middleware.
- Streamer-scoped GET-Endpoints (benötigen Auth).
- POST/Mutierende Endpoints.
- Live-Proxy-Umschaltung (Python → Rust) — das ist ein user-gated Go-Live-Schritt nach erfolgtem Shadow-Diff.

### Bewusste Design-Entscheide

| Entscheid | Begründung |
|---|---|
| Timestamps via `<spalte>::text` selektieren, `Option<String>` mappen | Python nutzt `.isoformat()` ohne feste TZ-Normalisierung; RFC3339 via chrono würde Suffix-Drift einführen (`+00:00` vs. `+00:00.000000` etc.). `::text` gibt Postgres-Textdarstellung, die dem Python-Output am nächsten kommt. Shadow-Diff klärt verbleibende Drift. |
| Kein `chrono` im Workspace (neu) | Timestamps sind nur Durchleitungs-Strings Richtung Frontend — kein Rechnen, kein Parsen nötig. YAGNI. |
| Port 8767 für `tb-dashboard` | Python läuft auf 8765; 8766 = Admin. Kein Konflikt während Parallel-Betrieb (Shadow-Diff-Phase). |
| `tower-http::cors::CorsLayer::permissive()` | Exaktes Python-Verhalten (`Access-Control-Allow-Origin: *`). Auth-Routen (1b) bekommen separaten, restriktiven Layer. |
| `is_partner` immer `true` in `/network` | Endpoint filtert bereits auf `is_partner_active=1`; jede zurückgegebene Zeile ist per Definition ein aktiver Partner — kein DB-Feld nötig. |
| `viewers` in `twitch_raid_history` als `i64` mappen | Viewer-Zahlen sind nie negativ, aber sqlx mag `INTEGER`/`BIGINT` vs. `i32`/`i64` je Schema; der Impl-Worker verifiziert den exakten Spaltentyp. |

### Offene Verifikationspunkte (Impl-Worker muss klären)

1. **`twitch_raid_history` Spaltennamen:** Plan geht von `from_channel`, `to_channel`, `viewers`, `executed_at` aus — gegen das echte Schema prüfen (`\d twitch_raid_history` oder `information_schema.columns`). Timescale-Hypertable → Spaltennamen sind regulär sichtbar. Falls Spalten abweichen: DDL im Test + Query anpassen.
2. **`twitch_streamers_partner_state` — View oder Tabelle?** Plan geht von einer View aus. Falls nicht vorhanden: Fallback auf `twitch_partners` mit `WHERE status = 'active'`. Im hermetischen Test die View oder den Fallback-Join anlegen.
3. **`twitch_live_state` Spaltentypen:** `is_live` wird als `INTEGER` (0/1) und `last_viewer_count` als `INTEGER` angenommen. Bei `BIGINT`: sqlx-Mapping auf `i64` anpassen.
4. **`twitch_ban_events` `received_at` Spaltentyp:** Annahme `TIMESTAMPTZ`. `::text`-Cast im SQL gibt `2024-01-15 10:23:45+00` → JSON-String. Python-`.isoformat()` gibt `2024-01-15T10:23:45+00:00` (mit `T`, mit `:` in TZ). Differenz ist Shadow-Diff-Toleranzpunkt — nicht als Bug behandeln, sondern dokumentieren.
5. **`twitch_ban_events` `moderator_login`, `reason` — nullable?** Annahme ja. SQL-Mapping auf `Option<String>` — verifizieren.
6. **`tower-http` `cors`-Feature:** Workspace-Dep hat aktuell nur `features = ["trace"]`; Impl-Worker ergänzt `"cors"` (Task 0).

---

## Dateistruktur nach 1a

```
rust/
  Cargo.toml                          ← members + tower-http cors-Feature + neue Paths
  crates/
    tb-analytics/
      Cargo.toml
      src/
        lib.rs                        ← pub mod bans; pub mod raids; pub mod network;
        bans.rs                       ← BanRow, BanStats, RecentBansResult + recent_bans()
        raids.rs                      ← RaidRow + recent_raids()
        network.rs                    ← NetworkStreamerRow + network_streamers()
    tb-dashboard-api/
      Cargo.toml
      src/
        lib.rs                        ← pub fn build_public_router(pool: PgPool) -> Router; pub mod handlers;
        handlers/
          mod.rs
          bans.rs                     ← Handler recent_bans_handler + BansResponse (serde)
          raids.rs                    ← Handler recent_raids_handler + RaidsResponse (serde)
          network.rs                  ← Handler network_handler + NetworkResponse (serde)
  bin/
    tb-dashboard/
      Cargo.toml
      src/
        main.rs                       ← Port aus Env (DASHBOARD_PORT, Default 8767), Pool, mount
  docs/
    plans/
      2026-06-09-schritt-1a-public-analytics-cutover.md   ← dieser Plan
```

---

## Task 0: Workspace-Erweiterung

**Files:** Modify `rust/Cargo.toml`

- [ ] **Step 1: `members` ergänzen**

In `rust/Cargo.toml` die `members`-Liste erweitern:

```toml
members = [
    "crates/tb-error",
    "crates/tb-crypto",
    "crates/tb-domain",
    "crates/tb-config",
    "crates/tb-observability",
    "crates/tb-db",
    "crates/tb-transport-twitch",
    "crates/tb-transport-discord",
    "crates/tb-http-core",
    "crates/tb-analytics",
    "crates/tb-dashboard-api",
    "bin/tb-dashboard",
]
```

- [ ] **Step 2: `tower-http` `cors`-Feature ergänzen**

Den bestehenden Eintrag in `[workspace.dependencies]` ersetzen:

```toml
tower-http = { version = "0.6", features = ["trace", "cors"] }
```

- [ ] **Step 3: Neue Workspace-Paths ergänzen** (unter `[workspace.dependencies]` nach `tb-http-core`)

```toml
tb-analytics    = { path = "crates/tb-analytics" }
tb-dashboard-api = { path = "crates/tb-dashboard-api" }
```

- [ ] **Step 4: Verzeichnisse anlegen**

Run:
```bash
mkdir -p /home/naniadm/Documents/Deadlock-Twitch-Bot/rust/crates/tb-analytics/src
mkdir -p /home/naniadm/Documents/Deadlock-Twitch-Bot/rust/crates/tb-dashboard-api/src/handlers
mkdir -p /home/naniadm/Documents/Deadlock-Twitch-Bot/rust/bin/tb-dashboard/src
```
Expected: kein Fehler.

---

## Task 1: `tb-analytics` — Row-Structs + Query-Funktionen

**Files:** Create `crates/tb-analytics/Cargo.toml`, `src/lib.rs`, `src/bans.rs`, `src/raids.rs`, `src/network.rs`

### Step 1: `Cargo.toml`

- [ ] Datei anlegen:

```toml
[package]
name = "tb-analytics"
version = "0.1.0"
edition.workspace = true

[dependencies]
sqlx       = { workspace = true }
serde      = { workspace = true }
tb-error   = { workspace = true }

[dev-dependencies]
tokio      = { workspace = true }
tb-config  = { workspace = true }
tb-db      = { workspace = true }
```

### Step 2: `src/lib.rs`

- [ ] Datei anlegen:

```rust
//! Analytics-Queries für den Twitch-Bot.
//!
//! Jede Funktion nimmt einen `&PgPool` entgegen und gibt typisierte Structs zurück.
//! Kein HTTP, kein Serde-JSON — nur reine Query-Logik.

pub mod bans;
pub mod network;
pub mod raids;
```

### Step 3: `src/bans.rs`

- [ ] Datei anlegen:

```rust
//! Queries für `GET /twitch/api/v2/public/recent-bans`.

use sqlx::PgPool;

/// Eine Zeile aus `twitch_ban_events`.
///
/// `received_at` wird via `received_at::text` als `Option<String>` gelesen, damit
/// das Postgres-Textformat (`2024-01-15 10:23:45+00`) erhalten bleibt und keine
/// chrono-Dep nötig ist. Python-`.isoformat()` weicht im `T`-Trennzeichen ab —
/// das ist ein dokumentierter Shadow-Diff-Toleranzpunkt.
#[derive(Debug, sqlx::FromRow)]
pub struct BanRow {
    pub target_login:    String,
    pub moderator_login: Option<String>,
    pub reason:          Option<String>,
    pub received_at:     Option<String>,
}

/// Stats aus `twitch_ban_events` (30-Tage-Fenster).
#[derive(Debug)]
pub struct BanStats {
    pub today:              i64,
    pub total_30d:          i64,
    pub channels_protected: i64,
}

/// Kombiniertes Ergebnis für den `/recent-bans`-Endpoint.
#[derive(Debug)]
pub struct RecentBansResult {
    pub bans:  Vec<BanRow>,
    pub stats: BanStats,
}

/// Lädt die letzten 20 Bans + 30-Tage-Stats.
///
/// Beide Queries laufen sequenziell auf demselben Pool; kein Transaktions-Snapshot
/// nötig (read-only, kleine Drift zwischen den Queries ist akzeptabel).
pub async fn recent_bans(pool: &PgPool) -> Result<RecentBansResult, sqlx::Error> {
    let bans: Vec<BanRow> = sqlx::query_as(
        r#"
        SELECT
            target_login,
            moderator_login,
            reason,
            received_at::text AS received_at
        FROM twitch_ban_events
        ORDER BY received_at DESC
        LIMIT 20
        "#,
    )
    .fetch_all(pool)
    .await?;

    let row: (Option<i64>, Option<i64>, Option<i64>) = sqlx::query_as(
        r#"
        SELECT
            COUNT(*) FILTER (WHERE received_at >= CURRENT_DATE)                   AS today,
            COUNT(*) FILTER (WHERE received_at >= NOW() - INTERVAL '30 days')     AS total_30d,
            COUNT(DISTINCT channel_login)                                          AS channels_protected
        FROM twitch_ban_events
        WHERE received_at >= NOW() - INTERVAL '30 days'
        "#,
    )
    .fetch_one(pool)
    .await?;

    Ok(RecentBansResult {
        bans,
        stats: BanStats {
            today:              row.0.unwrap_or(0),
            total_30d:          row.1.unwrap_or(0),
            channels_protected: row.2.unwrap_or(0),
        },
    })
}
```

### Step 4: `src/raids.rs`

- [ ] Datei anlegen:

```rust
//! Queries für `GET /twitch/api/v2/public/recent-raids`.
//!
//! Spaltennamen basieren auf Annahmen (from_channel, to_channel, viewers, executed_at).
//! **Impl-Worker muss verifizieren:** `\d twitch_raid_history` gegen echtes Schema.

use sqlx::PgPool;

/// Eine Zeile aus `twitch_raid_history`.
///
/// `viewers` als `Option<i64>` — Timescale-Hypertables erlauben nullable.
/// Impl-Worker prüft den genauen Typ (`INTEGER` vs. `BIGINT`) und passt ggf.
/// auf `Option<i32>` an.
///
/// `executed_at` analog zu `received_at` in BanRow via `::text`.
#[derive(Debug, sqlx::FromRow)]
pub struct RaidRow {
    pub from_channel: String,
    pub to_channel:   String,
    pub viewers:      Option<i64>,
    pub executed_at:  Option<String>,
}

/// Lädt die letzten 20 Raids.
pub async fn recent_raids(pool: &PgPool) -> Result<Vec<RaidRow>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT
            from_channel,
            to_channel,
            viewers,
            executed_at::text AS executed_at
        FROM twitch_raid_history
        ORDER BY executed_at DESC
        LIMIT 20
        "#,
    )
    .fetch_all(pool)
    .await
}
```

### Step 5: `src/network.rs`

- [ ] Datei anlegen:

```rust
//! Queries für `GET /twitch/api/v2/public/network`.
//!
//! Voraussetzung: `twitch_streamers_partner_state` ist eine VIEW (oder Tabelle).
//! **Impl-Worker muss verifizieren:** `\dv twitch_streamers_partner_state`.
//! Falls View nicht existiert → Fallback: `twitch_partners WHERE status = 'active'`
//! mit passendem Alias (siehe Fallback-Hinweis unten).

use sqlx::PgPool;

/// Eine Zeile aus dem Netzwerk-Query.
///
/// `is_live` kommt als `i32` (0/1) aus der DB (COALESCE auf Integer-Spalte).
/// Die JSON-Serialisierung wandelt es in `bool` um (→ Handler).
#[derive(Debug, sqlx::FromRow)]
pub struct NetworkStreamerRow {
    pub twitch_login: String,
    pub is_live:      i32,
    pub viewer_count: i32,
}

/// Lädt alle aktiven Partner, sortiert nach Live-Status und Viewer-Anzahl.
///
/// Fallback (falls VIEW nicht existiert — Impl-Worker entscheidet):
/// ```sql
/// SELECT
///     tp.twitch_login,
///     COALESCE(ls.is_live, 0)             AS is_live,
///     COALESCE(ls.last_viewer_count, 0)   AS viewer_count
/// FROM twitch_partners tp
/// LEFT JOIN twitch_live_state ls
///        ON LOWER(ls.streamer_login) = LOWER(tp.twitch_login)
/// WHERE tp.status = 'active'
/// ORDER BY COALESCE(ls.is_live, 0) DESC,
///          COALESCE(ls.last_viewer_count, 0) DESC,
///          LOWER(tp.twitch_login) ASC
/// ```
pub async fn network_streamers(pool: &PgPool) -> Result<Vec<NetworkStreamerRow>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT
            sp.twitch_login,
            COALESCE(ls.is_live, 0)            AS is_live,
            COALESCE(ls.last_viewer_count, 0)  AS viewer_count
        FROM twitch_streamers_partner_state sp
        LEFT JOIN twitch_live_state ls
               ON LOWER(ls.streamer_login) = LOWER(sp.twitch_login)
        WHERE sp.is_partner_active = 1
        ORDER BY COALESCE(ls.is_live, 0) DESC,
                 COALESCE(ls.last_viewer_count, 0) DESC,
                 LOWER(sp.twitch_login) ASC
        "#,
    )
    .fetch_all(pool)
    .await
}
```

### Step 6: Hermetische Query-Tests (benötigen laufenden Container)

- [ ] In `crates/tb-analytics/src/bans.rs` am Ende ergänzen:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;

    /// DSN aus `TB_TEST_DATABASE_URL` (via `rust/scripts/test_db.sh up`).
    /// Test überspringt sich, wenn Variable nicht gesetzt.
    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    /// Minimales DDL: nur die Spalten, die `recent_bans()` selektiert.
    async fn setup(pool: &PgPool) {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_ban_events (
                id              BIGSERIAL PRIMARY KEY,
                target_login    TEXT NOT NULL,
                moderator_login TEXT,
                reason          TEXT,
                channel_login   TEXT,
                received_at     TIMESTAMPTZ
            )
            "#,
        )
        .execute(pool)
        .await
        .expect("DDL fehlgeschlagen");
    }

    #[tokio::test]
    async fn recent_bans_leere_tabelle() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = PgPool::connect(&dsn).await.expect("connect test-db");
        setup(&pool).await;
        sqlx::query("TRUNCATE twitch_ban_events").execute(&pool).await.unwrap();

        let result = recent_bans(&pool).await.unwrap();
        assert!(result.bans.is_empty(), "erwartet leere Liste");
        assert_eq!(result.stats.today, 0);
        assert_eq!(result.stats.total_30d, 0);
        assert_eq!(result.stats.channels_protected, 0);
    }

    #[tokio::test]
    async fn recent_bans_fixture_und_stats() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = PgPool::connect(&dsn).await.expect("connect test-db");
        setup(&pool).await;
        sqlx::query("TRUNCATE twitch_ban_events").execute(&pool).await.unwrap();

        // 2 aktuelle Bans in 2 verschiedenen Kanälen
        sqlx::query(
            r#"
            INSERT INTO twitch_ban_events (target_login, moderator_login, reason, channel_login, received_at)
            VALUES
                ('spammer1', 'mod_a', 'Spam',  'kanal_a', NOW()),
                ('spammer2', NULL,    NULL,     'kanal_b', NOW()),
                ('alter_ban', 'mod_c', 'Werbung', 'kanal_a', NOW() - INTERVAL '60 days')
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let result = recent_bans(&pool).await.unwrap();

        // Nur 2 Bans im 30-Tage-Fenster (alter_ban ist 60 Tage alt)
        assert_eq!(result.stats.total_30d, 2);
        // 2 distinct Kanäle im Fenster
        assert_eq!(result.stats.channels_protected, 2);
        // today ≥ 2 (beide frisch)
        assert!(result.stats.today >= 2);

        // Alle 3 Einträge kommen zurück (LIMIT 20, ORDER BY DESC)
        assert_eq!(result.bans.len(), 3);
        // NULL-Felder erhalten
        let null_ban = result.bans.iter().find(|b| b.target_login == "spammer2").unwrap();
        assert!(null_ban.moderator_login.is_none());
        assert!(null_ban.reason.is_none());
        // received_at ist ein nicht-leerer String
        assert!(result.bans[0].received_at.as_deref().map(|s| !s.is_empty()).unwrap_or(false));
    }
}
```

- [ ] Analog in `src/raids.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    /// DDL-Annahme: Impl-Worker passt Spaltentypen nach Verifikation an.
    async fn setup(pool: &PgPool) {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_raid_history (
                id           BIGSERIAL PRIMARY KEY,
                from_channel TEXT NOT NULL,
                to_channel   TEXT NOT NULL,
                viewers      BIGINT,
                executed_at  TIMESTAMPTZ
            )
            "#,
        )
        .execute(pool)
        .await
        .expect("DDL fehlgeschlagen");
    }

    #[tokio::test]
    async fn recent_raids_leere_tabelle() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = PgPool::connect(&dsn).await.expect("connect test-db");
        setup(&pool).await;
        sqlx::query("TRUNCATE twitch_raid_history").execute(&pool).await.unwrap();

        let rows = recent_raids(&pool).await.unwrap();
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn recent_raids_reihenfolge_und_felder() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = PgPool::connect(&dsn).await.expect("connect test-db");
        setup(&pool).await;
        sqlx::query("TRUNCATE twitch_raid_history").execute(&pool).await.unwrap();

        sqlx::query(
            r#"
            INSERT INTO twitch_raid_history (from_channel, to_channel, viewers, executed_at)
            VALUES
                ('kanal_a', 'kanal_b', 150, NOW() - INTERVAL '2 hours'),
                ('kanal_b', 'kanal_c', 80,  NOW() - INTERVAL '1 hour')
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let rows = recent_raids(&pool).await.unwrap();
        assert_eq!(rows.len(), 2);
        // Neuester zuerst (kanal_b→kanal_c, 1h alt)
        assert_eq!(rows[0].from_channel, "kanal_b");
        assert_eq!(rows[0].to_channel,   "kanal_c");
        assert_eq!(rows[0].viewers,       Some(80));
        assert!(rows[0].executed_at.as_deref().map(|s| !s.is_empty()).unwrap_or(false));
    }
}
```

- [ ] Analog in `src/network.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    /// Hermetisches DDL: View + live_state-Tabelle anlegen.
    ///
    /// Falls die echte DB eine Tabelle statt View hat, reicht dieses DDL trotzdem
    /// (CREATE VIEW über eine Basistabelle ist funktional identisch).
    async fn setup(pool: &PgPool) {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_live_state (
                streamer_login   TEXT PRIMARY KEY,
                is_live          INTEGER NOT NULL DEFAULT 0,
                last_viewer_count INTEGER NOT NULL DEFAULT 0
            )
            "#,
        )
        .execute(pool)
        .await
        .expect("DDL live_state fehlgeschlagen");

        // Basistabelle für die View
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS _partner_state_base (
                twitch_login       TEXT PRIMARY KEY,
                is_partner_active  INTEGER NOT NULL DEFAULT 1
            )
            "#,
        )
        .execute(pool)
        .await
        .expect("DDL partner_state_base fehlgeschlagen");

        // View anlegen (idempotent via OR REPLACE)
        sqlx::query(
            r#"
            CREATE OR REPLACE VIEW twitch_streamers_partner_state AS
            SELECT twitch_login, is_partner_active
            FROM _partner_state_base
            "#,
        )
        .execute(pool)
        .await
        .expect("DDL view fehlgeschlagen");
    }

    #[tokio::test]
    async fn network_leeres_ergebnis() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = PgPool::connect(&dsn).await.expect("connect test-db");
        setup(&pool).await;
        sqlx::query("TRUNCATE _partner_state_base CASCADE").execute(&pool).await.unwrap();
        sqlx::query("TRUNCATE twitch_live_state").execute(&pool).await.unwrap();

        let rows = network_streamers(&pool).await.unwrap();
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn network_sortierung_und_is_live_konvertierung() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = PgPool::connect(&dsn).await.expect("connect test-db");
        setup(&pool).await;
        sqlx::query("TRUNCATE _partner_state_base CASCADE").execute(&pool).await.unwrap();
        sqlx::query("TRUNCATE twitch_live_state").execute(&pool).await.unwrap();

        sqlx::query(
            "INSERT INTO _partner_state_base (twitch_login, is_partner_active) VALUES ('dragskope', 1), ('anderer', 1), ('offline_streamer', 1)"
        ).execute(&pool).await.unwrap();

        sqlx::query(
            "INSERT INTO twitch_live_state (streamer_login, is_live, last_viewer_count) VALUES ('dragskope', 1, 500), ('anderer', 0, 0)"
        ).execute(&pool).await.unwrap();

        let rows = network_streamers(&pool).await.unwrap();
        assert_eq!(rows.len(), 3);
        // dragskope zuerst (is_live=1, 500 viewer)
        assert_eq!(rows[0].twitch_login, "dragskope");
        assert_eq!(rows[0].is_live,      1);
        assert_eq!(rows[0].viewer_count, 500);
        // offline_streamer ohne live_state → COALESCE → 0/0
        let offline = rows.iter().find(|r| r.twitch_login == "offline_streamer").unwrap();
        assert_eq!(offline.is_live,      0);
        assert_eq!(offline.viewer_count, 0);
    }
}
```

### Step 7: Query-Tests ausführen

- [ ] Run:

```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust && source "$HOME/.cargo/env"
bash scripts/test_db.sh up
TB_TEST_DATABASE_URL="postgres://postgres:tbtest@127.0.0.1:5434/postgres" \
  cargo test -p tb-analytics -- --nocapture
```
Expected: alle Tests `ok` (oder `SKIP: TB_TEST_DATABASE_URL nicht gesetzt` wenn Container nicht läuft, was hier nicht zutreffen soll).

---

## Task 2: `tb-dashboard-api` — Handler + Router + JSON-Response-Typen

**Files:** Create `crates/tb-dashboard-api/Cargo.toml`, `src/lib.rs`, `src/handlers/mod.rs`, `src/handlers/bans.rs`, `src/handlers/raids.rs`, `src/handlers/network.rs`

### Step 1: `Cargo.toml`

- [ ] Datei anlegen:

```toml
[package]
name = "tb-dashboard-api"
version = "0.1.0"
edition.workspace = true

[dependencies]
axum         = { workspace = true }
tower        = { workspace = true }
tower-http   = { workspace = true }
serde        = { workspace = true }
serde_json   = { workspace = true }
sqlx         = { workspace = true }
tb-analytics = { workspace = true }
tb-error     = { workspace = true }

[dev-dependencies]
tokio        = { workspace = true }
tb-db        = { workspace = true }
tb-config    = { workspace = true }
```

### Step 2: `src/lib.rs`

- [ ] Datei anlegen:

```rust
//! HTTP-Router für das public Analytics-Dashboard.
//!
//! Öffentlicher Einstiegspunkt: `build_public_router(pool)`.
//! Kein Auth, kein Loopback-Gate — diese Routen sind explizit public (`CORS: *`).

pub mod handlers;

use axum::Router;
use sqlx::PgPool;
use tower_http::cors::CorsLayer;

/// Baut den axum-Router für alle public Analytics-GET-Endpoints.
///
/// CORS-Policy: `CorsLayer::permissive()` (entspricht Python-`Access-Control-Allow-Origin: *`).
/// Auth-Routen kommen in `build_authed_router` (Slice 1b).
pub fn build_public_router(pool: PgPool) -> Router {
    use axum::routing::get;
    use handlers::{bans, network, raids};

    let api = Router::new()
        .route("/twitch/api/v2/public/recent-bans",  get(bans::recent_bans_handler))
        .route("/twitch/api/v2/public/recent-raids", get(raids::recent_raids_handler))
        .route("/twitch/api/v2/public/network",      get(network::network_handler))
        .with_state(pool)
        .layer(CorsLayer::permissive());

    api
}
```

### Step 3: `src/handlers/mod.rs`

- [ ] Datei anlegen:

```rust
//! Handler-Module: je ein Modul pro public Endpoint.
pub mod bans;
pub mod network;
pub mod raids;
```

### Step 4: `src/handlers/bans.rs`

- [ ] Datei anlegen:

```rust
//! Handler für `GET /twitch/api/v2/public/recent-bans`.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;
use sqlx::PgPool;
use tb_analytics::bans::{recent_bans, BanRow};

/// JSON-Repräsentation einer einzelnen Ban-Zeile.
///
/// Feldnamen 1:1 wie Python-Response (`target_login`, `moderator_login`, `reason`, `received_at`).
#[derive(Serialize)]
pub struct BanRowJson {
    pub target_login:    String,
    pub moderator_login: Option<String>,
    pub reason:          Option<String>,
    pub received_at:     Option<String>,
}

impl From<BanRow> for BanRowJson {
    fn from(r: BanRow) -> Self {
        Self {
            target_login:    r.target_login,
            moderator_login: r.moderator_login,
            reason:          r.reason,
            received_at:     r.received_at,
        }
    }
}

/// JSON-Stats-Block.
#[derive(Serialize)]
pub struct BanStatsJson {
    pub today:              i64,
    pub total_30d:          i64,
    pub channels_protected: i64,
}

/// Top-Level-Response.
#[derive(Serialize)]
pub struct BansResponse {
    pub bans:  Vec<BanRowJson>,
    pub stats: BanStatsJson,
}

/// `GET /twitch/api/v2/public/recent-bans`
pub async fn recent_bans_handler(
    State(pool): State<PgPool>,
) -> impl IntoResponse {
    match recent_bans(&pool).await {
        Ok(result) => {
            let resp = BansResponse {
                bans: result.bans.into_iter().map(BanRowJson::from).collect(),
                stats: BanStatsJson {
                    today:              result.stats.today,
                    total_30d:          result.stats.total_30d,
                    channels_protected: result.stats.channels_protected,
                },
            };
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(e) => {
            tracing::error!("recent_bans Query-Fehler: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
```

### Step 5: `src/handlers/raids.rs`

- [ ] Datei anlegen:

```rust
//! Handler für `GET /twitch/api/v2/public/recent-raids`.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;
use sqlx::PgPool;
use tb_analytics::raids::{recent_raids, RaidRow};

/// JSON-Repräsentation einer einzelnen Raid-Zeile.
///
/// `viewers` als `Option<i64>` — nullable in der DB (Hypertable).
#[derive(Serialize)]
pub struct RaidRowJson {
    pub from_channel: String,
    pub to_channel:   String,
    pub viewers:      Option<i64>,
    pub executed_at:  Option<String>,
}

impl From<RaidRow> for RaidRowJson {
    fn from(r: RaidRow) -> Self {
        Self {
            from_channel: r.from_channel,
            to_channel:   r.to_channel,
            viewers:      r.viewers,
            executed_at:  r.executed_at,
        }
    }
}

/// Top-Level-Response.
#[derive(Serialize)]
pub struct RaidsResponse {
    pub raids: Vec<RaidRowJson>,
}

/// `GET /twitch/api/v2/public/recent-raids`
pub async fn recent_raids_handler(
    State(pool): State<PgPool>,
) -> impl IntoResponse {
    match recent_raids(&pool).await {
        Ok(rows) => {
            let resp = RaidsResponse {
                raids: rows.into_iter().map(RaidRowJson::from).collect(),
            };
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(e) => {
            tracing::error!("recent_raids Query-Fehler: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
```

### Step 6: `src/handlers/network.rs`

- [ ] Datei anlegen:

```rust
//! Handler für `GET /twitch/api/v2/public/network`.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;
use sqlx::PgPool;
use tb_analytics::network::{network_streamers, NetworkStreamerRow};

/// JSON-Repräsentation eines Streamers im Netzwerk.
///
/// `is_partner` ist immer `true`: der Endpoint filtert bereits auf aktive Partner.
/// `is_live` kommt als `i32` aus der DB und wird hier zu `bool` (Python-Verhalten: truthy).
#[derive(Serialize)]
pub struct NetworkStreamerJson {
    pub login:        String,
    pub is_partner:   bool,
    pub is_live:      bool,
    pub viewer_count: i32,
}

impl From<NetworkStreamerRow> for NetworkStreamerJson {
    fn from(r: NetworkStreamerRow) -> Self {
        Self {
            login:        r.twitch_login,
            is_partner:   true,
            is_live:      r.is_live != 0,
            viewer_count: r.viewer_count,
        }
    }
}

/// Top-Level-Response.
#[derive(Serialize)]
pub struct NetworkResponse {
    pub streamers: Vec<NetworkStreamerJson>,
}

/// `GET /twitch/api/v2/public/network`
pub async fn network_handler(
    State(pool): State<PgPool>,
) -> impl IntoResponse {
    match network_streamers(&pool).await {
        Ok(rows) => {
            let resp = NetworkResponse {
                streamers: rows.into_iter().map(NetworkStreamerJson::from).collect(),
            };
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(e) => {
            tracing::error!("network Query-Fehler: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
```

### Step 7: Handler-Tests via axum-oneshot (exakte JSON-Form)

Die Tests für `tb-dashboard-api` nutzen `tower::ServiceExt::oneshot` — kein echter TCP-Listener, kein Port. Sie setzen den Test-Container voraus und testen die vollständige HTTP-Response-Form.

- [ ] In `crates/tb-dashboard-api/src/handlers/bans.rs` am Ende ergänzen:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::{Request, StatusCode};
    use sqlx::PgPool;
    use tower::ServiceExt;
    use crate::build_public_router;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    async fn setup_bans(pool: &PgPool) {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_ban_events (
                id              BIGSERIAL PRIMARY KEY,
                target_login    TEXT NOT NULL,
                moderator_login TEXT,
                reason          TEXT,
                channel_login   TEXT,
                received_at     TIMESTAMPTZ
            )
            "#,
        )
        .execute(pool)
        .await
        .expect("DDL fehlgeschlagen");
    }

    #[tokio::test]
    async fn bans_endpoint_leere_tabelle_json_form() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => { eprintln!("SKIP"); return; }
        };
        let pool = PgPool::connect(&dsn).await.unwrap();
        setup_bans(&pool).await;
        sqlx::query("TRUNCATE twitch_ban_events").execute(&pool).await.unwrap();

        let app = build_public_router(pool);
        let req = Request::builder()
            .uri("/twitch/api/v2/public/recent-bans")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Exakte Schlüssel-Struktur
        assert!(json.get("bans").is_some(), "Feld 'bans' fehlt");
        assert!(json.get("stats").is_some(), "Feld 'stats' fehlt");
        assert!(json["bans"].as_array().unwrap().is_empty(), "bans muss [] sein");
        assert_eq!(json["stats"]["today"],              0);
        assert_eq!(json["stats"]["total_30d"],          0);
        assert_eq!(json["stats"]["channels_protected"], 0);
    }

    #[tokio::test]
    async fn bans_endpoint_null_felder_korrekt() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => { eprintln!("SKIP"); return; }
        };
        let pool = PgPool::connect(&dsn).await.unwrap();
        setup_bans(&pool).await;
        sqlx::query("TRUNCATE twitch_ban_events").execute(&pool).await.unwrap();

        sqlx::query(
            "INSERT INTO twitch_ban_events (target_login, moderator_login, reason, channel_login, received_at) VALUES ('null_user', NULL, NULL, 'ch', NULL)"
        ).execute(&pool).await.unwrap();

        let app = build_public_router(pool);
        let req = Request::builder()
            .uri("/twitch/api/v2/public/recent-bans")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let ban = &json["bans"][0];
        assert_eq!(ban["target_login"],    "null_user");
        assert!(ban["moderator_login"].is_null(), "moderator_login muss null sein");
        assert!(ban["reason"].is_null(),          "reason muss null sein");
        assert!(ban["received_at"].is_null(),     "received_at muss null sein");
    }
}
```

- [ ] Analog für `raids.rs` (oneshot, leere Tabelle → `{"raids":[]}`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::{Request, StatusCode};
    use sqlx::PgPool;
    use tower::ServiceExt;
    use crate::build_public_router;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    async fn setup_raids(pool: &PgPool) {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_raid_history (
                id           BIGSERIAL PRIMARY KEY,
                from_channel TEXT NOT NULL,
                to_channel   TEXT NOT NULL,
                viewers      BIGINT,
                executed_at  TIMESTAMPTZ
            )
            "#,
        )
        .execute(pool)
        .await
        .expect("DDL fehlgeschlagen");
    }

    #[tokio::test]
    async fn raids_endpoint_leere_tabelle_json_form() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => { eprintln!("SKIP"); return; }
        };
        let pool = PgPool::connect(&dsn).await.unwrap();
        setup_raids(&pool).await;
        sqlx::query("TRUNCATE twitch_raid_history").execute(&pool).await.unwrap();

        let app = build_public_router(pool);
        let req = Request::builder()
            .uri("/twitch/api/v2/public/recent-raids")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert!(json.get("raids").is_some(), "Feld 'raids' fehlt");
        assert!(json["raids"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn raids_endpoint_fixture_feldnamen() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => { eprintln!("SKIP"); return; }
        };
        let pool = PgPool::connect(&dsn).await.unwrap();
        setup_raids(&pool).await;
        sqlx::query("TRUNCATE twitch_raid_history").execute(&pool).await.unwrap();

        sqlx::query(
            "INSERT INTO twitch_raid_history (from_channel, to_channel, viewers, executed_at) VALUES ('von', 'nach', 200, NOW())"
        ).execute(&pool).await.unwrap();

        let app = build_public_router(pool);
        let req = Request::builder()
            .uri("/twitch/api/v2/public/recent-raids")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let raid = &json["raids"][0];
        assert_eq!(raid["from_channel"], "von");
        assert_eq!(raid["to_channel"],   "nach");
        assert_eq!(raid["viewers"],      200);
        assert!(raid["executed_at"].is_string());
    }
}
```

- [ ] Analog für `network.rs` (oneshot, `is_live`-bool-Konvertierung + `is_partner: true`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::{Request, StatusCode};
    use sqlx::PgPool;
    use tower::ServiceExt;
    use crate::build_public_router;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    async fn setup_network(pool: &PgPool) {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS twitch_live_state (
                streamer_login    TEXT PRIMARY KEY,
                is_live           INTEGER NOT NULL DEFAULT 0,
                last_viewer_count INTEGER NOT NULL DEFAULT 0
            )"#,
        ).execute(pool).await.expect("DDL live_state");

        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS _partner_state_base (
                twitch_login      TEXT PRIMARY KEY,
                is_partner_active INTEGER NOT NULL DEFAULT 1
            )"#,
        ).execute(pool).await.expect("DDL partner_state_base");

        sqlx::query(
            r#"CREATE OR REPLACE VIEW twitch_streamers_partner_state AS
               SELECT twitch_login, is_partner_active FROM _partner_state_base"#,
        ).execute(pool).await.expect("DDL view");
    }

    #[tokio::test]
    async fn network_endpoint_leere_tabelle_json_form() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => { eprintln!("SKIP"); return; }
        };
        let pool = PgPool::connect(&dsn).await.unwrap();
        setup_network(&pool).await;
        sqlx::query("TRUNCATE _partner_state_base CASCADE").execute(&pool).await.unwrap();
        sqlx::query("TRUNCATE twitch_live_state").execute(&pool).await.unwrap();

        let app = build_public_router(pool);
        let req = Request::builder()
            .uri("/twitch/api/v2/public/network")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert!(json.get("streamers").is_some(), "Feld 'streamers' fehlt");
        assert!(json["streamers"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn network_is_live_bool_und_is_partner_true() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => { eprintln!("SKIP"); return; }
        };
        let pool = PgPool::connect(&dsn).await.unwrap();
        setup_network(&pool).await;
        sqlx::query("TRUNCATE _partner_state_base CASCADE").execute(&pool).await.unwrap();
        sqlx::query("TRUNCATE twitch_live_state").execute(&pool).await.unwrap();

        sqlx::query("INSERT INTO _partner_state_base VALUES ('liveuser', 1), ('offuser', 1)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_live_state VALUES ('liveuser', 1, 300)")
            .execute(&pool).await.unwrap();

        let app = build_public_router(pool);
        let req = Request::builder()
            .uri("/twitch/api/v2/public/network")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let live = json["streamers"].as_array().unwrap()
            .iter().find(|s| s["login"] == "liveuser").unwrap();
        assert_eq!(live["is_live"],      true,   "is_live muss bool true sein");
        assert_eq!(live["is_partner"],   true,   "is_partner muss immer true sein");
        assert_eq!(live["viewer_count"], 300);

        let offline = json["streamers"].as_array().unwrap()
            .iter().find(|s| s["login"] == "offuser").unwrap();
        assert_eq!(offline["is_live"], false, "is_live muss bool false sein");
    }
}
```

### Step 8: Handler-Tests ausführen

- [ ] Run:

```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust && source "$HOME/.cargo/env"
bash scripts/test_db.sh up
TB_TEST_DATABASE_URL="postgres://postgres:tbtest@127.0.0.1:5434/postgres" \
  cargo test -p tb-dashboard-api -- --nocapture
```
Expected: alle Tests `ok`.

---

## Task 3: Binary `bin/tb-dashboard`

**Files:** Create `bin/tb-dashboard/Cargo.toml`, `src/main.rs`

### Step 1: `Cargo.toml`

- [ ] Datei anlegen:

```toml
[package]
name = "tb-dashboard"
version = "0.1.0"
edition.workspace = true

[[bin]]
name = "tb-dashboard"
path = "src/main.rs"

[dependencies]
axum              = { workspace = true }
tokio             = { workspace = true }
tracing           = { workspace = true }
tracing-subscriber = { workspace = true }
sqlx              = { workspace = true }
tb-config         = { workspace = true }
tb-db             = { workspace = true }
tb-dashboard-api  = { workspace = true }
```

### Step 2: `src/main.rs`

- [ ] Datei anlegen:

```rust
//! Minimaler HTTP-Server für das public Analytics-Dashboard.
//!
//! Port: `DASHBOARD_PORT` Env-Variable, Default 8767.
//! DSN:  `TWITCH_ANALYTICS_DSN` (via tb-config).
//!
//! **Nicht automatisch starten** — Start ist user-gated (erfordert echtes DSN).

use tb_config::Settings;
use tb_dashboard_api::build_public_router;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let settings = Settings::from_env().unwrap_or_else(|e| {
        tracing::error!("Konfigurationsfehler: {e}");
        std::process::exit(1);
    });

    let pool = tb_db::connect(&settings.db).await.unwrap_or_else(|e| {
        tracing::error!("DB-Verbindungsfehler: {e}");
        std::process::exit(1);
    });

    let port: u16 = std::env::var("DASHBOARD_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8767);

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let app = build_public_router(pool);

    tracing::info!("tb-dashboard lauscht auf {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap_or_else(|e| {
        tracing::error!("Bind-Fehler auf {addr}: {e}");
        std::process::exit(1);
    });
    axum::serve(listener, app).await.unwrap();
}
```

### Step 3: Binary kompilieren (ohne Starten)

- [ ] Run:

```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust && source "$HOME/.cargo/env"
cargo build -p tb-dashboard
```
Expected: `Finished` (keine Warnungen dank `-D warnings` in QS-Task).

---

## Task 4: QS + Doku-Sync + Commits

### Step 1: fmt + clippy + alle Tests

- [ ] Run:

```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust && source "$HOME/.cargo/env"
cargo fmt --all -- --check
cargo clippy -p tb-analytics -p tb-dashboard-api -p tb-dashboard --all-targets -- -D warnings
```
Expected: fmt sauber, clippy keine Warnungen.

- [ ] Run (alle Tests mit Container):

```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust && source "$HOME/.cargo/env"
bash scripts/test_db.sh up
TB_TEST_DATABASE_URL="postgres://postgres:tbtest@127.0.0.1:5434/postgres" \
  cargo test -p tb-analytics -p tb-dashboard-api -- --nocapture
bash scripts/test_db.sh down
```
Expected: alle Tests `ok`, Container aufgeräumt.

### Step 2: Doku-Sync

- [ ] **`01-architecture.md`:** Unter „Feature-Crates" die Zeilen für `tb-analytics` und `tb-dashboard-api` als „implementiert (Slice 1a)" markieren. Binary `tb-dashboard` unter „Binaries" ergänzen mit Port 8767 und dem Hinweis, dass der Live-Start user-gated ist.

- [ ] **`04-cutover-plan.md`:** Schritt 1 in zwei Slices untergliedern:
  - Slice 1a: public GET-Endpoints (dieser Plan) — Status: Code + Tests fertig, kein Proxy-Flip.
  - Slice 1b: Auth + streamer-scoped Reads — Status: offen.

### Step 3: Commits + Push

- [ ] Run:

```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot
git add rust/Cargo.toml rust/Cargo.lock rust/crates/tb-analytics rust/crates/tb-dashboard-api rust/bin/tb-dashboard
git commit -m "$(printf 'feat(rust): tb-analytics + tb-dashboard-api + tb-dashboard Binary (Slice 1a)\n\nDrei public read-only GET-Endpoints als Rust-Crates implementiert:\nrecent-bans, recent-raids, network. Hermetische axum-oneshot-Tests.\nKein Proxy-Flip — user-gated Go-Live via Shadow-Diff.\n\nCo-authored-by: Claude Code (Claude Sonnet 4.6) <claude-code@local>')"
```

- [ ] Run:

```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot
git add rust/docs/01-architecture.md rust/docs/04-cutover-plan.md rust/docs/plans/2026-06-09-schritt-1a-public-analytics-cutover.md
git commit -m "$(printf 'docs(rust): Slice 1a in Architektur + Cutover-Plan nachgezogen\n\nCo-authored-by: Claude Code (Claude Sonnet 4.6) <claude-code@local>')"
git push origin main
```
Expected: `main` aktuell, beide Commits gepusht.

> Interne Implementierung ohne user-sichtbares Verhalten (kein Live-Flip) → **kein** CHANGELOG-Eintrag, keine Discord-/In-App-Spiegelung.

---

## Task 5 (optional, user-gated): Shadow-Diff — Rust vs. Python JSON

> Dieser Task läuft NICHT in CI und NICHT ohne Freigabe. Er setzt einen echten DSN und einen laufenden `tb-dashboard`-Prozess voraus.

### Voraussetzungen

1. `TWITCH_ANALYTICS_DSN` verfügbar (über Infisical-Wrapper laden).
2. Python-Dashboard läuft auf Port 8765, Rust-Dashboard auf 8767.

### Durchführung

- [ ] Run (einmalig, als naniadm, DSN zuvor laden):

```bash
eval "$(/home/naniadm/Documents/Infisical/export_gpt_secret.py --secret TWITCH_ANALYTICS_DSN)"
# Rust-Dashboard starten (separates Terminal oder &)
cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust
source "$HOME/.cargo/env"
TWITCH_ANALYTICS_DSN="$TWITCH_ANALYTICS_DSN" DASHBOARD_PORT=8767 cargo run -p tb-dashboard &
RUST_PID=$!

# Kurz warten bis Port offen
sleep 2

# Shadow-Diff für alle 3 Endpoints
for ep in recent-bans recent-raids network; do
  echo "=== $ep ==="
  PYTHON_JSON=$(curl -sf "http://127.0.0.1:8765/twitch/api/v2/public/$ep")
  RUST_JSON=$(  curl -sf "http://127.0.0.1:8767/twitch/api/v2/public/$ep")
  # Felder normalisieren: Timestamp-Format-Toleranz (T vs. Leerzeichen, TZ-Suffix)
  # Vergleich auf Schlüssel-Ebene: python gibt dict zurück, rust gibt dict zurück
  echo "Python-Keys: $(echo "$PYTHON_JSON" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(list(d.keys()))')"
  echo "Rust-Keys:   $(echo "$RUST_JSON"   | python3 -c 'import sys,json; d=json.load(sys.stdin); print(list(d.keys()))')"
  # Numerische Felder direkt vergleichen
  diff <(echo "$PYTHON_JSON" | python3 -m json.tool --sort-keys 2>/dev/null) \
       <(echo "$RUST_JSON"   | python3 -m json.tool --sort-keys 2>/dev/null) || true
done

kill $RUST_PID 2>/dev/null || true
unset TWITCH_ANALYTICS_DSN
```

### Bekannte Toleranzpunkte

| Feld | Python-Format | Rust-Format (`::text`) | Ursache | Handlungsbedarf |
|---|---|---|---|---|
| `received_at`, `executed_at` | `2024-01-15T10:23:45+00:00` (ISO mit T, Doppelpunkt in TZ) | `2024-01-15 10:23:45+00` (Leerzeichen, ohne Doppelpunkt) | Python `.isoformat()` vs. Postgres `::text` | Kein Fix nötig — Frontend parst beides; bei Bedarf `REPLACE(val, ' ', 'T')` in SQL |
| `viewers` NULL | `null` | `null` | — | Identisch |
| Reihenfolge | deterministisch via ORDER BY | identisch | — | Identisch |
| `is_partner` | `true` | `true` | — | Identisch |
| `is_live` | `false`/`true` | `false`/`true` | — | Identisch |

**Erfolgskriterium:** Alle numerischen Felder stimmen exakt überein. Timestamp-Strings sind inhaltlich gleich (gleiche UTC-Zeit), nur Format-Differenz. Reihenfolge identisch.

---

## Self-Review (vom Plan-Autor)

**1. Spec-Abdeckung (`04-cutover-plan.md` Schritt 1 / Aufgaben-Scope):**
- Drei public GET-Endpoints ohne Auth ✓
- Crates `tb-analytics`, `tb-dashboard-api` ✓; Binary `tb-dashboard` (Port 8767) ✓
- Kein Live-Proxy-Flip im Plan — user-gated ✓
- Hermetische Tests (Timescale-Container, axum-oneshot, exakte JSON-Form) ✓
- `CorsLayer::permissive()` auf public-Router, keine Auth-Middleware ✓
- Kein chrono — Timestamps als `Option<String>` via `::text`-Cast ✓
- Shadow-Diff als optionaler user-gated Task, Toleranzpunkte dokumentiert ✓
- `is_live` int→bool Konvertierung im Handler ✓; `is_partner` immer `true` ✓

**2. Platzhalter-Scan:**
- Alle Code-Blocks vollständig und syntaktisch korrekt.
- Alle Run-Kommandos mit erwarteter Ausgabe versehen.
- Offene Verifikationspunkte (Spaltennamen, View-Existenz) explizit markiert.

**3. Typ-/Namens-Konsistenz:**
- `BanRow`/`BanStats`/`RecentBansResult` ↔ `BanRowJson`/`BanStatsJson`/`BansResponse` ✓
- `RaidRow` ↔ `RaidRowJson`/`RaidsResponse` ✓
- `NetworkStreamerRow` ↔ `NetworkStreamerJson`/`NetworkResponse` ✓
- `build_public_router(pool: PgPool) -> Router` — öffentliche API des Crate ✓
- `tb-analytics`-Funktionen: `recent_bans`, `recent_raids`, `network_streamers` ✓

**4. Bewusste Grenzen:**
- Tests setzen laufenden Container voraus — Ausführung ohne Container überspringt (`SKIP`), schlägt nicht fehl.
- `tracing`-Dep in `tb-dashboard-api` nur indirekt (über Handler-Fehlerlog) — ggf. als direkte Dep ergänzen, falls clippy meckert.
- Binary bindet nur auf `127.0.0.1` (Loopback) — für Production-Bind auf `0.0.0.0` oder via Reverse-Proxy. Das ist bewusst: kein unauthentisierter Port nach außen bis Proxy konfiguriert ist.
- rustfmt-Falle: Datei nach Schreiben einmalig mit `cargo fmt -p tb-analytics -p tb-dashboard-api -p tb-dashboard` formatieren, bevor `--check` läuft.
