use crate::{
    evidence::continuous_seconds, valid_id, Engine, Error, Event, EventKind, Partner, Result,
};
use chrono::{DateTime, Duration, Utc};
use futures_util::{stream, StreamExt};
use serde_json::{json, Value};
use sqlx::{Postgres, Transaction};
use std::collections::{HashMap, HashSet};

const SHARED_CHAT_LOCK_CLASS: i32 = 713_219;
const SHARED_CHAT_LOCK_OBJECT: i32 = 28;

async fn lock_shared_chat(tx: &mut Transaction<'_, Postgres>) -> Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock($1, $2)")
        .bind(SHARED_CHAT_LOCK_CLASS)
        .bind(SHARED_CHAT_LOCK_OBJECT)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct Live {
    twitch_user_id: String,
    stream_id: String,
    started_at: DateTime<Utc>,
    is_live: Option<i32>,
    last_seen_at: Option<String>,
}

fn current_live(row: Live, now: DateTime<Utc>, max_gap_seconds: i64) -> Result<Option<Live>> {
    match row.is_live {
        Some(0) => Ok(None),
        Some(1) => {
            let last_seen = row
                .last_seen_at
                .as_deref()
                .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
                .map(|value| value.with_timezone(&Utc))
                .ok_or(Error::Source("live_state_stale"))?;
            if last_seen < now - Duration::seconds(max_gap_seconds) {
                return Err(Error::Source("live_state_stale"));
            }
            if row.stream_id.is_empty() {
                return Err(Error::Source("live_stream_missing"));
            }
            Ok(Some(row))
        }
        Some(_) => Err(Error::Source("live_state_invalid")),
        None => Err(Error::Source("live_state_missing")),
    }
}

#[cfg(test)]
fn current_lives(
    rows: Vec<Live>,
    partner_ids: &HashSet<String>,
    now: DateTime<Utc>,
    max_gap_seconds: i64,
) -> Result<Vec<Live>> {
    let mut live = Vec::new();
    for row in rows {
        if !partner_ids.contains(&row.twitch_user_id) {
            continue;
        }
        if let Some(row) = current_live(row, now, max_gap_seconds)? {
            live.push(row);
        }
    }
    Ok(live)
}

#[derive(Clone)]
struct Link {
    twitch: Option<String>,
    discord: i64,
    steam: String,
}

#[derive(sqlx::FromRow)]
struct IdentityLink {
    twitch: String,
    discord: String,
    enabled: Option<bool>,
    steam: Option<i64>,
    on_discord: Option<i32>,
    active: bool,
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

fn json_id(value: &Value) -> Option<String> {
    value
        .as_u64()
        .map(|n| n.to_string())
        .or_else(|| value.as_str().map(str::to_owned))
}

fn account_id(steam: &str) -> Result<String> {
    let steam = steam
        .parse::<u64>()
        .map_err(|_| Error::Invalid("steam_id"))?;
    let account = steam
        .checked_sub(76_561_197_960_265_728)
        .and_then(|id| u32::try_from(id).ok())
        .filter(|id| *id > 0)
        .ok_or(Error::Invalid("steam_id"))?;
    Ok(account.to_string())
}

fn completed_matches(
    result: &Value,
    payload: &Value,
    confirmed_at: DateTime<Utc>,
    expected_steam: &str,
) -> Result<Vec<CompletedMatch>> {
    let expected_account = account_id(expected_steam)?;
    let data = result
        .get("data")
        .ok_or(Error::Invalid("match_history_envelope"))?;
    let claimed_steam = ["steam_id", "steam_id64"]
        .into_iter()
        .filter_map(|key| payload.get(key).and_then(json_id))
        .collect::<Vec<_>>();
    let claimed_account = payload.get("account_id").and_then(json_id);
    let result_steam = data.get("steam_id64").and_then(json_id);
    let result_account = data.get("account_id").and_then(json_id);
    if result.get("ok").and_then(Value::as_bool) != Some(true)
        || (claimed_steam.is_empty() && claimed_account.is_none())
        || (result_steam.is_none() && result_account.is_none())
        || claimed_steam.iter().any(|id| id != expected_steam)
        || claimed_account
            .as_ref()
            .is_some_and(|id| id != &expected_account)
        || result_steam.as_ref().is_some_and(|id| id != expected_steam)
        || result_account
            .as_ref()
            .is_some_and(|id| id != &expected_account)
    {
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
            || !matches!(m.get("match_result").and_then(Value::as_u64), Some(0 | 1))
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
        let partner_ids: HashSet<_> = ids.iter().cloned().collect();
        let rows: Vec<Live>=sqlx::query_as("SELECT s.twitch_user_id,COALESCE(s.stream_id,'') AS stream_id,s.started_at,l.is_live,l.last_seen_at FROM twitch_stream_sessions s LEFT JOIN twitch_live_state l ON l.twitch_user_id=s.twitch_user_id AND l.active_session_id=s.id WHERE s.twitch_user_id=ANY($1) AND s.ended_at IS NULL AND s.started_at <= $2")
            .bind(ids).bind(now).fetch_all(&self.pool).await?;
        let mut live = Vec::new();
        for row in rows {
            if !partner_ids.contains(&row.twitch_user_id) {
                continue;
            }
            match current_live(row, now, self.cfg.evidence_max_gap_seconds) {
                Ok(Some(row)) => live.push(row),
                Ok(None) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(live)
    }

    pub(super) async fn interrupt_shared_chat_observations(&self) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        lock_shared_chat(&mut tx).await?;
        sqlx::query("DELETE FROM partner_effort_shared_chat_observations")
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
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
        let active: HashSet<_> = live.iter().map(|p| p.twitch_user_id.clone()).collect();
        let active = &active;
        let mut results =
            stream::iter(live)
                .map(|partner| async move {
                    self.shared_chat_partner(helix, &partner, active, now).await
                })
                .buffer_unordered(4);
        let mut failure = None;
        while let Some(result) = results.next().await {
            if let Err(error) = result {
                failure = Some(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }

    async fn shared_chat_partner(
        &self,
        helix: &tb_transport_twitch::HelixClient,
        partner: &Live,
        active: &HashSet<String>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let session = helix
            .get_shared_chat_session(&partner.twitch_user_id)
            .await
            .map_err(|_| Error::Source("shared_chat"))?;
        let mut tx = self.pool.begin().await?;
        lock_shared_chat(&mut tx).await?;
        let Some(session) = session else {
            sqlx::query("DELETE FROM partner_effort_shared_chat_observations WHERE partner_twitch_user_id=$1")
                .bind(&partner.twitch_user_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            return Ok(());
        };
        let peers: Vec<_> = session
            .participants
            .iter()
            .filter(|p| {
                p.broadcaster_id != partner.twitch_user_id && active.contains(&p.broadcaster_id)
            })
            .map(|p| p.broadcaster_id.clone())
            .collect();
        sqlx::query("DELETE FROM partner_effort_shared_chat_observations WHERE partner_twitch_user_id=$1 AND (stream_id<>$2 OR NOT(other_partner_twitch_user_id=ANY($3)))")
            .bind(&partner.twitch_user_id)
            .bind(&partner.stream_id)
            .bind(&peers)
            .execute(&mut *tx)
            .await?;
        for other in peers {
            let previous: Option<(String, DateTime<Utc>, DateTime<Utc>, i64)> = sqlx::query_as("SELECT shared_chat_session_id,first_seen_at,last_seen_at,confirmed_seconds FROM partner_effort_shared_chat_observations WHERE partner_twitch_user_id=$1 AND stream_id=$2 AND other_partner_twitch_user_id=$3")
                .bind(&partner.twitch_user_id)
                .bind(&partner.stream_id)
                .bind(&other)
                .fetch_optional(&mut *tx)
                .await?;
            let (seconds, first) =
                previous.map_or((0, now), |(session_id, first, last, seconds)| {
                    let elapsed = continuous_seconds(
                        last,
                        now,
                        seconds,
                        session_id == session.session_id,
                        self.cfg.evidence_max_gap_seconds,
                    );
                    (elapsed, if elapsed == 0 { now } else { first })
                });
            sqlx::query("INSERT INTO partner_effort_shared_chat_observations(partner_twitch_user_id,stream_id,other_partner_twitch_user_id,shared_chat_session_id,first_seen_at,last_seen_at,confirmed_seconds) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(partner_twitch_user_id,stream_id,other_partner_twitch_user_id) DO UPDATE SET shared_chat_session_id=EXCLUDED.shared_chat_session_id,first_seen_at=EXCLUDED.first_seen_at,last_seen_at=EXCLUDED.last_seen_at,confirmed_seconds=EXCLUDED.confirmed_seconds")
                .bind(&partner.twitch_user_id)
                .bind(&partner.stream_id)
                .bind(&other)
                .bind(&session.session_id)
                .bind(first)
                .bind(now)
                .bind(seconds)
                .execute(&mut *tx)
                .await?;
            let source = format!("stream:{}:{}", partner.twitch_user_id, partner.stream_id);
            let co_stream_exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM partner_effort_events WHERE partner_twitch_user_id=$1 AND event_type='co_stream' AND source_id=$2)",
            )
            .bind(&partner.twitch_user_id)
            .bind(&source)
            .fetch_one(&mut *tx)
            .await?;
            if seconds >= 1800 && !co_stream_exists {
                let event = Event {
                    partner_twitch_user_id: partner.twitch_user_id.clone(),
                    kind: EventKind::CoStream,
                    source_id: source,
                    occurred_at: now,
                    viewer_twitch_user_id: None,
                    metadata: json!({
                        "stream_id": partner.stream_id,
                        "shared_chat_session_id": session.session_id,
                        "other_partner_twitch_user_id": other,
                        "confirmed_seconds": seconds,
                        "first_seen_at": first
                    }),
                };
                self.append_tx(&mut tx, &event, self.points(&event)?, &self.rules_hash, now)
                    .await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    async fn event_exists(&self, id: &str, kind: &str, source: &str) -> Result<bool> {
        Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM partner_effort_events WHERE partner_twitch_user_id=$1 AND event_type=$2 AND source_id=$3)").bind(id).bind(kind).bind(source).fetch_one(&self.pool).await?)
    }

    async fn links(&self) -> Result<Vec<Link>> {
        let identities: Vec<IdentityLink>=sqlx::query_as("SELECT i.twitch_user_id AS twitch,i.discord_user_id AS discord,l.lookup_enabled AS enabled,l.steam_id64 AS steam,i.is_on_discord AS on_discord,EXISTS(SELECT 1 FROM twitch_partners p WHERE p.twitch_user_id=i.twitch_user_id AND p.status='active' AND p.departnered_at IS NULL AND p.admin_archived_at IS NULL AND COALESCE(p.manual_partner_opt_out,0)=0 AND COALESCE(trim(p.technical_pause_reason),'')='') AS active FROM twitch_streamer_identities i LEFT JOIN twitch_player_steam_links l ON l.twitch_user_id=i.twitch_user_id WHERE NULLIF(trim(i.discord_user_id),'') IS NOT NULL").fetch_all(&self.pool).await?;
        let mut by_discord: HashMap<i64, Vec<(String, Option<i64>)>> = HashMap::new();
        let mut excluded = HashSet::new();
        for IdentityLink {
            twitch,
            discord,
            enabled,
            steam,
            on_discord,
            active,
        } in identities
        {
            if !valid_id(&twitch) {
                return Err(Error::Invalid("twitch_link"));
            }
            let discord = discord
                .parse::<i64>()
                .map_err(|_| Error::Invalid("discord_link"))?;
            if (on_discord != Some(1) && !active)
                || enabled == Some(false)
                || (enabled == Some(true) && steam.is_none())
            {
                excluded.insert(discord);
            } else {
                by_discord.entry(discord).or_default().push((twitch, steam));
            }
        }
        if by_discord.is_empty() {
            return Ok(Vec::new());
        }
        let rows: Vec<(i64, String)> = sqlx::query_as(
            "SELECT discord_id,steam_id FROM core.steam_links WHERE verified=TRUE AND steam_id<>''",
        )
        .fetch_all(self.central()?)
        .await?;
        let mut result = Vec::new();
        for (discord, steam) in rows {
            if excluded.contains(&discord) {
                continue;
            }
            if let Some(twitches) = by_discord.get(&discord) {
                for (twitch, selected) in twitches {
                    if selected.is_none_or(|s| s.to_string() == steam) {
                        result.push(Link {
                            twitch: Some(twitch.clone()),
                            discord,
                            steam: steam.clone(),
                        });
                    }
                }
            } else {
                result.push(Link {
                    twitch: None,
                    discord,
                    steam,
                });
            }
        }
        Ok(result)
    }

    pub(crate) async fn party_available(&self, id: &str) -> Result<bool> {
        let links = self.links().await?;
        Ok(links.iter().any(|own| {
            own.twitch.as_deref() == Some(id)
                && links.iter().any(|other| {
                    other.twitch != own.twitch
                        && other.discord != own.discord
                        && other.steam != own.steam
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
                for own in links
                    .iter()
                    .filter(|l| l.twitch.as_deref() == Some(partner.twitch_user_id.as_str()))
                {
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
            let rows: Vec<(Value,Value,DateTime<Utc>)>=sqlx::query_as("SELECT result,payload,finished_at FROM steam.steam_tasks WHERE type='GC_GET_MATCH_HISTORY' AND status='DONE' AND (payload->>'steam_id'=$1 OR payload->>'steam_id64'=$1 OR payload->>'account_id'=$3) AND result IS NOT NULL AND finished_at IS NOT NULL AND finished_at >= $2-INTERVAL '7 days' AND finished_at <= $2 ORDER BY finished_at DESC")
                .bind(&steam).bind(now).bind(account_id(&steam)?).fetch_all(self.central()?).await?;
            let mut matches = Vec::new();
            for (value, payload, at) in rows {
                matches.extend(completed_matches(&value, &payload, at, &steam)?);
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
    fn stale_live_partner_fails_even_when_unrelated_poll_is_fresh() {
        let now = DateTime::from_timestamp(10_000, 0).unwrap();
        let stale_partner = Live {
            twitch_user_id: "101".into(),
            stream_id: "stream-a".into(),
            started_at: now - Duration::minutes(10),
            is_live: Some(1),
            last_seen_at: Some((now - Duration::minutes(5)).to_rfc3339()),
        };
        let fresh_unrelated = Live {
            twitch_user_id: "999".into(),
            stream_id: "stream-other".into(),
            started_at: now,
            is_live: Some(1),
            last_seen_at: Some(now.to_rfc3339()),
        };

        let relevant_partners = HashSet::from(["101".to_string()]);

        assert!(current_lives(
            vec![stale_partner, fresh_unrelated],
            &relevant_partners,
            now,
            150
        )
        .is_err());
    }

    #[test]
    fn offline_partner_does_not_require_a_fresh_poll() {
        let now = DateTime::from_timestamp(10_000, 0).unwrap();
        let offline = Live {
            twitch_user_id: "101".into(),
            stream_id: "old-stream".into(),
            started_at: now - Duration::hours(1),
            is_live: Some(0),
            last_seen_at: Some((now - Duration::hours(1)).to_rfc3339()),
        };

        assert!(current_live(offline, now, 150).unwrap().is_none());
    }

    #[test]
    fn missing_poll_for_open_partner_session_fails_closed() {
        let now = DateTime::from_timestamp(10_000, 0).unwrap();
        let missing = Live {
            twitch_user_id: "101".into(),
            stream_id: "stream-a".into(),
            started_at: now - Duration::minutes(1),
            is_live: None,
            last_seen_at: None,
        };

        assert!(matches!(
            current_live(missing, now, 150),
            Err(Error::Source("live_state_missing"))
        ));
    }

    #[test]
    fn history_requires_owner_and_final_result() {
        let now = DateTime::from_timestamp(1000, 0).unwrap();
        let steam = "76561197960265828";
        let result = json!({"ok":true,"data":{"steam_id64":steam,"account_id":100,"matches":[{"match_id":1,"start_time":500,"match_result":1},{"match_id":2,"start_time":600,"match_result":null}]}});
        let direct = json!({"steam_id":steam});
        let ranked = json!({"account_id":100,"ranked_only":true});
        assert_eq!(
            completed_matches(&result, &direct, now, steam)
                .unwrap()
                .len(),
            1
        );
        let account_result = json!({"ok":true,"data":{"steam_id64":null,"account_id":100,"matches":[{"match_id":1,"start_time":500,"match_result":1}]}});
        assert_eq!(
            completed_matches(&account_result, &ranked, now, steam)
                .unwrap()
                .len(),
            1
        );
        assert!(
            completed_matches(&account_result, &json!({"account_id":101}), now, steam).is_err()
        );
        assert!(completed_matches(
            &result,
            &json!({"steam_id":steam,"account_id":101}),
            now,
            steam
        )
        .is_err());
        assert!(completed_matches(
            &json!({"ok":true,"data":{"account_id":101,"matches":[]}}),
            &ranked,
            now,
            steam
        )
        .is_err());
        assert!(completed_matches(&result, &direct, now, "76561197960265829").is_err());
    }
}
