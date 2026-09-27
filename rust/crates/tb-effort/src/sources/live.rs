use crate::{
    evidence::continuous_seconds, valid_id, Engine, Error, Event, EventKind, Partner, Result,
};
use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

#[derive(sqlx::FromRow)]
struct Live {
    twitch_user_id: String,
    stream_id: String,
    started_at: DateTime<Utc>,
    last_seen_at: Option<String>,
}

#[derive(Clone)]
struct Link {
    twitch: String,
    discord: i64,
    steam: String,
}

#[derive(sqlx::FromRow)]
struct Presence {
    party_id: String,
    steam_id: String,
    seen_at: DateTime<Utc>,
    deadlock_minutes: Option<i32>,
    deadlock_updated_at: Option<DateTime<Utc>>,
}

#[derive(Clone, sqlx::FromRow)]
struct Observation {
    partner_twitch_user_id: String,
    stream_id: String,
    party_id: String,
    steam_id: String,
    other_steam_id: String,
    observed_at: DateTime<Utc>,
    match_started_at: DateTime<Utc>,
    stream_started_at: DateTime<Utc>,
}

#[derive(Clone)]
struct CompletedMatch {
    id: String,
    started_at: DateTime<Utc>,
    confirmed_at: DateTime<Utc>,
}

fn completed_matches(
    result: &Value,
    confirmed_at: DateTime<Utc>,
    expected_steam: &str,
) -> Result<Vec<CompletedMatch>> {
    let data = result
        .get("data")
        .ok_or(Error::Invalid("match_history_envelope"))?;
    let owner = data
        .get("steam_id64")
        .and_then(|v| {
            v.as_u64()
                .map(|v| v.to_string())
                .or_else(|| v.as_str().map(str::to_owned))
        })
        .ok_or(Error::Invalid("match_history_owner"))?;
    if result.get("ok").and_then(Value::as_bool) != Some(true) || owner != expected_steam {
        return Err(Error::Invalid("match_history_owner"));
    }
    let matches = data
        .get("matches")
        .and_then(Value::as_array)
        .ok_or(Error::Invalid("match_history"))?;
    let mut output = Vec::new();
    for m in matches {
        let Some(id) = m.get("match_id").and_then(Value::as_u64).filter(|n| *n > 0) else {
            continue;
        };
        let Some(start) = m
            .get("start_time")
            .and_then(Value::as_i64)
            .and_then(|v| DateTime::from_timestamp(v, 0))
        else {
            continue;
        };
        if start >= confirmed_at
            || !matches!(m.get("match_result").and_then(Value::as_u64), Some(1 | 2))
        {
            continue;
        }
        output.push(CompletedMatch {
            id: id.to_string(),
            started_at: start,
            confirmed_at,
        });
    }
    Ok(output)
}

impl Engine {
    async fn live(&self, now: DateTime<Utc>) -> Result<Vec<Live>> {
        let ids: Vec<_> = self
            .active_partners()
            .await?
            .into_iter()
            .map(|p| p.twitch_user_id)
            .collect();
        let rows: Vec<Live>=sqlx::query_as("SELECT s.twitch_user_id,s.stream_id,s.started_at,l.last_seen_at FROM twitch_stream_sessions s JOIN twitch_live_state l ON l.twitch_user_id=s.twitch_user_id AND l.active_session_id=s.id WHERE s.twitch_user_id=ANY($1) AND s.ended_at IS NULL AND s.stream_id IS NOT NULL AND l.is_live=1 AND s.started_at <= $2 AND s.started_at >= $2-INTERVAL '48 hours'")
            .bind(ids).bind(now).fetch_all(&self.pool).await?;
        Ok(rows
            .into_iter()
            .filter(|r| {
                !r.stream_id.is_empty()
                    && r.last_seen_at
                        .as_deref()
                        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                        .is_some_and(|t| {
                            t.with_timezone(&Utc) <= now
                                && t.with_timezone(&Utc)
                                    >= now - Duration::seconds(self.cfg.evidence_max_gap_seconds)
                        })
            })
            .collect())
    }

    pub(super) async fn shared_chat(&self, now: DateTime<Utc>) -> Result<()> {
        let helix = self
            .helix
            .as_ref()
            .ok_or(Error::Source("helix_not_configured"))?;
        let live = self.live(now).await?;
        if live.is_empty() {
            return Ok(());
        }
        let previous_ok: bool=sqlx::query_scalar("SELECT COALESCE((SELECT healthy FROM partner_effort_source_state WHERE source='shared_chat'),FALSE)").fetch_one(&self.pool).await?;
        if !previous_ok {
            sqlx::query("DELETE FROM partner_effort_shared_chat_observations")
                .execute(&self.pool)
                .await?;
        }
        let active: HashSet<_> = live.iter().map(|p| p.twitch_user_id.clone()).collect();
        for partner in live {
            let response = helix.get_shared_chat_session(&partner.twitch_user_id).await;
            if response.is_err() {
                sqlx::query("DELETE FROM partner_effort_shared_chat_observations WHERE partner_twitch_user_id=$1").bind(&partner.twitch_user_id).execute(&self.pool).await?;
                return Err(Error::Source("shared_chat"));
            }
            let session = response.map_err(|_| Error::Source("shared_chat"))?;
            let Some(session) = session else {
                sqlx::query("DELETE FROM partner_effort_shared_chat_observations WHERE partner_twitch_user_id=$1").bind(&partner.twitch_user_id).execute(&self.pool).await?;
                continue;
            };
            let peers: Vec<_> = session
                .participants
                .iter()
                .filter(|p| {
                    p.broadcaster_id != partner.twitch_user_id && active.contains(&p.broadcaster_id)
                })
                .map(|p| p.broadcaster_id.clone())
                .collect();
            sqlx::query("DELETE FROM partner_effort_shared_chat_observations WHERE partner_twitch_user_id=$1 AND (stream_id<>$2 OR NOT(other_partner_twitch_user_id=ANY($3)))").bind(&partner.twitch_user_id).bind(&partner.stream_id).bind(&peers).execute(&self.pool).await?;
            for other in peers {
                let previous: Option<(String,DateTime<Utc>,DateTime<Utc>,i64)>=sqlx::query_as("SELECT shared_chat_session_id,first_seen_at,last_seen_at,confirmed_seconds FROM partner_effort_shared_chat_observations WHERE partner_twitch_user_id=$1 AND stream_id=$2 AND other_partner_twitch_user_id=$3")
                    .bind(&partner.twitch_user_id).bind(&partner.stream_id).bind(&other).fetch_optional(&self.pool).await?;
                let (seconds, first) = previous.map_or((0, now), |(s, first, last, seconds)| {
                    let elapsed = continuous_seconds(
                        last,
                        now,
                        seconds,
                        s == session.session_id,
                        self.cfg.evidence_max_gap_seconds,
                    );
                    (elapsed, if elapsed == 0 { now } else { first })
                });
                sqlx::query("INSERT INTO partner_effort_shared_chat_observations(partner_twitch_user_id,stream_id,other_partner_twitch_user_id,shared_chat_session_id,first_seen_at,last_seen_at,confirmed_seconds) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(partner_twitch_user_id,stream_id,other_partner_twitch_user_id) DO UPDATE SET shared_chat_session_id=EXCLUDED.shared_chat_session_id,first_seen_at=EXCLUDED.first_seen_at,last_seen_at=EXCLUDED.last_seen_at,confirmed_seconds=EXCLUDED.confirmed_seconds")
                    .bind(&partner.twitch_user_id).bind(&partner.stream_id).bind(&other).bind(&session.session_id).bind(first).bind(now).bind(seconds).execute(&self.pool).await?;
                let source = format!("stream:{}:{}", partner.twitch_user_id, partner.stream_id);
                if seconds >= 1800
                    && !self
                        .event_exists(&partner.twitch_user_id, "co_stream", &source)
                        .await?
                {
                    self.append(&Event{partner_twitch_user_id:partner.twitch_user_id.clone(),kind:EventKind::CoStream,source_id:source,occurred_at:now,viewer_twitch_user_id:None,metadata:json!({"stream_id":partner.stream_id,"shared_chat_session_id":session.session_id,"other_partner_twitch_user_id":other,"confirmed_seconds":seconds,"first_seen_at":first})},now).await?;
                }
            }
        }
        Ok(())
    }

    async fn event_exists(&self, id: &str, kind: &str, source: &str) -> Result<bool> {
        Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM partner_effort_events WHERE partner_twitch_user_id=$1 AND event_type=$2 AND source_id=$3)").bind(id).bind(kind).bind(source).fetch_one(&self.pool).await?)
    }

    async fn links(&self) -> Result<Vec<Link>> {
        let identities: Vec<(String,String,Option<bool>,Option<i64>)>=sqlx::query_as("SELECT i.twitch_user_id,i.discord_user_id,l.lookup_enabled,l.steam_id64 FROM twitch_streamer_identities i LEFT JOIN twitch_player_steam_links l ON l.twitch_user_id=i.twitch_user_id WHERE NULLIF(trim(i.discord_user_id),'') IS NOT NULL AND (i.is_on_discord=1 OR EXISTS(SELECT 1 FROM twitch_partners p WHERE p.twitch_user_id=i.twitch_user_id AND p.status='active' AND p.departnered_at IS NULL AND p.admin_archived_at IS NULL AND COALESCE(p.manual_partner_opt_out,0)=0 AND COALESCE(trim(p.technical_pause_reason),'')=''))").fetch_all(&self.pool).await?;
        let mut by_discord: HashMap<i64, Vec<(String, Option<i64>)>> = HashMap::new();
        for (twitch, discord, enabled, steam) in identities {
            if enabled == Some(false) || (enabled == Some(true) && steam.is_none()) {
                continue;
            }
            if !valid_id(&twitch) {
                return Err(Error::Invalid("twitch_link"));
            }
            let discord = discord
                .parse::<i64>()
                .map_err(|_| Error::Invalid("discord_link"))?;
            by_discord.entry(discord).or_default().push((twitch, steam));
        }
        if by_discord.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<_> = by_discord.keys().copied().collect();
        let rows: Vec<(i64,String)>=sqlx::query_as("SELECT discord_id,steam_id FROM core.steam_links WHERE discord_id=ANY($1) AND verified=TRUE AND steam_id<>''")
            .bind(ids).fetch_all(self.central()?).await?;
        let mut result = Vec::new();
        for (discord, steam) in rows {
            if let Some(twitches) = by_discord.get(&discord) {
                for (twitch, selected) in twitches {
                    if selected.is_none_or(|s| s.to_string() == steam) {
                        result.push(Link {
                            twitch: twitch.clone(),
                            discord,
                            steam: steam.clone(),
                        });
                    }
                }
            }
        }
        Ok(result)
    }

    pub(crate) async fn party_available(&self, id: &str) -> Result<bool> {
        let links = self.links().await?;
        Ok(links.iter().any(|own| {
            own.twitch == id
                && links.iter().any(|other| {
                    other.twitch != id && other.discord != own.discord && other.steam != own.steam
                })
        }))
    }

    pub(crate) async fn clip_available_for_goal(
        &self,
        partner: &Partner,
        now: DateTime<Utc>,
    ) -> Result<bool> {
        self.clip_available(partner, now).await
    }

    pub(super) async fn party_play(&self, now: DateTime<Utc>) -> Result<()> {
        sqlx::query("DELETE FROM partner_effort_party_observations WHERE observed_at < $1-INTERVAL '7 days'")
            .bind(now).execute(&self.pool).await?;

        let live = self.live(now).await?;
        let links = self.links().await?;
        if !live.is_empty() && links.len() > 1 {
            let ids: Vec<_> = links.iter().map(|l| l.steam.clone()).collect();
            let rows: Vec<Presence>=sqlx::query_as("SELECT pm.party_id,pm.steam_id,pm.seen_at,p.deadlock_minutes,p.deadlock_updated_at FROM voice.deadlock_party_members pm JOIN activity.live_player_state p ON p.steam_id=pm.steam_id WHERE pm.steam_id=ANY($1) AND pm.seen_at >= $2 AND pm.seen_at <= $3 AND p.deadlock_updated_at >= $2 AND p.deadlock_updated_at <= $3 AND p.in_deadlock_now=TRUE AND p.in_match_now_strict=TRUE")
                .bind(ids).bind(now-Duration::seconds(self.cfg.evidence_max_gap_seconds)).bind(now).fetch_all(self.central()?).await?;
            for partner in &live {
                for own in links.iter().filter(|l| l.twitch == partner.twitch_user_id) {
                    for p in rows
                        .iter()
                        .filter(|p| p.steam_id == own.steam && !p.party_id.is_empty())
                    {
                        let (Some(minutes), Some(updated)) =
                            (p.deadlock_minutes, p.deadlock_updated_at)
                        else {
                            continue;
                        };
                        if !(0..=180).contains(&minutes) {
                            continue;
                        }
                        let start = updated - Duration::minutes(i64::from(minutes));
                        for peer in rows.iter().filter(|s| {
                            s.party_id == p.party_id
                                && s.steam_id != own.steam
                                && (s.seen_at - p.seen_at).num_seconds().abs()
                                    <= self.cfg.evidence_max_gap_seconds
                        }) {
                            let Some(other) = links.iter().find(|l| {
                                l.steam == peer.steam_id
                                    && l.discord != own.discord
                                    && l.twitch != own.twitch
                            }) else {
                                continue;
                            };
                            let (Some(peer_minutes), Some(peer_updated)) =
                                (peer.deadlock_minutes, peer.deadlock_updated_at)
                            else {
                                continue;
                            };
                            let peer_start =
                                peer_updated - Duration::minutes(i64::from(peer_minutes));
                            if (peer_start - start).num_seconds().abs() > 90 {
                                continue;
                            }
                            sqlx::query("INSERT INTO partner_effort_party_observations(partner_twitch_user_id,stream_id,party_id,steam_id,other_steam_id,discord_id,other_discord_id,observed_at,match_started_at,stream_started_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) ON CONFLICT DO NOTHING")
                                .bind(&partner.twitch_user_id).bind(&partner.stream_id).bind(&p.party_id).bind(&own.steam).bind(&peer.steam_id).bind(own.discord).bind(other.discord).bind(now).bind(start).bind(partner.started_at).execute(&self.pool).await?;
                        }
                    }
                }
            }
        }
        self.settle_party(now).await
    }

    async fn settle_party(&self, now: DateTime<Utc>) -> Result<()> {
        let observations: Vec<Observation>=sqlx::query_as("SELECT DISTINCT ON (partner_twitch_user_id,stream_id,steam_id,other_steam_id,match_started_at) partner_twitch_user_id,stream_id,party_id,steam_id,other_steam_id,observed_at,match_started_at,stream_started_at FROM partner_effort_party_observations WHERE observed_at >= $1-INTERVAL '7 days' AND observed_at <= $1 ORDER BY partner_twitch_user_id,stream_id,steam_id,other_steam_id,match_started_at,observed_at")
            .bind(now).fetch_all(&self.pool).await?;
        if observations.is_empty() {
            return Ok(());
        }
        let ids: HashSet<_> = observations
            .iter()
            .flat_map(|o| [o.steam_id.clone(), o.other_steam_id.clone()])
            .collect();
        let mut histories = HashMap::new();
        for steam in ids {
            let rows: Vec<(Value,DateTime<Utc>)>=sqlx::query_as("SELECT result,finished_at FROM steam.steam_tasks WHERE type='GC_GET_MATCH_HISTORY' AND status='DONE' AND payload->>'steam_id'=$1 AND result IS NOT NULL AND finished_at IS NOT NULL AND finished_at >= $2-INTERVAL '7 days' AND finished_at <= $2 ORDER BY finished_at DESC LIMIT 8")
                .bind(&steam).bind(now).fetch_all(self.central()?).await?;
            let mut matches = Vec::new();
            for (value, at) in rows {
                matches.extend(completed_matches(&value, at, &steam)?);
            }
            histories.insert(steam, matches);
        }
        for o in observations {
            let (Some(own), Some(other)) =
                (histories.get(&o.steam_id), histories.get(&o.other_steam_id))
            else {
                continue;
            };
            for m in own {
                if m.confirmed_at < o.observed_at
                    || m.started_at > o.observed_at
                    || o.observed_at < o.stream_started_at
                    || (m.started_at - o.match_started_at).num_seconds().abs() > 90
                {
                    continue;
                }
                if !other.iter().any(|p| {
                    p.id == m.id && p.started_at == m.started_at && p.confirmed_at >= o.observed_at
                }) {
                    continue;
                }
                let source = format!("match:{}:{}", o.partner_twitch_user_id, m.id);
                if self
                    .event_exists(&o.partner_twitch_user_id, "party_play", &source)
                    .await?
                {
                    continue;
                }
                match self.append(&Event{partner_twitch_user_id:o.partner_twitch_user_id.clone(),kind:EventKind::PartyPlay,source_id:source,occurred_at:o.observed_at,viewer_twitch_user_id:None,metadata:json!({"match_id":m.id,"party_id":o.party_id,"stream_id":o.stream_id,"observed_at":o.observed_at,"match_started_at":m.started_at})},now).await {
                    Ok(_)|Err(Error::NotFound)=>{}, Err(error)=>return Err(error),
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_requires_owner_and_final_result() {
        let now = DateTime::from_timestamp(1000, 0).unwrap();
        let result = json!({"ok":true,"data":{"steam_id64":"100","matches":[{"match_id":1,"start_time":500,"match_result":1},{"match_id":2,"start_time":600,"match_result":null}]}});
        assert_eq!(completed_matches(&result, now, "100").unwrap().len(), 1);
        assert!(completed_matches(&result, now, "101").is_err());
    }
}
