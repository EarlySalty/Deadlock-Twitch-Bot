//! Gemeinsames MiniMax-Usage-Ledger (geteiltes SQLite über alle Bots/Worker).
//!
//! Jeder MiniMax-Call wird hier mit den **echten** Token-Zahlen aus der
//! API-Antwort verbucht — pro Quelle (`source`) und Zweck (`purpose`). Dasselbe
//! SQLite nutzen auch der Python-Helfer
//! `~/Documents/.claude/minimax-usage/minimax_usage.py` und der Rust-TradingBot
//! (`tb-ai`); das Schema ist byte-genau identisch (gleiche Spalten, gleicher
//! `ts`-Stil, gleiche Fenster-Logik), damit alle Seiten dieselbe Datei
//! lesen/schreiben.
//!
//! **Best-effort-Prinzip:** Tracking darf den eigentlichen LLM-Call NIE kippen.
//! Jeder Fehler (Ledger nicht erreichbar, Schreibfehler) wird ausschließlich per
//! [`tracing::warn`] geloggt und verschluckt — der Aufrufer läuft weiter. Secrets
//! landen niemals im Ledger oder Log.
//!
//! **Pfad:** Env `MINIMAX_USAGE_DB`, sonst `~/.claude/minimax-usage/ledger.db`.
//!
//! **Quelle:** dieser Bot schreibt durchgängig [`SOURCE`] (`twitch-bot`) — der
//! Identifier, mit dem die rollierende 5h-Budget-Logik den Verbrauch dieses Bots
//! vom Verbrauch anderer Bots trennt.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use chrono::{SecondsFormat, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;
use tokio::sync::{Mutex, OnceCell};

/// Quellen-Kennung dieses Bots im geteilten Ledger (Python: `source="twitch-bot"`).
pub const SOURCE: &str = "twitch-bot";

/// Env-Variable für den Ledger-Pfad (gleich wie im Python-Helfer).
const ENV_DB_PATH: &str = "MINIMAX_USAGE_DB";
/// Env-Variable für das rollierende 5h-Token-Budget (0/leer = aus).
const ENV_BUDGET: &str = "MINIMAX_5H_TOKEN_BUDGET";
/// Standard-Fensterbreite in Stunden (Python: `WINDOW_HOURS = 5`).
const WINDOW_HOURS: i64 = 5;
/// Mindestabstand zwischen zwei Budget-Prüfungen, damit nicht jeder Call die DB
/// für die teurere `SUM`-Abfrage anfasst (siehe [`warn_if_over_budget`]).
const BUDGET_CHECK_INTERVAL: Duration = Duration::from_secs(60);

/// Lazy gebauter, prozessweit gecachter Pool auf das Ledger-SQLite.
///
/// Der Pool wird beim ersten Zugriff einmal aufgebaut (Schema-Ensure inklusive)
/// und danach wiederverwendet. Scheitert der Aufbau, bleibt der Cache leer und
/// jeder Aufruf versucht es erneut (best-effort, kein dauerhaftes Vergiften).
static POOL: OnceCell<SqlitePool> = OnceCell::const_new();

/// Zeitpunkt der letzten Budget-Prüfung. `None` = noch nie geprüft. Drosselt
/// [`warn_if_over_budget`] auf höchstens eine DB-Abfrage pro
/// [`BUDGET_CHECK_INTERVAL`], damit das Fenster-`SUM` nicht bei jedem Call läuft.
static LAST_BUDGET_CHECK: Mutex<Option<Instant>> = Mutex::const_new(None);

/// Ermittelt den Ledger-Pfad: Env `MINIMAX_USAGE_DB`, sonst
/// `~/.claude/minimax-usage/ledger.db`. Identisch zum Python-Default.
fn resolve_path() -> PathBuf {
    if let Ok(v) = std::env::var(ENV_DB_PATH) {
        let trimmed = v.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".claude")
        .join("minimax-usage")
        .join("ledger.db")
}

/// Liest das 5h-Token-Budget aus der Umgebung. 0/leer/ungültig → 0 (aus).
fn budget_from_env() -> i64 {
    std::env::var(ENV_BUDGET)
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(0)
}

/// Baut den Ledger-Pool und stellt das Schema sicher. Legt das Parent-Verzeichnis
/// an, öffnet WAL/`create_if_missing` und läuft das Schema einmal durch.
async fn build_pool() -> sqlx::Result<SqlitePool> {
    let path = resolve_path();
    if let Some(parent) = path.parent() {
        // Best-effort: schlägt das Anlegen fehl, scheitert gleich der Connect mit
        // sprechendem Fehler — wir verschlucken hier nichts Stilles.
        let _ = std::fs::create_dir_all(parent);
    }
    let options = SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_millis(10_000));
    // Wenige Verbindungen genügen — das Ledger wird nur sporadisch beschrieben.
    let pool = SqlitePoolOptions::new()
        .max_connections(2)
        .connect_with(options)
        .await?;
    ensure_schema(&pool).await?;
    Ok(pool)
}

/// Stellt das Python-kompatible Ledger-Schema sicher. Die Statements sind
/// einzeln gehalten, damit sqlx sie gegen den separaten SQLite-Cache prüfen kann.
async fn ensure_schema(pool: &SqlitePool) -> sqlx::Result<()> {
    sqlx::query!(
        r#"
        CREATE TABLE IF NOT EXISTS minimax_usage (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            ts         TEXT    NOT NULL,
            source     TEXT    NOT NULL,
            purpose    TEXT,
            model      TEXT,
            tokens_in  INTEGER DEFAULT 0,
            tokens_out INTEGER DEFAULT 0,
            total      INTEGER DEFAULT 0,
            success    INTEGER DEFAULT 1,
            meta       TEXT
        )
        "#
    )
    .execute(pool)
    .await?;
    sqlx::query!(
        r#"
        CREATE INDEX IF NOT EXISTS idx_mmu_ts ON minimax_usage(ts)
        "#
    )
    .execute(pool)
    .await?;
    sqlx::query!(
        r#"
        CREATE INDEX IF NOT EXISTS idx_mmu_source ON minimax_usage(source)
        "#
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Holt den gecachten Pool oder baut ihn beim ersten Mal. `None`, wenn der Aufbau
/// scheitert (z. B. Pfad nicht beschreibbar) — dann loggt der Aufrufer und macht
/// best-effort weiter.
async fn pool() -> Option<&'static SqlitePool> {
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
    pool: &SqlitePool,
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

    sqlx::query!(
        r#"
        INSERT INTO minimax_usage
            (ts, source, purpose, model, tokens_in, tokens_out, total, success, meta)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
        ts,
        source,
        purpose_opt,
        model_opt,
        ti,
        to,
        total,
        success_int,
        meta
    )
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
async fn window_tokens_with_pool(pool: &SqlitePool, hours: i64) -> sqlx::Result<i64> {
    let hours = hours.max(0);
    sqlx::query_scalar!(
        r#"
        SELECT COALESCE(SUM(total), 0) AS "sum!: i64"
        FROM minimax_usage
        WHERE ts >= datetime('now', '-' || ? || ' hours')
        "#,
        hours
    )
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
    use std::sync::Mutex as StdMutex;

    /// Serialisiert die Tests, die prozessglobale Env-Variablen anfassen
    /// (`set_var`/`remove_var`), damit sie sich nicht gegenseitig sehen. Die
    /// DB-Tests laufen über explizite Temp-Pools und brauchen diesen Lock nicht.
    static ENV_LOCK: StdMutex<()> = StdMutex::new(());

    /// Öffnet einen frischen Temp-Pool mit Schema — entkoppelt vom gecachten
    /// Prozess-Pool, damit jeder Test seine eigene Datei prüfen kann. Genau
    /// derselbe Schema-/PRAGMA-Aufbau wie [`build_pool`].
    async fn open_temp(name: &str) -> SqlitePool {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "tb_llm_ledger_test_{}_{}.db",
            name,
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("Temp-Ledger öffnen");
        ensure_schema(&pool).await.expect("Schema");
        pool
    }

    #[tokio::test]
    async fn record_schreibt_zeile_mit_korrekten_spalten() {
        let pool = open_temp("record").await;
        record_with_pool(&pool, SOURCE, "engagement", "MiniMax-M3", 120, 80, true)
            .await
            .expect("Insert");

        let row = sqlx::query!(
            r#"
            SELECT
                ts AS "ts!: String",
                source AS "source!: String",
                purpose AS "purpose?: String",
                model AS "model?: String",
                tokens_in AS "tokens_in!: i64",
                tokens_out AS "tokens_out!: i64",
                total AS "total!: i64",
                success AS "success!: i64"
            FROM minimax_usage
            ORDER BY id DESC
            LIMIT 1
            "#
        )
        .fetch_one(&pool)
        .await
        .expect("Zeile vorhanden");

        assert_eq!(row.source, "twitch-bot");
        assert_eq!(row.purpose.as_deref(), Some("engagement"));
        assert_eq!(row.model.as_deref(), Some("MiniMax-M3"));
        assert_eq!(row.tokens_in, 120);
        assert_eq!(row.tokens_out, 80);
        assert_eq!(row.total, 200, "total = tokens_in + tokens_out");
        assert_eq!(row.success, 1);
        // ts-Stil: ISO-8601 UTC mit +00:00 (Python-kompatibel).
        assert!(
            row.ts.ends_with("+00:00"),
            "ts endet auf +00:00: {}",
            row.ts
        );
        assert!(row.ts.contains('T'));
    }

    #[tokio::test]
    async fn schema_hat_exakt_die_python_spalten() {
        // Schema-Kompatibilität: identische Spaltennamen wie im Python-Helfer.
        let pool = open_temp("schema").await;
        let cols_csv = sqlx::query_scalar!(
            r#"
            SELECT group_concat(name, ',') AS "cols!: String"
            FROM (
                SELECT name
                FROM pragma_table_info('minimax_usage')
                ORDER BY cid
            )
            "#
        )
        .fetch_one(&pool)
        .await
        .expect("table_info");
        let cols: Vec<&str> = cols_csv.split(',').collect();
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
        let pool = open_temp("nulls").await;
        record_with_pool(&pool, SOURCE, "  ", "", 5, 5, false)
            .await
            .expect("Insert");

        let row = sqlx::query!(
            r#"
            SELECT
                purpose AS "purpose?: String",
                model AS "model?: String",
                success AS "success!: i64"
            FROM minimax_usage
            ORDER BY id DESC
            LIMIT 1
            "#
        )
        .fetch_one(&pool)
        .await
        .expect("Zeile");
        assert_eq!(row.purpose, None, "leere purpose → NULL");
        assert_eq!(row.model, None, "leeres model → NULL");
        assert_eq!(row.success, 0, "success=false → 0");
    }

    #[tokio::test]
    async fn record_clampt_negative_tokens_auf_null() {
        let pool = open_temp("clamp").await;
        record_with_pool(&pool, SOURCE, "engagement", "MiniMax-M3", -5, -10, true)
            .await
            .expect("Insert");
        let row = sqlx::query!(
            r#"
            SELECT
                tokens_in AS "tokens_in!: i64",
                tokens_out AS "tokens_out!: i64",
                total AS "total!: i64"
            FROM minimax_usage
            ORDER BY id DESC
            LIMIT 1
            "#
        )
        .fetch_one(&pool)
        .await
        .expect("Zeile");
        assert_eq!((row.tokens_in, row.tokens_out, row.total), (0, 0, 0));
    }

    #[tokio::test]
    async fn window_tokens_summiert_nur_letzte_5h() {
        let pool = open_temp("window").await;
        // Aktuell (zählt): zweimal je 100 total.
        for _ in 0..2 {
            sqlx::query!(
                r#"
                INSERT INTO minimax_usage (ts, source, total)
                VALUES (datetime('now'), 'twitch-bot', 100)
                "#
            )
            .execute(&pool)
            .await
            .unwrap();
        }
        // Alt (>5h, zählt NICHT): 999 total vor 6 Stunden.
        sqlx::query!(
            r#"
            INSERT INTO minimax_usage (ts, source, total)
            VALUES (datetime('now', '-6 hours'), 'twitch-bot', 999)
            "#
        )
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
    fn resolve_path_nimmt_env_vor_default() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var(ENV_DB_PATH, "/tmp/mein_ledger.db");
        assert_eq!(resolve_path(), PathBuf::from("/tmp/mein_ledger.db"));
        std::env::remove_var(ENV_DB_PATH);
        // Ohne Env endet der Default auf dem bekannten Pfad-Suffix.
        let p = resolve_path();
        assert!(
            p.ends_with(".claude/minimax-usage/ledger.db"),
            "Default-Pfad: {p:?}"
        );
    }
}
