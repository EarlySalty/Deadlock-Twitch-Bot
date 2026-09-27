use crate::{
    calendar::berlin_week_bounds, valid_id, Engine, Error, Event, EventKind, Partner, Result,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{Postgres, Transaction};

pub(crate) const ACTIVE: &str = "status='active' AND departnered_at IS NULL AND admin_archived_at IS NULL AND COALESCE(manual_partner_opt_out,0)=0 AND COALESCE(trim(technical_pause_reason),'')=''";

impl Engine {
    pub(crate) async fn total_points(&self, id: &str) -> Result<i64> {
        Ok(sqlx::query_scalar!(
            r#"SELECT COALESCE(SUM(points),0)::bigint AS "total!" FROM partner_effort_events WHERE partner_twitch_user_id=$1"#,
            id
        ).fetch_one(&self.pool).await?)
    }

    pub async fn active_partners(&self) -> Result<Vec<Partner>> {
        Ok(sqlx::query_as(&format!("SELECT twitch_user_id,lower(twitch_login) AS login FROM twitch_partners WHERE {ACTIVE} ORDER BY twitch_user_id"))
            .fetch_all(&self.pool).await?)
    }

    pub async fn partner(&self, id: &str) -> Result<Partner> {
        if !valid_id(id) {
            return Err(Error::NotFound);
        }
        sqlx::query_as(&format!("SELECT twitch_user_id,lower(twitch_login) AS login FROM twitch_partners WHERE twitch_user_id=$1 AND {ACTIVE}"))
            .bind(id).fetch_optional(&self.pool).await?.ok_or(Error::NotFound)
    }

    pub async fn partner_by_login(&self, login: &str) -> Result<Partner> {
        sqlx::query_as(&format!("SELECT twitch_user_id,lower(twitch_login) AS login FROM twitch_partners WHERE lower(twitch_login)=lower($1) AND {ACTIVE}"))
            .bind(login).fetch_optional(&self.pool).await?.ok_or(Error::NotFound)
    }

    pub(crate) fn points(&self, event: &Event) -> Result<i32> {
        let p = &self.cfg.points;
        Ok(match event.kind {
            EventKind::QualifiedInvite => p.qualified_invite,
            EventKind::StreamerReferral => p.streamer_referral,
            EventKind::CoStream => p.co_stream,
            EventKind::PartyPlay => p.party_play,
            EventKind::ClipSubmitted => p.clip_submitted,
            EventKind::ClipTop3 => {
                let rank = event
                    .metadata
                    .get("rank")
                    .and_then(Value::as_u64)
                    .filter(|r| (1..=3).contains(r))
                    .ok_or(Error::Invalid("clip_rank"))?;
                p.clip_top3[(rank - 1) as usize]
            }
            EventKind::QuestDone => return Err(Error::Invalid("quest_reward_requires_assignment")),
        })
    }

    pub async fn append(&self, event: &Event, now: DateTime<Utc>) -> Result<bool> {
        let points = self.points(event)?;
        let mut tx = self.pool.begin().await?;
        let inserted = self
            .append_tx(&mut tx, event, points, &self.rules_hash, now)
            .await?;
        tx.commit().await?;
        Ok(inserted)
    }

    pub(crate) async fn append_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        event: &Event,
        points: i32,
        rules_hash: &str,
        now: DateTime<Utc>,
    ) -> Result<bool> {
        if !valid_id(&event.partner_twitch_user_id)
            || event.source_id.trim().is_empty()
            || event.source_id.len() > 512
            || !event.metadata.is_object()
            || event.occurred_at > now
            || points <= 0
            || event
                .viewer_twitch_user_id
                .as_deref()
                .is_some_and(|id| !valid_id(id))
            || (event.viewer_twitch_user_id.is_some() && event.kind != EventKind::QualifiedInvite)
        {
            return Err(Error::Invalid("event"));
        }
        let (_, _, week) = berlin_week_bounds(event.occurred_at);
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,713219))")
            .bind(format!("{}:{week}", event.partner_twitch_user_id))
            .execute(&mut **tx)
            .await?;
        let login: Option<String> = sqlx::query_scalar(&format!("SELECT lower(twitch_login) FROM twitch_partners WHERE twitch_user_id=$1 AND {ACTIVE} FOR SHARE"))
            .bind(&event.partner_twitch_user_id).fetch_optional(&mut **tx).await?;
        let Some(login) = login else {
            return Err(Error::NotFound);
        };
        let existing: Option<(DateTime<Utc>,Option<String>,Value)> = sqlx::query_as("SELECT occurred_at,viewer_twitch_user_id,metadata FROM partner_effort_events WHERE partner_twitch_user_id=$1 AND event_type=$2 AND source_id=$3")
            .bind(&event.partner_twitch_user_id).bind(event.kind.key()).bind(&event.source_id).fetch_optional(&mut **tx).await?;
        if let Some((occurred, viewer, metadata)) = existing {
            if occurred != event.occurred_at
                || viewer != event.viewer_twitch_user_id
                || metadata != event.metadata
            {
                return Err(Error::Invalid("conflicting_source_replay"));
            }
            return Ok(false);
        }
        let cap = match event.kind {
            EventKind::PartyPlay => Some(self.cfg.weekly_caps.party_play),
            EventKind::ClipSubmitted => Some(self.cfg.weekly_caps.clip_submitted),
            _ => None,
        };
        let mut awarded = points;
        if let Some(cap) = cap {
            let (start, end, _) = berlin_week_bounds(event.occurred_at);
            let used: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM partner_effort_events WHERE partner_twitch_user_id=$1 AND event_type=$2 AND occurred_at >= $3 AND occurred_at < $4 AND points > 0")
                .bind(&event.partner_twitch_user_id).bind(event.kind.key()).bind(start).bind(end).fetch_one(&mut **tx).await?;
            if used >= cap {
                awarded = 0;
            }
        }
        let inserted = sqlx::query("INSERT INTO partner_effort_events(partner_twitch_user_id,partner_login,event_type,source_id,points,occurred_at,viewer_twitch_user_id,metadata,rules_hash) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT DO NOTHING")
            .bind(&event.partner_twitch_user_id).bind(login).bind(event.kind.key()).bind(&event.source_id)
            .bind(awarded).bind(event.occurred_at).bind(&event.viewer_twitch_user_id).bind(&event.metadata).bind(rules_hash)
            .execute(&mut **tx).await?.rows_affected() == 1;
        if !inserted {
            return Err(Error::Invalid("source_attributed_to_another_partner"));
        }
        Ok(true)
    }

    pub(crate) async fn source_state(
        &self,
        source: &str,
        now: DateTime<Utc>,
        result: &Result<()>,
    ) -> Result<()> {
        sqlx::query("INSERT INTO partner_effort_source_state(source,checked_at,successful_at,healthy,error_code) VALUES($1,$2,CASE WHEN $3 THEN $2 END,$3,CASE WHEN $3 THEN NULL ELSE 'source_unavailable' END) ON CONFLICT(source) DO UPDATE SET checked_at=EXCLUDED.checked_at,successful_at=COALESCE(EXCLUDED.successful_at,partner_effort_source_state.successful_at),healthy=EXCLUDED.healthy,error_code=EXCLUDED.error_code")
            .bind(source).bind(now).bind(result.is_ok()).execute(&self.pool).await?;
        Ok(())
    }

    pub(crate) async fn receipt(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        source: &str,
        id: &str,
    ) -> Result<()> {
        sqlx::query("INSERT INTO partner_effort_source_receipts(source,source_id) VALUES($1,$2) ON CONFLICT DO NOTHING")
            .bind(source).bind(id).execute(&mut **tx).await?;
        Ok(())
    }
}
