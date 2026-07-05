//! Gemeinsames MiniMax-Usage-Ledger (zentrale Postgres über alle Crates dieses Bots).
//!
//! Jeder MiniMax-Call wird hier mit den **echten** Token-Zahlen aus der
//! API-Antwort verbucht — pro Quelle (`source`) und Zweck (`purpose`). Die Tabelle
//! `minimax_usage` liegt in der **zentralen Postgres** (per Migration angelegt),
//! nicht mehr in einer separaten SQLite-Datei. Der Python-Helfer
//! `~/Documents/.claude/minimax-usage/minimax_usage.py` und der Rust-TradingBot
//! (`tb-ai`) schreiben weiterhin in ihr eigenes SQLite — deren Anbindung an dieses
//! Postgres ist eine **separate** Aufgabe (siehe Report), damit die cross-bot-
//! Aggregation wieder auf einem gemeinsamen Speicher steht.
//!
//! **Best-effort-Prinzip:** Tracking darf den eigentlichen LLM-Call NIE kippen.
//! Jeder Fehler (Ledger nicht erreichbar, Schreibfehler) wird ausschließlich per
//! [`tracing::warn`] geloggt und verschluckt — der Aufrufer läuft weiter. Secrets
//! landen niemals im Ledger oder Log.
//!
//! **Verbindung:** DSN aus Env `TWITCH_ANALYTICS_DSN` (der kanonische zentrale
//! DSN dieses Bots, siehe `tb-config`), sonst `DATABASE_URL`. `TWITCH_ANALYTICS_DSN`
//! hat bewusst Vorrang: `DATABASE_URL` kann in manchen Prod/CI-Umgebungen auf eine
//! andere/Test-DB zeigen — sonst schriebe das Ledger still in die falsche DB. Ohne
//! DSN bleibt das Ledger still inaktiv (best-effort).
//!
//! **Zeitspalte:** `ts` bleibt bewusst `TEXT` (ISO-8601 UTC mit Sekunden und
//! `+00:00`-Offset, byte-gleich zum Python-Stil `isoformat(timespec="seconds")`).
//! Dadurch bleibt die rollierende Fensterabfrage **textbasiert**: der Schwellwert
//! wird als ISO-String gebildet und lexikografisch verglichen — bei identischem
//! Format/Offset ist die lexikografische Ordnung gleich der chronologischen.
//!
//! **Quelle:** dieser Bot schreibt durchgängig [`SOURCE`] (`twitch-bot`) — der
//! Identifier, mit dem die rollierende 5h-Budget-Logik den Verbrauch dieses Bots
//! vom Verbrauch anderer Bots trennt.

use std::time::{Duration, Instant};

use chrono::{SecondsFormat, Utc};
use sqlx::postgres::{PgPool, PgPoolOptions};
use tokio::sync::{Mutex, OnceCell};

/// Quellen-Kennung dieses Bots im geteilten Ledger (Python: `source="twitch-bot"`).
pub const SOURCE: &str = "twitch-bot";

/// Primäre Env-Variable: der kanonische zentrale DSN dieses Bots (siehe
/// `tb-config`, `TwitchConfig.dsn`). Bewusst VOR `DATABASE_URL`, damit das Ledger
/// nicht versehentlich in eine abweichende `DATABASE_URL`-DB (z. B. Test) schreibt.
const ENV_DSN_PRIMARY: &str = "TWITCH_ANALYTICS_DSN";
/// Fallback-Env-Variable, falls `TWITCH_ANALYTICS_DSN` nicht gesetzt ist.
const ENV_DSN_FALLBACK: &str = "DATABASE_URL";
/// Env-Variable für das rollierende 5h-Token-Budget (0/leer = aus).
const ENV_BUDGET: &str = "MINIMAX_5H_TOKEN_BUDGET";
/// Standard-Fensterbreite in Stunden (Python: `WINDOW_HOURS = 5`).
const WINDOW_HOURS: i64 = 5;
/// Mindestabstand zwischen zwei Budget-Prüfungen, damit nicht jeder Call die DB
/// für die teurere `SUM`-Abfrage anfasst (siehe [`warn_if_over_budget`]).
const BUDGET_CHECK_INTERVAL: Duration = Duration::from_secs(60);

/// Lazy gebauter, prozessweit gecachter Pool auf die zentrale Postgres.
///
/// Der Pool wird beim ersten Zugriff einmal aufgebaut (Schema-Ensure inklusive)
/// und danach wiederverwendet. Scheitert der Aufbau, bleibt der Cache leer und
/// jeder Aufruf versucht es erneut (best-effort, kein dauerhaftes Vergiften).
static POOL: OnceCell<PgPool> = OnceCell::const_new();

/// Zeitpunkt der letzten Budget-Prüfung. `None` = noch nie geprüft. Drosselt
/// [`warn_if_over_budget`] auf höchstens eine DB-Abfrage pro
/// [`BUDGET_CHECK_INTERVAL`], damit das Fenster-`SUM` nicht bei jedem Call läuft.
static LAST_BUDGET_CHECK: Mutex<Option<Instant>> = Mutex::const_new(None);

/// Liest den zentralen DSN: Env `TWITCH_ANALYTICS_DSN`, sonst `DATABASE_URL`.
/// Leere/whitespace-Werte zählen als nicht gesetzt.
fn dsn_from_env() -> Option<String> {
    for key in [ENV_DSN_PRIMARY, ENV_DSN_FALLBACK] {
        if let Ok(v) = std::env::var(key) {
            let trimmed = v.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Liest das 5h-Token-Budget aus der Umgebung. 0/leer/ungültig → 0 (aus).
fn budget_from_env() -> i64 {
    std::env::var(ENV_BUDGET)
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(0)
}

/// Baut den Ledger-Pool gegen die zentrale Postgres und stellt das Schema sicher.
/// Ohne DSN (`TWITCH_ANALYTICS_DSN`/`DATABASE_URL`) scheitert der Aufbau bewusst —
/// der Aufrufer loggt und macht best-effort weiter.
async fn build_pool() -> sqlx::Result<PgPool> {
    let dsn = dsn_from_env().ok_or_else(|| {
        sqlx::Error::Configuration(
            "kein DSN (TWITCH_ANALYTICS_DSN/DATABASE_URL) gesetzt".into(),
        )
    })?;
    // Wenige Verbindungen genügen — das Ledger wird nur sporadisch beschrieben.
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&dsn)
        .await?;
    ensure_schema(&pool).await?;
    Ok(pool)
}

/// Stellt das Ledger-Schema best-effort sicher (self-sufficient auch ohne die
/// Migration). `CREATE TABLE/INDEX IF NOT EXISTS` sind idempotent; existiert die
/// Tabelle bereits (Regelfall via Migration), sind die Statements No-Ops.
async fn ensure_schema(pool: &PgPool) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS minimax_usage (
            id         BIGSERIAL PRIMARY KEY,
            ts         TEXT      NOT NULL,
            source     TEXT      NOT NULL,
            purpose    TEXT,
            model      TEXT,
            tokens_in  BIGINT    DEFAULT 0,
            tokens_out BIGINT    DEFAULT 0,
            total      BIGINT    DEFAULT 0,
            success    BIGINT    DEFAULT 1,
            meta       TEXT
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_mmu_ts ON minimax_usage(ts)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_mmu_source ON minimax_usage(source)")
        .execute(pool)
        .await?;
    Ok(())
}

/// Holt den gecachten Pool oder baut ihn beim ersten Mal. `None`, wenn der Aufbau
/// scheitert (z. B. kein DSN, DB nicht erreichbar) — dann loggt der Aufrufer und
/// macht best-effort weiter.
async fn pool() -> Option<&'static PgPool> {
    POOL.get_or_try_init(build_pool).await.map_or_else(
        |err| {
            tracing::warn!(error = %err, "MiniMax-Usage-Ledger: Pool-Aufbau fehlgeschlagen");
            None
        },
        Some,
    )
}

/// Verbucht einen einzelnen MiniMax-Call im Ledger unter [`SOURCE`]. **Best-effort:**
/// wirft nie, blockiert den LLM-Call nicht und schluckt jeden Fehler in einen
/// `warn`-Log.
///
/// - `purpose` — Verwendungszweck (z. B. `engagement`, `spam-review`, `title`);
///   leer → `NULL` (wie Python).
/// - `model`   — Modellname; leer → `NULL`.
/// - `tokens_in`/`tokens_out` — echte Werte aus dem `usage`-Objekt der Antwort.
/// - `success` — ob der Call erfolgreich war (1/0).
///
/// `ts` wird als ISO-8601 UTC mit Sekunden-Auflösung und `+00:00`-Offset
/// geschrieben — byte-gleich zum Python-Stil
/// `datetime.now(timezone.utc).isoformat(timespec="seconds")`. `total` =
/// `tokens_in + tokens_out`.
pub async fn record(purpose: &str, model: &str, tokens_in: i64, tokens_out: i64, success: bool) {
    let Some(pool) = pool().await else {
        return;
    };
    if let Err(err) =
        record_with_pool(pool, SOURCE, purpose, model, tokens_in, tokens_out, success).await
    {
        tracing::warn!(error = %err, source = SOURCE, "MiniMax-Usage-Ledger: record fehlgeschlagen");
    }
}

/// Kern-Insert gegen einen expliziten Pool — von [`record`] (gecachter Pool) und
/// den Tests (Temp-Pool) genutzt. Trennt die SQL-Logik vom prozessweiten Cache.
async fn record_with_pool(
    pool: &PgPool,
    source: &str,
    purpose: &str,
    model: &str,
    tokens_in: i64,
    tokens_out: i64,
    success: bool,
) -> sqlx::Result<()> {
    let ti = tokens_in.max(0);
    let to = tokens_out.max(0);
    let ts = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, false);
    // Leere purpose/model als NULL ablegen — Parität mit `purpose or None`.
    let purpose_opt = if purpose.trim().is_empty() {
        None
    } else {
        Some(purpose)
    };
    let model_opt = if model.trim().is_empty() {
        None
    } else {
        Some(model)
    };
    let total = ti + to;
    let success_int = if success { 1_i64 } else { 0_i64 };
    let meta: Option<&str> = None;

    sqlx::query(
        r#"
        INSERT INTO minimax_usage
            (ts, source, purpose, model, tokens_in, tokens_out, total, success, meta)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(ts)
    .bind(source)
    .bind(purpose_opt)
    .bind(model_opt)
    .bind(ti)
    .bind(to)
    .bind(total)
    .bind(success_int)
    .bind(meta)
    .execute(pool)
    .await?;
    Ok(())
}

/// Summe der `total`-Tokens im rollierenden Fenster der letzten `hours` Stunden.
///
/// Nutzt dieselbe Bedingung wie der Python-Helfer
/// (`ts >= datetime('now','-N hours')`), damit alle Seiten identisch zählen.
/// **Best-effort:** bei jedem Fehler `0` + `warn`-Log.
pub async fn window_tokens(hours: i64) -> i64 {
    let Some(pool) = pool().await else {
        return 0;
    };
    match window_tokens_with_pool(pool, hours).await {
        Ok(sum) => sum,
        Err(err) => {
            tracing::warn!(error = %err, "MiniMax-Usage-Ledger: window_tokens fehlgeschlagen");
            0
        }
    }
}

/// Kern-Abfrage des Fensters gegen einen expliziten Pool — von [`window_tokens`]
/// (gecachter Pool) und den Tests (Temp-Pool) genutzt.
async fn window_tokens_with_pool(pool: &PgPool, hours: i64) -> sqlx::Result<i64> {
    // `make_interval` nimmt int4 → hours auf i32 klemmen; negatives → 0.
    let hours = i32::try_from(hours.max(0)).unwrap_or(i32::MAX);
    // Textbasiertes Fenster (ts ist TEXT, siehe Modul-Doc): der Schwellwert wird
    // als ISO-8601-UTC-String im **exakt gleichen** Format wie beim Schreiben
    // gebildet (`YYYY-MM-DDThh:mm:ss+00:00`) und lexikografisch verglichen. Das
    // spiegelt Pythons `ts >= datetime('now','-N hours')`, ohne `ts` zu casten.
    sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COALESCE(SUM(total), 0)::bigint
        FROM minimax_usage
        WHERE ts >= to_char(
            (now() AT TIME ZONE 'UTC') - make_interval(hours => $1),
            'YYYY-MM-DD"T"HH24:MI:SS'
        ) || '+00:00'
        "#,
    )
    .bind(hours)
    .fetch_one(pool)
    .await
}

/// Misst den 5h-Verbrauch und **warnt** bei Budget-Überschreitung — KEIN Block,
/// KEIN Fehler. Budget aus Env `MINIMAX_5H_TOKEN_BUDGET` (0/leer = aus).
///
/// Damit nicht jeder Call die DB für das Fenster-`SUM` anfasst, ist die Prüfung
/// auf höchstens einmal pro [`BUDGET_CHECK_INTERVAL`] (60 s) gedrosselt: liegt die
/// letzte Prüfung näher zurück, kehrt die Funktion sofort zurück.
pub async fn warn_if_over_budget() {
    let budget = budget_from_env();
    if budget <= 0 {
        return; // Budget aus → nichts zu tun.
    }

    // Drosseln: nur prüfen, wenn das Intervall seit der letzten Prüfung um ist.
    {
        let mut last = LAST_BUDGET_CHECK.lock().await;
        let now = Instant::now();
        if let Some(prev) = *last {
            if now.duration_since(prev) < BUDGET_CHECK_INTERVAL {
                return;
            }
        }
        *last = Some(now);
    }

    let used = window_tokens(WINDOW_HOURS).await;
    if used > budget {
        tracing::warn!(
            used,
            budget,
            window_hours = WINDOW_HOURS,
            "MiniMax 5h-Token-Budget überschritten (nur Warnung, kein Block)"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use std::sync::Mutex as StdMutex;

    /// Serialisiert die Tests, die prozessglobale Env-Variablen anfassen
    /// (`set_var`/`remove_var`), damit sie sich nicht gegenseitig sehen. Die
    /// DB-Tests laufen über explizite Pools und brauchen diesen Lock nicht.
    static ENV_LOCK: StdMutex<()> = StdMutex::new(());

    /// Öffnet einen frischen, isolierten Postgres-Schema-Pool mit Ledger-Tabelle —
    /// entkoppelt vom gecachten Prozess-Pool, damit jeder Test seine eigene Tabelle
    /// prüfen kann. Ohne `TB_TEST_DATABASE_URL` (keine Test-DB) → `None`, der Test
    /// überspringt sich. Nutzt die produktive [`ensure_schema`] für 1:1-Aufbau.
    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .expect("Test-DSN verbinden");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .expect("Schema-Pool");
        ensure_schema(&pool).await.expect("Schema");
        Some(pool)
    }

    #[tokio::test]
    async fn record_schreibt_zeile_mit_korrekten_spalten() {
        let Some(pool) = make_pool("t_mmu_record").await else {
            return;
        };
        record_with_pool(&pool, SOURCE, "engagement", "MiniMax-M3", 120, 80, true)
            .await
            .expect("Insert");

        // Rückgelesen per Runtime-Query (kein Makro → kein Test-Cache nötig).
        let (ts, source, purpose, model, tokens_in, tokens_out, total, success): (
            String,
            String,
            Option<String>,
            Option<String>,
            i64,
            i64,
            i64,
            i64,
        ) = sqlx::query_as(
            "SELECT ts, source, purpose, model, tokens_in, tokens_out, total, success \
             FROM minimax_usage ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .expect("Zeile vorhanden");

        assert_eq!(source, "twitch-bot");
        assert_eq!(purpose.as_deref(), Some("engagement"));
        assert_eq!(model.as_deref(), Some("MiniMax-M3"));
        assert_eq!(tokens_in, 120);
        assert_eq!(tokens_out, 80);
        assert_eq!(total, 200, "total = tokens_in + tokens_out");
        assert_eq!(success, 1);
        // ts-Stil: ISO-8601 UTC mit +00:00 (Python-kompatibel).
        assert!(ts.ends_with("+00:00"), "ts endet auf +00:00: {ts}");
        assert!(ts.contains('T'));
    }

    #[tokio::test]
    async fn schema_hat_exakt_die_python_spalten() {
        // Schema-Kompatibilität: identische Spaltennamen/-reihenfolge wie im
        // Python-Helfer (via information_schema statt SQLite-pragma).
        let Some(pool) = make_pool("t_mmu_schema").await else {
            return;
        };
        let cols: Vec<String> = sqlx::query_scalar(
            "SELECT column_name FROM information_schema.columns \
             WHERE table_schema = current_schema() AND table_name = 'minimax_usage' \
             ORDER BY ordinal_position",
        )
        .fetch_all(&pool)
        .await
        .expect("columns");
        assert_eq!(
            cols,
            vec![
                "id",
                "ts",
                "source",
                "purpose",
                "model",
                "tokens_in",
                "tokens_out",
                "total",
                "success",
                "meta"
            ]
        );
    }

    #[tokio::test]
    async fn leere_purpose_und_model_werden_null() {
        let Some(pool) = make_pool("t_mmu_nulls").await else {
            return;
        };
        record_with_pool(&pool, SOURCE, "  ", "", 5, 5, false)
            .await
            .expect("Insert");

        let (purpose, model, success): (Option<String>, Option<String>, i64) = sqlx::query_as(
            "SELECT purpose, model, success FROM minimax_usage ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .expect("Zeile");
        assert_eq!(purpose, None, "leere purpose → NULL");
        assert_eq!(model, None, "leeres model → NULL");
        assert_eq!(success, 0, "success=false → 0");
    }

    #[tokio::test]
    async fn record_clampt_negative_tokens_auf_null() {
        let Some(pool) = make_pool("t_mmu_clamp").await else {
            return;
        };
        record_with_pool(&pool, SOURCE, "engagement", "MiniMax-M3", -5, -10, true)
            .await
            .expect("Insert");
        let (tokens_in, tokens_out, total): (i64, i64, i64) = sqlx::query_as(
            "SELECT tokens_in, tokens_out, total FROM minimax_usage ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .expect("Zeile");
        assert_eq!((tokens_in, tokens_out, total), (0, 0, 0));
    }

    #[tokio::test]
    async fn window_tokens_summiert_nur_letzte_5h() {
        let Some(pool) = make_pool("t_mmu_window").await else {
            return;
        };
        // ts im exakten Schreibformat (ISO-8601 UTC, Sekunden, +00:00) bilden.
        let now_ts = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, false);
        let old_ts = (Utc::now() - chrono::Duration::hours(6))
            .to_rfc3339_opts(SecondsFormat::Secs, false);
        // Aktuell (zählt): zweimal je 100 total.
        for _ in 0..2 {
            sqlx::query("INSERT INTO minimax_usage (ts, source, total) VALUES ($1, 'twitch-bot', 100)")
                .bind(&now_ts)
                .execute(&pool)
                .await
                .unwrap();
        }
        // Alt (>5h, zählt NICHT): 999 total vor 6 Stunden.
        sqlx::query("INSERT INTO minimax_usage (ts, source, total) VALUES ($1, 'twitch-bot', 999)")
            .bind(&old_ts)
            .execute(&pool)
            .await
            .unwrap();

        let sum = window_tokens_with_pool(&pool, 5)
            .await
            .expect("Fenster-Summe");
        assert_eq!(
            sum, 200,
            "nur die zwei aktuellen 100er zählen, nicht die alten 999"
        );
    }

    #[test]
    fn budget_from_env_parst_und_faellt_auf_null() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var(ENV_BUDGET, "50000");
        assert_eq!(budget_from_env(), 50_000);
        std::env::set_var(ENV_BUDGET, "");
        assert_eq!(budget_from_env(), 0);
        std::env::set_var(ENV_BUDGET, "kaputt");
        assert_eq!(budget_from_env(), 0);
        std::env::remove_var(ENV_BUDGET);
        assert_eq!(budget_from_env(), 0);
    }

    #[test]
    fn dsn_from_env_bevorzugt_primary_dann_fallback() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::remove_var(ENV_DSN_PRIMARY);
        std::env::remove_var(ENV_DSN_FALLBACK);
        assert_eq!(dsn_from_env(), None, "ohne Env → None");

        std::env::set_var(ENV_DSN_FALLBACK, "postgres://fallback");
        assert_eq!(dsn_from_env().as_deref(), Some("postgres://fallback"));

        std::env::set_var(ENV_DSN_PRIMARY, "postgres://primary");
        assert_eq!(
            dsn_from_env().as_deref(),
            Some("postgres://primary"),
            "TWITCH_ANALYTICS_DSN hat Vorrang"
        );

        // Whitespace-only zählt als nicht gesetzt → Fallback greift.
        std::env::set_var(ENV_DSN_PRIMARY, "   ");
        assert_eq!(dsn_from_env().as_deref(), Some("postgres://fallback"));

        std::env::remove_var(ENV_DSN_PRIMARY);
        std::env::remove_var(ENV_DSN_FALLBACK);
    }
}
