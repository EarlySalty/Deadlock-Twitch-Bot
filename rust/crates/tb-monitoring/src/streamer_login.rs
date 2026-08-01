use sqlx::{PgPool, Postgres, Transaction};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenameCounts {
    pub twitch_streamers: u64,
    pub twitch_live_state: u64,
    pub twitch_partners: u64,
    pub twitch_raid_auth: u64,
    pub twitch_partner_raid_scores: u64,
    pub twitch_streamer_identities: u64,
    pub twitch_engagement_settings: u64,
    pub twitch_engagement_channel_profile: u64,
    pub twitch_stream_sessions: u64,
    pub twitch_streamer_invites: u64,
    pub twitch_raw_chat_ingest_health: u64,
}

impl RenameCounts {
    pub fn total(&self) -> u64 {
        self.twitch_streamers
            + self.twitch_live_state
            + self.twitch_partners
            + self.twitch_raid_auth
            + self.twitch_partner_raid_scores
            + self.twitch_streamer_identities
            + self.twitch_engagement_settings
            + self.twitch_engagement_channel_profile
            + self.twitch_stream_sessions
            + self.twitch_streamer_invites
            + self.twitch_raw_chat_ingest_health
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameReport {
    pub twitch_user_id: String,
    pub old_login: String,
    pub new_login: String,
    pub counts: RenameCounts,
}

#[derive(Clone)]
pub struct StreamerLoginStore {
    pool: PgPool,
}

impl StreamerLoginStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn reconcile(
        &self,
        twitch_user_id: &str,
        new_login: &str,
    ) -> Result<Option<RenameReport>, sqlx::Error> {
        let user_id = twitch_user_id.trim();
        let new_login = new_login.trim().to_lowercase();
        if user_id.is_empty() || new_login.is_empty() {
            tracing::debug!(
                twitch_user_id = %user_id,
                new_login = %new_login,
                "Twitch-Rename-Prüfung übersprungen: user_id oder Login fehlt"
            );
            return Ok(None);
        }

        let old_login = sqlx::query_scalar::<_, String>(
            r#"
            SELECT twitch_login
              FROM (
                    SELECT twitch_login, 1 AS priority
                      FROM twitch_streamers WHERE twitch_user_id = $1
                    UNION ALL
                    SELECT twitch_login, 2 AS priority
                      FROM twitch_partners WHERE twitch_user_id = $1
                    UNION ALL
                    SELECT twitch_login, 3 AS priority
                      FROM twitch_streamer_identities WHERE twitch_user_id = $1
                    UNION ALL
                    SELECT twitch_login, 4 AS priority
                      FROM twitch_raid_auth WHERE twitch_user_id = $1
                    UNION ALL
                    SELECT twitch_login, 5 AS priority
                      FROM twitch_partner_raid_scores WHERE twitch_user_id = $1
                    UNION ALL
                    SELECT streamer_login AS twitch_login, 6 AS priority
                      FROM twitch_live_state WHERE twitch_user_id = $1
              ) known
             WHERE COALESCE(TRIM(twitch_login), '') <> ''
             ORDER BY priority
             LIMIT 1
            "#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        let Some(old_login) = old_login else {
            tracing::debug!(
                twitch_user_id = %user_id,
                new_login = %new_login,
                "Twitch-Rename-Prüfung: keine bekannte Identität, kein Rename nötig"
            );
            return Ok(None);
        };
        if old_login.trim().eq_ignore_ascii_case(&new_login) {
            tracing::debug!(
                twitch_user_id = %user_id,
                old_login = %old_login,
                new_login = %new_login,
                "Twitch-Rename-Prüfung: Login unverändert"
            );
            return Ok(None);
        }

        rename_streamer_login(&self.pool, user_id, &old_login, &new_login)
            .await
            .map(Some)
    }
}

pub async fn rename_streamer_login(
    pool: &PgPool,
    twitch_user_id: &str,
    old_login: &str,
    new_login: &str,
) -> Result<RenameReport, sqlx::Error> {
    let user_id = twitch_user_id.trim();
    let old_login = old_login.trim().to_lowercase();
    let new_login = new_login.trim().to_lowercase();
    if user_id.is_empty() || old_login.is_empty() || new_login.is_empty() {
        return Err(sqlx::Error::Protocol(
            "twitch rename requires user_id, old_login and new_login".into(),
        ));
    }

    let mut tx = pool.begin().await?;
    let counts = RenameCounts {
        twitch_streamers: rewrite_user_scoped(
            &mut tx,
            "twitch_streamers",
            "twitch_login",
            user_id,
            &old_login,
            &new_login,
            true,
        )
        .await?,
        twitch_live_state: rewrite_user_scoped(
            &mut tx,
            "twitch_live_state",
            "streamer_login",
            user_id,
            &old_login,
            &new_login,
            false,
        )
        .await?,
        twitch_partners: rewrite_partners(&mut tx, user_id, &old_login, &new_login).await?,
        twitch_raid_auth: rewrite_user_scoped(
            &mut tx,
            "twitch_raid_auth",
            "twitch_login",
            user_id,
            &old_login,
            &new_login,
            true,
        )
        .await?,
        twitch_partner_raid_scores: rewrite_user_scoped(
            &mut tx,
            "twitch_partner_raid_scores",
            "twitch_login",
            user_id,
            &old_login,
            &new_login,
            false,
        )
        .await?,
        twitch_streamer_identities: rewrite_user_scoped(
            &mut tx,
            "twitch_streamer_identities",
            "twitch_login",
            user_id,
            &old_login,
            &new_login,
            true,
        )
        .await?,
        twitch_engagement_settings: rewrite_login_keyed(
            &mut tx,
            "twitch_engagement_settings",
            "channel_login",
            &old_login,
            &new_login,
        )
        .await?,
        twitch_engagement_channel_profile: rewrite_login_keyed(
            &mut tx,
            "twitch_engagement_channel_profile",
            "channel_login",
            &old_login,
            &new_login,
        )
        .await?,
        twitch_stream_sessions: sqlx::query(
            "UPDATE twitch_stream_sessions SET streamer_login = $2
             WHERE LOWER(streamer_login) = LOWER($1) AND ended_at IS NULL",
        )
        .bind(&old_login)
        .bind(&new_login)
        .execute(&mut *tx)
        .await?
        .rows_affected(),
        twitch_streamer_invites: rewrite_login_keyed(
            &mut tx,
            "twitch_streamer_invites",
            "streamer_login",
            &old_login,
            &new_login,
        )
        .await?,
        twitch_raw_chat_ingest_health: rewrite_login_keyed(
            &mut tx,
            "twitch_raw_chat_ingest_health",
            "streamer_login",
            &old_login,
            &new_login,
        )
        .await?,
    };
    tx.commit().await?;

    tracing::info!(
        twitch_user_id = %user_id,
        old_login = %old_login,
        new_login = %new_login,
        twitch_streamers = counts.twitch_streamers,
        twitch_live_state = counts.twitch_live_state,
        twitch_partners = counts.twitch_partners,
        twitch_raid_auth = counts.twitch_raid_auth,
        twitch_partner_raid_scores = counts.twitch_partner_raid_scores,
        twitch_streamer_identities = counts.twitch_streamer_identities,
        twitch_engagement_settings = counts.twitch_engagement_settings,
        twitch_engagement_channel_profile = counts.twitch_engagement_channel_profile,
        twitch_stream_sessions = counts.twitch_stream_sessions,
        twitch_streamer_invites = counts.twitch_streamer_invites,
        twitch_raw_chat_ingest_health = counts.twitch_raw_chat_ingest_health,
        total = counts.total(),
        "Twitch-Login umbenannt"
    );
    Ok(RenameReport {
        twitch_user_id: user_id.to_string(),
        old_login,
        new_login,
        counts,
    })
}

async fn rewrite_user_scoped(
    tx: &mut Transaction<'_, Postgres>,
    table: &str,
    login_column: &str,
    user_id: &str,
    old_login: &str,
    new_login: &str,
    login_is_unique: bool,
) -> Result<u64, sqlx::Error> {
    let mut affected = 0;
    if login_is_unique {
        let delete = format!(
            "DELETE FROM {table} target
             WHERE target.twitch_user_id = $1
               AND LOWER(target.{login_column}) = LOWER($2)
               AND EXISTS (
                   SELECT 1 FROM {table} current
                    WHERE LOWER(current.{login_column}) = LOWER($3)
                      AND current.twitch_user_id IS DISTINCT FROM $1
               )"
        );
        affected += sqlx::query(&delete)
            .bind(user_id)
            .bind(old_login)
            .bind(new_login)
            .execute(&mut **tx)
            .await?
            .rows_affected();
    }
    let update = format!(
        "UPDATE {table} SET {login_column} = $3
         WHERE twitch_user_id = $1 AND LOWER({login_column}) = LOWER($2)"
    );
    affected += sqlx::query(&update)
        .bind(user_id)
        .bind(old_login)
        .bind(new_login)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    Ok(affected)
}

async fn rewrite_partners(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    old_login: &str,
    new_login: &str,
) -> Result<u64, sqlx::Error> {
    let deleted = sqlx::query(
        "DELETE FROM twitch_partners target
         WHERE target.twitch_user_id = $1
           AND LOWER(target.twitch_login) = LOWER($2)
           AND target.status = 'active'
           AND EXISTS (
               SELECT 1 FROM twitch_partners current
                WHERE LOWER(current.twitch_login) = LOWER($3)
                  AND current.status = 'active'
                  AND current.id <> target.id
           )",
    )
    .bind(user_id)
    .bind(old_login)
    .bind(new_login)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    let updated = sqlx::query(
        "UPDATE twitch_partners SET twitch_login = $3
         WHERE twitch_user_id = $1 AND LOWER(twitch_login) = LOWER($2)",
    )
    .bind(user_id)
    .bind(old_login)
    .bind(new_login)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    Ok(deleted + updated)
}

async fn rewrite_login_keyed(
    tx: &mut Transaction<'_, Postgres>,
    table: &str,
    login_column: &str,
    old_login: &str,
    new_login: &str,
) -> Result<u64, sqlx::Error> {
    let update = format!(
        "UPDATE {table} target SET {login_column} = $2
         WHERE LOWER(target.{login_column}) = LOWER($1)
           AND NOT EXISTS (
               SELECT 1 FROM {table} current
                WHERE LOWER(current.{login_column}) = LOWER($2)
           )"
    );
    let updated = sqlx::query(&update)
        .bind(old_login)
        .bind(new_login)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    let delete = format!(
        "DELETE FROM {table} WHERE LOWER({login_column}) = LOWER($1)"
    );
    let deleted = sqlx::query(&delete)
        .bind(old_login)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    Ok(updated + deleted)
}
