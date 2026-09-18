//! Live-only selection evidence. No outreach rows are created or modified.
use super::*;

#[derive(Debug, sqlx::FromRow)]
pub struct LivePreflightTarget {
    pub twitch_user_id: String,
    pub channel_login: String,
}

impl SmalltalkLoopStore {
    /// Check at most one identity per tick. Once a session has run, only that
    /// still-open session may be refreshed; finishing never selects another.
    pub async fn live_preflight_target(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Option<LivePreflightTarget>, StoreError> {
        if !self.live_send {
            return Ok(None);
        }
        if self.live_started_once.load(Ordering::Acquire) && self.active_session().await?.is_none()
        {
            return Ok(None);
        }
        Ok(sqlx::query_as::<_, LivePreflightTarget>(
            "SELECT ls.twitch_user_id, LOWER(BTRIM(ls.streamer_login)) AS channel_login
             FROM twitch_live_state ls
             LEFT JOIN twitch_smalltalk_candidate_state c ON c.twitch_user_id = ls.twitch_user_id
             WHERE ls.twitch_user_id ~ '^[0-9]+$'
               AND LOWER(BTRIM(ls.streamer_login)) ~ '^[a-z0-9_]+$'
               AND COALESCE(ls.is_live, 0) = 1 AND LOWER(ls.last_game) = 'deadlock'
               AND (c.next_refresh_at IS NULL OR c.next_refresh_at <= $1
                    OR c.channel_login <> LOWER(BTRIM(ls.streamer_login)))
               AND (c.cooldown_until IS NULL OR c.cooldown_until <= $1)
               AND NOT EXISTS (SELECT 1 FROM twitch_partners p WHERE p.status = 'active'
                   AND (p.twitch_user_id = ls.twitch_user_id OR LOWER(p.twitch_login) = LOWER(ls.streamer_login)))
               AND NOT EXISTS (SELECT 1 FROM twitch_streamers_partner_state p WHERE COALESCE(p.is_partner_active, 0) <> 0
                   AND (p.twitch_user_id = ls.twitch_user_id OR LOWER(p.twitch_login) = LOWER(ls.streamer_login)))
               AND NOT EXISTS (SELECT 1 FROM twitch_raid_blacklist b
                   WHERE b.target_id = ls.twitch_user_id OR LOWER(b.target_login) = LOWER(ls.streamer_login))
               AND NOT EXISTS (SELECT 1 FROM twitch_partner_outreach o
                   WHERE (o.streamer_user_id = ls.twitch_user_id OR LOWER(o.streamer_login) = LOWER(ls.streamer_login))
                     AND NULLIF(BTRIM(o.cooldown_until), '')::timestamptz > $1)
               AND NOT EXISTS (SELECT 1 FROM twitch_smalltalk_sessions s
                   WHERE (s.streamer_user_id = ls.twitch_user_id OR LOWER(s.channel_login) = LOWER(ls.streamer_login))
                     AND s.ended_at > $1 - INTERVAL '24 hours')
               AND (NOT EXISTS (SELECT 1 FROM twitch_smalltalk_sessions WHERE ended_at IS NULL)
                    OR EXISTS (SELECT 1 FROM twitch_smalltalk_sessions s WHERE s.ended_at IS NULL
                        AND s.streamer_user_id = ls.twitch_user_id AND LOWER(s.channel_login) = LOWER(ls.streamer_login)))
             ORDER BY c.next_refresh_at NULLS FIRST, ls.last_viewer_count NULLS LAST, ls.twitch_user_id
             LIMIT 1",
        )
        .bind(now)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Only call with the result of the official Helix read. An unsuccessful
    /// read clears old evidence rather than inventing zero followers.
    pub async fn record_live_preflight(
        &self,
        target: &LivePreflightTarget,
        followers: Option<i32>,
        live_deadlock: bool,
        now: DateTime<Utc>,
        error: Option<&str>,
    ) -> Result<(), StoreError> {
        let followers = followers.filter(|count| *count >= 0);
        let retry =
            if live_deadlock && followers.is_some_and(|count| count < SMALLTALK_FOLLOWER_LIMIT) {
                Duration::seconds(30)
            } else {
                Duration::minutes(15)
            };
        sqlx::query(
            "INSERT INTO twitch_smalltalk_candidate_state
                (twitch_user_id, channel_login, follower_count, checked_at, source,
                 live_deadlock, live_checked_at, next_refresh_at, last_error)
             VALUES ($1, $2, $3, $4, 'helix', $5, $4, $6, $7)
             ON CONFLICT (twitch_user_id) DO UPDATE SET
                channel_login = EXCLUDED.channel_login, follower_count = EXCLUDED.follower_count,
                checked_at = EXCLUDED.checked_at, source = EXCLUDED.source,
                live_deadlock = EXCLUDED.live_deadlock, live_checked_at = EXCLUDED.live_checked_at,
                next_refresh_at = EXCLUDED.next_refresh_at, last_error = EXCLUDED.last_error",
        )
        .bind(&target.twitch_user_id)
        .bind(&target.channel_login)
        .bind(followers)
        .bind(now)
        .bind(live_deadlock)
        .bind(now + retry)
        .bind(error.map(|value| value.chars().take(128).collect::<String>()))
        .execute(&self.pool)
        .await?;
        tracing::info!(event = "smalltalk_loop.preflight", channel = %target.channel_login,
            twitch_user_id = %target.twitch_user_id, ?followers, live_deadlock,
            source = "helix", error = error.unwrap_or("none"));
        Ok(())
    }
}

/// One SQL snapshot at the final chat-send boundary, after token refresh.
/// Failed queries, missing evidence and malformed cooldowns deny the send.
pub(crate) async fn live_send_allowed(
    pool: &PgPool,
    user_id: &str,
    login: &str,
    now: DateTime<Utc>,
) -> Result<bool, sqlx::Error> {
    let sql = format!(
        "WITH candidates AS ({LIVE_CANDIDATES_SQL})
         SELECT EXISTS (
           SELECT 1 FROM candidates c
           JOIN twitch_smalltalk_sessions s ON s.streamer_user_id = c.streamer_user_id
             AND LOWER(s.channel_login) = c.channel_login
           JOIN twitch_engagement_settings e ON e.channel_login = c.channel_login
           WHERE c.channel_login = $3 AND s.ended_at IS NULL AND s.live_test
             AND s.started_at BETWEEN $1 - INTERVAL '60 minutes' AND $1
             AND e.enabled AND e.irc_read AND e.output_mode = 'smalltalk_live'
             AND c.is_live AND LOWER(c.game) = 'deadlock'
             AND NOT c.partner_table AND NOT c.partner_state AND NOT c.blacklisted
             AND c.follower_count BETWEEN 0 AND 49
             AND (c.cooldown_until IS NULL OR c.cooldown_until::timestamptz <= $1)
             AND (c.last_observed_at IS NULL OR c.last_observed_at <= $1 - INTERVAL '24 hours')
         )"
    );
    sqlx::query_scalar(&sql)
        .bind(now)
        .bind(Some(user_id))
        .bind(login)
        .fetch_one(pool)
        .await
}

pub(super) async fn set_live_cooldown(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    login: &str,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO twitch_smalltalk_candidate_state (twitch_user_id, channel_login, cooldown_until)
         VALUES ($1, $2, $3)
         ON CONFLICT (twitch_user_id) DO UPDATE SET cooldown_until = EXCLUDED.cooldown_until",
    )
    .bind(user_id).bind(login).bind(now + CHANNEL_COOLDOWN)
    .execute(&mut **tx).await?;
    Ok(())
}
