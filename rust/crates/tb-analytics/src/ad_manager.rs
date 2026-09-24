//! Deterministischer Werbemanager und sein PostgreSQL-Store.
//!
//! Der Entscheider ist I/O-frei. Twitch-Aufrufe bleiben im `tb-bot`, während
//! dieser Store Einstellungen, aktuellen Zustand und die lease-geschützte
//! Aktions-Queue verwaltet.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use tb_transport_twitch::{streams::normalize_ad_time, AdSchedule};

mod steam;

pub const READ_SCOPE: &str = "channel:read:ads";
pub const SNOOZE_SCOPE: &str = "channel:manage:ads";
pub const COMMERCIAL_SCOPE: &str = "channel:edit:commercial";
pub const LIVE_STATE_MAX_AGE: Duration = Duration::minutes(5);
/// Frische-Schranke für den Steam-Match-Status in Werbeentscheidungen. Bewusst
/// enger als die Presence-Toleranz des Titel-Generators (600 s, siehe
/// tb-chat/steam_lookup.rs), weil Queue-Phasen kurz sind; bei Überschreitung
/// wird kein sicheres Werbefenster angenommen.
pub const MATCH_STATUS_FRESH_SECS: i64 = 180;
pub const UNRESOLVED_DETAIL: &str =
    "Ausgang konnte nach 15 Minuten nicht eindeutig bestätigt werden; Sperre wurde aufgehoben.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Strategy {
    Snooze,
    Smart,
}

pub const DEFAULT_RETRY_AFTER_SECS: i32 = 8 * 60;
const RAID_LOCK_MIN: i64 = 10;
const FIRST_CHATTER_LOCK_MIN: i64 = 5;
const POST_MATCH_WAIT_MIN: i64 = 1;
const PULL_FORWARD_HORIZON_MIN: i64 = 12;
// Two 25-second worker ticks plus network margin; a 10-second setting can miss a tick.
const MIN_ACTION_LEAD_SECS: i32 = 60;
pub const HINT_WINDOW_SECS: i64 = 45;

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
            Self::Snooze => "snooze",
            Self::Smart => "smart",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "snooze" => Some(Self::Snooze),
            "smart" => Some(Self::Smart),
            _ => None,
        }
    }
    pub fn required_scopes(self) -> &'static [&'static str] {
        match self {
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
    pub budget_minutes_per_hour: i32,
    pub ad_duration_seconds: i32,
    pub min_interval_minutes: i32,
    pub startup_delay_minutes: i32,
    pub quiet_window_minutes: i32,
    pub action_lead_seconds: i32,
    #[serde(default = "default_chat_notice_before_ad")]
    pub chat_notice_before_ad: bool,
}

fn default_chat_notice_before_ad() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            strategy: Strategy::Smart,
            budget_minutes_per_hour: 3,
            ad_duration_seconds: 90,
            min_interval_minutes: 30,
            startup_delay_minutes: 15,
            quiet_window_minutes: 5,
            action_lead_seconds: 60,
            chat_notice_before_ad: true,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(1..=8).contains(&self.budget_minutes_per_hour) {
            return Err("budgetMinutesPerHour muss zwischen 1 und 8 liegen");
        }
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

/// Frischer Matchstatus aus der Steam-Bot-API mit ursprünglichem Messzeitpunkt.
/// Die Kontozuordnung wird im Twitch-System über die stabile Benutzer-ID gewählt.
/// `in_match` schützt das Match; `in_deadlock` unterscheidet Queue und Menü von
/// einem bestätigten Zustand außerhalb von Deadlock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SteamMatchState {
    pub in_match: bool,
    pub in_deadlock: bool,
    pub hero: Option<String>,
    pub stage: Option<String>,
    pub observed_at: DateTime<Utc>,
}

/// Steam-Anknüpfung eines Kanals samt Zustand; `state` ist nur gesetzt, solange
/// die Presence frisch ist. Der Dashboard-Status zeigt auch den veralteten
/// Stand; ohne frischen Status ist kein Werbefenster bestätigt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamMatchSummary {
    pub steam_linked: bool,
    pub state: Option<SteamMatchState>,
    pub observed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdPlan {
    pub next_block_at: Option<DateTime<Utc>>,
    pub block_seconds: i32,
    pub blocks_per_hour: i32,
    pub budget_used_seconds_this_hour: i32,
}

pub fn plan_next_block(
    now: DateTime<Utc>,
    stream_started_at: Option<DateTime<Utc>>,
    budget_minutes_per_hour: i32,
    budget_used_seconds_this_hour: i32,
    last_block_at: Option<DateTime<Utc>>,
    retry_after_seconds: i32,
) -> AdPlan {
    let budget_seconds = budget_minutes_per_hour.clamp(1, 8) * 60;
    let retry = retry_after_seconds.max(1);

    let count_30 = (budget_seconds / 30).max(1);
    let period_30 = 3600 / count_30;
    let block_seconds = if period_30 >= retry { 30 } else { 60 };
    let desired_blocks = (budget_seconds / block_seconds).max(1);
    let period = (3600 / desired_blocks).max(retry);
    let blocks_per_hour = (3600 / period).max(1);

    let next_block_at = if budget_used_seconds_this_hour >= budget_seconds {
        None
    } else {
        Some(match last_block_at {
            Some(last) => last + Duration::seconds(i64::from(period)),
            None => stream_started_at.unwrap_or(now),
        })
    };

    AdPlan {
        next_block_at,
        block_seconds,
        blocks_per_hour,
        budget_used_seconds_this_hour,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchTiming {
    pub match_started_at: Option<DateTime<Utc>>,
    pub match_ended_at: Option<DateTime<Utc>>,
    pub avg_match_seconds: Option<i32>,
    pub avg_queue_seconds: Option<i32>,
}

fn ema(prev: Option<i32>, sample: i32) -> i32 {
    match prev {
        Some(previous) => (previous * 3 + sample) / 4,
        None => sample,
    }
}

fn clamp_sample(seconds: i64, low: i64, high: i64) -> Option<i32> {
    if seconds < low || seconds > high {
        return None;
    }
    i32::try_from(seconds).ok()
}

pub fn assess_plan(
    twitch_is_source: bool,
    planned_interval_seconds: Option<i32>,
    snooze_count: i64,
    avg_match_seconds: Option<i32>,
    avg_queue_seconds: Option<i32>,
) -> &'static str {
    if !twitch_is_source {
        return "good";
    }
    let (Some(interval), Some(avg_match)) = (planned_interval_seconds, avg_match_seconds) else {
        return "good";
    };
    if interval <= 0 || avg_match <= 0 {
        return "good";
    }
    let cycle = avg_match + avg_queue_seconds.unwrap_or(0);
    if interval >= cycle {
        "good"
    } else if interval >= avg_match && snooze_count > 0 {
        "tight"
    } else {
        "unprotectable"
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
    pub recent_chat_messages: i64,
    pub chat_ingest_healthy: bool,
    pub steam_match_state: Option<SteamMatchState>,
    pub plan: AdPlan,
    pub match_started_at: Option<DateTime<Utc>>,
    pub match_ended_at: Option<DateTime<Utc>>,
    pub last_raid_at: Option<DateTime<Utc>>,
    pub last_raider: Option<String>,
    pub last_first_chatter_at: Option<DateTime<Utc>>,
    pub last_first_chatter: Option<String>,
    pub retry_after_seconds: i32,
    pub pull_forward_seconds: i32,
    pub plan_fit: &'static str,
}

impl DecisionInput {
    pub fn twitch_is_budget_source(&self) -> bool {
        self.next_ad_at.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionAction {
    None,
    Postpone,
    Snooze,
    Commercial { duration_seconds: i32 },
}

impl DecisionAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Postpone => "postpone",
            Self::Snooze => "snooze",
            Self::Commercial { .. } => "commercial",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub action: DecisionAction,
    pub reason: &'static str,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub at: String,
    pub decision: String,
    pub reason: String,
    pub block_seconds: Option<i32>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistorySummary {
    pub blocks_run: i64,
    pub blocks_in_window: i64,
    pub budget_seconds_used: i64,
    pub budget_seconds_planned: i64,
    pub postponed: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdManagerHistory {
    pub session_started_at: Option<String>,
    pub summary: HistorySummary,
    pub entries: Vec<HistoryEntry>,
}

fn chat_is_quiet(input: &DecisionInput) -> bool {
    if input.settings.quiet_window_minutes == 0 {
        return true;
    }
    input.chat_ingest_healthy && input.quiet_chat_messages == 0
}

fn active_lock(input: &DecisionInput) -> Option<(&'static str, Option<String>)> {
    let now = input.now;
    match input.steam_match_state.as_ref() {
        Some(state) if state.observed_at <= now + Duration::seconds(30)
            && now.signed_duration_since(state.observed_at) <= Duration::seconds(MATCH_STATUS_FRESH_SECS) => {
                if state.in_match { return Some(("in_match", None)); }
            }
        _ => return Some(("match_status_unknown", None)),
    }
    match input.stream_started_at {
        Some(start)
            if now
                >= start + Duration::minutes(i64::from(input.settings.startup_delay_minutes)) => {}
        _ => return Some(("startup_protection", None)),
    }
    if let Some(at) = input.last_raid_at {
        if now >= at && now.signed_duration_since(at) < Duration::minutes(RAID_LOCK_MIN) {
            return Some(("recent_raid", input.last_raider.clone()));
        }
    }
    if let Some(at) = input.last_first_chatter_at {
        if now >= at && now.signed_duration_since(at) < Duration::minutes(FIRST_CHATTER_LOCK_MIN) {
            return Some(("recent_first_chatter", input.last_first_chatter.clone()));
        }
    }
    None
}

fn is_overdue(input: &DecisionInput) -> bool {
    if input.plan.blocks_per_hour <= 0 {
        return false;
    }
    let half_period = i64::from(3600 / input.plan.blocks_per_hour) / 2;
    input
        .plan
        .next_block_at
        .map(|at| input.now >= at + Duration::seconds(half_period))
        .unwrap_or(false)
}

fn twitch_cooldown_allows(input: &DecisionInput) -> bool {
    let now = input.now;
    let Some(last) = input.last_ad_at else {
        return true;
    };
    let retry = i64::from(input.retry_after_seconds.max(1));
    now >= last + Duration::seconds(retry)
}

fn pull_forward_window_open(input: &DecisionInput) -> bool {
    let in_window = input
        .steam_match_state
        .as_ref()
        .map(|state| state.in_deadlock && !state.in_match)
        .unwrap_or(false);
    if !in_window {
        return false;
    }
    if let Some(ended) = input.match_ended_at {
        let since = input.now.signed_duration_since(ended);
        if since >= Duration::zero() && since < Duration::minutes(POST_MATCH_WAIT_MIN) {
            return false;
        }
        if since >= Duration::minutes(POST_MATCH_WAIT_MIN)
            && since < Duration::minutes(POST_MATCH_WAIT_MIN + 1)
            && input.recent_chat_messages > 0
        {
            return false;
        }
    }
    true
}

fn should_pull_forward(input: &DecisionInput) -> bool {
    if !pull_forward_window_open(input) {
        return false;
    }
    let now = input.now;
    let Some(next_ad) = input.next_ad_at else {
        return false;
    };
    let lead = i64::from(input.settings.action_lead_seconds.max(MIN_ACTION_LEAD_SECS));
    let beyond_lead = next_ad > now + Duration::seconds(lead);
    let in_reach = next_ad <= now + Duration::minutes(PULL_FORWARD_HORIZON_MIN);
    let match_risk = input.plan_fit != "good";
    beyond_lead && (in_reach || match_risk) && twitch_cooldown_allows(input)
}

pub fn decide(input: &DecisionInput) -> Decision {
    let none = |reason| Decision {
        action: DecisionAction::None,
        reason,
        detail: None,
    };
    let postpone = |reason, detail| Decision {
        action: DecisionAction::Postpone,
        reason,
        detail,
    };
    let commercial = |reason| Decision {
        action: DecisionAction::Commercial {
            duration_seconds: input.plan.block_seconds,
        },
        reason,
        detail: None,
    };

    if !input.settings.enabled {
        return none("disabled");
    }

    let now = input.now;
    let twitch_ad_imminent = input
        .next_ad_at
        .map(|at| {
            at > now
                && at.signed_duration_since(now).num_seconds()
                    <= i64::from(input.settings.action_lead_seconds.max(MIN_ACTION_LEAD_SECS))
        })
        .unwrap_or(false);

    if input.settings.strategy == Strategy::Snooze {
        if twitch_ad_imminent {
            return if input.snooze_count > 0 {
                Decision {
                    action: DecisionAction::Snooze,
                    reason: "twitch_ad_moved",
                    detail: None,
                }
            } else {
                none("no_snoozes")
            };
        }
        return none("cooldown");
    }

    let lock = active_lock(input);

    if twitch_ad_imminent {
        return match lock {
            Some((reason, detail)) => {
                let valuable = matches!(reason, "in_match" | "match_status_unknown" | "recent_raid");
                let dense = input.plan_fit == "tight" || input.plan_fit == "unprotectable";
                if input.snooze_count > 0 && (valuable || !dense) {
                    Decision {
                        action: DecisionAction::Snooze,
                        reason: "twitch_ad_moved",
                        detail,
                    }
                } else if input.snooze_count > 0 {
                    none("twitch_plan_active")
                } else {
                    postpone(reason, detail)
                }
            }
            None if pull_forward_window_open(input) && twitch_cooldown_allows(input) => {
                Decision {
                    action: DecisionAction::Commercial {
                        duration_seconds: input.pull_forward_seconds,
                    },
                    reason: "pulled_forward",
                    detail: input.next_ad_at.map(|at| at.to_rfc3339()),
                }
            }
            None => none("twitch_plan_active"),
        };
    }

    if input.twitch_is_budget_source() {
        if let Some((reason, detail)) = lock {
            return postpone(reason, detail);
        }
        if should_pull_forward(input) {
            return Decision {
                action: DecisionAction::Commercial {
                    duration_seconds: input.pull_forward_seconds,
                },
                reason: "pulled_forward",
                detail: input.next_ad_at.map(|at| at.to_rfc3339()),
            };
        }
        return none("twitch_plan_active");
    }

    let block_due = input
        .plan
        .next_block_at
        .map(|at| now >= at)
        .unwrap_or(false);
    if !block_due {
        return if input.plan.next_block_at.is_none() {
            postpone("budget_reached", None)
        } else {
            none("cooldown")
        };
    }

    if let Some((reason, detail)) = lock {
        return postpone(reason, detail);
    }

    if let Some(ended) = input.match_ended_at {
        let since = now.signed_duration_since(ended);
        if since >= Duration::zero() && since < Duration::minutes(POST_MATCH_WAIT_MIN) {
            return postpone("post_match_wait", None);
        }
        if since >= Duration::minutes(POST_MATCH_WAIT_MIN)
            && since < Duration::minutes(POST_MATCH_WAIT_MIN + 1)
        {
            return if input.recent_chat_messages > 0 {
                postpone("post_match_chat_active", None)
            } else {
                commercial("post_match_quiet")
            };
        }
    }

    if let Some(state) = input.steam_match_state.as_ref() {
        if state.in_deadlock {
            return commercial("in_queue");
        }
    }

    if chat_is_quiet(input) {
        commercial("quiet_chat")
    } else if is_overdue(input) {
        commercial("fallback_least_bad")
    } else {
        postpone("cooldown", None)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdHint {
    pub key: String,
    pub duration_seconds: Option<i32>,
}

const AD_HINT_WITH_DURATION: [&str; 6] = [
    "Kurze Werbung in etwa 30 Sekunden, {dur} Sekunden lang, danach geht es weiter.",
    "Gleich läuft kurz Werbung, {dur} Sekunden. Holt euch was zu trinken, bis gleich.",
    "In einer halben Minute kommt Werbung, {dur} Sekunden, dann sind wir gleich wieder da.",
    "Kurze Pause für die Werbung, {dur} Sekunden. Gleich geht es normal weiter.",
    "Werbung in etwa 30 Sekunden, {dur} Sekunden lang. Bleibt dran, wir sehen uns gleich.",
    "Gleich {dur} Sekunden Werbung. Perfekter Moment für einen Schluck, bis gleich.",
];

const AD_HINT_WITHOUT_DURATION: [&str; 6] = [
    "Gleich kommt kurz Werbung, danach geht es normal weiter.",
    "In etwa 30 Sekunden läuft kurz Werbung. Holt euch was zu trinken, bis gleich.",
    "Kurze Werbung steht an. Wir sind gleich wieder da.",
    "Kurze Pause für die Werbung. Danach geht es sofort weiter.",
    "Werbung in etwa 30 Sekunden. Bleibt dran, dauert nicht lang.",
    "Gleich kommt kurz Werbung. Einmal durchatmen, wir sehen uns gleich.",
];

const AD_HINT_IMMEDIATE_WITH_DURATION: [&str; 6] = [
    "Gleich läuft kurz Werbung, {dur} Sekunden, danach geht es weiter.",
    "Guter Moment: gleich {dur} Sekunden Werbung, dann sind wir wieder da.",
    "Gleich kommt kurz Werbung, {dur} Sekunden. Holt euch was zu trinken, bis gleich.",
    "Kurze Pause für die Werbung, {dur} Sekunden. Gleich geht es normal weiter.",
    "Gleich {dur} Sekunden Werbung, wir nutzen die Pause vor dem Match.",
    "Kurz Werbung, {dur} Sekunden, danach machen wir sofort weiter.",
];

const AD_HINT_IMMEDIATE_WITHOUT_DURATION: [&str; 6] = [
    "Gleich läuft kurz Werbung, danach geht es normal weiter.",
    "Guter Moment: gleich kurz Werbung, dann sind wir wieder da.",
    "Gleich kommt kurz Werbung. Holt euch was zu trinken, bis gleich.",
    "Kurze Pause für die Werbung. Gleich geht es normal weiter.",
    "Gleich kurz Werbung, wir nutzen die Pause vor dem Match.",
    "Kurz Werbung, danach machen wir sofort weiter.",
];

pub fn ad_hint(
    input: &DecisionInput,
    decision: &Decision,
    twitch_ad_duration_seconds: Option<i32>,
    window_secs: i64,
) -> Option<AdHint> {
    if !input.settings.enabled || !input.settings.chat_notice_before_ad {
        return None;
    }
    let now = input.now;
    let secs_until = |at: DateTime<Utc>| at.signed_duration_since(now).num_seconds();

    if let Some(next_ad) = input.next_ad_at {
        if decision.reason == "pulled_forward" {
            return Some(AdHint {
                key: format!("pull:{}", next_ad.to_rfc3339()),
                duration_seconds: Some(input.pull_forward_seconds).filter(|value| *value > 0),
            });
        }
        let twitch_window = window_secs.min(i64::from(input.settings.action_lead_seconds.max(MIN_ACTION_LEAD_SECS)));
        let secs = secs_until(next_ad);
        if secs <= 0 || secs > twitch_window {
            return None;
        }
        if matches!(
            decision.action,
            DecisionAction::Snooze | DecisionAction::Commercial { .. }
        ) || matches!(decision.reason, "in_match" | "match_status_unknown")
        {
            return None;
        }
        return Some(AdHint {
            key: format!("twitch:{}", next_ad.to_rfc3339()),
            duration_seconds: twitch_ad_duration_seconds.filter(|value| *value > 0),
        });
    }

    let block_at = input.plan.next_block_at?;
    let secs = secs_until(block_at);
    let window_open = input
        .steam_match_state
        .as_ref()
        .map(|state| state.in_deadlock || state.in_match)
        .unwrap_or(false);
    let post_match_zone = input
        .match_ended_at
        .map(|ended| {
            let since = now.signed_duration_since(ended);
            since >= Duration::zero() && since < Duration::minutes(POST_MATCH_WAIT_MIN + 1)
        })
        .unwrap_or(false);
    if !window_open
        || post_match_zone
        || secs <= 0
        || secs > window_secs
        || active_lock(input).is_some()
    {
        return None;
    }
    Some(AdHint {
        key: format!("own:{}", block_at.to_rfc3339()),
        duration_seconds: Some(input.plan.block_seconds),
    })
}

pub fn ad_hint_text(
    duration_seconds: Option<i32>,
    previous_variant: Option<i16>,
    seed: u64,
    immediate: bool,
) -> (String, i16) {
    let (with_dur, without_dur): (&[&str; 6], &[&str; 6]) = if immediate {
        (
            &AD_HINT_IMMEDIATE_WITH_DURATION,
            &AD_HINT_IMMEDIATE_WITHOUT_DURATION,
        )
    } else {
        (&AD_HINT_WITH_DURATION, &AD_HINT_WITHOUT_DURATION)
    };
    let count = with_dur.len();
    let mut index = (seed % count as u64) as usize;
    if Some(index as i16) == previous_variant {
        index = (index + 1) % count;
    }
    let text = match duration_seconds.filter(|value| *value > 0) {
        Some(dur) => with_dur[index].replace("{dur}", &dur.to_string()),
        None => without_dur[index].to_string(),
    };
    (text, index as i16)
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
    steam: steam::Client,
}

impl AdManagerStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool, steam: steam::Client::new() }
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
        let rows = sqlx::query("SELECT s.twitch_user_id,s.twitch_login,s.enabled,s.strategy,s.budget_minutes_per_hour,s.ad_duration_seconds,s.min_interval_minutes,s.startup_delay_minutes,s.quiet_window_minutes,s.action_lead_seconds,s.chat_notice_before_ad FROM twitch_ad_manager_settings s ORDER BY s.twitch_user_id").fetch_all(&self.pool).await?;
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
                        budget_minutes_per_hour: r.try_get("budget_minutes_per_hour")?,
                        ad_duration_seconds: r.try_get("ad_duration_seconds")?,
                        min_interval_minutes: r.try_get("min_interval_minutes")?,
                        startup_delay_minutes: r.try_get("startup_delay_minutes")?,
                        quiet_window_minutes: r.try_get("quiet_window_minutes")?,
                        action_lead_seconds: r.try_get("action_lead_seconds")?,
                        chat_notice_before_ad: r.try_get("chat_notice_before_ad")?,
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
        let row = sqlx::query("SELECT enabled,strategy,budget_minutes_per_hour,ad_duration_seconds,min_interval_minutes,startup_delay_minutes,quiet_window_minutes,action_lead_seconds,chat_notice_before_ad,updated_at FROM twitch_ad_manager_settings WHERE twitch_user_id=$1").bind(uid).fetch_optional(&self.pool).await?;
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
                budget_minutes_per_hour: row.try_get("budget_minutes_per_hour")?,
                ad_duration_seconds: row.try_get("ad_duration_seconds")?,
                min_interval_minutes: row.try_get("min_interval_minutes")?,
                startup_delay_minutes: row.try_get("startup_delay_minutes")?,
                quiet_window_minutes: row.try_get("quiet_window_minutes")?,
                action_lead_seconds: row.try_get("action_lead_seconds")?,
                chat_notice_before_ad: row.try_get("chat_notice_before_ad")?,
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
        let row=sqlx::query("INSERT INTO twitch_ad_manager_settings(twitch_user_id,twitch_login,enabled,strategy,budget_minutes_per_hour,ad_duration_seconds,min_interval_minutes,startup_delay_minutes,quiet_window_minutes,action_lead_seconds,chat_notice_before_ad) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) ON CONFLICT(twitch_user_id) DO UPDATE SET twitch_login=EXCLUDED.twitch_login,enabled=EXCLUDED.enabled,strategy=EXCLUDED.strategy,budget_minutes_per_hour=EXCLUDED.budget_minutes_per_hour,ad_duration_seconds=EXCLUDED.ad_duration_seconds,min_interval_minutes=EXCLUDED.min_interval_minutes,startup_delay_minutes=EXCLUDED.startup_delay_minutes,quiet_window_minutes=EXCLUDED.quiet_window_minutes,action_lead_seconds=EXCLUDED.action_lead_seconds,chat_notice_before_ad=EXCLUDED.chat_notice_before_ad,updated_at=NOW() RETURNING updated_at").bind(uid).bind(login).bind(settings.enabled).bind(settings.strategy.as_str()).bind(settings.budget_minutes_per_hour).bind(settings.ad_duration_seconds).bind(settings.min_interval_minutes).bind(settings.startup_delay_minutes).bind(settings.quiet_window_minutes).bind(settings.action_lead_seconds).bind(settings.chat_notice_before_ad).fetch_one(&mut *tx).await?;
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

    /// The identity belongs to Twitch; Steam presence is read through Steam's API.
    /// Callers pass the authenticated stable Twitch ID, never a display name.
    pub async fn steam_match_summary(
        &self,
        twitch_user_id: &str,
        now: DateTime<Utc>,
    ) -> Result<SteamMatchSummary, sqlx::Error> {
        steam::summary(&self.pool, &self.steam, twitch_user_id, now).await
    }

    pub async fn budget_used_this_hour(
        &self,
        uid: &str,
        now: DateTime<Utc>,
    ) -> Result<(i32, Option<DateTime<Utc>>), sqlx::Error> {
        let row = sqlx::query("SELECT COALESCE(SUM(duration_seconds),0)::bigint AS used,MAX(completed_at) AS last FROM twitch_ad_manager_actions WHERE twitch_user_id=$1 AND action='commercial' AND status='succeeded' AND completed_at>=$2")
            .bind(uid)
            .bind(now - Duration::hours(1))
            .fetch_one(&self.pool)
            .await?;
        let used: i64 = row.try_get("used")?;
        let last: Option<DateTime<Utc>> = row.try_get("last")?;
        Ok((i32::try_from(used).unwrap_or(i32::MAX), last))
    }

    pub async fn last_commercial_retry_after(&self, uid: &str) -> Result<Option<i32>, sqlx::Error> {
        sqlx::query_scalar("SELECT retry_after_seconds FROM twitch_ad_manager_actions WHERE twitch_user_id=$1 AND action='commercial' AND status='succeeded' AND retry_after_seconds IS NOT NULL ORDER BY completed_at DESC LIMIT 1")
            .bind(uid)
            .fetch_optional(&self.pool)
            .await
            .map(Option::flatten)
    }

    pub async fn last_incoming_raid(
        &self,
        uid: &str,
    ) -> Result<Option<(DateTime<Utc>, String)>, sqlx::Error> {
        let row = sqlx::query("SELECT detected_at,from_broadcaster_login FROM twitch_raid_arrival_tracking WHERE to_broadcaster_id=$1 ORDER BY detected_at DESC LIMIT 1")
            .bind(uid)
            .fetch_optional(&self.pool)
            .await?;
        Ok(match row {
            Some(row) => Some((
                row.try_get("detected_at")?,
                row.try_get("from_broadcaster_login")?,
            )),
            None => None,
        })
    }

    pub async fn last_first_chatter(
        &self,
        session_id: i64,
    ) -> Result<Option<(DateTime<Utc>, String)>, sqlx::Error> {
        let row = sqlx::query("SELECT first_message_at AS at,chatter_login FROM twitch_session_chatters WHERE session_id=$1 AND confirmed_first_ever ORDER BY first_message_at DESC NULLS LAST LIMIT 1")
            .bind(session_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(match row {
            Some(row) => Some((row.try_get("at")?, row.try_get("chatter_login")?)),
            None => None,
        })
    }

    pub async fn record_match_transition(
        &self,
        uid: &str,
        login: &str,
        in_match: bool,
        now: DateTime<Utc>,
    ) -> Result<MatchTiming, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let prev = sqlx::query("SELECT match_active,match_started_at,match_ended_at,avg_match_seconds,avg_queue_seconds FROM twitch_ad_manager_state WHERE twitch_user_id=$1 FOR UPDATE")
            .bind(uid)
            .fetch_optional(&mut *tx)
            .await?;
        let (was_active, prev_started, prev_ended, mut avg_match, mut avg_queue) =
            match prev.as_ref() {
                Some(row) => (
                    row.try_get::<bool, _>("match_active")?,
                    row.try_get::<Option<DateTime<Utc>>, _>("match_started_at")?,
                    row.try_get::<Option<DateTime<Utc>>, _>("match_ended_at")?,
                    row.try_get::<Option<i32>, _>("avg_match_seconds")?,
                    row.try_get::<Option<i32>, _>("avg_queue_seconds")?,
                ),
                None => (false, None, None, None, None),
            };
        let (started, ended) = match (was_active, in_match) {
            (false, true) => {
                if let Some(prev_end) = prev_ended {
                    if let Some(sample) =
                        clamp_sample(now.signed_duration_since(prev_end).num_seconds(), 5, 1800)
                    {
                        avg_queue = Some(ema(avg_queue, sample));
                    }
                }
                (Some(now), prev_ended)
            }
            (true, false) => {
                if let Some(prev_start) = prev_started {
                    if let Some(sample) = clamp_sample(
                        now.signed_duration_since(prev_start).num_seconds(),
                        60,
                        3600,
                    ) {
                        avg_match = Some(ema(avg_match, sample));
                    }
                }
                (prev_started, Some(now))
            }
            _ => (prev_started, prev_ended),
        };
        sqlx::query("INSERT INTO twitch_ad_manager_state(twitch_user_id,twitch_login,match_active,match_started_at,match_ended_at,avg_match_seconds,avg_queue_seconds) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(twitch_user_id) DO UPDATE SET twitch_login=EXCLUDED.twitch_login,match_active=EXCLUDED.match_active,match_started_at=EXCLUDED.match_started_at,match_ended_at=EXCLUDED.match_ended_at,avg_match_seconds=EXCLUDED.avg_match_seconds,avg_queue_seconds=EXCLUDED.avg_queue_seconds,updated_at=NOW()")
            .bind(uid).bind(login).bind(in_match).bind(started).bind(ended).bind(avg_match).bind(avg_queue)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(MatchTiming {
            match_started_at: started,
            match_ended_at: ended,
            avg_match_seconds: avg_match,
            avg_queue_seconds: avg_queue,
        })
    }

    pub async fn match_timing(&self, uid: &str) -> Result<MatchTiming, sqlx::Error> {
        let row = sqlx::query("SELECT match_started_at,match_ended_at,avg_match_seconds,avg_queue_seconds FROM twitch_ad_manager_state WHERE twitch_user_id=$1")
            .bind(uid)
            .fetch_optional(&self.pool)
            .await?;
        Ok(match row {
            Some(row) => MatchTiming {
                match_started_at: row.try_get("match_started_at")?,
                match_ended_at: row.try_get("match_ended_at")?,
                avg_match_seconds: row.try_get("avg_match_seconds")?,
                avg_queue_seconds: row.try_get("avg_queue_seconds")?,
            },
            None => MatchTiming {
                match_started_at: None,
                match_ended_at: None,
                avg_match_seconds: None,
                avg_queue_seconds: None,
            },
        })
    }

    pub async fn store_plan_fit(
        &self,
        uid: &str,
        fit: &str,
        session_id: Option<i64>,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let previous: Option<Option<String>> = sqlx::query_scalar(
            "SELECT plan_fit FROM twitch_ad_manager_state WHERE twitch_user_id=$1 FOR UPDATE",
        )
        .bind(uid)
        .fetch_optional(&mut *tx)
        .await?;
        let changed = match previous {
            Some(value) => value.as_deref() != Some(fit),
            None => true,
        };
        sqlx::query("UPDATE twitch_ad_manager_state SET plan_fit=$2,updated_at=NOW() WHERE twitch_user_id=$1")
            .bind(uid)
            .bind(fit)
            .execute(&mut *tx)
            .await?;
        if changed {
            sqlx::query("INSERT INTO twitch_ad_manager_decisions(twitch_user_id,session_id,decided_at,decision,reason,block_seconds,detail) VALUES($1,$2,$3,'none','plan_fit_changed',NULL,$4)")
                .bind(uid)
                .bind(session_id)
                .bind(now)
                .bind(fit)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn store_plan(&self, uid: &str, plan: &AdPlan) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE twitch_ad_manager_state SET plan_next_block_at=$2,plan_block_seconds=$3,plan_blocks_per_hour=$4,budget_used_seconds=$5,updated_at=NOW() WHERE twitch_user_id=$1")
            .bind(uid)
            .bind(plan.next_block_at)
            .bind(plan.block_seconds)
            .bind(plan.blocks_per_hour)
            .bind(plan.budget_used_seconds_this_hour)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn last_hint(&self, uid: &str) -> Result<(Option<String>, Option<i16>), sqlx::Error> {
        let row = sqlx::query(
            "SELECT last_hint_key,last_hint_variant FROM twitch_ad_manager_state WHERE twitch_user_id=$1",
        )
        .bind(uid)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(r) => Ok((r.try_get("last_hint_key")?, r.try_get("last_hint_variant")?)),
            None => Ok((None, None)),
        }
    }

    pub async fn record_hint(
        &self,
        uid: &str,
        login: &str,
        key: &str,
        variant: i16,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO twitch_ad_manager_state(twitch_user_id,twitch_login,last_hint_key,last_hint_variant,last_hint_at) VALUES($1,$2,$3,$4,$5) ON CONFLICT(twitch_user_id) DO UPDATE SET twitch_login=EXCLUDED.twitch_login,last_hint_key=EXCLUDED.last_hint_key,last_hint_variant=EXCLUDED.last_hint_variant,last_hint_at=EXCLUDED.last_hint_at,updated_at=NOW()",
        )
        .bind(uid)
        .bind(login)
        .bind(key)
        .bind(variant)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn record_decision_if_changed(
        &self,
        uid: &str,
        session_id: Option<i64>,
        decision: &Decision,
        block_seconds: Option<i32>,
        now: DateTime<Utc>,
    ) -> Result<bool, sqlx::Error> {
        let last = sqlx::query("SELECT decision,reason FROM twitch_ad_manager_decisions WHERE twitch_user_id=$1 ORDER BY decided_at DESC,id DESC LIMIT 1")
            .bind(uid)
            .fetch_optional(&self.pool)
            .await?;
        if let Some(row) = last {
            let same = row.try_get::<String, _>("decision")? == decision.action.as_str()
                && row.try_get::<String, _>("reason")? == decision.reason;
            if same {
                return Ok(false);
            }
        }
        sqlx::query("INSERT INTO twitch_ad_manager_decisions(twitch_user_id,session_id,decided_at,decision,reason,block_seconds,detail) VALUES($1,$2,$3,$4,$5,$6,$7)")
            .bind(uid)
            .bind(session_id)
            .bind(now)
            .bind(decision.action.as_str())
            .bind(decision.reason)
            .bind(block_seconds)
            .bind(decision.detail.as_deref())
            .execute(&self.pool)
            .await?;
        Ok(true)
    }

    pub async fn cleanup_old_decisions(&self) -> Result<u64, sqlx::Error> {
        let result = sqlx::query(
            "DELETE FROM twitch_ad_manager_decisions WHERE decided_at<NOW()-INTERVAL '30 days'",
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn history(&self, uid: &str) -> Result<AdManagerHistory, sqlx::Error> {
        let state = sqlx::query("SELECT is_live,active_session_id,stream_started_at FROM twitch_ad_manager_state WHERE twitch_user_id=$1")
            .bind(uid)
            .fetch_optional(&self.pool)
            .await?;
        let budget_minutes: i32 = sqlx::query_scalar("SELECT budget_minutes_per_hour FROM twitch_ad_manager_settings WHERE twitch_user_id=$1")
            .bind(uid)
            .fetch_optional(&self.pool)
            .await?
            .unwrap_or(Settings::default().budget_minutes_per_hour);

        let live = state
            .as_ref()
            .and_then(|r| r.try_get::<bool, _>("is_live").ok())
            .unwrap_or(false);
        let active_session = state.as_ref().and_then(|r| {
            r.try_get::<Option<i64>, _>("active_session_id")
                .ok()
                .flatten()
        });
        let stream_started: Option<DateTime<Utc>> = state
            .as_ref()
            .and_then(|r| r.try_get("stream_started_at").ok())
            .flatten();

        let session_id = if live && active_session.is_some() {
            active_session
        } else {
            sqlx::query_scalar("SELECT session_id FROM twitch_ad_manager_decisions WHERE twitch_user_id=$1 ORDER BY decided_at DESC,id DESC LIMIT 1")
                .bind(uid)
                .fetch_optional(&self.pool)
                .await?
                .flatten()
        };

        let summary_row = sqlx::query("SELECT COUNT(*) FILTER (WHERE decision='commercial')::bigint AS blocks_run,COUNT(*) FILTER (WHERE decision='commercial' AND reason IN ('in_queue','match_start_window','post_match_quiet'))::bigint AS blocks_in_window,COALESCE(SUM(block_seconds) FILTER (WHERE decision='commercial'),0)::bigint AS budget_used,COUNT(*) FILTER (WHERE decision='postpone')::bigint AS postponed,MIN(decided_at) AS first_at FROM twitch_ad_manager_decisions WHERE twitch_user_id=$1 AND session_id IS NOT DISTINCT FROM $2")
            .bind(uid)
            .bind(session_id)
            .fetch_one(&self.pool)
            .await?;
        let summary = HistorySummary {
            blocks_run: summary_row.try_get("blocks_run")?,
            blocks_in_window: summary_row.try_get("blocks_in_window")?,
            budget_seconds_used: summary_row.try_get("budget_used")?,
            budget_seconds_planned: i64::from(budget_minutes) * 60,
            postponed: summary_row.try_get("postponed")?,
        };
        let first_at: Option<DateTime<Utc>> = summary_row.try_get("first_at")?;
        let session_started_at = if live && active_session.is_some() {
            stream_started.or(first_at)
        } else {
            first_at
        }
        .map(|at| at.to_rfc3339());

        let rows = sqlx::query("SELECT decided_at,decision,reason,block_seconds,detail FROM (SELECT decided_at,decision,reason,block_seconds,detail,id FROM twitch_ad_manager_decisions WHERE twitch_user_id=$1 AND session_id IS NOT DISTINCT FROM $2 ORDER BY decided_at DESC,id DESC LIMIT 200) t ORDER BY decided_at ASC,id ASC")
            .bind(uid)
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?;
        let entries = rows
            .into_iter()
            .map(|row| {
                Ok::<_, sqlx::Error>(HistoryEntry {
                    at: row.try_get::<DateTime<Utc>, _>("decided_at")?.to_rfc3339(),
                    decision: row.try_get("decision")?,
                    reason: row.try_get("reason")?,
                    block_seconds: row.try_get("block_seconds")?,
                    detail: row.try_get("detail")?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(AdManagerHistory {
            session_started_at,
            summary,
            entries,
        })
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
            .bind(uid).bind(login).bind(live.is_live).bind(live.active_session_id).bind(live.stream_started_at).bind(next).bind(last).bind(duration).bind(preroll).bind(snoozes).bind(refresh).bind(schedule.is_some()).bind(decision.map(|d|d.action.as_str())).bind(decision.map(|d|d.reason)).execute(&self.pool).await?;
        Ok(())
    }
}
