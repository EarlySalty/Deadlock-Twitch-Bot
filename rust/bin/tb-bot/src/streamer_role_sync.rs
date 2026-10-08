use sqlx::PgPool;
use std::collections::{BTreeMap, BTreeSet};
use tb_transport_discord::{relay::TwitchLink, BrokerRelay};

static SYNC_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entitlement {
    Active,
    Inactive,
    Unknown,
}

#[derive(sqlx::FromRow)]
struct Channel {
    twitch_user_id: String,
    discord_user_id: Option<String>,
    status: Option<String>,
    has_bot_auth: bool,
    manual_partner_opt_out: Option<i32>,
    technical_pause_reason: Option<String>,
    departnered_at: Option<String>,
    admin_archived_at: Option<String>,
    denied: bool,
}

impl Channel {
    fn entitlement(&self) -> Entitlement {
        if self.denied
            || self.manual_partner_opt_out.is_some_and(|value| value != 0)
            || self.departnered_at.is_some()
            || self.admin_archived_at.is_some()
            || matches!(
                self.status.as_deref(),
                Some("departnered" | "archived" | "blocked")
            )
            || self.technical_pause_reason.as_deref() == Some("blocked")
        {
            return Entitlement::Inactive;
        }
        if self.status.as_deref() == Some("active")
            && self.has_bot_auth
            && self.manual_partner_opt_out == Some(0)
        {
            Entitlement::Active
        } else {
            Entitlement::Unknown
        }
    }
}

async fn channels(pool: &PgPool) -> Result<Vec<Channel>, sqlx::Error> {
    sqlx::query_as(
        "SELECT COALESCE(p.twitch_user_id, i.twitch_user_id) AS twitch_user_id,
                NULLIF(TRIM(i.discord_user_id), '') AS discord_user_id,
                LOWER(TRIM(p.status)) AS status, p.manual_partner_opt_out,
                LOWER(TRIM(p.technical_pause_reason)) AS technical_pause_reason,
                NULLIF(TRIM(p.departnered_at), '') AS departnered_at,
                NULLIF(TRIM(p.admin_archived_at), '') AS admin_archived_at,
                EXISTS (SELECT 1 FROM twitch_raid_auth a
                         WHERE a.twitch_user_id = COALESCE(p.twitch_user_id, i.twitch_user_id)) AS has_bot_auth,
                EXISTS (SELECT 1 FROM twitch_partner_signup_denylist d
                         WHERE d.twitch_user_id = COALESCE(p.twitch_user_id, i.twitch_user_id)) AS denied
           FROM twitch_partners p FULL OUTER JOIN twitch_streamer_identities i USING (twitch_user_id)"
    ).fetch_all(pool).await
}

fn entitlements(
    channels: &[Channel],
    links: &[TwitchLink],
) -> Result<BTreeMap<u64, Entitlement>, String> {
    let mut result = BTreeMap::new();
    let mut merge = |id: u64, state: Entitlement| {
        result
            .entry(id)
            .and_modify(|previous| {
                *previous = match (*previous, state) {
                    (Entitlement::Active, _) | (_, Entitlement::Active) => Entitlement::Active,
                    (Entitlement::Unknown, _) | (_, Entitlement::Unknown) => Entitlement::Unknown,
                    _ => Entitlement::Inactive,
                };
            })
            .or_insert(state);
    };
    for channel in channels {
        if let Some(id) = &channel.discord_user_id {
            let id = id
                .parse::<u64>()
                .ok()
                .filter(|id| *id > 0)
                .ok_or("Ungültige Discord-Verknüpfung")?;
            merge(id, channel.entitlement());
        }
    }
    for link in links {
        let id = link
            .discord_id
            .parse::<u64>()
            .ok()
            .filter(|id| *id > 0)
            .ok_or("Ungültige Plattform-Verknüpfung")?;
        let matching: Vec<_> = channels
            .iter()
            .filter(|channel| channel.twitch_user_id == link.twitch_user_id)
            .collect();
        if !link.verified || matching.is_empty() {
            merge(id, Entitlement::Unknown);
        } else {
            for channel in matching {
                merge(id, channel.entitlement());
            }
        }
    }
    Ok(result)
}

pub async fn reconcile(
    pool: &PgPool,
    relay: &BrokerRelay,
    guild_id: u64,
    role_id: u64,
    only_user: Option<u64>,
) -> Result<Option<bool>, String> {
    let _guard = SYNC_LOCK.lock().await;
    let holders = relay
        .live_role_members(guild_id, role_id)
        .await
        .map_err(|error| error.to_string())?;
    let links = relay
        .twitch_links()
        .await
        .map_err(|error| error.to_string())?;
    let desired = entitlements(
        &channels(pool).await.map_err(|error| error.to_string())?,
        &links,
    )?;
    let users: BTreeSet<_> = match only_user {
        Some(id) => BTreeSet::from([id]),
        None => holders
            .iter()
            .copied()
            .chain(desired.keys().copied())
            .collect(),
    };
    let mut outcome = None;
    let mut changed = 0;
    let mut unknown = 0;
    let mut failures = Vec::new();
    for user_id in users {
        let state = desired
            .get(&user_id)
            .copied()
            .unwrap_or(Entitlement::Inactive);
        let enabled = match state {
            Entitlement::Active => true,
            Entitlement::Inactive => false,
            Entitlement::Unknown => {
                unknown += 1;
                continue;
            }
        };
        if enabled != holders.contains(&user_id) {
            let current = entitlements(
                &channels(pool).await.map_err(|error| error.to_string())?,
                &links,
            )?;
            if current
                .get(&user_id)
                .copied()
                .unwrap_or(Entitlement::Inactive)
                != state
            {
                continue;
            }
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
                    changed += 1;
                    tracing::info!(user_id, enabled, "Streamer-Rolle abgeglichen");
                }
                Err(error) => {
                    tracing::warn!(%error, user_id, enabled, "Streamer-Rollenabgleich wird wiederholt");
                    failures.push(error.to_string());
                    continue;
                }
            }
        }
        if only_user == Some(user_id) {
            outcome = Some(enabled);
        }
    }
    tracing::info!(
        holders = holders.len(),
        linked = desired.len(),
        changed,
        unknown,
        failed = failures.len(),
        "Streamer-Rollenabgleich abgeschlossen"
    );
    if failures.is_empty() {
        Ok(outcome)
    } else {
        Err(failures.join("; "))
    }
}

pub async fn task(pool: PgPool, relay: BrokerRelay, config: tb_config::discord::OAuthFollowup) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(
        config.role_sync_interval_secs,
    ));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        if let Err(error) = reconcile(
            &pool,
            &relay,
            config.guild_id,
            config.streamer_role_id,
            None,
        )
        .await
        {
            tracing::warn!(%error, "Streamer-Rollenabgleich ausgesetzt, nächster Lauf versucht erneut");
        }
    }
}

#[cfg(test)]
#[path = "streamer_role_sync_tests.rs"]
mod tests;
