use crate::{
    calendar::{berlin_month_bounds, berlin_week_bounds, berlin_week_start},
    evidence::union_seconds,
    store::ACTIVE,
    types::*,
    Engine, Error, Partner, Result,
};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

type StreakRows = Vec<(NaiveDate, bool, i32)>;
type StreakCalculation = (i32, i32, StreakRows, Option<NaiveDate>);

pub(crate) fn streak_weeks(
    weeks: &BTreeMap<NaiveDate, bool>,
    current_week: NaiveDate,
) -> StreakCalculation {
    let Some(first) = weeks.keys().next().copied() else {
        return (0, 0, Vec::new(), None);
    };
    let mut cursor = first;
    let mut current = 0;
    let mut longest = 0;
    let mut freezes = HashSet::new();
    let mut rows = Vec::new();
    let mut last = None;
    while cursor <= current_week {
        let qualified = weeks.get(&cursor).copied().unwrap_or(false);
        let mut frozen = false;
        if qualified {
            current += 1;
            longest = longest.max(current);
            last = Some(cursor);
        } else if cursor < current_week && current > 0 {
            if freezes.insert((cursor.year(), cursor.month())) {
                frozen = true;
            } else {
                current = 0;
            }
        }
        rows.push((cursor, frozen, current));
        cursor += Duration::weeks(1);
    }
    (current, longest, rows, last)
}

impl Engine {
    pub(crate) async fn refresh_streak(&self, partner: &Partner, now: DateTime<Utc>) -> Result<()> {
        let id = &partner.twitch_user_id;
        let qualified: Vec<(NaiveDate,bool,bool)> = sqlx::query_as("WITH weeks AS (SELECT week_start FROM partner_effort_stream_weeks WHERE partner_twitch_user_id=$1 UNION SELECT date_trunc('week',occurred_at AT TIME ZONE 'Europe/Berlin')::date FROM partner_effort_events WHERE partner_twitch_user_id=$1) SELECT w.week_start,EXISTS(SELECT 1 FROM partner_effort_stream_weeks s WHERE s.partner_twitch_user_id=$1 AND s.week_start=w.week_start AND deadlock_seconds>=1800) AS streamed,EXISTS(SELECT 1 FROM partner_effort_events e WHERE e.partner_twitch_user_id=$1 AND e.occurred_at>=w.week_start::timestamp AT TIME ZONE 'Europe/Berlin' AND e.occurred_at<(w.week_start+7)::timestamp AT TIME ZONE 'Europe/Berlin' AND NOT (e.event_type='quest_done' AND e.metadata->>'quest'='stream_above_average')) AS effort FROM weeks w WHERE w.week_start <= $2 ORDER BY w.week_start")
            .bind(id).bind(berlin_week_start(now)).fetch_all(&self.pool).await?;
        let evidence: BTreeMap<_, _> = qualified.iter().map(|(w, s, e)| (*w, (*s, *e))).collect();
        let weeks = qualified.into_iter().map(|(w, s, e)| (w, s && e)).collect();
        let (current, longest, rows, last) = streak_weeks(&weeks, berlin_week_start(now));
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,713221))")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE partner_effort_weeks SET frozen=FALSE WHERE partner_twitch_user_id=$1 AND frozen").bind(id).execute(&mut *tx).await?;
        for (week, frozen, streak) in rows {
            let (streamed, effort) = evidence.get(&week).copied().unwrap_or((false, false));
            sqlx::query("INSERT INTO partner_effort_weeks(partner_twitch_user_id,week_start,streamed,effort,frozen,streak,evaluated_at) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(partner_twitch_user_id,week_start) DO UPDATE SET streamed=EXCLUDED.streamed,effort=EXCLUDED.effort,frozen=EXCLUDED.frozen,streak=EXCLUDED.streak,evaluated_at=EXCLUDED.evaluated_at")
                .bind(id).bind(week).bind(streamed).bind(effort).bind(frozen).bind(streak).bind(now).execute(&mut *tx).await?;
        }
        sqlx::query("INSERT INTO partner_effort_streaks(partner_twitch_user_id,current_streak,longest_streak,last_qualified_week,updated_at) VALUES($1,$2,$3,$4,$5) ON CONFLICT(partner_twitch_user_id) DO UPDATE SET current_streak=EXCLUDED.current_streak,longest_streak=GREATEST(partner_effort_streaks.longest_streak,EXCLUDED.longest_streak),last_qualified_week=EXCLUDED.last_qualified_week,updated_at=EXCLUDED.updated_at")
            .bind(id).bind(current).bind(longest).bind(last).bind(now).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn streak(&self, id: &str, now: DateTime<Utc>) -> Result<StreakResponse> {
        let (current,longest): (i32,i32)=sqlx::query_as("SELECT current_streak,longest_streak FROM partner_effort_streaks WHERE partner_twitch_user_id=$1").bind(id).fetch_optional(&self.pool).await?.ok_or(Error::Source("streak_not_settled"))?;
        let (start, end, _) = berlin_month_bounds(now);
        let freeze_used_this_month: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM partner_effort_weeks WHERE partner_twitch_user_id=$1 AND frozen AND week_start >= ($2::timestamptz AT TIME ZONE 'Europe/Berlin')::date AND week_start < ($3::timestamptz AT TIME ZONE 'Europe/Berlin')::date)").bind(id).bind(start).bind(end).fetch_one(&self.pool).await?;
        let week_qualified: bool=sqlx::query_scalar("SELECT COALESCE((SELECT streamed AND effort FROM partner_effort_weeks WHERE partner_twitch_user_id=$1 AND week_start=$2),FALSE)").bind(id).bind(berlin_week_start(now)).fetch_one(&self.pool).await?;
        Ok(StreakResponse {
            current,
            longest,
            freeze_used_this_month,
            week_qualified,
        })
    }

    async fn counts(&self, id: &str) -> Result<HashMap<String, i64>> {
        let rows: Vec<(String,i64)>=sqlx::query_as("SELECT event_type,COUNT(*) FROM partner_effort_events WHERE partner_twitch_user_id=$1 GROUP BY event_type").bind(id).fetch_all(&self.pool).await?;
        Ok(rows.into_iter().collect())
    }

    fn achievement_definitions(&self) -> Vec<(&'static str, &'static str, &'static str, &[i64])> {
        let a = &self.cfg.achievements;
        vec![
            ("recruiter", "Recruiter", "qualified_invite", &a.recruiter),
            ("team_player", "Teamplayer", "party_play", &a.team_player),
            ("duo", "Duo", "co_stream", &a.duo),
            ("stamina", "Ausdauer", "stamina", &a.stamina),
            (
                "talent_scout",
                "Talentscout",
                "streamer_referral",
                &a.talent_scout,
            ),
            ("clip_hunter", "Clipjäger", "clip_top3", &a.clip_hunter),
        ]
    }

    pub(crate) async fn award_achievements(
        &self,
        partner: &Partner,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let mut counts = self.counts(&partner.twitch_user_id).await?;
        counts.insert(
            "stamina".into(),
            i64::from(self.streak(&partner.twitch_user_id, now).await?.longest),
        );
        for (key, _, event, targets) in self.achievement_definitions() {
            for target in targets {
                if counts.get(event).copied().unwrap_or(0) >= *target {
                    sqlx::query("INSERT INTO partner_effort_achievements(partner_twitch_user_id,achievement_key,target,earned_at) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING").bind(&partner.twitch_user_id).bind(key).bind(target).bind(now).execute(&self.pool).await?;
                }
            }
        }
        Ok(())
    }

    async fn achievements(
        &self,
        id: &str,
        streak: &StreakResponse,
    ) -> Result<Vec<AchievementResponse>> {
        let mut counts = self.counts(id).await?;
        counts.insert("stamina".into(), i64::from(streak.longest));
        let rows: Vec<(String,i64,DateTime<Utc>)>=sqlx::query_as("SELECT achievement_key,target,earned_at FROM partner_effort_achievements WHERE partner_twitch_user_id=$1").bind(id).fetch_all(&self.pool).await?;
        let earned: HashMap<_, _> = rows.into_iter().map(|(k, t, at)| ((k, t), at)).collect();
        Ok(self
            .achievement_definitions()
            .into_iter()
            .map(|(key, name, event, targets)| {
                let mut all: BTreeSet<i64> = targets.iter().copied().collect();
                all.extend(earned.keys().filter(|(k, _)| k == key).map(|(_, t)| *t));
                let tiers = all
                    .into_iter()
                    .map(|target| {
                        let earned_at = earned.get(&(key.into(), target)).copied();
                        AchievementTier {
                            target,
                            unlocked: earned_at.is_some(),
                            earned_at,
                        }
                    })
                    .collect();
                AchievementResponse {
                    key,
                    name,
                    progress: counts.get(event).copied().unwrap_or(0),
                    tiers,
                }
            })
            .collect())
    }

    async fn level(
        &self,
        id: &str,
        now: DateTime<Utc>,
        quests: &[QuestResponse],
    ) -> Result<(LevelResponse, NextGoalResponse)> {
        let total = self.total_points(id).await?;
        let index = self
            .cfg
            .level_thresholds
            .iter()
            .rposition(|t| *t <= total)
            .unwrap_or(0);
        let next = self.cfg.level_thresholds.get(index + 1).copied();
        let missing = next.map_or(0, |n| n - total);
        let p = &self.cfg.points;
        let (start, end, _) = berlin_week_bounds(now);
        let caps: Vec<(String,i64)>=sqlx::query_as("SELECT event_type,COUNT(*) FROM partner_effort_events WHERE partner_twitch_user_id=$1 AND occurred_at >= $2 AND occurred_at < $3 AND points>0 GROUP BY event_type").bind(id).bind(start).bind(end).fetch_all(&self.pool).await?;
        let caps: HashMap<_, _> = caps.into_iter().collect();
        let rewards: Vec<(String,i32,i32)>=sqlx::query_as("SELECT quest_key,reward_points,bonus_points FROM partner_effort_weekly_quests WHERE partner_twitch_user_id=$1 AND week_start=$2 ORDER BY position")
            .bind(id).bind(berlin_week_start(now)).fetch_all(&self.pool).await?;
        let reward = |key: &str| -> i64 {
            if quests.iter().any(|q| q.key == key && !q.completed) {
                rewards
                    .iter()
                    .find(|(k, _, _)| k == key)
                    .map_or(0, |(_, r, _)| i64::from(*r))
            } else {
                0
            }
        };
        let route = |points: i32, cap: i64, one: &str, many: &str, key: &str| crate::goal::Route {
            points: i64::from(points),
            cap,
            singular: one.into(),
            plural: many.into(),
            quest_reward: reward(key),
        };
        let partner = self.partner(id).await?;
        let mut routes = vec![route(
            p.streamer_referral,
            i64::MAX,
            "geworbener Partner",
            "geworbene Partner",
            "",
        )];
        if self
            .invite_available(&partner, berlin_week_start(now))
            .await?
        {
            routes.push(route(
                p.qualified_invite,
                1,
                "aktive Einladung",
                "aktive Einladungen",
                "active_discord_invite",
            ));
        }
        if (end - now).num_minutes() >= 30
            && self.helix.is_some()
            && self.active_partners().await?.len() > 1
        {
            routes.push(route(
                p.co_stream,
                i64::MAX,
                "Stream-Together-Session",
                "Stream-Together-Sessions",
                "stream_together",
            ));
        }
        if self.party_available(id).await? {
            routes.push(route(
                p.party_play,
                (self.cfg.weekly_caps.party_play - caps.get("party_play").copied().unwrap_or(0))
                    .max(0),
                "Community-Match",
                "Community-Matches",
                "community_match",
            ));
        }
        let partner = self.partner(id).await?;
        if self.clip_available_for_goal(&partner, now).await? {
            routes.push(route(
                p.clip_submitted,
                (self.cfg.weekly_caps.clip_submitted
                    - caps.get("clip_submitted").copied().unwrap_or(0))
                .max(0),
                "Clip-Einreichung",
                "Clip-Einreichungen",
                "submit_clip",
            ));
        }
        if let Some(q) = quests
            .iter()
            .find(|q| q.key == "stream_above_average" && !q.completed)
        {
            let text = format!("Stream-Verlängerung um {} Minuten", q.goal - q.progress);
            routes.push(route(0, 1, &text, &text, "stream_above_average"));
        }
        let fastest_route = crate::goal::fastest(
            &routes,
            missing,
            quests.iter().filter(|q| !q.completed).count(),
            rewards.first().map_or(0, |(_, _, b)| i64::from(*b)),
        )?;
        Ok((
            LevelResponse {
                level: index + 1,
                total_points: total,
                current_threshold: self.cfg.level_thresholds[index],
                next_threshold: next,
            },
            NextGoalResponse {
                missing_points: missing,
                fastest_route,
            },
        ))
    }

    async fn season(&self, id: &str, now: DateTime<Utc>) -> Result<SeasonResponse> {
        let (start, end, month) = berlin_month_bounds(now);
        let sql = format!(
            r#"WITH event_running AS (
            SELECT e.partner_twitch_user_id,e.event_type,e.credited_at,e.id,
                SUM(e.points) OVER (PARTITION BY e.partner_twitch_user_id ORDER BY e.credited_at,e.id ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)::bigint AS running_points,
                SUM(e.points) OVER (PARTITION BY e.partner_twitch_user_id)::bigint AS final_points
            FROM partner_effort_events e WHERE e.credited_at >= $1 AND e.credited_at < $2
        ), monthly AS (
            SELECT partner_twitch_user_id AS twitch_user_id,MAX(final_points)::bigint AS points,
                COUNT(*) FILTER(WHERE event_type='qualified_invite')::bigint AS invites,
                MIN(credited_at) FILTER(WHERE running_points=final_points) AS reached
            FROM event_running GROUP BY partner_twitch_user_id
        ), scores AS (
            SELECT p.twitch_user_id,COALESCE(m.points,0)::bigint AS points,
                COALESCE(m.invites,0)::bigint AS invites,
                CASE WHEN COALESCE(m.points,0)=0 THEN $1 ELSE m.reached END AS reached
            FROM (SELECT twitch_user_id FROM twitch_partners WHERE {ACTIVE}) p
            LEFT JOIN monthly m ON m.twitch_user_id=p.twitch_user_id
        ), ranked AS (
            SELECT twitch_user_id,points,ROW_NUMBER() OVER(ORDER BY points DESC,invites DESC,reached ASC NULLS LAST,twitch_user_id) AS rank,COUNT(*) OVER() AS active_partners FROM scores
        ) SELECT points,rank,active_partners FROM ranked WHERE twitch_user_id=$3"#
        );
        let (points, rank, active_partners): (i64, i64, i64) =
            sqlx::query_as(sqlx::AssertSqlSafe(sql))
                .bind(start)
                .bind(end)
                .bind(id)
                .fetch_one(&self.pool)
                .await?;
        Ok(SeasonResponse {
            next_reset_at: end,
            month,
            points,
            rank,
            active_partners,
        })
    }

    async fn with_us(&self, id: &str) -> Result<WithUsResponse> {
        let people: i64=sqlx::query_scalar("SELECT COUNT(*) FROM partner_effort_events WHERE partner_twitch_user_id=$1 AND event_type='qualified_invite'").bind(id).fetch_one(&self.pool).await?;
        let received_raids: i64=sqlx::query_scalar("SELECT COUNT(*) FROM twitch_raid_history WHERE to_broadcaster_id=$1 AND success IS TRUE").bind(id).fetch_one(&self.pool).await?;
        let discord: Option<String>=sqlx::query_scalar("SELECT NULLIF(trim(discord_user_id),'') FROM twitch_streamer_identities WHERE twitch_user_id=$1").bind(id).fetch_optional(&self.pool).await?.flatten();
        let seconds = if let Some(discord) = discord {
            let discord = discord
                .parse::<i64>()
                .map_err(|_| Error::Invalid("discord_link"))?;
            let spans: Vec<(DateTime<Utc>,DateTime<Utc>)>=sqlx::query_as("SELECT GREATEST(a.started_at,b.started_at),LEAST(a.ended_at,b.ended_at) FROM activity.voice_session_log a JOIN activity.voice_session_log b ON b.guild_id=a.guild_id AND b.channel_id=a.channel_id AND b.user_id<>a.user_id AND b.started_at<a.ended_at AND b.ended_at>a.started_at WHERE a.user_id=$1 AND a.guild_id=$2 AND a.ended_at>a.started_at AND b.ended_at>b.started_at")
                .bind(discord).bind(self.cfg.community_guild_id).fetch_all(self.central()?).await?;
            union_seconds(spans)
        } else {
            0
        };
        Ok(WithUsResponse {
            people_brought_in_who_stayed: people,
            community_hours: (seconds as f64 / 360.0).round() / 10.0,
            received_raids,
        })
    }

    pub async fn me(&self, id: &str, now: DateTime<Utc>) -> Result<MeResponse> {
        if !self.cfg.enabled {
            return Err(Error::Source("disabled"));
        }
        let partner = self.partner(id).await?;
        let (_, next_reset_at, week_start) = berlin_week_bounds(now);
        let quests = self.quests(id, week_start).await?;
        let streak = self.streak(id, now).await?;
        let achievements = self.achievements(id, &streak).await?;
        let (level, next_goal) = self.level(id, now, &quests).await?;
        let with_us = self.with_us(id).await?;
        let season = self.season(id, now).await?;
        Ok(MeResponse {
            twitch_user_id: id.into(),
            week_start,
            next_reset_at,
            generated_at: now,
            timezone: "Europe/Berlin",
            streamer: partner.login,
            quests,
            streak,
            level,
            next_goal,
            achievements,
            with_us,
            season,
        })
    }

    pub async fn viewers(&self, id: &str, now: DateTime<Utc>) -> Result<ViewersResponse> {
        let partner = self.partner(id).await?;
        let rows: Vec<(String,i64)>=sqlx::query_as("SELECT viewer_twitch_user_id,COUNT(*) FROM partner_effort_events WHERE partner_twitch_user_id=$1 AND event_type='qualified_invite' AND viewer_twitch_user_id IS NOT NULL AND viewer_twitch_user_id<>partner_twitch_user_id GROUP BY viewer_twitch_user_id ORDER BY COUNT(*) DESC,viewer_twitch_user_id LIMIT 50").bind(id).fetch_all(&self.pool).await?;
        let mut recruiters = Vec::new();
        if !rows.is_empty() {
            let helix = self
                .helix
                .as_ref()
                .ok_or(Error::Source("helix_not_configured"))?;
            let ids: Vec<_> = rows.iter().map(|(id, _)| id.clone()).collect();
            let profiles = helix
                .get_public_profiles(&ids)
                .await
                .map_err(|_| Error::Source("helix_profiles"))?;
            for (twitch_user_id, qualified_invites) in rows {
                let display_name = profiles
                    .iter()
                    .find(|p| p.id == twitch_user_id)
                    .map(|p| p.display_name.clone());
                recruiters.push(ViewerRecruiter {
                    twitch_user_id,
                    display_name,
                    qualified_invites,
                });
            }
        }
        Ok(ViewersResponse {
            generated_at: now,
            streamer: partner.login,
            recruiters,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn freeze_is_not_a_free_week_or_an_ongoing_week_penalty() {
        let a = NaiveDate::from_ymd_opt(2026, 9, 7).unwrap();
        let weeks = BTreeMap::from([(a, true), (a + Duration::weeks(3), true)]);
        let (current, longest, rows, _) = streak_weeks(&weeks, a + Duration::weeks(3));
        assert_eq!((current, longest), (1, 1));
        assert_eq!(rows.iter().filter(|(_, f, _)| *f).count(), 1);
        let weeks = BTreeMap::from([(a, true)]);
        assert_eq!(streak_weeks(&weeks, a + Duration::weeks(1)).0, 1);
        assert!(
            !streak_weeks(&weeks, a + Duration::weeks(1))
                .2
                .last()
                .unwrap()
                .1
        );
    }
}
