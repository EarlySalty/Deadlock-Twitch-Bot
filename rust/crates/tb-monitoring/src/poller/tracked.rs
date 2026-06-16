//! Lädt die vom Monitoring zu überwachenden Kanäle
//! (Python `_load_tracked_streamers`): aktive/historische Partner aus der
//! View `twitch_streamers_partner_state` plus monitored-only Kanäle aus
//! `twitch_streamers`. Dazu die Auto-Archiv-Kandidaten.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// Ein getrackter Kanal mit Announcement-relevanter Partner-Config.
#[derive(Debug, Clone)]
pub struct TrackedEntry {
    pub login: String,
    pub twitch_user_id: Option<String>,
    pub require_link: bool,
    pub is_verified: bool,
    pub is_archived: bool,
    pub discord_user_id: Option<String>,
    pub live_ping_role_id: Option<i64>,
    pub live_ping_enabled: bool,
}

#[derive(sqlx::FromRow)]
struct TrackedRow {
    twitch_login: String,
    twitch_user_id: Option<String>,
    require_discord_link: Option<i32>,
    archived_at: Option<String>,
    is_partner_active: Option<i32>,
    discord_user_id: Option<String>,
    live_ping_role_id: Option<i64>,
    live_ping_enabled: Option<i32>,
}

#[derive(Clone)]
pub struct TrackedStore {
    pool: PgPool,
}

impl TrackedStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Liefert (tracked, partner_logins). `partner_logins` = verifizierte
    /// Partner (lowercase) für die `is_partner`-Markierung der Stats.
    pub async fn load(
        &self,
    ) -> Result<(Vec<TrackedEntry>, std::collections::HashSet<String>), sqlx::Error> {
        let rows: Vec<TrackedRow> = sqlx::query_as(
            r#"
            SELECT twitch_login, twitch_user_id, require_discord_link,
                   archived_at::text AS archived_at, is_partner_active, discord_user_id,
                   live_ping_role_id, COALESCE(live_ping_enabled, 1) AS live_ping_enabled
              FROM twitch_streamers_partner_state
            UNION ALL
            -- Monitored-only Kanäle sind keine Partner: Partner-Config als
            -- Spalten-Default, Archive-Status ist partner-spezifisch.
            SELECT s.twitch_login, s.twitch_user_id, 0 AS require_discord_link,
                   NULL::text AS archived_at, 0 AS is_partner_active, i.discord_user_id,
                   NULL::bigint AS live_ping_role_id, 1 AS live_ping_enabled
              FROM twitch_streamers s
              LEFT JOIN twitch_streamer_identities i
                ON i.twitch_user_id = s.twitch_user_id
             WHERE NOT EXISTS (
                   SELECT 1
                     FROM twitch_partners p
                    WHERE p.twitch_user_id = s.twitch_user_id
                       OR LOWER(p.twitch_login) = LOWER(s.twitch_login)
               )
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut tracked = Vec::with_capacity(rows.len());
        let mut partner_logins = std::collections::HashSet::new();
        for row in rows {
            let login = row.twitch_login.trim().to_string();
            if login.is_empty() {
                continue;
            }
            let is_verified = row.is_partner_active.unwrap_or(0) != 0;
            if is_verified {
                partner_logins.insert(login.to_lowercase());
            }
            tracked.push(TrackedEntry {
                login,
                twitch_user_id: row
                    .twitch_user_id
                    .map(|u| u.trim().to_string())
                    .filter(|u| !u.is_empty()),
                require_link: row.require_discord_link.unwrap_or(0) != 0,
                is_verified,
                is_archived: row
                    .archived_at
                    .as_deref()
                    .is_some_and(|a| !a.trim().is_empty()),
                discord_user_id: row.discord_user_id,
                live_ping_role_id: row.live_ping_role_id,
                live_ping_enabled: row.live_ping_enabled.unwrap_or(1) != 0,
            });
        }
        Ok((tracked, partner_logins))
    }

    /// Partner, deren letzter Deadlock-Stream vor `cutoff` liegt
    /// (Python `_auto_archive_inactive_streamers`-Query). Nur nicht-archivierte
    /// Partner mit vorhandener Historie.
    pub async fn archive_candidates(
        &self,
        target_game: &str,
        cutoff: DateTime<Utc>,
    ) -> Result<Vec<String>, sqlx::Error> {
        let rows: Vec<(String, Option<String>, Option<DateTime<Utc>>)> = sqlx::query_as(
            r#"
            SELECT s.twitch_login,
                   s.archived_at::text AS archived_at,
                   MAX(
                       CASE
                         WHEN LOWER(COALESCE(sess.game_name,'')) = LOWER($1)
                         THEN COALESCE(sess.ended_at, sess.started_at)
                       END
                    ) AS last_deadlock_stream_at
              FROM twitch_streamers_partner_state s
              LEFT JOIN twitch_stream_sessions sess
                ON LOWER(sess.streamer_login) = LOWER(s.twitch_login)
             WHERE s.is_partner = 1
             GROUP BY s.twitch_login, s.archived_at
            "#,
        )
        .bind(target_game)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|(login, archived_at, last_stream)| {
                let login = login.trim().to_lowercase();
                if login.is_empty() {
                    return None;
                }
                if archived_at.is_some_and(|a| !a.trim().is_empty()) {
                    return None;
                }
                // Keine Historie → keine automatische Archivierung.
                let last = last_stream?;
                (last < cutoff).then_some(login)
            })
            .collect())
    }
}
