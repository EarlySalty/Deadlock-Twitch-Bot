use crate::{Engine, Error, Result};
use chrono::{DateTime, Duration, Utc};
use sqlx::{postgres::PgRow, PgPool, Postgres, QueryBuilder};

#[derive(sqlx::FromRow)]
pub(super) struct Cursor {
    pub source_id: String,
    pub occurred_at: DateTime<Utc>,
    pub completed_once: bool,
}

impl Engine {
    pub(super) async fn cursor(
        &self,
        source: &str,
        lane: &str,
        now: DateTime<Utc>,
    ) -> Result<Cursor> {
        sqlx::query("INSERT INTO partner_effort_source_cursors(source,lane,occurred_at) VALUES($1,$2,$3) ON CONFLICT DO NOTHING")
            .bind(source).bind(lane).bind(now-Duration::days(1)).execute(&self.pool).await?;
        Ok(sqlx::query_as("SELECT source_id,occurred_at,completed_once FROM partner_effort_source_cursors WHERE source=$1 AND lane=$2")
            .bind(source).bind(lane).fetch_one(&self.pool).await?)
    }

    pub(super) async fn advance_cursor(
        &self,
        source: &str,
        lane: &str,
        id: &str,
        at: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query("UPDATE partner_effort_source_cursors SET source_id=$3,occurred_at=$4 WHERE source=$1 AND lane=$2")
            .bind(source).bind(lane).bind(id).bind(at).execute(&self.pool).await?;
        Ok(())
    }

    pub(super) async fn finish_page(
        &self,
        source: &str,
        lane: &str,
        count: usize,
        completed_once: bool,
    ) -> Result<bool> {
        let exhausted = count < self.cfg.source_batch_size as usize;
        if lane == "reconcile" && exhausted {
            sqlx::query("UPDATE partner_effort_source_cursors SET source_id='',completed_once=TRUE WHERE source=$1 AND lane=$2")
                .bind(source).bind(lane).execute(&self.pool).await?;
        }
        Ok(exhausted || (lane == "reconcile" && completed_once))
    }

    pub(super) async fn source_page(
        &self,
        source: &str,
        lane: &str,
        cursor: &Cursor,
        now: DateTime<Utc>,
    ) -> Result<Vec<PgRow>> {
        let (select, predicate, at, id, guild, pool): (&str, &str, &str, &str, Option<&str>, &PgPool) = match source {
            "invites" => (
                "SELECT q.join_id::text AS cursor_id,q.guild_id,q.user_id,q.streamer_twitch_user_id,q.inviter_twitch_user_id,q.qualified_at FROM bot.twitch_invite_joins q",
                "q.status='qualified' AND q.qualified_at IS NOT NULL",
                "q.qualified_at", "q.join_id::numeric", Some("q.guild_id"), self.central()?,
            ),
            "referrals" => (
                "SELECT r.referred_twitch_user_id AS cursor_id,r.source_id,r.streamer_twitch_user_id,r.referred_twitch_user_id,r.credited_at FROM twitch_streamer_referral_credits r",
                "TRUE", "r.credited_at", "r.referred_twitch_user_id::numeric", None, &self.pool,
            ),
            _ => return Err(Error::Invalid("source_cursor")),
        };
        let mut query = QueryBuilder::<Postgres>::new(select);
        let after = if cursor.source_id.is_empty() {
            "0"
        } else {
            &cursor.source_id
        };
        query.push(" WHERE ").push(predicate);
        if let Some(guild) = guild {
            query
                .push(" AND ")
                .push(guild)
                .push("=")
                .push_bind(self.cfg.community_guild_id);
        }
        query.push(" AND ").push(at).push(" <= ").push_bind(now);
        match lane {
            "recent" => {
                query
                    .push(" AND (")
                    .push(at)
                    .push(",")
                    .push(id)
                    .push(") > (")
                    .push_bind(cursor.occurred_at)
                    .push(",")
                    .push_bind(after)
                    .push("::numeric) ORDER BY ")
                    .push(at)
                    .push(",")
                    .push(id);
            }
            "reconcile" => {
                query
                    .push(" AND ")
                    .push(id)
                    .push(" > ")
                    .push_bind(after)
                    .push("::numeric ORDER BY ")
                    .push(id);
            }
            _ => return Err(Error::Invalid("source_cursor_lane")),
        }
        query.push(" LIMIT ").push_bind(self.cfg.source_batch_size);
        Ok(query.build().fetch_all(pool).await?)
    }
}
