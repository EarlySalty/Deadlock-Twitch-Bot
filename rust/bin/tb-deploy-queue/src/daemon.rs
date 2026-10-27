use crate::config::Config;
use crate::db::{self, RunRecord, Ticket};
use crate::plan;
use crate::run::{self, Batch, Report};
use crate::t3::{Client, Compaction};
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Local, Utc};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::task::JoinHandle;

const PREFIX: &str = "[deploy-queue]";

pub async fn serve(config: Config) -> Result<()> {
    let config = Arc::new(config);
    let pool = db::connect(&config.database_url).await?;
    let client = Client::new(&config.t3)?;
    recover(&pool).await?;
    eprintln!("deploy-queue daemon started");
    let mut running: Option<JoinHandle<()>> = None;
    loop {
        if let Err(error) = compaction_tick(&pool, &client, &config).await {
            eprintln!("compaction tick failed: {error:#}");
        }
        if running.as_ref().is_some_and(JoinHandle::is_finished) {
            running = None;
        }
        if running.is_none() {
            match due_batch(&pool, &config).await {
                Ok(Some(tickets)) => {
                    running = Some(spawn_batch(pool.clone(), config.clone(), tickets))
                }
                Ok(None) => {}
                Err(error) => eprintln!("batch check failed: {error:#}"),
            }
        }
        if let Err(error) = notify_tick(&pool, &client).await {
            eprintln!("notify tick failed: {error:#}");
        }
        tokio::time::sleep(std::time::Duration::from_secs(config.poll_seconds)).await;
    }
}

async fn recover(pool: &PgPool) -> Result<()> {
    for interrupted in db::interrupted_runs(pool).await? {
        let record = RunRecord {
            parent_sha: interrupted.parent_sha.clone(),
            target_sha: interrupted.target_sha.clone(),
            outcome: "failed".into(),
            failed_step: Some("daemon-restart".into()),
            ..RunRecord::default()
        };
        db::finish_run(pool, interrupted.id, &record).await?;
        for ticket in db::run_tickets(pool, interrupted.id).await? {
            if ticket.state == "done" {
                continue;
            }
            let message = failure_message(
                &ticket,
                "daemon-restart",
                &interrupted.log_path,
                "the deploy queue daemon stopped while this run was in progress; check the log and `deploy-twitch-release --pruefen` before resubmitting",
                &[],
            );
            db::finish_ticket(pool, ticket.id, "failed", &message).await?;
        }
        eprintln!("run {} marked failed after daemon restart", interrupted.id);
    }
    Ok(())
}

async fn compaction_tick(pool: &PgPool, client: &Client, config: &Config) -> Result<()> {
    for ticket in db::compaction_open(pool).await? {
        if let Err(error) = advance_compaction(pool, client, config, &ticket).await {
            eprintln!("ticket {} compaction: {error:#}", ticket.id);
        }
    }
    Ok(())
}

async fn advance_compaction(
    pool: &PgPool,
    client: &Client,
    config: &Config,
    ticket: &Ticket,
) -> Result<()> {
    let now = Utc::now();
    if ticket.compaction == "pending" {
        let waited = now - ticket.submitted_at;
        let idle_limit = Duration::minutes(config.idle_wait_minutes);
        let thread = match client.thread(&ticket.thread_id).await {
            Ok(thread) => thread,
            Err(error) if waited >= idle_limit => {
                let detail = format!("thread unreachable: {error:#}");
                return db::set_compaction(pool, ticket.id, "skipped", &detail).await;
            }
            Err(error) => return Err(error),
        };
        if thread.is_idle() {
            let message_id = client.send_message(&thread, "/compact").await?;
            return db::set_compaction_requested(pool, ticket.id, &message_id).await;
        }
        if waited >= idle_limit {
            let detail = format!(
                "thread stayed busy for {} minutes",
                config.idle_wait_minutes
            );
            return db::set_compaction(pool, ticket.id, "skipped", &detail).await;
        }
        return Ok(());
    }
    let Some(message_id) = ticket.compaction_message_id.as_deref() else {
        return db::set_compaction(pool, ticket.id, "failed", "no compact message id recorded")
            .await;
    };
    let requested = ticket
        .compaction_requested_at
        .unwrap_or(ticket.submitted_at);
    let timed_out = now - requested >= Duration::minutes(config.compaction_timeout_minutes);
    match client.thread(&ticket.thread_id).await {
        Ok(thread) => match thread.compaction(message_id) {
            Compaction::Confirmed(detail) => {
                db::set_compaction(pool, ticket.id, "confirmed", &detail).await
            }
            Compaction::Failed(reason) => {
                db::set_compaction(pool, ticket.id, "failed", &reason).await
            }
            Compaction::Waiting if timed_out => {
                let detail = format!(
                    "no compaction item after {} minutes",
                    config.compaction_timeout_minutes
                );
                db::set_compaction(pool, ticket.id, "timeout", &detail).await
            }
            Compaction::Waiting => Ok(()),
        },
        Err(error) if timed_out => {
            db::set_compaction(
                pool,
                ticket.id,
                "timeout",
                &format!("thread unreachable: {error:#}"),
            )
            .await
        }
        Err(error) => Err(error),
    }
}

async fn due_batch(pool: &PgPool, config: &Config) -> Result<Option<Vec<Ticket>>> {
    let pending = db::pending_tickets(pool).await?;
    Ok(batch_due(&pending, Utc::now(), config).then_some(pending))
}

fn batch_due(pending: &[Ticket], now: DateTime<Utc>, config: &Config) -> bool {
    let (Some(oldest), Some(newest)) = (
        pending.iter().map(|ticket| ticket.submitted_at).min(),
        pending.iter().map(|ticket| ticket.submitted_at).max(),
    ) else {
        return false;
    };
    pending.iter().any(|ticket| ticket.deploy_now)
        || now - newest >= Duration::minutes(config.quiet_minutes)
        || now - oldest >= Duration::minutes(config.max_wait_minutes)
}

fn spawn_batch(pool: PgPool, config: Arc<Config>, tickets: Vec<Ticket>) -> JoinHandle<()> {
    tokio::spawn(async move {
        if let Err(error) = run_batch(&pool, config, tickets).await {
            eprintln!("batch failed: {error:#}");
        }
    })
}

async fn run_batch(pool: &PgPool, config: Arc<Config>, tickets: Vec<Ticket>) -> Result<()> {
    let ids: Vec<i64> = tickets.iter().map(|ticket| ticket.id).collect();
    let log_dir = config.log_dir.to_string_lossy().into_owned();
    let (run_id, log_path) = db::start_run(pool, &ids, &log_dir).await?;
    eprintln!("run {run_id} started for tickets {ids:?}");
    let batch = Batch {
        run_id,
        log_path: log_path.clone(),
        tickets: tickets
            .iter()
            .map(|ticket| (ticket.id, ticket.sha.clone()))
            .collect(),
    };
    let worker_config = config.clone();
    let report = tokio::task::spawn_blocking(move || run::execute(&worker_config, &batch))
        .await
        .unwrap_or_else(|error| panicked(&error.to_string()));
    db::finish_run(pool, run_id, &report.record).await?;
    for ticket in db::run_tickets(pool, run_id).await? {
        let (outcome, message) = ticket_result(&ticket, &report, &log_path);
        db::finish_ticket(pool, ticket.id, &outcome, &message).await?;
    }
    eprintln!("run {run_id} finished: {}", report.record.outcome);
    Ok(())
}

fn panicked(error: &str) -> Report {
    Report {
        record: RunRecord {
            outcome: "failed".into(),
            failed_step: Some("executor".into()),
            ..RunRecord::default()
        },
        rejected: Vec::new(),
        commits: Vec::new(),
        note: format!("the run executor stopped unexpectedly: {error}"),
    }
}

fn ticket_result(ticket: &Ticket, report: &Report, log_path: &str) -> (String, String) {
    if let Some((_, reason)) = report.rejected.iter().find(|(id, _)| *id == ticket.id) {
        let message = failure_message(ticket, "resolve", log_path, reason, &report.commits);
        return ("failed".into(), message);
    }
    let record = &report.record;
    match record.outcome.as_str() {
        "deployed" => ("deployed".into(), success_message(ticket, report)),
        "no_deploy_needed" => ("no_deploy_needed".into(), no_deploy_message(ticket, report)),
        _ => {
            let step = record.failed_step.as_deref().unwrap_or("unknown");
            (
                "failed".into(),
                failure_message(ticket, step, log_path, &report.note, &report.commits),
            )
        }
    }
}

fn target(report: &Report) -> &str {
    report.record.target_sha.as_deref().unwrap_or("unknown")
}

fn or_none(text: &str) -> &str {
    if text.trim().is_empty() {
        "none"
    } else {
        text
    }
}

fn success_message(ticket: &Ticket, report: &Report) -> String {
    let record = &report.record;
    let live = record
        .live_at
        .map(|at| {
            at.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S %Z")
                .to_string()
        })
        .unwrap_or_else(|| "unknown".into());
    let build = record
        .build_seconds
        .map(|seconds| format!("{seconds}s"))
        .unwrap_or_else(|| "unknown".into());
    format!(
        "{PREFIX} Ticket #{id}: your commit {sha} is live.\n\
         Release: {target} (live since {live}).\n\
         Restarted services: {restarted}.\n\
         Built: {built} (build {build}). Carried: {carried}.\n\
         deploy-twitch-release --pruefen: exit {pruefen}.\n\
         Running binaries: {exe}.\n\
         Read {task}/TODO.md and do the function proof.",
        id = ticket.id,
        sha = plan::short(&ticket.sha),
        target = target(report),
        restarted = or_none(&record.restarted),
        built = or_none(&record.built),
        carried = or_none(&record.carried),
        pruefen = record
            .pruefen_exit
            .map(|code| code.to_string())
            .unwrap_or_else(|| "unknown".into()),
        exe = or_none(&record.exe_report),
        task = ticket.task_dir.trim_end_matches('/'),
    )
}

fn no_deploy_message(ticket: &Ticket, report: &Report) -> String {
    format!(
        "{PREFIX} Ticket #{id}: no deploy needed. Your commit {sha} is contained in {target}, and {note}. \
         Nothing was built or restarted. Read {task}/TODO.md and do the function proof against the live release.",
        id = ticket.id,
        sha = plan::short(&ticket.sha),
        target = target(report),
        note = report.note,
        task = ticket.task_dir.trim_end_matches('/'),
    )
}

fn failure_message(
    ticket: &Ticket,
    step: &str,
    log_path: &str,
    note: &str,
    commits: &[String],
) -> String {
    let commits = if commits.is_empty() {
        "none recorded".to_string()
    } else {
        commits.join("; ")
    };
    format!(
        "{PREFIX} Ticket #{id}: deploy FAILED at step \"{step}\". Your commit {sha} is NOT live.\n\
         Error: {note}\n\
         Log: {log_path}\n\
         Batch commits: {commits}\n\
         Fix the cause, then submit a new ticket.",
        id = ticket.id,
        sha = plan::short(&ticket.sha),
    )
}

async fn notify_tick(pool: &PgPool, client: &Client) -> Result<()> {
    for ticket in db::notification_due(pool).await? {
        let message = ticket.final_message.clone().unwrap_or_else(|| {
            format!(
                "{PREFIX} Ticket #{}: finished without a recorded result.",
                ticket.id
            )
        });
        let sent = match client.thread(&ticket.thread_id).await {
            Ok(thread) => client.send_message(&thread, &message).await,
            Err(error) => Err(error),
        };
        match sent {
            Ok(_) => db::mark_notified(pool, ticket.id).await?,
            Err(error) => {
                let text = format!("{error:#}");
                eprintln!("ticket {} notification failed: {text}", ticket.id);
                db::notify_failed(pool, ticket.id, &text)
                    .await
                    .context("cannot record the failed notification")?;
            }
        }
    }
    Ok(())
}
