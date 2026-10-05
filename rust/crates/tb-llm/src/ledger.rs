//! Zentrale Postgres-Verbrauchserfassung ohne Prompts oder Chatinhalte.

use chrono::{SecondsFormat, Utc};
use sqlx::postgres::PgPool;
use tokio::sync::OnceCell;

/// Quellen-Kennung dieses Bots im geteilten Ledger (Python: `source="twitch-bot"`).
pub const SOURCE: &str = "twitch-bot";

/// Dienststart übergibt denselben Pool wie den übrigen Schreibern.
static POOL: OnceCell<PgPool> = OnceCell::const_new();

/// Dienststart übergibt den vorhandenen Pool und die überprüfbare Herkunft.
static IDENTITY: OnceCell<(&'static str, &'static str)> = OnceCell::const_new();
pub fn initialize(
    pool: PgPool,
    project: &'static str,
    service: &'static str,
) -> Result<(), &'static str> {
    if project.is_empty() || service.is_empty() {
        return Err("Verbrauchsherkunft fehlt");
    }
    POOL.set(pool)
        .map_err(|_| "Verbrauchspool bereits gesetzt")?;
    IDENTITY
        .set((project, service))
        .map_err(|_| "Verbrauchsherkunft bereits gesetzt")?;
    Ok(())
}
pub(crate) async fn pool() -> Option<&'static PgPool> {
    POOL.get()
}

/// Vor jedem kostenpflichtigen Versuch muss diese Zeile dauerhaft stehen.
pub async fn begin(purpose: &str, model: &str, provider: &str) -> Result<i64, crate::LlmError> {
    let fail = || {
        crate::LlmError::Unavailable(
            "Verbrauchserfassung nicht bereit; KI-Aufruf angehalten".into(),
        )
    };
    let pool = POOL.get().ok_or_else(fail)?;
    let (project, service) = IDENTITY.get().ok_or_else(fail)?;
    sqlx::query_scalar("INSERT INTO public.llm_usage (ts,source,purpose,model,tokens_in,tokens_out,total,success,project,service,provider,attempt_state) VALUES ($1,$2,$3,$4,NULL,NULL,NULL,0,$5,$6,$7,'started') RETURNING id")
        .bind(Utc::now().to_rfc3339_opts(SecondsFormat::Secs, false))
        .bind(SOURCE).bind(purpose).bind(model).bind(project).bind(service).bind(provider)
        .fetch_one(pool).await.map_err(|_| {
            tracing::error!(purpose, provider, "LLM_USAGE_BLOCKED: Start konnte nicht gespeichert werden");
            fail()
        })
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Completion {
    pub id: i64,
    pub tokens_in: Option<i64>,
    pub tokens_out: Option<i64>,
    pub total: Option<i64>,
    pub request_id: Option<String>,
    pub success: bool,
    pub error_code: Option<String>,
    pub http_status: Option<i32>,
    pub latency_ms: i64,
}
/// Auch nach einem Abschlussfehler bleibt der bereits gespeicherte Start sichtbar.
/// Die strukturierte Journalzeile enthält nur Zähler und technische IDs.
pub(crate) fn journal_completion(completion: &Completion) {
    let (project, service) = IDENTITY.get().expect("Erfasster Versuch besitzt Herkunft");
    let recovery = serde_json::to_string(&Recovery {
        project: (*project).into(),
        service: (*service).into(),
        completion: completion.clone(),
    })
    .expect("Zähler sind serialisierbar");
    eprintln!("LLM_USAGE_RECOVERY recovery={recovery}");
}

pub async fn finish(completion: Completion) {
    if recover(&completion).await.is_err() {
        let (project, service) = IDENTITY.get().expect("Erfasster Versuch besitzt Herkunft");
        let recovery = serde_json::to_string(&Recovery {
            project: (*project).into(),
            service: (*service).into(),
            completion,
        })
        .expect("Zähler sind serialisierbar");
        tracing::error!(recovery = %recovery, "LLM_USAGE_RECOVERY: Abschluss aus dem Journal nachliefern");
    }
}
/// Idempotente Wiederaufnahme eines Abschlusses aus LLM_USAGE_RECOVERY.
pub async fn recover(c: &Completion) -> Result<(), RecoveryError> {
    let pool = POOL.get().ok_or(RecoveryError)?;
    let (project, service) = IDENTITY.get().ok_or(RecoveryError)?;
    recover_with_pool(pool, c, project, service).await
}

#[derive(Debug)]
pub struct RecoveryError;
impl std::fmt::Display for RecoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Verbrauchsabschluss konnte nicht gespeichert werden")
    }
}
impl std::error::Error for RecoveryError {}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recovery {
    pub project: String,
    pub service: String,
    pub completion: Completion,
}
pub async fn recover_with_pool(
    pool: &PgPool,
    c: &Completion,
    project: &str,
    service: &str,
) -> Result<(), RecoveryError> {
    let result = sqlx::query("UPDATE public.llm_usage SET tokens_in=$2,tokens_out=$3,total=$4,success=$5,attempt_state=$6,request_id=$7,error_code=$8,http_status=$9,latency_ms=$10,finished_at=now() WHERE id=$1 AND attempt_state='started' AND project=$11 AND service=$12")
        .bind(c.id).bind(c.tokens_in).bind(c.tokens_out).bind(c.total)
        .bind(i64::from(c.success)).bind(if c.success { "succeeded" } else { "failed" })
        .bind(&c.request_id).bind(&c.error_code).bind(c.http_status).bind(c.latency_ms).bind(project).bind(service)
        .execute(pool).await.map_err(|_| RecoveryError)?;
    if result.rows_affected() == 0 {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM public.llm_usage WHERE id=$1 AND project=$2 AND service=$3 AND attempt_state IN ('succeeded','failed'))")
            .bind(c.id).bind(project).bind(service).fetch_one(pool).await.map_err(|_| RecoveryError)?;
        if !exists {
            return Err(RecoveryError);
        }
    }
    Ok(())
}

#[cfg(test)]
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
        INSERT INTO llm_usage
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

#[cfg(test)]
async fn window_tokens_with_pool(pool: &PgPool, hours: i64) -> sqlx::Result<i64> {
    let hours = i32::try_from(hours.max(0)).unwrap_or(i32::MAX);
    // Textbasiertes Fenster (ts ist TEXT, siehe Modul-Doc): der Schwellwert wird
    // als ISO-8601-UTC-String im **exakt gleichen** Format wie beim Schreiben
    // gebildet (`YYYY-MM-DDThh:mm:ss+00:00`) und lexikografisch verglichen.
    sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COALESCE(SUM(total), 0)::bigint
        FROM llm_usage
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

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    #[test]
    fn journalbeleg_bleibt_bei_error_filter_sichtbar() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "ledger::tests::journalbeleg_stderr_kind",
                "--nocapture",
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        let stderr = String::from_utf8(output.stderr).unwrap();
        let record = stderr
            .lines()
            .find_map(|line| line.strip_prefix("LLM_USAGE_RECOVERY recovery="))
            .expect("Vorabbeleg wird unabhängig vom Tracingfilter geschrieben");
        let recovery: Recovery = serde_json::from_str(record).unwrap();
        assert_eq!(recovery.completion.total, Some(15));
    }

    #[tokio::test]
    #[ignore = "Isolierter Kindprozess für den Error-Filter"]
    async fn journalbeleg_stderr_kind() {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::ERROR)
            .init();
        let pool =
            PgPoolOptions::new().connect_lazy_with(PgConnectOptions::new().database("fixture"));
        initialize(pool, "twitch-test", "stderr-fixture").unwrap();
        journal_completion(&Completion {
            id: 7,
            tokens_in: Some(12),
            tokens_out: Some(3),
            total: Some(15),
            request_id: None,
            success: true,
            error_code: None,
            http_status: Some(200),
            latency_ms: 1,
        });
    }

    /// Öffnet einen frischen, isolierten Postgres-Schema-Pool mit Ledger-Tabelle —
    /// entkoppelt vom gecachten Prozess-Pool, damit jeder Test seine eigene Tabelle
    /// prüfen kann. Ohne `TB_TEST_DATABASE_URL` (keine Test-DB) → `None`, der Test
    /// überspringt sich. Das Fixture entspricht der produktiven Migration.
    async fn make_pool(schema: &str) -> Option<PgPool> {
        // Schemanamen sind SQL-Bezeichner und können nicht gebunden werden.
        assert!(
            schema.starts_with("t_mmu_")
                && schema
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_'),
            "Ungültiger Ledger-Testschemaname"
        );
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .expect("Test-DSN verbinden");
        sqlx::QueryBuilder::<sqlx::Postgres>::new("DROP SCHEMA IF EXISTS ")
            .push(schema)
            .push(" CASCADE")
            .build()
            .execute(&admin)
            .await
            .unwrap();
        sqlx::QueryBuilder::<sqlx::Postgres>::new("CREATE SCHEMA ")
            .push(schema)
            .build()
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
        sqlx::query(
            r#"
            CREATE TABLE llm_usage (
                id BIGSERIAL PRIMARY KEY,
                ts TEXT NOT NULL,
                source TEXT NOT NULL,
                purpose TEXT,
                model TEXT,
                tokens_in BIGINT DEFAULT 0,
                tokens_out BIGINT DEFAULT 0,
                total BIGINT DEFAULT 0,
                success BIGINT DEFAULT 1,
                meta TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("Ledger-Testtabelle");
        Some(pool)
    }

    #[tokio::test]
    async fn record_schreibt_zeile_mit_korrekten_spalten() {
        let Some(pool) = make_pool("t_mmu_record").await else {
            return;
        };
        record_with_pool(
            &pool,
            SOURCE,
            "engagement",
            "deepseek-v4-flash",
            120,
            80,
            true,
        )
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
             FROM llm_usage ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .expect("Zeile vorhanden");

        assert_eq!(source, "twitch-bot");
        assert_eq!(purpose.as_deref(), Some("engagement"));
        assert_eq!(model.as_deref(), Some("deepseek-v4-flash"));
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
             WHERE table_schema = current_schema() AND table_name = 'llm_usage' \
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
            "SELECT purpose, model, success FROM llm_usage ORDER BY id DESC LIMIT 1",
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
        record_with_pool(
            &pool,
            SOURCE,
            "engagement",
            "deepseek-v4-flash",
            -5,
            -10,
            true,
        )
        .await
        .expect("Insert");
        let (tokens_in, tokens_out, total): (i64, i64, i64) = sqlx::query_as(
            "SELECT tokens_in, tokens_out, total FROM llm_usage ORDER BY id DESC LIMIT 1",
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
        let old_ts =
            (Utc::now() - chrono::Duration::hours(6)).to_rfc3339_opts(SecondsFormat::Secs, false);
        // Aktuell (zählt): zweimal je 100 total.
        for _ in 0..2 {
            sqlx::query("INSERT INTO llm_usage (ts, source, total) VALUES ($1, 'twitch-bot', 100)")
                .bind(&now_ts)
                .execute(&pool)
                .await
                .unwrap();
        }
        // Alt (>5h, zählt NICHT): 999 total vor 6 Stunden.
        sqlx::query("INSERT INTO llm_usage (ts, source, total) VALUES ($1, 'twitch-bot', 999)")
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
}
