mod cursor;
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
        let mut ready = true;
        for lane in ["recent", "reconcile"] {
            let cursor = self.cursor("invites", lane, now).await?;
            let rows = self.source_page("invites", lane, &cursor, now).await?;
            let count = rows.len();
            let ids: Vec<String> = rows
                .iter()
                .map(|row| row.try_get::<String, _>("cursor_id"))
                .collect::<std::result::Result<_, _>>()?;
            let seen = self.seen("invites", &ids).await?;
            for row in rows {
                let join: String = row.try_get("cursor_id")?;
                let at: DateTime<Utc> = row.try_get("qualified_at")?;
                if !seen.contains(&join) {
                    let id: Option<String> = row.try_get("streamer_twitch_user_id")?;
                    let Some(id) = id.filter(|id| crate::valid_id(id)) else {
                        let mut tx = self.pool.begin().await?;
                        self.receipt(&mut tx, "invites", &join).await?;
                        tx.commit().await?;
                        tracing::warn!(join_id = %join, "qualified invite has no valid streamer ID");
                        self.advance_cursor("invites", lane, &join, at).await?;
                        continue;
                    };
                    let viewer: Option<String> = row.try_get("inviter_twitch_user_id")?;
                    if viewer.as_deref().is_some_and(|v| !crate::valid_id(v)) {
                        tracing::warn!(join_id = %join, "qualified invite has an invalid inviter ID");
                    }
                    let event = Event {
                        partner_twitch_user_id: id.clone(),
                        kind: EventKind::QualifiedInvite,
                        source_id: format!("discord-join:{join}"),
                        occurred_at: at,
                        viewer_twitch_user_id: viewer.filter(|v| v != &id && crate::valid_id(v)),
                        metadata: json!({"join_id":join,"guild_id":row.try_get::<i64,_>("guild_id")?,"discord_user_id":row.try_get::<i64,_>("user_id")?}),
                    };
                    self.consume("invites", &join, event, now).await?;
                }
                self.advance_cursor("invites", lane, &join, at).await?;
            }
            ready &= self
                .finish_page("invites", lane, count, cursor.completed_once)
                .await?;
        }
        if !ready {
            return Err(Error::Source("invites_backfill_pending"));
        }
        Ok(())
    }

    async fn referrals(&self, now: DateTime<Utc>) -> Result<()> {
        let mut ready = true;
        for lane in ["recent", "reconcile"] {
            let cursor = self.cursor("referrals", lane, now).await?;
            let rows = self.source_page("referrals", lane, &cursor, now).await?;
            let count = rows.len();
            let ids: Vec<String> = rows
                .iter()
                .map(|row| row.try_get::<String, _>("cursor_id"))
                .collect::<std::result::Result<_, _>>()?;
            let seen = self.seen("referrals", &ids).await?;
            for row in rows {
                let referred: String = row.try_get("cursor_id")?;
                let at: DateTime<Utc> = row.try_get("credited_at")?;
                if !seen.contains(&referred) {
                    let id: String = row.try_get("streamer_twitch_user_id")?;
                    let source_id: String = row.try_get("source_id")?;
                    if !crate::valid_id(&id)
                        || !crate::valid_id(&referred)
                        || id == referred
                        || source_id != format!("streamer_referral:{referred}")
                    {
                        return Err(Error::Invalid("referral_identity"));
                    }
                    let event = Event {
                        partner_twitch_user_id: id,
                        kind: EventKind::StreamerReferral,
                        source_id,
                        occurred_at: at,
                        viewer_twitch_user_id: None,
                        metadata: json!({"referred_twitch_user_id":referred}),
                    };
                    self.consume("referrals", &referred, event, now).await?;
                }
                self.advance_cursor("referrals", lane, &referred, at)
                    .await?;
            }
            ready &= self
                .finish_page("referrals", lane, count, cursor.completed_once)
                .await?;
        }
        if !ready {
            return Err(Error::Source("referrals_backfill_pending"));
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
