use crate::{Engine, Error, Result};
use chrono::{DateTime, Duration, Utc};
use sqlx::{postgres::PgRow, Postgres, QueryBuilder};

#[derive(sqlx::FromRow)]
pub(super) struct Cursor {
    pub source_id: i64,
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
        sqlx::query("INSERT INTO partner_effort_source_cursors(source,lane,source_id,occurred_at) VALUES($1,$2,0,$3) ON CONFLICT DO NOTHING")
            .bind(source).bind(lane).bind(now-Duration::days(1)).execute(&self.pool).await?;
        Ok(sqlx::query_as("SELECT source_id,occurred_at,completed_once FROM partner_effort_source_cursors WHERE source=$1 AND lane=$2")
            .bind(source).bind(lane).fetch_one(&self.pool).await?)
    }

    pub(super) async fn advance_cursor(
        &self,
        source: &str,
        lane: &str,
        id: i64,
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
            sqlx::query("UPDATE partner_effort_source_cursors SET source_id=0,completed_once=TRUE WHERE source=$1 AND lane=$2")
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
        let (select,predicate,at,id,guild)=match source {
            "invites"=>(
                "SELECT q.join_event_id,q.guild_id,q.user_id,q.inviter_twitch_user_id,q.qualified_at,i.twitch_user_id FROM activity.twitch_invite_qualifications q JOIN bot.twitch_streamer_invites i ON i.streamer_login=q.streamer_login AND i.guild_id=q.guild_id",
                "q.status='qualified' AND q.qualified_at IS NOT NULL", "q.qualified_at","q.join_event_id","q.guild_id"
            ),
            "referrals"=>(
                "SELECT join_event_id,inviter_twitch_user_id,invited_twitch_user_id,credited_at FROM activity.streamer_referral_credits",
                "credited_at IS NOT NULL","credited_at","join_event_id","guild_id"
            ),
            _=>return Err(Error::Invalid("source_cursor")),
        };
        let mut query = QueryBuilder::<Postgres>::new(select);
        query
            .push(" WHERE ")
            .push(predicate)
            .push(" AND ")
            .push(guild)
            .push("=")
            .push_bind(self.cfg.community_guild_id)
            .push(" AND ")
            .push(at)
            .push(" <= ")
            .push_bind(now);
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
                    .push_bind(cursor.source_id)
                    .push(") ORDER BY ")
                    .push(at)
                    .push(",")
                    .push(id);
            }
            "reconcile" => {
                query
                    .push(" AND ")
                    .push(id)
                    .push(" > ")
                    .push_bind(cursor.source_id)
                    .push(" ORDER BY ")
                    .push(id);
            }
            _ => return Err(Error::Invalid("source_cursor_lane")),
        }
        query.push(" LIMIT ").push_bind(self.cfg.source_batch_size);
        Ok(query.build().fetch_all(self.central()?).await?)
    }
}
