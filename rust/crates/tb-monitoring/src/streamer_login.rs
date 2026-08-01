use sqlx::{PgPool, Postgres, Transaction};

const KNOWN_LOGIN_SQL: &str = r#"
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
"#;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenameTableCounts {
    pub renamed: u64,
    pub deleted: u64,
    pub skipped: u64,
}

impl RenameTableCounts {
    fn add(&mut self, other: &Self) {
        self.renamed += other.renamed;
        self.deleted += other.deleted;
        self.skipped += other.skipped;
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenameCounts {
    pub twitch_streamers: RenameTableCounts,
    pub twitch_live_state: RenameTableCounts,
    pub twitch_partners: RenameTableCounts,
    pub twitch_raid_auth: RenameTableCounts,
    pub twitch_partner_raid_scores: RenameTableCounts,
    pub twitch_streamer_identities: RenameTableCounts,
    pub twitch_engagement_settings: RenameTableCounts,
    pub twitch_engagement_channel_profile: RenameTableCounts,
    pub twitch_stream_sessions: RenameTableCounts,
    pub twitch_streamer_invites: RenameTableCounts,
    pub twitch_raw_chat_ingest_health: RenameTableCounts,
}

impl RenameCounts {
    pub fn total(&self) -> RenameTableCounts {
        let mut total = RenameTableCounts::default();
        for (_, counts) in self.tables() {
            total.add(counts);
        }
        total
    }

    fn tables(&self) -> [(&'static str, &RenameTableCounts); 11] {
        [
            ("twitch_streamers", &self.twitch_streamers),
            ("twitch_live_state", &self.twitch_live_state),
            ("twitch_partners", &self.twitch_partners),
            ("twitch_raid_auth", &self.twitch_raid_auth),
            (
                "twitch_partner_raid_scores",
                &self.twitch_partner_raid_scores,
            ),
            (
                "twitch_streamer_identities",
                &self.twitch_streamer_identities,
            ),
            (
                "twitch_engagement_settings",
                &self.twitch_engagement_settings,
            ),
            (
                "twitch_engagement_channel_profile",
                &self.twitch_engagement_channel_profile,
            ),
            ("twitch_stream_sessions", &self.twitch_stream_sessions),
            ("twitch_streamer_invites", &self.twitch_streamer_invites),
            (
                "twitch_raw_chat_ingest_health",
                &self.twitch_raw_chat_ingest_health,
            ),
        ]
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

        let old_login = sqlx::query_scalar::<_, String>(KNOWN_LOGIN_SQL)
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
    let requested_old_login = old_login.trim().to_lowercase();
    let new_login = new_login.trim().to_lowercase();
    if user_id.is_empty() || requested_old_login.is_empty() || new_login.is_empty() {
        return Err(sqlx::Error::Protocol(
            "twitch rename requires user_id, old_login and new_login".into(),
        ));
    }

    let mut tx = pool.begin().await?;
    sqlx::query(
        "SELECT pg_advisory_xact_lock(
             hashtextextended('tb-monitoring:twitch-login:' || $1, 0)
         )",
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    let old_login = sqlx::query_scalar::<_, String>(KNOWN_LOGIN_SQL)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?
        .map(|login| login.trim().to_lowercase())
        .filter(|login| !login.is_empty())
        .unwrap_or_else(|| requested_old_login.clone());
    if old_login.eq_ignore_ascii_case(&new_login) {
        tx.commit().await?;
        tracing::debug!(
            twitch_user_id = %user_id,
            requested_old_login = %requested_old_login,
            new_login = %new_login,
            "Twitch-Rename nach Transaktions-Lock nicht mehr nötig"
        );
        return Ok(RenameReport {
            twitch_user_id: user_id.to_string(),
            old_login: requested_old_login,
            new_login,
            counts: RenameCounts::default(),
        });
    }
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
            user_id,
            &old_login,
            &new_login,
        )
        .await?,
        twitch_engagement_channel_profile: rewrite_login_keyed(
            &mut tx,
            "twitch_engagement_channel_profile",
            "channel_login",
            user_id,
            &old_login,
            &new_login,
        )
        .await?,
        twitch_stream_sessions: RenameTableCounts {
            renamed: sqlx::query(
                "UPDATE twitch_stream_sessions SET streamer_login = $2
                 WHERE LOWER(streamer_login) = LOWER($1) AND ended_at IS NULL",
            )
            .bind(&old_login)
            .bind(&new_login)
            .execute(&mut *tx)
            .await?
            .rows_affected(),
            ..RenameTableCounts::default()
        },
        twitch_streamer_invites: rewrite_login_keyed(
            &mut tx,
            "twitch_streamer_invites",
            "streamer_login",
            user_id,
            &old_login,
            &new_login,
        )
        .await?,
        twitch_raw_chat_ingest_health: rewrite_login_keyed(
            &mut tx,
            "twitch_raw_chat_ingest_health",
            "streamer_login",
            user_id,
            &old_login,
            &new_login,
        )
        .await?,
    };
    record_login_aliases(&mut tx, user_id, &old_login, &new_login).await?;
    tx.commit().await?;

    for (table, table_counts) in counts.tables() {
        tracing::info!(
            twitch_user_id = %user_id,
            old_login = %old_login,
            new_login = %new_login,
            table = %table,
            renamed = table_counts.renamed,
            deleted = table_counts.deleted,
            skipped = table_counts.skipped,
            "Twitch-Login-Tabelle aktualisiert"
        );
    }
    let total = counts.total();
    tracing::info!(
        twitch_user_id = %user_id,
        old_login = %old_login,
        new_login = %new_login,
        renamed = total.renamed,
        deleted = total.deleted,
        skipped = total.skipped,
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
) -> Result<RenameTableCounts, sqlx::Error> {
    let deleted = if login_is_unique {
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
        sqlx::query(&delete)
            .bind(user_id)
            .bind(old_login)
            .bind(new_login)
            .execute(&mut **tx)
            .await?
            .rows_affected()
    } else {
        0
    };
    let update = format!(
        "UPDATE {table} SET {login_column} = $3
         WHERE twitch_user_id = $1 AND LOWER({login_column}) = LOWER($2)"
    );
    let renamed = sqlx::query(&update)
        .bind(user_id)
        .bind(old_login)
        .bind(new_login)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    Ok(RenameTableCounts {
        renamed,
        deleted,
        skipped: 0,
    })
}

async fn rewrite_partners(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    old_login: &str,
    new_login: &str,
) -> Result<RenameTableCounts, sqlx::Error> {
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
    Ok(RenameTableCounts {
        renamed: updated,
        deleted,
        skipped: 0,
    })
}

async fn rewrite_login_keyed(
    tx: &mut Transaction<'_, Postgres>,
    table: &str,
    login_column: &str,
    user_id: &str,
    old_login: &str,
    new_login: &str,
) -> Result<RenameTableCounts, sqlx::Error> {
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
    let conflict = format!(
        "SELECT COUNT(*) FROM {table} target
         WHERE LOWER(target.{login_column}) = LOWER($1)
           AND EXISTS (
               SELECT 1 FROM {table} current
                WHERE LOWER(current.{login_column}) = LOWER($2)
           )"
    );
    let skipped = sqlx::query_scalar::<_, i64>(&conflict)
        .bind(old_login)
        .bind(new_login)
        .fetch_one(&mut **tx)
        .await?
        .unsigned_abs();
    if skipped > 0 {
        tracing::warn!(
            table = %table,
            twitch_user_id = %user_id,
            old_login = %old_login,
            new_login = %new_login,
            skipped,
            "Twitch-Login-Konflikt: login-gebundene Zeile bleibt unverändert"
        );
    }
    Ok(RenameTableCounts {
        renamed: updated,
        deleted: 0,
        skipped,
    })
}

async fn record_login_aliases(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    old_login: &str,
    new_login: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE twitch_login_aliases SET is_current = FALSE WHERE twitch_user_id = $1")
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query(
        "INSERT INTO twitch_login_aliases
             (twitch_user_id, login, first_seen_at, last_seen_at, is_current)
         VALUES ($1, $2, NOW(), NOW(), FALSE)
         ON CONFLICT (twitch_user_id, login) DO UPDATE
             SET last_seen_at = EXCLUDED.last_seen_at,
                 is_current = FALSE",
    )
    .bind(user_id)
    .bind(old_login)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT INTO twitch_login_aliases
             (twitch_user_id, login, first_seen_at, last_seen_at, is_current)
         VALUES ($1, $2, NOW(), NOW(), TRUE)
         ON CONFLICT (twitch_user_id, login) DO UPDATE
             SET last_seen_at = EXCLUDED.last_seen_at,
                 is_current = TRUE",
    )
    .bind(user_id)
    .bind(new_login)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
