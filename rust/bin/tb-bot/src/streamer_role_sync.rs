use sqlx::PgPool;
use std::collections::{BTreeMap, BTreeSet};
use tb_transport_discord::{relay::TwitchLink, BrokerRelay};

static SYNC_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const LIVE_FRESH_SECS: i64 = 300;
const BOT_UNAVAILABLE_REASONS: [&str; 3] = ["bot_banned", "token_error", "blocked"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entitlement {
    Active,
    Inactive,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Activity {
    pub keep_window_days: u32,
    pub gain_window_days: u32,
    pub gain_min_days: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Targets {
    pub guild_id: u64,
    pub streamer_role_id: u64,
    pub live_role_id: Option<u64>,
    pub activity: Activity,
}

impl Targets {
    pub fn from_config(config: &tb_config::discord::OAuthFollowup) -> Self {
        Self {
            guild_id: config.guild_id,
            streamer_role_id: config.streamer_role_id,
            live_role_id: Some(config.live_streamer_role_id),
            activity: Activity {
                keep_window_days: config.role_keep_window_days,
                gain_window_days: config.role_gain_window_days,
                gain_min_days: config.role_gain_min_days,
            },
        }
    }
}

#[derive(sqlx::FromRow, Default)]
struct Channel {
    twitch_user_id: String,
    discord_user_id: Option<String>,
    status: Option<String>,
    has_bot_auth: bool,
    manual_partner_opt_out: Option<i32>,
    technical_pause_reason: Option<String>,
    departnered_at: Option<String>,
    admin_archived_at: Option<String>,
    bot_unmodded: bool,
    denied: bool,
    kept_active: bool,
    deadlock_days: i64,
}

impl Channel {
    fn availability(&self) -> Entitlement {
        if self.denied
            || self.manual_partner_opt_out.is_some_and(|value| value != 0)
            || self.departnered_at.is_some()
            || self.admin_archived_at.is_some()
            || matches!(
                self.status.as_deref(),
                Some("departnered" | "archived" | "blocked")
            )
            || self
                .technical_pause_reason
                .as_deref()
                .is_some_and(|reason| BOT_UNAVAILABLE_REASONS.contains(&reason))
            || self.bot_unmodded
        {
            return Entitlement::Inactive;
        }
        if self.status.as_deref() != Some("active") || self.manual_partner_opt_out != Some(0) {
            Entitlement::Unknown
        } else if self.has_bot_auth {
            Entitlement::Active
        } else {
            Entitlement::Inactive
        }
    }

    fn active_enough(&self, holder: bool, activity: &Activity) -> bool {
        if holder {
            self.kept_active
        } else {
            self.deadlock_days >= i64::from(activity.gain_min_days)
        }
    }

    fn entitlement(&self, holder: bool, activity: &Activity) -> Entitlement {
        match self.availability() {
            Entitlement::Active if !self.active_enough(holder, activity) => Entitlement::Inactive,
            state => state,
        }
    }
}

async fn channels(pool: &PgPool, activity: &Activity) -> Result<Vec<Channel>, sqlx::Error> {
    sqlx::query_as(
        "WITH activity AS (
             SELECT s.twitch_user_id,
                    BOOL_OR(s.started_at >= NOW() - make_interval(days => $1)) AS kept_active,
                    COUNT(DISTINCT (s.started_at AT TIME ZONE 'Europe/Berlin')::date)
                        FILTER (WHERE s.started_at >= NOW() - make_interval(days => $2)) AS deadlock_days
               FROM twitch_stream_sessions s
              WHERE s.had_deadlock_in_session
                AND s.started_at >= NOW() - make_interval(days => GREATEST($1, $2))
              GROUP BY s.twitch_user_id)
         SELECT COALESCE(p.twitch_user_id, i.twitch_user_id) AS twitch_user_id,
                NULLIF(TRIM(i.discord_user_id), '') AS discord_user_id,
                LOWER(TRIM(p.status)) AS status, p.manual_partner_opt_out,
                LOWER(TRIM(p.technical_pause_reason)) AS technical_pause_reason,
                NULLIF(TRIM(p.departnered_at), '') AS departnered_at,
                NULLIF(TRIM(p.admin_archived_at), '') AS admin_archived_at,
                p.deadlock_pause_unmodded_at IS NOT NULL AS bot_unmodded,
                EXISTS (SELECT 1 FROM twitch_raid_auth a
                         WHERE a.twitch_user_id = COALESCE(p.twitch_user_id, i.twitch_user_id)) AS has_bot_auth,
                EXISTS (SELECT 1 FROM twitch_partner_signup_denylist d
                         WHERE d.twitch_user_id = COALESCE(p.twitch_user_id, i.twitch_user_id)) AS denied,
                COALESCE(act.kept_active, false) AS kept_active,
                COALESCE(act.deadlock_days, 0) AS deadlock_days
           FROM twitch_partners p FULL OUTER JOIN twitch_streamer_identities i USING (twitch_user_id)
           LEFT JOIN activity act ON act.twitch_user_id = COALESCE(p.twitch_user_id, i.twitch_user_id)",
    )
    .bind(window_days(activity.keep_window_days))
    .bind(window_days(activity.gain_window_days))
    .fetch_all(pool)
    .await
}

fn window_days(days: u32) -> i32 {
    i32::try_from(days).unwrap_or(i32::MAX)
}

#[derive(sqlx::FromRow)]
struct LiveRow {
    twitch_user_id: String,
    last_seen_at: Option<String>,
}

async fn live_deadlock_channels(pool: &PgPool) -> Result<BTreeSet<String>, sqlx::Error> {
    let rows: Vec<LiveRow> = sqlx::query_as(
        "SELECT twitch_user_id, last_seen_at FROM twitch_live_state
          WHERE COALESCE(is_live, 0) <> 0
            AND LOWER(TRIM(COALESCE(last_game, ''))) = 'deadlock'",
    )
    .fetch_all(pool)
    .await?;
    let now = chrono::Utc::now();
    Ok(rows
        .into_iter()
        .filter(|row| fresh_live(row.last_seen_at.as_deref(), now))
        .map(|row| row.twitch_user_id)
        .collect())
}

fn fresh_live(last_seen_at: Option<&str>, now: chrono::DateTime<chrono::Utc>) -> bool {
    last_seen_at
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value.trim()).ok())
        .is_some_and(|stamp| {
            let age = now.signed_duration_since(stamp);
            age >= chrono::Duration::zero() && age <= chrono::Duration::seconds(LIVE_FRESH_SECS)
        })
}

fn discord_id(value: &str, error: &'static str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(|| error.to_string())
}

fn linked_channels<'a>(
    channels: &'a [Channel],
    links: &[TwitchLink],
) -> Result<BTreeMap<u64, Vec<Option<&'a Channel>>>, String> {
    let mut result: BTreeMap<u64, Vec<Option<&Channel>>> = BTreeMap::new();
    for channel in channels {
        if let Some(id) = &channel.discord_user_id {
            let id = discord_id(id, "Ungültige Discord-Verknüpfung")?;
            result.entry(id).or_default().push(Some(channel));
        }
    }
    for link in links {
        let id = discord_id(&link.discord_id, "Ungültige Plattform-Verknüpfung")?;
        let matching: Vec<_> = channels
            .iter()
            .filter(|channel| channel.twitch_user_id == link.twitch_user_id)
            .collect();
        let reliably_linked = link.verified
            || matching.iter().any(|channel| {
                channel
                    .discord_user_id
                    .as_deref()
                    .and_then(|value| value.parse::<u64>().ok())
                    == Some(id)
            })
            || links.iter().any(|other| {
                other.verified
                    && other.twitch_user_id == link.twitch_user_id
                    && other.discord_id.parse::<u64>().ok() == Some(id)
            });
        let entry = result.entry(id).or_default();
        if !reliably_linked || matching.is_empty() {
            entry.push(None);
        } else {
            entry.extend(matching.into_iter().map(Some));
        }
    }
    Ok(result)
}

fn merge(previous: Entitlement, state: Entitlement) -> Entitlement {
    match (previous, state) {
        (Entitlement::Active, _) | (_, Entitlement::Active) => Entitlement::Active,
        (Entitlement::Unknown, _) | (_, Entitlement::Unknown) => Entitlement::Unknown,
        _ => Entitlement::Inactive,
    }
}

fn entitlements(
    channels: &[Channel],
    links: &[TwitchLink],
    holders: &BTreeSet<u64>,
    activity: &Activity,
) -> Result<BTreeMap<u64, Entitlement>, String> {
    Ok(linked_channels(channels, links)?
        .into_iter()
        .filter_map(|(id, linked)| {
            let holder = holders.contains(&id);
            linked
                .into_iter()
                .map(|channel| {
                    channel.map_or(Entitlement::Unknown, |channel| {
                        channel.entitlement(holder, activity)
                    })
                })
                .reduce(merge)
                .map(|state| (id, state))
        })
        .collect())
}

fn live_desired(
    channels: &[Channel],
    links: &[TwitchLink],
    desired: &BTreeMap<u64, Entitlement>,
    live: &BTreeSet<String>,
) -> Result<BTreeSet<u64>, String> {
    Ok(linked_channels(channels, links)?
        .into_iter()
        .filter(|(id, linked)| {
            desired.get(id) == Some(&Entitlement::Active)
                && linked
                    .iter()
                    .flatten()
                    .any(|channel| live.contains(&channel.twitch_user_id))
        })
        .map(|(id, _)| id)
        .collect())
}

struct Counters {
    changed: usize,
    unknown: usize,
    nonmembers: usize,
    failures: Vec<String>,
}

async fn write_role(
    relay: &BrokerRelay,
    guild_id: u64,
    user_id: u64,
    role_id: u64,
    enabled: bool,
    counters: &mut Counters,
) -> bool {
    match relay
        .set_member_role_current(
            guild_id,
            user_id,
            role_id,
            enabled,
            "Streamer-Bot Rollenabgleich",
        )
        .await
    {
        Ok(()) => {
            counters.changed += 1;
            tracing::info!(user_id, role_id, enabled, "Streamer-Rolle abgeglichen");
            true
        }
        Err(error) => {
            tracing::warn!(%error, user_id, role_id, enabled, "Streamer-Rollenabgleich wird wiederholt");
            counters.failures.push(error.to_string());
            false
        }
    }
}

struct LiveTarget<'a> {
    guild_id: u64,
    role_id: u64,
    members: &'a BTreeSet<u64>,
    only_user: Option<u64>,
}

async fn sync_live_role(
    relay: &BrokerRelay,
    target: LiveTarget<'_>,
    desired: &BTreeSet<u64>,
    counters: &mut Counters,
) -> Result<(), String> {
    let holders = relay
        .live_role_members(target.guild_id, target.role_id)
        .await
        .map_err(|error| error.to_string())?;
    let users: BTreeSet<_> = match target.only_user {
        Some(id) => BTreeSet::from([id]),
        None => holders.union(desired).copied().collect(),
    };
    for user_id in users.into_iter().filter(|id| target.members.contains(id)) {
        let enabled = desired.contains(&user_id);
        if enabled != holders.contains(&user_id) {
            write_role(
                relay,
                target.guild_id,
                user_id,
                target.role_id,
                enabled,
                counters,
            )
            .await;
        }
    }
    Ok(())
}

pub async fn reconcile(
    pool: &PgPool,
    relay: &BrokerRelay,
    targets: &Targets,
    only_user: Option<u64>,
) -> Result<Option<bool>, String> {
    let _guard = SYNC_LOCK.lock().await;
    let guild_id = targets.guild_id;
    let role_id = targets.streamer_role_id;
    let activity = &targets.activity;
    let members = relay
        .live_role_members(guild_id, guild_id)
        .await
        .map_err(|error| error.to_string())?;
    let holders = relay
        .live_role_members(guild_id, role_id)
        .await
        .map_err(|error| error.to_string())?;
    let links = relay
        .twitch_links()
        .await
        .map_err(|error| error.to_string())?;
    let snapshot = channels(pool, activity)
        .await
        .map_err(|error| error.to_string())?;
    let desired = entitlements(&snapshot, &links, &holders, activity)?;
    let users: BTreeSet<_> = match only_user {
        Some(id) => BTreeSet::from([id]),
        None => holders
            .iter()
            .copied()
            .chain(desired.keys().copied())
            .collect(),
    };
    let mut outcome = None;
    let mut counters = Counters {
        changed: 0,
        unknown: 0,
        nonmembers: 0,
        failures: Vec::new(),
    };
    for user_id in users {
        if !members.contains(&user_id) {
            counters.nonmembers += 1;
            continue;
        }
        let state = desired
            .get(&user_id)
            .copied()
            .unwrap_or(Entitlement::Inactive);
        let enabled = match state {
            Entitlement::Active => true,
            Entitlement::Inactive => false,
            Entitlement::Unknown => {
                counters.unknown += 1;
                continue;
            }
        };
        if enabled != holders.contains(&user_id) {
            let current = entitlements(
                &channels(pool, activity)
                    .await
                    .map_err(|error| error.to_string())?,
                &links,
                &holders,
                activity,
            )?;
            if current
                .get(&user_id)
                .copied()
                .unwrap_or(Entitlement::Inactive)
                != state
            {
                continue;
            }
            if !write_role(relay, guild_id, user_id, role_id, enabled, &mut counters).await {
                continue;
            }
        }
        if only_user == Some(user_id) {
            outcome = Some(enabled);
        }
    }
    if let Some(live_role_id) = targets.live_role_id {
        let live = live_deadlock_channels(pool)
            .await
            .map_err(|error| error.to_string())?;
        let desired_live = live_desired(&snapshot, &links, &desired, &live)?;
        let target = LiveTarget {
            guild_id,
            role_id: live_role_id,
            members: &members,
            only_user,
        };
        sync_live_role(relay, target, &desired_live, &mut counters).await?;
    }
    tracing::info!(
        holders = holders.len(),
        linked = desired.len(),
        changed = counters.changed,
        unknown = counters.unknown,
        nonmembers = counters.nonmembers,
        failed = counters.failures.len(),
        "Streamer-Rollenabgleich abgeschlossen"
    );
    if counters.failures.is_empty() {
        Ok(outcome)
    } else {
        Err(counters.failures.join("; "))
    }
}

pub async fn task(pool: PgPool, relay: BrokerRelay, config: tb_config::discord::OAuthFollowup) {
    let targets = Targets::from_config(&config);
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(
        config.role_sync_interval_secs,
    ));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        if let Err(error) = reconcile(&pool, &relay, &targets, None).await {
            tracing::warn!(%error, "Streamer-Rollenabgleich ausgesetzt, nächster Lauf versucht erneut");
        }
    }
}

#[cfg(test)]
#[path = "streamer_role_sync_tests.rs"]
mod tests;
