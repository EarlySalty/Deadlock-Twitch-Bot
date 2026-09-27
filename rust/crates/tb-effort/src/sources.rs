mod live;

use crate::{Engine, Error, Event, EventKind, Result};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::Row;
use std::collections::HashSet;

impl Engine {
    pub(crate) async fn collect(&self, now: DateTime<Utc>) -> Result<()> {
        let mut failure = None;
        for source in [
            "invites",
            "referrals",
            "clips",
            "shared_chat",
            "steam_party",
        ] {
            let work = async {
                match source {
                    "invites" => self.invites(now).await,
                    "referrals" => self.referrals(now).await,
                    "clips" => self.clips(now).await,
                    "shared_chat" => self.shared_chat(now).await,
                    _ => self.party_play(now).await,
                }
            };
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(self.cfg.source_timeout_seconds),
                work,
            )
            .await
            .unwrap_or(Err(Error::Source("timeout")));
            self.source_state(source, now, &result).await?;
            if let Err(error) = result {
                failure = Some(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }

    async fn seen(&self, source: &str, ids: &[String]) -> Result<HashSet<String>> {
        let rows: Vec<String>=sqlx::query_scalar("SELECT source_id FROM partner_effort_source_receipts WHERE source=$1 AND source_id=ANY($2)")
            .bind(source).bind(ids).fetch_all(&self.pool).await?;
        Ok(rows.into_iter().collect())
    }

    async fn consume(
        &self,
        source: &str,
        source_id: &str,
        event: Event,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let points = self.points(&event)?;
        match self
            .append_tx(&mut tx, &event, points, &self.rules_hash, now)
            .await
        {
            Ok(_) => {}
            Err(Error::NotFound) => {}
            Err(error) => return Err(error),
        }
        self.receipt(&mut tx, source, source_id).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn invites(&self, now: DateTime<Utc>) -> Result<()> {
        let central = self.central()?;
        let mut cursor = 0i64;
        loop {
            let rows=sqlx::query("SELECT q.join_event_id,q.guild_id,q.user_id,q.inviter_twitch_user_id,q.qualified_at,i.twitch_user_id FROM activity.twitch_invite_qualifications q JOIN bot.twitch_streamer_invites i ON i.streamer_login=q.streamer_login AND i.guild_id=q.guild_id WHERE q.status='qualified' AND q.qualified_at IS NOT NULL AND q.qualified_at <= $1 AND q.join_event_id>$2 AND q.guild_id=$3 ORDER BY q.join_event_id LIMIT $4")
                .bind(now).bind(cursor).bind(self.cfg.community_guild_id).bind(self.cfg.source_batch_size).fetch_all(central).await?;
            if rows.is_empty() {
                break;
            }
            let ids: Vec<String> = rows
                .iter()
                .map(|r| {
                    r.try_get::<i64, _>("join_event_id")
                        .map(|id| id.to_string())
                })
                .collect::<std::result::Result<_, _>>()?;
            let seen = self.seen("invites", &ids).await?;
            for row in rows {
                let join: i64 = row.try_get("join_event_id")?;
                cursor = cursor.max(join);
                if seen.contains(&join.to_string()) {
                    continue;
                }
                let id: Option<String> = row.try_get("twitch_user_id")?;
                let id = id.ok_or(Error::Invalid("invite_streamer_identity"))?;
                let viewer: Option<String> = row.try_get("inviter_twitch_user_id")?;
                let event = Event {
                    partner_twitch_user_id: id.clone(),
                    kind: EventKind::QualifiedInvite,
                    source_id: format!("discord-join:{join}"),
                    occurred_at: row.try_get("qualified_at")?,
                    viewer_twitch_user_id: viewer.filter(|v| v != &id),
                    metadata: json!({"join_event_id":join,"guild_id":row.try_get::<i64,_>("guild_id")?,"discord_user_id":row.try_get::<i64,_>("user_id")?}),
                };
                self.consume("invites", &join.to_string(), event, now)
                    .await?;
            }
        }
        Ok(())
    }

    async fn referrals(&self, now: DateTime<Utc>) -> Result<()> {
        let central = self.central()?;
        let mut cursor = 0i64;
        loop {
            let rows=sqlx::query("SELECT join_event_id,inviter_twitch_user_id,invited_twitch_user_id,credited_at FROM activity.streamer_referral_credits WHERE join_event_id>$1 AND credited_at <= $2 AND guild_id=$3 ORDER BY join_event_id LIMIT $4")
                .bind(cursor).bind(now).bind(self.cfg.community_guild_id).bind(self.cfg.source_batch_size).fetch_all(central).await?;
            if rows.is_empty() {
                break;
            }
            let ids: Vec<String> = rows
                .iter()
                .map(|r| r.try_get::<i64, _>("join_event_id").map(|v| v.to_string()))
                .collect::<std::result::Result<_, _>>()?;
            let seen = self.seen("referrals", &ids).await?;
            for row in rows {
                let join: i64 = row.try_get("join_event_id")?;
                cursor = cursor.max(join);
                if seen.contains(&join.to_string()) {
                    continue;
                }
                let id: String = row.try_get("inviter_twitch_user_id")?;
                let referred: String = row.try_get("invited_twitch_user_id")?;
                if id == referred {
                    return Err(Error::Invalid("self_referral"));
                }
                let event = Event {
                    partner_twitch_user_id: id,
                    kind: EventKind::StreamerReferral,
                    source_id: format!("referred-partner:{referred}"),
                    occurred_at: row.try_get("credited_at")?,
                    viewer_twitch_user_id: None,
                    metadata: json!({"join_event_id":join,"referred_twitch_user_id":referred}),
                };
                self.consume("referrals", &join.to_string(), event, now)
                    .await?;
            }
        }
        Ok(())
    }

    async fn clips(&self, now: DateTime<Utc>) -> Result<()> {
        loop {
            let rows=sqlx::query("SELECT o.id,o.event_type,o.source_id,o.occurred_at,o.metadata,s.broadcaster_twitch_id FROM twitch_clip_contest_effort_outbox o JOIN twitch_clip_contest_submissions s ON s.id=(o.metadata->>'submission_id')::bigint WHERE o.occurred_at <= $1 AND NOT EXISTS(SELECT 1 FROM partner_effort_source_receipts r WHERE r.source='clips' AND r.source_id=o.id::text) ORDER BY o.occurred_at,o.id LIMIT $2")
                .bind(now).bind(self.cfg.source_batch_size).fetch_all(&self.pool).await?;
            if rows.is_empty() {
                break;
            }
            for row in rows {
                let outbox: i64 = row.try_get("id")?;
                let kind = match row.try_get::<String, _>("event_type")?.as_str() {
                    "clip_submitted" => EventKind::ClipSubmitted,
                    "clip_top3" => EventKind::ClipTop3,
                    _ => return Err(Error::Invalid("clip_event_type")),
                };
                let metadata: Value = row.try_get("metadata")?;
                let event = Event {
                    partner_twitch_user_id: row.try_get("broadcaster_twitch_id")?,
                    kind,
                    source_id: row.try_get("source_id")?,
                    occurred_at: row.try_get("occurred_at")?,
                    viewer_twitch_user_id: None,
                    metadata,
                };
                self.consume("clips", &outbox.to_string(), event, now)
                    .await?;
            }
        }
        Ok(())
    }
}
