include!(concat!(env!("OUT_DIR"), "/build_revision.rs"));

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::{collections::HashMap, error::Error, time::Duration};
use tokio::process::Command;
use zeroize::Zeroizing;

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
const BOT: &str = "deadlock-twitch-bot-rust.service";
const COLLECTOR: &str = "tb-category-collector.service";
const BROKER: &str = "http://127.0.0.1:8770/internal/master/v1/discord/send-message";
const GAP_QUERY: &str = "INSERT INTO twitch_watchdog_incidents(service,started_at,recovered_at)
    SELECT $1,previous_at,snapshot_at FROM (
      SELECT snapshot_at,poll_seconds,lag(snapshot_at) OVER(ORDER BY snapshot_at) AS previous_at,
        lag(poll_seconds) OVER(ORDER BY snapshot_at) AS previous_poll
      FROM category_collection_runs WHERE snapshot_at >= $2 - interval '2 days'
        OR snapshot_at = (SELECT MAX(snapshot_at) FROM category_collection_runs WHERE snapshot_at < $2 - interval '2 days')
    ) r WHERE snapshot_at - previous_at > make_interval(secs => 3*GREATEST(poll_seconds,previous_poll))
    AND NOT EXISTS(SELECT 1 FROM twitch_watchdog_incidents i WHERE i.service=$1
        AND i.started_at <= r.snapshot_at AND COALESCE(i.recovered_at,$2) >= r.previous_at)
    ON CONFLICT DO NOTHING";

#[cfg(test)]
#[path = "../../../../test-support/database.rs"]
mod test_database;

#[derive(Deserialize)]
struct NotifyCredential {
    user_id: u64,
    token: String,
}

#[derive(sqlx::FromRow)]
struct Incident {
    id: i64,
    service: String,
    started_at: DateTime<Utc>,
}

fn parse_config(input: &str) -> Result<HashMap<String, String>> {
    let mut values = HashMap::new();
    let mut in_section = false;
    for line in input.lines().map(str::trim) {
        if line.starts_with('[') {
            in_section = line == "[watchdog]";
        } else if in_section && !line.starts_with('#') && !line.is_empty() {
            let (key, value) = line
                .split_once('=')
                .ok_or("invalid watchdog configuration")?;
            values.insert(key.trim().into(), value.trim().into());
        }
    }
    if values.get("service").map(String::as_str) != Some(BOT)
        || values.get("broker_url").map(String::as_str) != Some(BROKER)
    {
        return Err("watchdog must use the existing bot and local broker".into());
    }
    Ok(values)
}

fn seconds(config: &HashMap<String, String>, key: &str) -> Result<i64> {
    let value: i64 = config
        .get(key)
        .ok_or("missing watchdog threshold")?
        .parse()?;
    if !(1..=86400).contains(&value) {
        return Err("invalid watchdog threshold".into());
    }
    Ok(value)
}

async fn running(service: &str) -> Result<bool> {
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        Command::new("/usr/bin/systemctl")
            .args(["show", service, "--property=ActiveState,SubState"])
            .output(),
    )
    .await??;
    if !output.status.success() {
        return Err("systemctl check failed".into());
    }
    let text = std::str::from_utf8(&output.stdout)?;
    if !text.lines().any(|s| s.starts_with("ActiveState="))
        || !text.lines().any(|s| s.starts_with("SubState="))
    {
        return Err("incomplete systemctl state".into());
    }
    Ok(text.lines().any(|s| s == "ActiveState=active")
        && text.lines().any(|s| s == "SubState=running"))
}

async fn record(
    pool: &PgPool,
    service: &str,
    healthy: bool,
    now: DateTime<Utc>,
    started: DateTime<Utc>,
) -> Result<()> {
    if healthy {
        sqlx::query("UPDATE twitch_watchdog_incidents SET recovered_at=$2 WHERE service=$1 AND recovered_at IS NULL")
            .bind(service).bind(now).execute(pool).await?;
    } else {
        sqlx::query("INSERT INTO twitch_watchdog_incidents(service,started_at) VALUES($1,$2) ON CONFLICT DO NOTHING")
            .bind(service).bind(started.min(now)).execute(pool).await?;
    }
    Ok(())
}

async fn notify(incident: &Incident) -> Result<()> {
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        Command::new("/usr/bin/systemd-creds")
            .args([
                "decrypt",
                "--name=category-notify",
                "/etc/credstore.encrypted/deadlock-category-notify.cred",
                "-",
            ])
            .output(),
    )
    .await??;
    if !output.status.success() {
        return Err("existing notification credential unavailable".into());
    }
    let bytes = Zeroizing::new(output.stdout);
    let credential: NotifyCredential = serde_json::from_slice(&bytes)?;
    let token = Zeroizing::new(credential.token);
    if credential.user_id == 0 || token.trim().is_empty() || token.contains(['\r', '\n']) {
        return Err("invalid notification credential".into());
    }
    let content = if incident.service == COLLECTOR {
        format!("Der Deadlock-Kategoriesammler hatte seit {} einen Ausfall. Kategorie- und Chatdaten können Lücken enthalten. Bereits bestätigte Punkte und Erfolge bleiben erhalten. Bitte tb-category-collector.service prüfen.", incident.started_at)
    } else {
        format!("Der Deadlock-Twitch-Bot hatte seit {} einen Ausfall. Bitte deadlock-twitch-bot-rust.service prüfen.", incident.started_at)
    };
    let response = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()?
        .post(BROKER)
        .header("X-Internal-Token", token.as_str())
        .json(&json!({"user_id":credential.user_id,"content":content,"idempotency_key":format!("twitch-watchdog-incident-{}",incident.id)}))
        .send().await?;
    if !response.status().is_success() {
        return Err(format!("local notification broker returned {}", response.status()).into());
    }
    Ok(())
}

#[tokio::main(worker_threads = 1)]
async fn main() -> Result<()> {
    if print_build_revision() {
        return Ok(());
    }
    let config = parse_config(&std::fs::read_to_string(
        "/etc/deadlock-twitch/bot-watchdog.conf",
    )?)?;
    let bot_delay = seconds(&config, "dm_after_seconds")?;
    let warning_delay = seconds(&config, "warning_after_seconds")?;
    let retry = seconds(&config, "dm_retry_seconds")?;
    let collector_config: Value = serde_json::from_slice(&std::fs::read(
        "/etc/deadlock-twitch/category-collector.json",
    )?)?;
    let database = collector_config["database_url"]
        .as_str()
        .ok_or("collector database missing")?;
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(5))
        .connect(database)
        .await?;
    let mut lock = pool.begin().await?;
    let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(713219, 28)")
        .fetch_one(&mut *lock)
        .await?;
    if !acquired {
        return Ok(());
    }
    let now = Utc::now();
    record(&pool, BOT, running(BOT).await?, now, now).await?;
    let (covered, latest): (bool, Option<DateTime<Utc>>) = sqlx::query_as(
        "SELECT COALESCE(c.enabled AND s.heartbeat_at >= $1 - interval '90 seconds'
            AND COALESCE((s.details->>'disk_paused')::boolean,TRUE)=FALSE
            AND r.latest >= $1 - make_interval(secs => 3*c.poll_seconds),FALSE),
            LEAST(s.heartbeat_at,r.latest)
         FROM category_collector_config c
         LEFT JOIN category_collector_status s ON s.singleton
         CROSS JOIN (SELECT MAX(snapshot_at) AS latest FROM category_collection_runs) r
         WHERE c.singleton",
    )
    .bind(now)
    .fetch_one(&pool)
    .await?;
    record(
        &pool,
        COLLECTOR,
        running(COLLECTOR).await? && covered,
        now,
        latest.unwrap_or(now),
    )
    .await?;
    sqlx::query(GAP_QUERY)
        .bind(COLLECTOR)
        .bind(now)
        .execute(&pool)
        .await?;
    let warnings: Vec<String> = sqlx::query_scalar(
        "UPDATE twitch_watchdog_incidents SET warning_at=$1 WHERE warning_at IS NULL
         AND recovered_at IS NULL AND started_at <= $1 - make_interval(secs => CASE WHEN service=$2 THEN $3 ELSE 90 END)
         RETURNING service"
    ).bind(now).bind(BOT).bind(warning_delay as i32).fetch_all(&pool).await?;
    for service in warnings {
        eprintln!("Dienst oder Datenquelle ausgefallen: {service}");
    }
    let incidents: Vec<Incident> = sqlx::query_as(
        "SELECT id,service,started_at FROM twitch_watchdog_incidents
         WHERE notified_at IS NULL AND started_at <= $1 - make_interval(secs => CASE WHEN service=$2 THEN $3 ELSE 90 END)
         AND (recovered_at IS NULL OR recovered_at-started_at >= make_interval(secs => CASE WHEN service=$2 THEN $3 ELSE 90 END))
         AND (last_attempt_at IS NULL OR last_attempt_at <= $1 - make_interval(secs => $4)) ORDER BY id LIMIT 2"
    ).bind(now).bind(BOT).bind(bot_delay as i32).bind(retry as i32).fetch_all(&pool).await?;
    let mut failed = false;
    for incident in incidents {
        sqlx::query("UPDATE twitch_watchdog_incidents SET last_attempt_at=$2 WHERE id=$1")
            .bind(incident.id)
            .bind(now)
            .execute(&pool)
            .await?;
        match notify(&incident).await {
            Ok(()) => {
                sqlx::query("UPDATE twitch_watchdog_incidents SET notified_at=$2 WHERE id=$1")
                    .bind(incident.id)
                    .bind(now)
                    .execute(&pool)
                    .await?;
                println!(
                    "Ausfallmeldung zugestellt: {} Vorfall {}",
                    incident.service, incident.id
                );
            }
            Err(error) => {
                eprintln!(
                    "Ausfallmeldung fehlgeschlagen: {}: {error}",
                    incident.service
                );
                failed = true;
            }
        }
    }
    lock.commit().await?;
    if failed {
        return Err("notification delivery failed; existing timer retries".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_config_is_accepted_and_remote_brokers_are_rejected() {
        let input = include_str!("../../../../../ops/systemd/deadlock-twitch-bot-watchdog.conf");
        let config = parse_config(input).unwrap();
        assert_eq!(seconds(&config, "dm_after_seconds").unwrap(), 3600);
        assert!(parse_config(&input.replace(BROKER, "http://example.org/send")).is_err());
        assert!(parse_config(&input.replace(BOT, "other.service")).is_err());
    }

    #[tokio::test]
    async fn incidents_survive_retries_and_recovery_opens_a_new_incident() {
        use sqlx::postgres::PgConnectOptions;
        use std::str::FromStr;
        let dsn =
            test_database::database_url().expect("isolated Postgres test configuration required");
        let options = PgConnectOptions::from_str(&dsn).unwrap();
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone())
            .await
            .unwrap();
        let schema = format!(
            "watchdog_{}_{}",
            std::process::id(),
            Utc::now().timestamp_subsec_nanos()
        );
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&admin)
            .await
            .unwrap();
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(options.options([("search_path", schema.as_str())]))
            .await
            .unwrap();
        sqlx::raw_sql(include_str!(
            "../../../../migrations/20261001220000_twitch_watchdog_incidents.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        let now = DateTime::from_timestamp_micros(Utc::now().timestamp_micros()).unwrap();
        record(&pool, COLLECTOR, false, now, now).await.unwrap();
        record(
            &pool,
            COLLECTOR,
            false,
            now + chrono::Duration::seconds(30),
            now,
        )
        .await
        .unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_watchdog_incidents")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
        sqlx::query("UPDATE twitch_watchdog_incidents SET last_attempt_at=$1")
            .bind(now)
            .execute(&pool)
            .await
            .unwrap();
        record(
            &pool,
            COLLECTOR,
            false,
            now + chrono::Duration::seconds(60),
            now,
        )
        .await
        .unwrap();
        let state: (Option<DateTime<Utc>>, Option<DateTime<Utc>>) =
            sqlx::query_as("SELECT last_attempt_at,notified_at FROM twitch_watchdog_incidents")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(state, (Some(now), None));
        record(
            &pool,
            COLLECTOR,
            true,
            now + chrono::Duration::seconds(90),
            now,
        )
        .await
        .unwrap();
        record(
            &pool,
            COLLECTOR,
            false,
            now + chrono::Duration::seconds(120),
            now + chrono::Duration::seconds(120),
        )
        .await
        .unwrap();
        let counts: (i64,i64) = sqlx::query_as("SELECT COUNT(*),COUNT(*) FILTER(WHERE recovered_at IS NULL) FROM twitch_watchdog_incidents").fetch_one(&pool).await.unwrap();
        assert_eq!(counts, (2, 1));
        sqlx::query("CREATE TABLE category_collection_runs(snapshot_at timestamptz PRIMARY KEY,poll_seconds integer NOT NULL)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO category_collection_runs VALUES($1,60),($2,60)")
            .bind(now - chrono::Duration::days(5))
            .bind(now - chrono::Duration::days(1))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(GAP_QUERY)
            .bind(COLLECTOR)
            .bind(now)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(GAP_QUERY)
            .bind(COLLECTOR)
            .bind(now)
            .execute(&pool)
            .await
            .unwrap();
        let gaps: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_watchdog_incidents WHERE started_at < $1",
        )
        .bind(now)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(gaps, 1);
        pool.close().await;
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
    }
}
