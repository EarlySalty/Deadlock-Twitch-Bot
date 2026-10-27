use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::postgres::{PgPool, PgPoolOptions};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Ticket {
    pub id: i64,
    pub sha: String,
    pub thread_id: String,
    pub task_dir: String,
    pub deploy_now: bool,
    pub submitted_at: DateTime<Utc>,
    pub state: String,
    pub compaction: String,
    pub compaction_message_id: Option<String>,
    pub compaction_requested_at: Option<DateTime<Utc>>,
    pub compaction_detail: Option<String>,
    pub run_id: Option<i64>,
    pub outcome: Option<String>,
    pub final_message: Option<String>,
    pub notify_attempts: i32,
    pub notify_error: Option<String>,
    pub notified_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Run {
    pub id: i64,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub parent_sha: Option<String>,
    pub target_sha: Option<String>,
    pub outcome: Option<String>,
    pub failed_step: Option<String>,
    pub log_path: String,
    pub built: String,
    pub carried: String,
    pub build_seconds: Option<i32>,
    pub restarted: String,
    pub pruefen_exit: Option<i32>,
    pub exe_report: String,
    pub live_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default)]
pub struct RunRecord {
    pub parent_sha: Option<String>,
    pub target_sha: Option<String>,
    pub outcome: String,
    pub failed_step: Option<String>,
    pub built: String,
    pub carried: String,
    pub build_seconds: Option<i32>,
    pub restarted: String,
    pub pruefen_exit: Option<i32>,
    pub exe_report: String,
    pub live_at: Option<DateTime<Utc>>,
}

const TICKET_COLUMNS: &str = "id, sha, thread_id, task_dir, deploy_now, submitted_at, state, \
     compaction, compaction_message_id, compaction_requested_at, compaction_detail, run_id, \
     outcome, final_message, notify_attempts, notify_error, notified_at";

pub async fn connect(url: &str) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(url)
        .await
        .context("cannot connect to the deploy queue database")?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("deploy queue migration failed")?;
    Ok(pool)
}

pub async fn insert_ticket(
    pool: &PgPool,
    sha: &str,
    thread_id: &str,
    task_dir: &str,
    deploy_now: bool,
) -> Result<i64> {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO tickets (sha, thread_id, task_dir, deploy_now) VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(sha)
    .bind(thread_id)
    .bind(task_dir)
    .bind(deploy_now)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

async fn tickets_where(pool: &PgPool, clause: &str) -> Result<Vec<Ticket>> {
    let sql = format!("SELECT {TICKET_COLUMNS} FROM tickets WHERE {clause} ORDER BY id");
    Ok(sqlx::query_as::<_, Ticket>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await?)
}

pub async fn ticket(pool: &PgPool, id: i64) -> Result<Option<Ticket>> {
    let sql = format!("SELECT {TICKET_COLUMNS} FROM tickets WHERE id = $1");
    Ok(sqlx::query_as::<_, Ticket>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await?)
}

pub async fn recent_tickets(pool: &PgPool) -> Result<Vec<Ticket>> {
    tickets_where(
        pool,
        "notified_at IS NULL OR submitted_at > now() - interval '1 day'",
    )
    .await
}

pub async fn compaction_open(pool: &PgPool) -> Result<Vec<Ticket>> {
    tickets_where(pool, "compaction IN ('pending', 'requested')").await
}

pub async fn pending_tickets(pool: &PgPool) -> Result<Vec<Ticket>> {
    tickets_where(pool, "state = 'pending'").await
}

pub async fn notification_due(pool: &PgPool) -> Result<Vec<Ticket>> {
    tickets_where(
        pool,
        "state = 'done' AND notified_at IS NULL AND compaction NOT IN ('pending', 'requested') \
         AND (notify_next_at IS NULL OR notify_next_at <= now())",
    )
    .await
}

pub async fn set_compaction_requested(pool: &PgPool, id: i64, message_id: &str) -> Result<()> {
    sqlx::query(
        "UPDATE tickets SET compaction = 'requested', compaction_message_id = $2, \
         compaction_requested_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(message_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_compaction(pool: &PgPool, id: i64, state: &str, detail: &str) -> Result<()> {
    sqlx::query("UPDATE tickets SET compaction = $2, compaction_detail = $3 WHERE id = $1")
        .bind(id)
        .bind(state)
        .bind(detail)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn start_run(pool: &PgPool, ticket_ids: &[i64], log_dir: &str) -> Result<(i64, String)> {
    let mut transaction = pool.begin().await?;
    let id: i64 = sqlx::query_scalar("INSERT INTO runs (log_path) VALUES ('') RETURNING id")
        .fetch_one(&mut *transaction)
        .await?;
    let log_path = format!("{log_dir}/run-{id}.log");
    sqlx::query("UPDATE runs SET log_path = $2 WHERE id = $1")
        .bind(id)
        .bind(&log_path)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("UPDATE tickets SET state = 'running', run_id = $1 WHERE id = ANY($2) AND state = 'pending'")
        .bind(id)
        .bind(ticket_ids)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok((id, log_path))
}

pub async fn finish_run(pool: &PgPool, id: i64, record: &RunRecord) -> Result<()> {
    sqlx::query(
        "UPDATE runs SET finished_at = now(), parent_sha = $2, target_sha = $3, outcome = $4, \
         failed_step = $5, built = $6, carried = $7, build_seconds = $8, restarted = $9, \
         pruefen_exit = $10, exe_report = $11, live_at = $12 WHERE id = $1",
    )
    .bind(id)
    .bind(&record.parent_sha)
    .bind(&record.target_sha)
    .bind(&record.outcome)
    .bind(&record.failed_step)
    .bind(&record.built)
    .bind(&record.carried)
    .bind(record.build_seconds)
    .bind(&record.restarted)
    .bind(record.pruefen_exit)
    .bind(&record.exe_report)
    .bind(record.live_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn finish_ticket(pool: &PgPool, id: i64, outcome: &str, message: &str) -> Result<()> {
    sqlx::query(
        "UPDATE tickets SET state = 'done', outcome = $2, final_message = $3 WHERE id = $1",
    )
    .bind(id)
    .bind(outcome)
    .bind(message)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn run(pool: &PgPool, id: i64) -> Result<Option<Run>> {
    Ok(sqlx::query_as::<_, Run>(
        "SELECT id, started_at, finished_at, parent_sha, target_sha, outcome, failed_step, \
         log_path, built, carried, build_seconds, restarted, pruefen_exit, exe_report, live_at \
         FROM runs WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?)
}

pub async fn interrupted_runs(pool: &PgPool) -> Result<Vec<Run>> {
    Ok(sqlx::query_as::<_, Run>(
        "SELECT id, started_at, finished_at, parent_sha, target_sha, outcome, failed_step, \
         log_path, built, carried, build_seconds, restarted, pruefen_exit, exe_report, live_at \
         FROM runs WHERE finished_at IS NULL ORDER BY id",
    )
    .fetch_all(pool)
    .await?)
}

pub async fn run_tickets(pool: &PgPool, run_id: i64) -> Result<Vec<Ticket>> {
    let sql = format!("SELECT {TICKET_COLUMNS} FROM tickets WHERE run_id = $1 ORDER BY id");
    Ok(sqlx::query_as::<_, Ticket>(sqlx::AssertSqlSafe(sql))
        .bind(run_id)
        .fetch_all(pool)
        .await?)
}

pub async fn mark_notified(pool: &PgPool, id: i64) -> Result<()> {
    sqlx::query(
        "UPDATE tickets SET notified_at = now(), notify_attempts = notify_attempts + 1, \
         notify_error = NULL WHERE id = $1",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn notify_failed(pool: &PgPool, id: i64, error: &str) -> Result<()> {
    sqlx::query(
        "UPDATE tickets SET notify_attempts = notify_attempts + 1, notify_error = $2, \
         notify_next_at = now() + least(interval '30 minutes', interval '30 seconds' * power(2, notify_attempts)) \
         WHERE id = $1",
    )
    .bind(id)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}
