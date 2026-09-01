//! Deterministischer Werbemanager und sein PostgreSQL-Store.
//!
//! Der Entscheider ist I/O-frei. Twitch-Aufrufe bleiben im `tb-bot`, während
//! dieser Store Einstellungen, aktuellen Zustand und die lease-geschützte
//! Aktions-Queue verwaltet.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use tb_transport_twitch::{streams::normalize_ad_time, AdSchedule};

pub const READ_SCOPE: &str = "channel:read:ads";
pub const SNOOZE_SCOPE: &str = "channel:manage:ads";
pub const COMMERCIAL_SCOPE: &str = "channel:edit:commercial";
pub const LIVE_STATE_MAX_AGE: Duration = Duration::minutes(5);
pub const UNRESOLVED_DETAIL: &str =
    "Ausgang konnte nach 15 Minuten nicht eindeutig bestätigt werden; Sperre wurde aufgehoben.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Strategy {
    Monitor,
    Snooze,
    Smart,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn chatruhe_braucht_subscription_aber_keine_chatnachricht() {
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 15, 0, 0).unwrap();
        assert!(chat_ingest_health_is_ok(
            Some(now - Duration::minutes(35)),
            None,
            None,
            None,
            now,
        ));
        assert!(!chat_ingest_health_is_ok(
            Some(now - Duration::minutes(35) - Duration::milliseconds(1)),
            None,
            None,
            None,
            now,
        ));
    }

    #[test]
    fn neuer_insert_fehler_und_unplausibler_lag_sind_ungesund() {
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 15, 0, 0).unwrap();
        let subscription = Some(now - Duration::minutes(1));
        assert!(!chat_ingest_health_is_ok(
            subscription,
            Some(now - Duration::minutes(2)),
            Some(now - Duration::minutes(1)),
            None,
            now,
        ));
        assert!(chat_ingest_health_is_ok(
            subscription,
            Some(now),
            Some(now - Duration::minutes(1)),
            Some(120),
            now,
        ));
        assert!(!chat_ingest_health_is_ok(
            subscription,
            Some(now),
            None,
            Some(121),
            now,
        ));
    }
}

impl Strategy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Monitor => "monitor",
            Self::Snooze => "snooze",
            Self::Smart => "smart",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "monitor" => Some(Self::Monitor),
            "snooze" => Some(Self::Snooze),
            "smart" => Some(Self::Smart),
            _ => None,
        }
    }
    pub fn required_scopes(self) -> &'static [&'static str] {
        match self {
            Self::Monitor => &[READ_SCOPE],
            Self::Snooze => &[READ_SCOPE, SNOOZE_SCOPE],
            Self::Smart => &[READ_SCOPE, SNOOZE_SCOPE, COMMERCIAL_SCOPE],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub enabled: bool,
    pub strategy: Strategy,
    pub ad_duration_seconds: i32,
    pub min_interval_minutes: i32,
    pub startup_delay_minutes: i32,
    pub quiet_window_minutes: i32,
    pub action_lead_seconds: i32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            strategy: Strategy::Monitor,
            ad_duration_seconds: 90,
            min_interval_minutes: 30,
            startup_delay_minutes: 15,
            quiet_window_minutes: 5,
            action_lead_seconds: 60,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), &'static str> {
        if ![30, 60, 90, 120, 150, 180].contains(&self.ad_duration_seconds) {
            return Err("adDurationSeconds ist ungültig");
        }
        if !(8..=180).contains(&self.min_interval_minutes) {
            return Err("minIntervalMinutes muss zwischen 8 und 180 liegen");
        }
        if !(0..=180).contains(&self.startup_delay_minutes) {
            return Err("startupDelayMinutes muss zwischen 0 und 180 liegen");
        }
        if !(0..=60).contains(&self.quiet_window_minutes) {
            return Err("quietWindowMinutes muss zwischen 0 und 60 liegen");
        }
        if !(10..=300).contains(&self.action_lead_seconds) {
            return Err("actionLeadSeconds muss zwischen 10 und 300 liegen");
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct DecisionInput {
    pub now: DateTime<Utc>,
    pub settings: Settings,
    pub stream_started_at: Option<DateTime<Utc>>,
    pub next_ad_at: Option<DateTime<Utc>>,
    pub last_ad_at: Option<DateTime<Utc>>,
    pub snooze_count: i64,
    pub quiet_chat_messages: i64,
    pub chat_ingest_healthy: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionAction {
    None,
    Snooze,
    Commercial { duration_seconds: i32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub action: DecisionAction,
    pub reason: &'static str,
}

pub fn decide(input: &DecisionInput) -> Decision {
    let none = |reason| Decision {
        action: DecisionAction::None,
        reason,
    };
    if !input.settings.enabled {
        return none("disabled");
    }
    if input.settings.strategy == Strategy::Monitor {
        return none("monitor_only");
    }
    let Some(next_ad) = input.next_ad_at else {
        return none("no_next_ad");
    };
    if next_ad < input.now {
        return none("ad_already_due");
    }
    let until = next_ad.signed_duration_since(input.now).num_seconds();
    if until > i64::from(input.settings.action_lead_seconds) {
        return none("outside_lead_window");
    }
    if input.settings.strategy == Strategy::Snooze {
        return if input.snooze_count > 0 {
            Decision {
                action: DecisionAction::Snooze,
                reason: "snooze_due",
            }
        } else {
            none("no_snoozes")
        };
    }
    let Some(stream_started_at) = input.stream_started_at else {
        return if input.snooze_count > 0 {
            Decision {
                action: DecisionAction::Snooze,
                reason: "stream_start_unknown",
            }
        } else {
            none("stream_start_unknown_no_snooze")
        };
    };
    if input.now
        < stream_started_at + Duration::minutes(i64::from(input.settings.startup_delay_minutes))
    {
        return if input.snooze_count > 0 {
            Decision {
                action: DecisionAction::Snooze,
                reason: "startup_protection",
            }
        } else {
            none("startup_protection_no_snooze")
        };
    }
    if input.settings.quiet_window_minutes > 0 && !input.chat_ingest_healthy {
        return if input.snooze_count > 0 {
            Decision {
                action: DecisionAction::Snooze,
                reason: "chat_ingest_unhealthy",
            }
        } else {
            none("chat_ingest_unhealthy_no_snooze")
        };
    }
    let cooldown_ready = input
        .last_ad_at
        .map(|last| {
            input.now >= last + Duration::minutes(i64::from(input.settings.min_interval_minutes))
        })
        .unwrap_or(true);
    if cooldown_ready && input.quiet_chat_messages == 0 {
        Decision {
            action: DecisionAction::Commercial {
                duration_seconds: input.settings.ad_duration_seconds,
            },
            reason: "quiet_window",
        }
    } else if input.snooze_count > 0 {
        Decision {
            action: DecisionAction::Snooze,
            reason: if cooldown_ready {
                "chat_active"
            } else {
                "commercial_cooldown"
            },
        }
    } else {
        none(if cooldown_ready {
            "chat_active_no_snooze"
        } else {
            "cooldown_no_snooze"
        })
    }
}

#[derive(Debug, Clone)]
pub struct ManagedChannel {
    pub twitch_user_id: String,
    pub twitch_login: String,
    pub settings: Settings,
}

#[derive(Debug, Clone)]
pub struct LiveState {
    pub is_live: bool,
    pub active_session_id: Option<i64>,
    pub stream_started_at: Option<DateTime<Utc>>,
    pub observed_at: Option<DateTime<Utc>>,
}

impl LiveState {
    pub fn is_fresh_live(&self, now: DateTime<Utc>) -> bool {
        self.is_live
            && self.active_session_id.filter(|id| *id > 0).is_some()
            && self
                .observed_at
                .map(|seen| {
                    now.signed_duration_since(seen) <= LIVE_STATE_MAX_AGE
                        && seen <= now + Duration::minutes(1)
                })
                .unwrap_or(false)
    }
}

fn chat_ingest_health_is_ok(
    subscription_ok: Option<DateTime<Utc>>,
    last_insert_ok: Option<DateTime<Utc>>,
    last_insert_error: Option<DateTime<Utc>>,
    lag_seconds: Option<i32>,
    now: DateTime<Utc>,
) -> bool {
    let subscription_is_fresh = subscription_ok
        .map(|seen| seen >= now - Duration::minutes(35) && seen <= now + Duration::minutes(1))
        .unwrap_or(false);
    let insert_path_is_healthy = last_insert_error
        .map(|error| last_insert_ok.map(|ok| ok >= error).unwrap_or(false))
        .unwrap_or(true);
    let lag_is_plausible = lag_seconds
        .map(|seconds| (0..=120).contains(&seconds))
        .unwrap_or(true);
    subscription_is_fresh && insert_path_is_healthy && lag_is_plausible
}

#[derive(Debug, Clone)]
pub struct QueuedAction {
    pub id: i64,
    pub twitch_user_id: String,
    pub twitch_login: String,
    pub action: ActionKind,
    pub duration_seconds: Option<i32>,
    pub source: String,
    pub lease_token: String,
    pub preflight_next_ad_at: Option<DateTime<Utc>>,
    pub preflight_last_ad_at: Option<DateTime<Utc>>,
    pub preflight_snooze_count: Option<i32>,
    pub marked_unknown_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    Snooze,
    Commercial,
}

impl ActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Snooze => "snooze",
            Self::Commercial => "commercial",
        }
    }

    fn parse(value: &str) -> Result<Self, sqlx::Error> {
        match value {
            "snooze" => Ok(Self::Snooze),
            "commercial" => Ok(Self::Commercial),
            _ => Err(sqlx::Error::Decode(
                format!("ungültige Werbeaktion: {value}").into(),
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnqueueOutcome {
    Queued,
    AlreadyAccepted,
    Conflict,
    RateLimited,
}

#[derive(Clone)]
pub struct AdManagerStore {
    pool: PgPool,
}

impl AdManagerStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Dauerhafte Kanal-Lease statt Session-Advisory-Lock. Sie blockiert keine
    /// Pool-Verbindung und läuft nach Prozessabbruch selbstständig aus.
    pub async fn try_acquire_worker_lease(&self, uid: &str) -> Result<Option<String>, sqlx::Error> {
        let token = format!(
            "{uid}:{}",
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        sqlx::query_scalar("UPDATE twitch_ad_manager_settings SET worker_lease_until=NOW()+INTERVAL '90 seconds',worker_lease_token=$2 WHERE twitch_user_id=$1 AND (worker_lease_until IS NULL OR worker_lease_until<NOW()) RETURNING worker_lease_token")
            .bind(uid).bind(&token).fetch_optional(&self.pool).await
    }

    pub async fn release_worker_lease(&self, uid: &str, token: &str) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("UPDATE twitch_ad_manager_settings SET worker_lease_until=NULL,worker_lease_token=NULL WHERE twitch_user_id=$1 AND worker_lease_token=$2")
            .bind(uid).bind(token).execute(&self.pool).await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn list_channels(&self) -> Result<Vec<ManagedChannel>, sqlx::Error> {
        let rows = sqlx::query("SELECT s.twitch_user_id,s.twitch_login,s.enabled,s.strategy,s.ad_duration_seconds,s.min_interval_minutes,s.startup_delay_minutes,s.quiet_window_minutes,s.action_lead_seconds FROM twitch_ad_manager_settings s ORDER BY s.twitch_user_id").fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|r| {
                let raw: String = r.try_get("strategy")?;
                let strategy = Strategy::parse(&raw).ok_or_else(|| {
                    sqlx::Error::Decode(format!("ungültige Strategie: {raw}").into())
                })?;
                Ok(ManagedChannel {
                    twitch_user_id: r.try_get("twitch_user_id")?,
                    twitch_login: r.try_get("twitch_login")?,
                    settings: Settings {
                        enabled: r.try_get("enabled")?,
                        strategy,
                        ad_duration_seconds: r.try_get("ad_duration_seconds")?,
                        min_interval_minutes: r.try_get("min_interval_minutes")?,
                        startup_delay_minutes: r.try_get("startup_delay_minutes")?,
                        quiet_window_minutes: r.try_get("quiet_window_minutes")?,
                        action_lead_seconds: r.try_get("action_lead_seconds")?,
                    },
                })
            })
            .collect()
    }

    /// Belegt, dass der Hintergrundworker diesen verwalteten Kanal tatsächlich
    /// erreicht hat. Das ist ausdrücklich keine Twitch-Beobachtung und setzt
    /// deshalb `observed_at` nicht.
    pub async fn touch_worker(&self, uid: &str, login: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO twitch_ad_manager_state(twitch_user_id,twitch_login,worker_heartbeat_at) VALUES($1,$2,NOW()) ON CONFLICT(twitch_user_id) DO UPDATE SET twitch_login=EXCLUDED.twitch_login,worker_heartbeat_at=NOW(),updated_at=NOW()",
        )
        .bind(uid)
        .bind(login)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn load_settings(
        &self,
        uid: &str,
    ) -> Result<Option<(Settings, DateTime<Utc>)>, sqlx::Error> {
        let row = sqlx::query("SELECT enabled,strategy,ad_duration_seconds,min_interval_minutes,startup_delay_minutes,quiet_window_minutes,action_lead_seconds,updated_at FROM twitch_ad_manager_settings WHERE twitch_user_id=$1").bind(uid).fetch_optional(&self.pool).await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let raw: String = row.try_get("strategy")?;
        let strategy = Strategy::parse(&raw)
            .ok_or_else(|| sqlx::Error::Decode(format!("ungültige Strategie: {raw}").into()))?;
        Ok(Some((
            Settings {
                enabled: row.try_get("enabled")?,
                strategy,
                ad_duration_seconds: row.try_get("ad_duration_seconds")?,
                min_interval_minutes: row.try_get("min_interval_minutes")?,
                startup_delay_minutes: row.try_get("startup_delay_minutes")?,
                quiet_window_minutes: row.try_get("quiet_window_minutes")?,
                action_lead_seconds: row.try_get("action_lead_seconds")?,
            },
            row.try_get("updated_at")?,
        )))
    }

    pub async fn save_settings(
        &self,
        uid: &str,
        login: &str,
        settings: &Settings,
    ) -> Result<DateTime<Utc>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let row=sqlx::query("INSERT INTO twitch_ad_manager_settings(twitch_user_id,twitch_login,enabled,strategy,ad_duration_seconds,min_interval_minutes,startup_delay_minutes,quiet_window_minutes,action_lead_seconds) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT(twitch_user_id) DO UPDATE SET twitch_login=EXCLUDED.twitch_login,enabled=EXCLUDED.enabled,strategy=EXCLUDED.strategy,ad_duration_seconds=EXCLUDED.ad_duration_seconds,min_interval_minutes=EXCLUDED.min_interval_minutes,startup_delay_minutes=EXCLUDED.startup_delay_minutes,quiet_window_minutes=EXCLUDED.quiet_window_minutes,action_lead_seconds=EXCLUDED.action_lead_seconds,updated_at=NOW() RETURNING updated_at").bind(uid).bind(login).bind(settings.enabled).bind(settings.strategy.as_str()).bind(settings.ad_duration_seconds).bind(settings.min_interval_minutes).bind(settings.startup_delay_minutes).bind(settings.quiet_window_minutes).bind(settings.action_lead_seconds).fetch_one(&mut *tx).await?;
        let allowed_action = match (settings.enabled, settings.strategy) {
            (true, Strategy::Snooze) => Some("snooze"),
            (true, Strategy::Smart) => None,
            _ => Some(""),
        };
        match allowed_action {
            None => {}
            Some("snooze") => {
                sqlx::query("UPDATE twitch_ad_manager_actions SET status='cancelled',completed_at=NOW(),lease_until=NULL,lease_token=NULL,outcome_detail='Automatikregeln wurden geändert' WHERE twitch_user_id=$1 AND source='automatic' AND status IN ('pending','leased') AND action<>'snooze'").bind(uid).execute(&mut *tx).await?;
            }
            Some(_) => {
                sqlx::query("UPDATE twitch_ad_manager_actions SET status='cancelled',completed_at=NOW(),lease_until=NULL,lease_token=NULL,outcome_detail='Automatik wurde deaktiviert oder auf Beobachtung gestellt' WHERE twitch_user_id=$1 AND source='automatic' AND status IN ('pending','leased')").bind(uid).execute(&mut *tx).await?;
            }
        }
        let updated = row.try_get("updated_at")?;
        tx.commit().await?;
        Ok(updated)
    }

    pub async fn live_state(&self, uid: &str) -> Result<LiveState, sqlx::Error> {
        let row=sqlx::query("SELECT COALESCE(is_live,0)<>0 AS is_live,active_session_id,last_started_at::timestamptz AS stream_started_at,last_seen_at::timestamptz AS observed_at FROM twitch_live_state WHERE twitch_user_id=$1").bind(uid).fetch_optional(&self.pool).await?;
        let Some(row) = row else {
            return Ok(LiveState {
                is_live: false,
                active_session_id: None,
                stream_started_at: None,
                observed_at: None,
            });
        };
        Ok(LiveState {
            is_live: row.try_get("is_live")?,
            active_session_id: row.try_get("active_session_id")?,
            stream_started_at: row.try_get("stream_started_at")?,
            observed_at: row.try_get("observed_at")?,
        })
    }

    pub async fn quiet_messages(
        &self,
        session_id: i64,
        now: DateTime<Utc>,
        minutes: i32,
    ) -> Result<i64, sqlx::Error> {
        if minutes == 0 {
            return Ok(0);
        }
        sqlx::query_scalar("SELECT COUNT(*)::bigint FROM twitch_chat_messages WHERE session_id=$1 AND message_ts::timestamptz >= $2").bind(session_id).bind(now-Duration::minutes(i64::from(minutes))).fetch_one(&self.pool).await
    }

    pub async fn chat_ingest_healthy(
        &self,
        uid: &str,
        login: &str,
        now: DateTime<Utc>,
        quiet_window_minutes: i32,
    ) -> Result<bool, sqlx::Error> {
        if quiet_window_minutes == 0 {
            return Ok(true);
        }
        let row = sqlx::query("SELECT last_subscription_ok_at,NULLIF(BTRIM(last_raw_chat_insert_ok_at),'')::timestamptz AS last_ok,NULLIF(BTRIM(last_raw_chat_insert_error_at),'')::timestamptz AS last_error,raw_chat_lag_seconds FROM twitch_raw_chat_ingest_health WHERE twitch_user_id=$1 OR LOWER(streamer_login)=LOWER($2) ORDER BY (twitch_user_id=$1) DESC LIMIT 1")
            .bind(uid)
            .bind(login)
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else { return Ok(false) };
        let subscription_ok: Option<DateTime<Utc>> = row.try_get("last_subscription_ok_at")?;
        let last_ok: Option<DateTime<Utc>> = row.try_get("last_ok")?;
        let last_error: Option<DateTime<Utc>> = row.try_get("last_error")?;
        let lag: Option<i32> = row.try_get("raw_chat_lag_seconds")?;
        Ok(chat_ingest_health_is_ok(
            subscription_ok,
            last_ok,
            last_error,
            lag,
            now,
        ))
    }

    pub async fn enqueue(
        &self,
        uid: &str,
        login: &str,
        action: &str,
        duration: Option<i32>,
        actor: &str,
        idempotency: &str,
    ) -> Result<EnqueueOutcome, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let locked:Option<String>=sqlx::query_scalar("SELECT twitch_user_id FROM twitch_ad_manager_settings WHERE twitch_user_id=$1 FOR UPDATE").bind(uid).fetch_optional(&mut *tx).await?;
        if locked.is_none() {
            return Err(sqlx::Error::RowNotFound);
        }
        let existing = sqlx::query("SELECT twitch_user_id,action,duration_seconds,source,requested_by_twitch_user_id FROM twitch_ad_manager_actions WHERE idempotency_key=$1")
            .bind(idempotency)
            .fetch_optional(&mut *tx)
            .await?;
        if let Some(row) = existing {
            let exact = row.try_get::<String, _>("twitch_user_id")? == uid
                && row.try_get::<String, _>("action")? == action
                && row.try_get::<Option<i32>, _>("duration_seconds")? == duration
                && row.try_get::<String, _>("source")? == "manual"
                && row
                    .try_get::<Option<String>, _>("requested_by_twitch_user_id")?
                    .as_deref()
                    == Some(actor);
            tx.commit().await?;
            return Ok(if exact {
                EnqueueOutcome::AlreadyAccepted
            } else {
                EnqueueOutcome::Conflict
            });
        }
        let count:i64=sqlx::query_scalar("SELECT COUNT(*)::bigint FROM twitch_ad_manager_actions WHERE twitch_user_id=$1 AND source='manual' AND created_at>=NOW()-INTERVAL '1 hour'").bind(uid).fetch_one(&mut *tx).await?;
        if count >= 12 {
            tx.commit().await?;
            return Ok(EnqueueOutcome::RateLimited);
        }
        let result=sqlx::query("INSERT INTO twitch_ad_manager_actions(twitch_user_id,twitch_login,action,duration_seconds,source,requested_by_twitch_user_id,idempotency_key) VALUES($1,$2,$3,$4,'manual',$5,$6) ON CONFLICT DO NOTHING").bind(uid).bind(login).bind(action).bind(duration).bind(actor).bind(idempotency).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(if result.rows_affected() == 1 {
            EnqueueOutcome::Queued
        } else {
            EnqueueOutcome::Conflict
        })
    }

    pub async fn enqueue_automatic(
        &self,
        uid: &str,
        login: &str,
        action: &str,
        duration: Option<i32>,
        idempotency: &str,
    ) -> Result<bool, sqlx::Error> {
        let result=sqlx::query("INSERT INTO twitch_ad_manager_actions(twitch_user_id,twitch_login,action,duration_seconds,source,idempotency_key) VALUES($1,$2,$3,$4,'automatic',$5) ON CONFLICT DO NOTHING").bind(uid).bind(login).bind(action).bind(duration).bind(idempotency).execute(&self.pool).await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn should_write_history(
        &self,
        uid: &str,
        schedule: &AdSchedule,
        now: DateTime<Utc>,
    ) -> Result<bool, sqlx::Error> {
        let row=sqlx::query("SELECT next_ad_at,last_ad_at,duration_seconds,preroll_free_seconds,snooze_count,snooze_refresh_at,last_history_at FROM twitch_ad_manager_state WHERE twitch_user_id=$1").bind(uid).fetch_optional(&self.pool).await?;
        let Some(row) = row else { return Ok(true) };
        let parse = |v: Option<&String>| {
            v.and_then(|s| normalize_ad_time(s))
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|d| d.with_timezone(&Utc))
        };
        let changed = row.try_get::<Option<DateTime<Utc>>, _>("next_ad_at")?
            != parse(schedule.next_ad_at.as_ref())
            || row.try_get::<Option<DateTime<Utc>>, _>("last_ad_at")?
                != parse(schedule.last_ad_at.as_ref())
            || row.try_get::<Option<i32>, _>("duration_seconds")? != Some(schedule.duration as i32)
            || row.try_get::<Option<i32>, _>("preroll_free_seconds")?
                != Some(schedule.preroll_free_time as i32)
            || row.try_get::<Option<i32>, _>("snooze_count")? != Some(schedule.snooze_count as i32)
            || row.try_get::<Option<DateTime<Utc>>, _>("snooze_refresh_at")?
                != parse(schedule.snooze_refresh_at.as_ref());
        let old = row
            .try_get::<Option<DateTime<Utc>>, _>("last_history_at")?
            .map(|t| now - t >= Duration::minutes(5))
            .unwrap_or(true);
        Ok(changed || old)
    }

    pub async fn write_history_snapshot(
        &self,
        uid: &str,
        login: &str,
        schedule: &AdSchedule,
    ) -> Result<(), sqlx::Error> {
        let parse = |value: Option<&String>| {
            value
                .and_then(|raw| normalize_ad_time(raw))
                .and_then(|raw| DateTime::parse_from_rfc3339(&raw).ok())
                .map(|value| value.with_timezone(&Utc))
        };
        let int4 = |value: i64| {
            i32::try_from(value)
                .map_err(|_| sqlx::Error::InvalidArgument("Twitch-Werbezahl außerhalb int4".into()))
        };
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO twitch_ads_schedule_snapshot(twitch_user_id,twitch_login,next_ad_at,last_ad_at,duration,preroll_free_time,snooze_count,snooze_refresh_at,snapshot_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,NOW())")
            .bind(uid).bind(login).bind(parse(schedule.next_ad_at.as_ref())).bind(parse(schedule.last_ad_at.as_ref())).bind(int4(schedule.duration)?).bind(int4(schedule.preroll_free_time)?).bind(int4(schedule.snooze_count)?).bind(parse(schedule.snooze_refresh_at.as_ref())).execute(&mut *tx).await?;
        let updated = sqlx::query(
            "UPDATE twitch_ad_manager_state SET last_history_at=NOW() WHERE twitch_user_id=$1",
        )
        .bind(uid)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(sqlx::Error::RowNotFound);
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn claim_due(&self, uid: &str) -> Result<Option<QueuedAction>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let row=sqlx::query("SELECT id,twitch_user_id,twitch_login,action,duration_seconds,source,preflight_next_ad_at,preflight_last_ad_at,preflight_snooze_count,marked_unknown_at FROM twitch_ad_manager_actions WHERE twitch_user_id=$1 AND due_at<=NOW() AND (status='pending' OR (status='leased' AND lease_until<NOW())) ORDER BY due_at,id FOR UPDATE SKIP LOCKED LIMIT 1").bind(uid).fetch_optional(&mut *tx).await?;
        let Some(row) = row else {
            tx.commit().await?;
            return Ok(None);
        };
        let id: i64 = row.try_get("id")?;
        let lease = format!(
            "{id}:{}",
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        sqlx::query("UPDATE twitch_ad_manager_actions SET status='leased',lease_until=NOW()+INTERVAL '90 seconds',lease_token=$2,attempt_count=attempt_count+1,updated_at=NOW() WHERE id=$1").bind(id).bind(&lease).execute(&mut *tx).await?;
        let raw_action: String = row.try_get("action")?;
        let action = QueuedAction {
            id,
            twitch_user_id: row.try_get("twitch_user_id")?,
            twitch_login: row.try_get("twitch_login")?,
            action: ActionKind::parse(&raw_action)?,
            duration_seconds: row.try_get("duration_seconds")?,
            source: row.try_get("source")?,
            lease_token: lease,
            preflight_next_ad_at: row.try_get("preflight_next_ad_at")?,
            preflight_last_ad_at: row.try_get("preflight_last_ad_at")?,
            preflight_snooze_count: row.try_get("preflight_snooze_count")?,
            marked_unknown_at: row.try_get("marked_unknown_at")?,
        };
        tx.commit().await?;
        Ok(Some(action))
    }

    pub async fn finish_action(
        &self,
        action: &QueuedAction,
        status: &str,
        detail: Option<&str>,
        retry_after: Option<i32>,
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let result=sqlx::query("UPDATE twitch_ad_manager_actions SET status=$3,outcome_detail=$4,retry_after_seconds=$5,completed_at=NOW(),lease_until=NULL,lease_token=NULL,completion_token=CASE WHEN $3='unknown' THEN COALESCE(completion_token,$2) ELSE NULL END,updated_at=NOW() WHERE id=$1 AND ((status='leased' AND lease_token=$2) OR (status='unknown' AND completion_token=$2))").bind(action.id).bind(&action.lease_token).bind(status).bind(detail).bind(retry_after).execute(&mut *tx).await?;
        if result.rows_affected() != 1 {
            return Err(sqlx::Error::RowNotFound);
        }
        sqlx::query("INSERT INTO twitch_ad_manager_state(twitch_user_id,twitch_login,last_action_kind,last_action_outcome,last_action_detail,last_action_at) VALUES($1,$2,$3,$4,$5,NOW()) ON CONFLICT(twitch_user_id) DO UPDATE SET last_action_kind=EXCLUDED.last_action_kind,last_action_outcome=EXCLUDED.last_action_outcome,last_action_detail=EXCLUDED.last_action_detail,last_action_at=NOW(),updated_at=NOW()") .bind(&action.twitch_user_id).bind(&action.twitch_login).bind(action.action.as_str()).bind(status).bind(detail).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Vor dem nicht-idempotenten POST terminal auf `unknown` setzen. Stirbt
    /// der Prozess nach dem Senden, wird die Aktion dadurch nie erneut geclaimt.
    pub async fn mark_unknown_before_send(
        &self,
        action: &QueuedAction,
        schedule: &AdSchedule,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        let parse = |value: Option<&String>| {
            value
                .and_then(|raw| normalize_ad_time(raw))
                .and_then(|raw| DateTime::parse_from_rfc3339(&raw).ok())
                .map(|value| value.with_timezone(&Utc))
        };
        let mut tx = self.pool.begin().await?;
        let result=sqlx::query("UPDATE twitch_ad_manager_actions SET status='unknown',outcome_detail='Twitch-Ergebnis noch unbekannt',lease_until=NULL,lease_token=NULL,completion_token=$2,preflight_next_ad_at=$3,preflight_last_ad_at=$4,preflight_snooze_count=$5,marked_unknown_at=$6,updated_at=NOW() WHERE id=$1 AND lease_token=$2 AND status='leased'").bind(action.id).bind(&action.lease_token).bind(parse(schedule.next_ad_at.as_ref())).bind(parse(schedule.last_ad_at.as_ref())).bind(i32::try_from(schedule.snooze_count).ok()).bind(now).execute(&mut *tx).await?;
        if result.rows_affected() != 1 {
            return Err(sqlx::Error::RowNotFound);
        }
        sqlx::query("INSERT INTO twitch_ad_manager_state(twitch_user_id,twitch_login,last_action_kind,last_action_outcome,last_action_detail,last_action_at) VALUES($1,$2,$3,'unknown','Twitch-Ergebnis noch unbekannt',NOW()) ON CONFLICT(twitch_user_id) DO UPDATE SET last_action_kind=EXCLUDED.last_action_kind,last_action_outcome='unknown',last_action_detail=EXCLUDED.last_action_detail,last_action_at=NOW(),updated_at=NOW()")
            .bind(&action.twitch_user_id).bind(&action.twitch_login).bind(action.action.as_str()).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn unknown_actions(&self, uid: &str) -> Result<Vec<QueuedAction>, sqlx::Error> {
        let rows=sqlx::query("SELECT id,twitch_user_id,twitch_login,action,duration_seconds,source,completion_token,preflight_next_ad_at,preflight_last_ad_at,preflight_snooze_count,marked_unknown_at FROM twitch_ad_manager_actions WHERE twitch_user_id=$1 AND status='unknown' ORDER BY id").bind(uid).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                let raw_action: String = row.try_get("action")?;
                Ok(QueuedAction {
                    id: row.try_get("id")?,
                    twitch_user_id: row.try_get("twitch_user_id")?,
                    twitch_login: row.try_get("twitch_login")?,
                    action: ActionKind::parse(&raw_action)?,
                    duration_seconds: row.try_get("duration_seconds")?,
                    source: row.try_get("source")?,
                    lease_token: row.try_get("completion_token")?,
                    preflight_next_ad_at: row.try_get("preflight_next_ad_at")?,
                    preflight_last_ad_at: row.try_get("preflight_last_ad_at")?,
                    preflight_snooze_count: row.try_get("preflight_snooze_count")?,
                    marked_unknown_at: row.try_get("marked_unknown_at")?,
                })
            })
            .collect()
    }

    /// Löst alte, nicht mehr sicher bestätigbare POST-Ausgänge unabhängig von
    /// Live-State und Helix-Erreichbarkeit nach Ablauf der Schutzfrist.
    pub async fn expire_unknown_actions(
        &self,
        uid: &str,
        now: DateTime<Utc>,
    ) -> Result<u64, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let rows = sqlx::query("UPDATE twitch_ad_manager_actions SET status='unresolved',outcome_detail=$3,retry_after_seconds=NULL,completed_at=$2,lease_until=NULL,lease_token=NULL,completion_token=NULL,updated_at=NOW() WHERE twitch_user_id=$1 AND status='unknown' AND COALESCE(marked_unknown_at,updated_at)<=$2-INTERVAL '15 minutes' RETURNING twitch_login,action")
            .bind(uid)
            .bind(now)
            .bind(UNRESOLVED_DETAIL)
            .fetch_all(&mut *tx)
            .await?;
        for row in &rows {
            let login: String = row.try_get("twitch_login")?;
            let raw_action: String = row.try_get("action")?;
            let action = ActionKind::parse(&raw_action)?;
            sqlx::query("INSERT INTO twitch_ad_manager_state(twitch_user_id,twitch_login,last_action_kind,last_action_outcome,last_action_detail,last_action_at) VALUES($1,$2,$3,'unresolved',$4,$5) ON CONFLICT(twitch_user_id) DO UPDATE SET twitch_login=EXCLUDED.twitch_login,last_action_kind=EXCLUDED.last_action_kind,last_action_outcome='unresolved',last_action_detail=EXCLUDED.last_action_detail,last_action_at=EXCLUDED.last_action_at,updated_at=NOW()")
                .bind(uid)
                .bind(login)
                .bind(action.as_str())
                .bind(UNRESOLVED_DETAIL)
                .bind(now)
                .execute(&mut *tx)
                .await?;
        }
        let expired = rows.len() as u64;
        tx.commit().await?;
        Ok(expired)
    }

    pub async fn cleanup_completed_actions(&self) -> Result<u64, sqlx::Error> {
        let result=sqlx::query("DELETE FROM twitch_ad_manager_actions WHERE status IN ('succeeded','failed','unresolved','cancelled') AND completed_at<NOW()-INTERVAL '90 days'").execute(&self.pool).await?;
        Ok(result.rows_affected())
    }

    pub async fn upsert_state(
        &self,
        uid: &str,
        login: &str,
        live: &LiveState,
        schedule: Option<&AdSchedule>,
        decision: Option<&Decision>,
    ) -> Result<(), sqlx::Error> {
        let parse = |v: Option<&String>| {
            v.and_then(|s| normalize_ad_time(s))
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|d| d.with_timezone(&Utc))
        };
        let (next, last, duration, preroll, snoozes, refresh) = schedule
            .map(|s| {
                (
                    parse(s.next_ad_at.as_ref()),
                    parse(s.last_ad_at.as_ref()),
                    Some(s.duration as i32),
                    Some(s.preroll_free_time as i32),
                    Some(s.snooze_count as i32),
                    parse(s.snooze_refresh_at.as_ref()),
                )
            })
            .unwrap_or((None, None, None, None, None, None));
        sqlx::query("INSERT INTO twitch_ad_manager_state(twitch_user_id,twitch_login,is_live,active_session_id,stream_started_at,next_ad_at,last_ad_at,duration_seconds,preroll_free_seconds,snooze_count,snooze_refresh_at,observed_at,last_decision,last_decision_reason) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,CASE WHEN $12 THEN NOW() ELSE NULL END,$13,$14) ON CONFLICT(twitch_user_id) DO UPDATE SET twitch_login=EXCLUDED.twitch_login,is_live=EXCLUDED.is_live,active_session_id=EXCLUDED.active_session_id,stream_started_at=EXCLUDED.stream_started_at,next_ad_at=EXCLUDED.next_ad_at,last_ad_at=EXCLUDED.last_ad_at,duration_seconds=EXCLUDED.duration_seconds,preroll_free_seconds=EXCLUDED.preroll_free_seconds,snooze_count=EXCLUDED.snooze_count,snooze_refresh_at=EXCLUDED.snooze_refresh_at,observed_at=CASE WHEN $12 THEN NOW() ELSE twitch_ad_manager_state.observed_at END,last_decision=EXCLUDED.last_decision,last_decision_reason=EXCLUDED.last_decision_reason,updated_at=NOW()")
            .bind(uid).bind(login).bind(live.is_live).bind(live.active_session_id).bind(live.stream_started_at).bind(next).bind(last).bind(duration).bind(preroll).bind(snoozes).bind(refresh).bind(schedule.is_some()).bind(decision.map(|d|match d.action{DecisionAction::None=>"none",DecisionAction::Snooze=>"snooze",DecisionAction::Commercial{..}=>"commercial"})).bind(decision.map(|d|d.reason)).execute(&self.pool).await?;
        Ok(())
    }
}
