use crate::{
    calendar::{berlin_week_bounds, berlin_week_start, midnight},
    types::QuestResponse,
    Engine, Error, Event, EventKind, Partner, Result,
};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use serde_json::json;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuestKind {
    Party,
    CoStream,
    Invite,
    Clip,
    StreamExtra,
}

impl QuestKind {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Party => "community_match",
            Self::CoStream => "stream_together",
            Self::Invite => "active_discord_invite",
            Self::Clip => "submit_clip",
            Self::StreamExtra => "stream_above_average",
        }
    }
    fn from_key(key: &str) -> Result<Self> {
        match key {
            "community_match" => Ok(Self::Party),
            "stream_together" => Ok(Self::CoStream),
            "active_discord_invite" => Ok(Self::Invite),
            "submit_clip" => Ok(Self::Clip),
            "stream_above_average" => Ok(Self::StreamExtra),
            _ => Err(Error::Invalid("quest_key")),
        }
    }
    fn event(self) -> Option<&'static str> {
        match self {
            Self::Party => Some("party_play"),
            Self::CoStream => Some("co_stream"),
            Self::Invite => Some("qualified_invite"),
            Self::Clip => Some("clip_submitted"),
            Self::StreamExtra => None,
        }
    }
}

pub(crate) fn draw(id: &str, week: NaiveDate, pool: &[QuestKind]) -> Result<Vec<QuestKind>> {
    let mut ranked: Vec<_> = pool
        .iter()
        .copied()
        .map(|q| (Sha256::digest(format!("v1:{id}:{week}:{}", q.key())), q))
        .collect();
    ranked.sort_unstable_by_key(|(hash, _)| *hash);
    let selected: Vec<_> = ranked.into_iter().take(3).map(|(_, q)| q).collect();
    if selected.is_empty() {
        return Err(Error::Source("no_achievable_quests"));
    }
    Ok(selected)
}

#[derive(sqlx::FromRow)]
struct Assignment {
    quest_key: String,
    goal: i64,
    baseline_minutes: i64,
    reward_points: i32,
    bonus_points: i32,
    rules_hash: String,
}

impl Engine {
    async fn assign(&self, partner: &Partner, now: DateTime<Utc>) -> Result<()> {
        let (_, _, week) = berlin_week_bounds(now);
        let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM partner_effort_weekly_quests WHERE partner_twitch_user_id=$1 AND week_start=$2")
            .bind(&partner.twitch_user_id).bind(week).fetch_one(&self.pool).await?;
        if (1..=3).contains(&existing) {
            return Ok(());
        }
        if existing != 0 {
            return Err(Error::Invalid("quest_assignment_count"));
        }
        let mut available = vec![QuestKind::Invite, QuestKind::StreamExtra];
        if self.helix.is_some() && self.active_partners().await?.len() > 1 {
            available.push(QuestKind::CoStream);
        }
        if self.party_available(&partner.twitch_user_id).await? {
            available.push(QuestKind::Party);
        }
        if self.clip_available(partner, now).await? {
            available.push(QuestKind::Clip);
        }
        let selected = draw(&partner.twitch_user_id, week, &available)?;
        let baseline = self
            .stream_minutes(&partner.twitch_user_id, week - Duration::weeks(4), week)
            .await?
            / 4;
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,713220))")
            .bind(format!("{}:{week}", partner.twitch_user_id))
            .execute(&mut *tx)
            .await?;
        for (position, kind) in selected.into_iter().enumerate() {
            sqlx::query("INSERT INTO partner_effort_weekly_quests(partner_twitch_user_id,week_start,position,quest_key,goal,baseline_minutes,reward_points,bonus_points,rules_hash,assigned_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) ON CONFLICT DO NOTHING")
                .bind(&partner.twitch_user_id).bind(week).bind((position+1) as i16).bind(kind.key())
                .bind(if kind==QuestKind::StreamExtra {self.cfg.stream_extra_minutes} else {1})
                .bind(if kind==QuestKind::StreamExtra {baseline} else {0})
                .bind(self.cfg.points.quest_done).bind(self.cfg.points.quest_all_three_bonus).bind(&self.rules_hash).bind(now)
                .execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn clip_available(
        &self,
        partner: &Partner,
        now: DateTime<Utc>,
    ) -> Result<bool> {
        let local = now.with_timezone(&chrono_tz::Europe::Berlin);
        let table: Option<String> = sqlx::query_scalar(
            "SELECT to_regclass('public.twitch_clip_contest_submissions')::text",
        )
        .fetch_one(&self.pool)
        .await?;
        if table.is_none() {
            return Ok(false);
        }
        let (_, end, _) = berlin_week_bounds(now);
        let reaches_next_month = (end - Duration::microseconds(1))
            .with_timezone(&chrono_tz::Europe::Berlin)
            .month()
            != local.month();
        if local.day() > 21 && !reaches_next_month {
            return Ok(false);
        }
        if reaches_next_month {
            return Ok(true);
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clip_contest_submissions WHERE ((submitter_provider='twitch' AND submitter_user_id=$1) OR (submitter_provider='discord' AND submitter_user_id IN (SELECT discord_user_id FROM twitch_streamer_identities WHERE twitch_user_id=$1))) AND contest_month=date_trunc('month',$2::timestamptz AT TIME ZONE 'Europe/Berlin')::date")
            .bind(&partner.twitch_user_id).bind(now).fetch_one(&self.pool).await?;
        Ok(count < 3)
    }

    async fn assignments(&self, id: &str, week: NaiveDate) -> Result<Vec<Assignment>> {
        let rows: Vec<Assignment> = sqlx::query_as("SELECT quest_key,goal,baseline_minutes,reward_points,bonus_points,rules_hash FROM partner_effort_weekly_quests WHERE partner_twitch_user_id=$1 AND week_start=$2 ORDER BY position")
            .bind(id).bind(week).fetch_all(&self.pool).await?;
        if rows.is_empty() || rows.len() > 3 {
            return Err(Error::Source("quests_not_assigned"));
        }
        Ok(rows)
    }

    async fn quest_progress(
        &self,
        id: &str,
        week: NaiveDate,
        q: &Assignment,
    ) -> Result<(QuestResponse, Option<DateTime<Utc>>)> {
        let kind = QuestKind::from_key(&q.quest_key)?;
        let (progress, at) = if let Some(event) = kind.event() {
            let (count,at): (i64,Option<DateTime<Utc>>) = sqlx::query_as("SELECT COUNT(*),MIN(occurred_at) FROM partner_effort_events WHERE partner_twitch_user_id=$1 AND event_type=$2 AND occurred_at >= $3 AND occurred_at < $4")
                .bind(id).bind(event).bind(midnight(week)).bind(midnight(week+Duration::weeks(1))).fetch_one(&self.pool).await?;
            (count, at)
        } else {
            let minutes = self
                .stream_minutes(id, week, week + Duration::weeks(1))
                .await?;
            let at: Option<DateTime<Utc>> = sqlx::query_scalar("SELECT MAX(observed_through) FROM partner_effort_stream_weeks WHERE partner_twitch_user_id=$1 AND week_start=$2")
                .bind(id).bind(week).fetch_one(&self.pool).await?;
            (
                (minutes - q.baseline_minutes).max(0),
                at.map(|t| t.min(midnight(week + Duration::weeks(1)) - Duration::microseconds(1))),
            )
        };
        let text = match kind {
            QuestKind::Party => "Spiele ein Match mit jemandem aus der Community".into(),
            QuestKind::CoStream => {
                "Streame 30 Minuten per Stream Together mit einem Partner".into()
            }
            QuestKind::Invite => "Bringe eine neue aktive Person in den Discord".into(),
            QuestKind::Clip => "Reiche einen Clip ein".into(),
            QuestKind::StreamExtra => format!(
                "Streame {} Minuten länger als dein 4-Wochen-Mittel von {} Minuten",
                q.goal, q.baseline_minutes
            ),
        };
        Ok((
            QuestResponse {
                key: kind.key(),
                text,
                progress: progress.min(q.goal),
                goal: q.goal,
                completed: progress >= q.goal,
            },
            at,
        ))
    }

    pub(crate) async fn quests(&self, id: &str, week: NaiveDate) -> Result<Vec<QuestResponse>> {
        let mut result = Vec::new();
        for q in self.assignments(id, week).await? {
            result.push(self.quest_progress(id, week, &q).await?.0);
        }
        Ok(result)
    }

    async fn reward_quests(&self, id: &str, week: NaiveDate, now: DateTime<Utc>) -> Result<()> {
        let assignments = self.assignments(id, week).await?;
        let mut done = Vec::new();
        for q in &assignments {
            let (progress, at) = self.quest_progress(id, week, q).await?;
            if progress.completed {
                done.push((q, at.ok_or(Error::Invalid("quest_completion_time"))?));
            }
        }
        let mut tx = self.pool.begin().await?;
        for (q, at) in &done {
            let source = format!("quest:{id}:{week}:{}", q.quest_key);
            let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM partner_effort_events WHERE partner_twitch_user_id=$1 AND event_type='quest_done' AND source_id=$2)")
                .bind(id).bind(&source).fetch_one(&mut *tx).await?;
            if exists {
                continue;
            }
            let event = Event {
                partner_twitch_user_id: id.into(),
                kind: EventKind::QuestDone,
                source_id: source,
                occurred_at: *at,
                viewer_twitch_user_id: None,
                metadata: json!({"week":week,"quest":q.quest_key}),
            };
            self.append_tx(&mut tx, &event, q.reward_points, &q.rules_hash, now)
                .await?;
        }
        if done.len() == assignments.len() {
            let source = format!("quest:{id}:{week}:all_three");
            let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM partner_effort_events WHERE partner_twitch_user_id=$1 AND event_type='quest_done' AND source_id=$2)").bind(id).bind(&source).fetch_one(&mut *tx).await?;
            if !exists {
                let event = Event {
                    partner_twitch_user_id: id.into(),
                    kind: EventKind::QuestDone,
                    source_id: source,
                    occurred_at: done
                        .iter()
                        .map(|(_, t)| *t)
                        .max()
                        .ok_or(Error::Invalid("quest_bonus"))?,
                    viewer_twitch_user_id: None,
                    metadata: json!({"week":week,"bonus":"all_three"}),
                };
                self.append_tx(
                    &mut tx,
                    &event,
                    assignments[0].bonus_points,
                    &assignments[0].rules_hash,
                    now,
                )
                .await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn settle(&self, now: DateTime<Utc>) -> Result<()> {
        self.refresh_stream_evidence(now).await?;
        for partner in self.active_partners().await? {
            self.assign(&partner, now).await?;
            let weeks: Vec<NaiveDate> = sqlx::query_scalar("SELECT DISTINCT week_start FROM partner_effort_weekly_quests q WHERE partner_twitch_user_id=$1 AND week_start <= $2 AND NOT EXISTS(SELECT 1 FROM partner_effort_events e WHERE e.partner_twitch_user_id=q.partner_twitch_user_id AND e.event_type='quest_done' AND e.source_id='quest:' || q.partner_twitch_user_id || ':' || q.week_start::text || ':all_three') ORDER BY week_start")
                .bind(&partner.twitch_user_id).bind(berlin_week_start(now)).fetch_all(&self.pool).await?;
            for week in weeks {
                self.reward_quests(&partner.twitch_user_id, week, now)
                    .await?;
            }
            self.refresh_streak(&partner, now).await?;
            self.award_achievements(&partner, now).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deterministic_pool_and_impossible_pool() {
        let week = NaiveDate::from_ymd_opt(2026, 9, 21).unwrap();
        let pool = [
            QuestKind::Invite,
            QuestKind::StreamExtra,
            QuestKind::CoStream,
        ];
        assert_eq!(
            draw("1", week, &pool).unwrap(),
            draw("1", week, &pool).unwrap()
        );
        assert!(!draw("1", week, &pool).unwrap().contains(&QuestKind::Party));
        assert_eq!(draw("1", week, &pool[..2]).unwrap().len(), 2);
        assert!(draw("1", week, &[]).is_err());
    }
}
