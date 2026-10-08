include!(concat!(env!("OUT_DIR"), "/build_revision.rs"));

use chrono::{DateTime, Utc};
use chrono_tz::Europe::Berlin;
use serde::Deserialize;
use serde_json::json;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool,
};
use std::path::Path;
use std::{collections::HashMap, error::Error, time::Duration};
use tb_config::{file::ConfigArguments, BotConfigSnapshot};
use tokio::process::Command;
use zeroize::{Zeroize, Zeroizing};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
const BOT: &str = "deadlock-twitch-bot-rust.service";
const COLLECTOR: &str = "category-collector";
const BROKER: &str = "http://127.0.0.1:8770/internal/master/v1/discord/send-message";
const GAP_QUERY: &str = "INSERT INTO twitch_watchdog_incidents(service,started_at,recovered_at)
    SELECT $1,previous_at,snapshot_at FROM (
      SELECT snapshot_at,poll_seconds,lag(snapshot_at) OVER(ORDER BY snapshot_at) AS previous_at,
        lag(poll_seconds) OVER(ORDER BY snapshot_at) AS previous_poll
      FROM category_collection_runs WHERE snapshot_at >= $2 - interval '2 days'
        OR snapshot_at = (SELECT MAX(snapshot_at) FROM category_collection_runs WHERE snapshot_at < $2 - interval '2 days')
    ) r WHERE snapshot_at - previous_at > make_interval(secs => 3*GREATEST(poll_seconds,previous_poll))
    AND NOT EXISTS(SELECT 1 FROM category_watchdog_suspensions p
        WHERE p.started_at < r.snapshot_at
            AND LEAST(COALESCE(p.ended_at,$2),p.last_seen_at + interval '90 seconds') > r.previous_at)
    AND NOT EXISTS(SELECT 1 FROM twitch_watchdog_incidents i WHERE i.service=$1
        AND i.started_at < r.snapshot_at AND COALESCE(i.recovered_at,$2) > r.previous_at)
    ON CONFLICT DO NOTHING";

#[cfg(test)]
#[path = "../../../../test-support/database.rs"]
mod test_database;

#[derive(Deserialize)]
struct NotifyCredential {
    user_id: u64,
    token: String,
}

impl Drop for NotifyCredential {
    fn drop(&mut self) {
        self.token.zeroize();
    }
}

#[derive(sqlx::FromRow)]
struct Incident {
    id: i64,
    service: String,
    started_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct CollectorState {
    enabled: bool,
    poll_seconds: i32,
    heartbeat_at: Option<DateTime<Utc>>,
    disk_paused: bool,
    raw_paused: bool,
    latest: Option<DateTime<Utc>>,
    resumed_at: Option<DateTime<Utc>>,
}

impl CollectorState {
    fn heartbeat_fresh(&self, now: DateTime<Utc>) -> bool {
        self.heartbeat_at
            .is_some_and(|at| at >= now - chrono::Duration::seconds(90))
    }

    fn healthy(&self, now: DateTime<Utc>) -> bool {
        !self.enabled
            || (self.heartbeat_fresh(now)
                && (self.disk_paused
                    || self.latest.max(self.resumed_at).is_some_and(|at| {
                        at >= now - chrono::Duration::seconds(i64::from(self.poll_seconds) * 3)
                    })))
    }

    fn storage_reason(&self, now: DateTime<Utc>) -> Option<&'static str> {
        if !self.enabled || !self.heartbeat_fresh(now) {
            None
        } else if self.disk_paused {
            Some("disk")
        } else if self.raw_paused {
            Some("budget")
        } else {
            None
        }
    }
}

#[derive(sqlx::FromRow)]
struct StorageIncident {
    id: i64,
    started_at: DateTime<Utc>,
    reason: String,
    repetitions: i32,
}

async fn record_suspensions(
    pool: &PgPool,
    state: &CollectorState,
    now: DateTime<Utc>,
) -> Result<()> {
    if state.enabled && !state.heartbeat_fresh(now) {
        return Ok(());
    }
    let started_at = if state.heartbeat_fresh(now) {
        state.heartbeat_at.unwrap_or(now).min(now)
    } else {
        now
    };
    for (reason, active) in [
        ("disabled", !state.enabled),
        ("disk", state.enabled && state.disk_paused),
    ] {
        if active {
            sqlx::query("INSERT INTO category_watchdog_suspensions(reason,started_at,last_seen_at) VALUES($1,$2,$3)
                ON CONFLICT(reason) WHERE ended_at IS NULL DO UPDATE SET last_seen_at=excluded.last_seen_at")
                .bind(reason).bind(started_at).bind(now).execute(pool).await?;
        } else {
            sqlx::query("UPDATE category_watchdog_suspensions SET ended_at=$2 WHERE reason=$1 AND ended_at IS NULL")
                .bind(reason).bind(now).execute(pool).await?;
        }
    }
    Ok(())
}

async fn record_storage(pool: &PgPool, reason: Option<&str>, now: DateTime<Utc>) -> Result<()> {
    if let Some(reason) = reason {
        sqlx::query("INSERT INTO category_watchdog_storage_incidents(started_at,last_paused_at,reason) VALUES($1,$1,$2)
            ON CONFLICT((true)) WHERE recovered_at IS NULL DO UPDATE SET
            last_paused_at=excluded.last_paused_at, reason=excluded.reason,
            repetitions=category_watchdog_storage_incidents.repetitions + CASE WHEN category_watchdog_storage_incidents.clear_since IS NULL THEN 0 ELSE 1 END,
            clear_since=NULL")
            .bind(now).bind(reason).execute(pool).await?;
    } else {
        sqlx::query(
            "UPDATE category_watchdog_storage_incidents SET clear_since=COALESCE(clear_since,$1),
            recovered_at=CASE WHEN clear_since <= $1 - interval '30 minutes' THEN $1 ELSE NULL END
            WHERE recovered_at IS NULL",
        )
        .bind(now)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn observe_storage(pool: &PgPool, state: &CollectorState, now: DateTime<Utc>) -> Result<()> {
    record_suspensions(pool, state, now).await?;
    if state.heartbeat_fresh(now) {
        record_storage(pool, state.storage_reason(now), now).await?;
    }
    Ok(())
}

fn storage_content(incident: &StorageIncident) -> String {
    let at = local_time(incident.started_at);
    let condition = if incident.reason == "disk" {
        "Bei einer unterschrittenen oder nicht prüfbaren Plattenreserve pausieren neue Kategorie- und Chatdaten."
    } else {
        "Bei erreichtem Chat-Speicherbudget pausieren neue Chatdaten; Kategoriemessungen werden weiter gesammelt."
    };
    format!("Der Deadlock-Kategoriesammler im Twitch-Bot meldete am {at} eine Speicherpause. {condition} Der bisherige Bestand bleibt erhalten. Bitte Speicherplatz und Speicherbudget prüfen (Wiederholungen dieses Vorfalls: {}).", incident.repetitions)
}

async fn prepare_storage_notification(pool: &PgPool, now: DateTime<Utc>) -> Result<()> {
    let incident: Option<StorageIncident> = sqlx::query_as("SELECT id,started_at,reason,repetitions FROM category_watchdog_storage_incidents
        WHERE started_at <= $1 - interval '5 minutes' AND recovered_at IS NULL AND clear_since IS NULL
        AND NOT EXISTS(SELECT 1 FROM category_watchdog_storage_notifications n WHERE n.incident_id=category_watchdog_storage_incidents.id)
        AND NOT EXISTS(SELECT 1 FROM category_watchdog_storage_notifications WHERE notified_at IS NULL)
        AND NOT EXISTS(SELECT 1 FROM category_watchdog_storage_notifications
            WHERE (notified_at AT TIME ZONE 'Europe/Berlin')::date=($1 AT TIME ZONE 'Europe/Berlin')::date)
        ORDER BY id LIMIT 1")
        .bind(now).fetch_optional(pool).await?;
    if let Some(incident) = incident {
        sqlx::query("INSERT INTO category_watchdog_storage_notifications(notification_day,incident_id,content) VALUES($1,$2,$3) ON CONFLICT DO NOTHING")
            .bind(now.with_timezone(&Berlin).date_naive()).bind(incident.id).bind(storage_content(&incident)).execute(pool).await?;
    }
    Ok(())
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
        sqlx::query(
            "INSERT INTO twitch_watchdog_incidents(service,started_at)
            SELECT $1,GREATEST($2,COALESCE(MAX(recovered_at)+interval '1 microsecond',$2))
            FROM twitch_watchdog_incidents WHERE service=$1 ON CONFLICT DO NOTHING",
        )
        .bind(service)
        .bind(started.min(now))
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn first_delivery_error(pool: &PgPool, id: i64, now: DateTime<Utc>) -> Result<bool> {
    Ok(sqlx::query("UPDATE twitch_watchdog_incidents SET delivery_error_at=$2 WHERE id=$1 AND delivery_error_at IS NULL")
        .bind(id).bind(now).execute(pool).await?.rows_affected() == 1)
}

async fn notify_credential() -> Result<NotifyCredential> {
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
    if credential.user_id == 0
        || credential.token.trim().is_empty()
        || credential.token.contains(['\r', '\n'])
    {
        return Err("invalid notification credential".into());
    }
    Ok(credential)
}

fn local_time(at: DateTime<Utc>) -> String {
    at.with_timezone(&chrono_tz::Europe::Berlin)
        .format("%d.%m.%Y um %H:%M:%S %Z")
        .to_string()
}

fn outage_content(incident: &Incident) -> String {
    let at = local_time(incident.started_at);
    if incident.service == COLLECTOR {
        format!("Seit {at} fehlen aktuelle Daten des Deadlock-Kategoriesammlers im Twitch-Bot. Kategorie- und Chatdaten können Lücken enthalten. Bereits erfasste Daten bleiben erhalten. Bitte den Twitch-Bot prüfen.")
    } else {
        format!(
            "Der Deadlock-Twitch-Bot war ab {at} nicht erreichbar. Bitte den Twitch-Bot prüfen."
        )
    }
}

async fn send(content: &str, key: &str, credential: &NotifyCredential) -> Result<()> {
    let response = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()?
        .post(BROKER)
        .header("X-Internal-Token", credential.token.as_str())
        .json(&json!({"user_id":credential.user_id,"content":content,"idempotency_key":key}))
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(format!("local notification broker returned {}", response.status()).into());
    }
    Ok(())
}

async fn storage_notifications(
    pool: &PgPool,
    now: DateTime<Utc>,
    retry: i64,
    credential: &Result<NotifyCredential>,
) -> Result<()> {
    prepare_storage_notification(pool, now).await?;
    let pending: Option<(i64,String)> = sqlx::query_as(
        "SELECT incident_id,content FROM category_watchdog_storage_notifications
         WHERE notified_at IS NULL AND (last_attempt_at IS NULL
             OR last_attempt_at <= $1 - make_interval(secs => $2))
         AND NOT EXISTS(SELECT FROM category_watchdog_storage_notifications
             WHERE (notified_at AT TIME ZONE 'Europe/Berlin')::date=($1 AT TIME ZONE 'Europe/Berlin')::date)
         ORDER BY notification_day LIMIT 1")
        .bind(now).bind(retry as i32).fetch_optional(pool).await?;
    if let Some((id, content)) = pending {
        sqlx::query("UPDATE category_watchdog_storage_notifications SET last_attempt_at=$2 WHERE incident_id=$1")
            .bind(id).bind(now).execute(pool).await?;
        let result = match credential {
            Ok(credential) => {
                send(
                    &content,
                    &format!("twitch-watchdog-storage-{id}"),
                    credential,
                )
                .await
            }
            Err(_) => Err("Benachrichtigungszugang nicht verfügbar".into()),
        };
        match result {
            Ok(()) => {
                sqlx::query("UPDATE category_watchdog_storage_notifications SET notified_at=$2 WHERE incident_id=$1")
                    .bind(id).bind(now).execute(pool).await?;
            }
            Err(_) => {
                if sqlx::query(
                    "UPDATE category_watchdog_storage_notifications SET delivery_error_at=$2
                    WHERE incident_id=$1 AND delivery_error_at IS NULL",
                )
                .bind(id)
                .bind(now)
                .execute(pool)
                .await?
                .rows_affected()
                    > 0
                {
                    eprintln!("Meldung zur Speicherpause konnte nicht zugestellt werden; erneuter Versuch folgt.");
                }
            }
        }
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
    let arguments = ConfigArguments::parse(std::env::args_os().skip(1))?;
    if !arguments.remaining.is_empty() {
        return Err("Unbekannte Watchdog-Option".into());
    }
    let notification = notify_credential().await;
    let identity = nix::unistd::User::from_name("twitchbot")?.ok_or("Bot-Dienstkonto fehlt")?;
    nix::unistd::setgroups(&[])?;
    nix::unistd::setgid(identity.gid)?;
    nix::unistd::setuid(identity.uid)?;
    let operating = BotConfigSnapshot::load(Path::new(&arguments.path))?;
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_millis(
            operating.settings().database.acquire_timeout_ms,
        ))
        .connect_with(
            PgConnectOptions::new()
                .host("/var/run/postgresql")
                .username("twitchbot")
                .database("twitch_analytics")
                .application_name("tb-twitch-watchdog"),
        )
        .await?;
    let mut lock = pool.begin().await?;
    let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(713219, 28)")
        .fetch_one(&mut *lock)
        .await?;
    if !acquired {
        return Ok(());
    }
    let now = Utc::now();
    let bot_running = running(BOT).await?;
    record(&pool, BOT, bot_running, now, now).await?;
    let mut state: CollectorState = sqlx::query_as(
        "SELECT c.enabled,c.poll_seconds,s.heartbeat_at,
            COALESCE((s.details->>'disk_paused')::boolean,FALSE) AS disk_paused,
            COALESCE((s.details->>'raw_paused')::boolean,FALSE) AS raw_paused,r.latest,
            (SELECT MAX(ended_at) FROM category_watchdog_suspensions) AS resumed_at
         FROM category_collector_config c
         LEFT JOIN category_collector_status s ON s.singleton
         CROSS JOIN (SELECT MAX(snapshot_at) AS latest FROM category_collection_runs) r
         WHERE c.singleton",
    )
    .fetch_one(&pool)
    .await?;
    observe_storage(&pool, &state, now).await?;
    state.resumed_at =
        sqlx::query_scalar("SELECT MAX(ended_at) FROM category_watchdog_suspensions")
            .fetch_one(&pool)
            .await?;
    let started = if !state.heartbeat_fresh(now) {
        state.heartbeat_at.unwrap_or(now)
    } else {
        state
            .latest
            .into_iter()
            .chain(state.resumed_at)
            .max()
            .unwrap_or(now)
    };
    record(
        &pool,
        COLLECTOR,
        !state.enabled || (bot_running && state.healthy(now)),
        now,
        started,
    )
    .await?;
    sqlx::query(
        "UPDATE twitch_watchdog_incidents SET superseded_at=COALESCE(superseded_at,$1)
        WHERE service='tb-category-collector.service'",
    )
    .bind(now)
    .execute(&pool)
    .await?;
    sqlx::query(GAP_QUERY)
        .bind(COLLECTOR)
        .bind(now)
        .execute(&pool)
        .await?;
    let warnings: Vec<String> = sqlx::query_scalar(
        "UPDATE twitch_watchdog_incidents SET warning_at=$1 WHERE warning_at IS NULL
         AND superseded_at IS NULL AND recovered_at IS NULL AND started_at <= $1 - make_interval(secs => CASE WHEN service=$2 THEN $3 ELSE 90 END)
         RETURNING service"
    ).bind(now).bind(BOT).bind(warning_delay as i32).fetch_all(&pool).await?;
    for service in warnings {
        eprintln!("Dienst oder Datenquelle ausgefallen: {service}");
    }
    let incidents: Vec<Incident> = sqlx::query_as(
        "SELECT id,service,started_at FROM twitch_watchdog_incidents
         WHERE superseded_at IS NULL AND notified_at IS NULL AND started_at <= $1 - make_interval(secs => CASE WHEN service=$2 THEN $3 ELSE 90 END)
         AND (recovered_at IS NULL OR recovered_at-started_at >= make_interval(secs => CASE WHEN service=$2 THEN $3 ELSE 90 END))
         AND (last_attempt_at IS NULL OR last_attempt_at <= $1 - make_interval(secs => $4)) ORDER BY id LIMIT 2"
    ).bind(now).bind(BOT).bind(bot_delay as i32).bind(retry as i32).fetch_all(&pool).await?;
    for incident in incidents {
        sqlx::query("UPDATE twitch_watchdog_incidents SET last_attempt_at=$2 WHERE id=$1")
            .bind(incident.id)
            .bind(now)
            .execute(&pool)
            .await?;
        let delivery = match &notification {
            Ok(credential) => {
                send(
                    &outage_content(&incident),
                    &format!("twitch-watchdog-incident-{}", incident.id),
                    credential,
                )
                .await
            }
            Err(_) => Err("existing notification credential unavailable".into()),
        };
        match delivery {
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
                if first_delivery_error(&pool, incident.id, now).await? {
                    eprintln!(
                        "Ausfallmeldung fehlgeschlagen: {}: {error}",
                        incident.service
                    );
                }
            }
        }
    }
    storage_notifications(&pool, now, retry, &notification).await?;
    lock.commit().await?;
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

    #[test]
    fn intentional_pauses_are_not_snapshot_outages() {
        let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
        let mut state = CollectorState {
            enabled: true,
            poll_seconds: 60,
            heartbeat_at: Some(now),
            disk_paused: true,
            raw_paused: true,
            latest: None,
            resumed_at: None,
        };
        assert!(state.healthy(now));
        assert_eq!(state.storage_reason(now), Some("disk"));
        state.disk_paused = false;
        assert!(!state.healthy(now));
        state.latest = Some(now);
        assert!(state.healthy(now));
        assert_eq!(state.storage_reason(now), Some("budget"));
        state.heartbeat_at = Some(now - chrono::Duration::seconds(91));
        assert!(!state.healthy(now));
        assert_eq!(state.storage_reason(now), None);
        state.enabled = false;
        assert!(state.healthy(now));
        state.enabled = true;
        state.heartbeat_at = Some(now);
        state.latest = None;
        state.resumed_at = Some(now);
        assert!(state.healthy(now));
        assert!(!state.healthy(now + chrono::Duration::seconds(181)));
    }

    #[test]
    fn local_time_uses_berlin_summer_and_winter_offsets() {
        let summer = DateTime::parse_from_rfc3339("2026-10-08T13:35:57Z")
            .unwrap()
            .with_timezone(&Utc);
        let winter = DateTime::parse_from_rfc3339("2026-12-08T13:35:57Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(summer.with_timezone(&Berlin).offset().to_string(), "CEST");
        assert_eq!(winter.with_timezone(&Berlin).offset().to_string(), "CET");
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
        sqlx::raw_sql(include_str!(
            "../../../../migrations/20261008161000_category_watchdog_native.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
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
            now,
        )
        .await
        .unwrap();
        let counts: (i64,i64) = sqlx::query_as("SELECT COUNT(*),COUNT(*) FILTER(WHERE recovered_at IS NULL) FROM twitch_watchdog_incidents").fetch_one(&pool).await.unwrap();
        assert_eq!(counts, (2, 1));
        let ids: Vec<i64> =
            sqlx::query_scalar("SELECT id FROM twitch_watchdog_incidents ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(first_delivery_error(&pool, ids[0], now).await.unwrap());
        assert!(!first_delivery_error(&pool, ids[0], now).await.unwrap());
        assert!(first_delivery_error(&pool, ids[1], now).await.unwrap());
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
        sqlx::query("INSERT INTO category_collection_runs VALUES($1,60)")
            .bind(now - chrono::Duration::hours(12))
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
        assert_eq!(gaps, 2);
        record_storage(&pool, Some("disk"), now).await.unwrap();
        record_storage(&pool, None, now + chrono::Duration::minutes(1))
            .await
            .unwrap();
        prepare_storage_notification(&pool, now + chrono::Duration::minutes(6))
            .await
            .unwrap();
        let transient_notifications: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM category_watchdog_storage_notifications")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(transient_notifications, 0);
        record_storage(&pool, Some("budget"), now + chrono::Duration::minutes(20))
            .await
            .unwrap();
        let incident: (i64, i32) = sqlx::query_as("SELECT id,repetitions FROM category_watchdog_storage_incidents WHERE recovered_at IS NULL")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(incident.1, 1);
        prepare_storage_notification(&pool, now + chrono::Duration::minutes(21))
            .await
            .unwrap();
        prepare_storage_notification(&pool, now + chrono::Duration::minutes(22))
            .await
            .unwrap();
        let queued: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM category_watchdog_storage_notifications")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(queued, 1);
        sqlx::query("UPDATE category_watchdog_storage_notifications SET notified_at=$1")
            .bind(now + chrono::Duration::minutes(22))
            .execute(&pool)
            .await
            .unwrap();
        record_storage(&pool, None, now + chrono::Duration::minutes(30))
            .await
            .unwrap();
        record_storage(&pool, None, now + chrono::Duration::minutes(60))
            .await
            .unwrap();
        record_storage(&pool, Some("disk"), now + chrono::Duration::minutes(61))
            .await
            .unwrap();
        prepare_storage_notification(&pool, now + chrono::Duration::minutes(70))
            .await
            .unwrap();
        let queued: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM category_watchdog_storage_notifications")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(queued, 1);
        prepare_storage_notification(&pool, now + chrono::Duration::days(1))
            .await
            .unwrap();
        let queued: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM category_watchdog_storage_notifications")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(queued, 2);
        sqlx::query("TRUNCATE twitch_watchdog_incidents,category_collection_runs RESTART IDENTITY")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO category_collection_runs VALUES($1,60),($2,60)")
            .bind(now)
            .bind(now + chrono::Duration::hours(1))
            .execute(&pool)
            .await
            .unwrap();
        let mut paused = CollectorState {
            enabled: true,
            poll_seconds: 60,
            heartbeat_at: Some(now),
            disk_paused: true,
            raw_paused: true,
            latest: Some(now),
            resumed_at: None,
        };
        record_suspensions(&pool, &paused, now).await.unwrap();
        paused.heartbeat_at = Some(now + chrono::Duration::hours(1));
        record_suspensions(&pool, &paused, now + chrono::Duration::hours(1))
            .await
            .unwrap();
        sqlx::query(GAP_QUERY)
            .bind(COLLECTOR)
            .bind(now + chrono::Duration::hours(1))
            .execute(&pool)
            .await
            .unwrap();
        let false_outages: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_watchdog_incidents")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(false_outages, 0);
        pool.close().await;
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
    }
}
