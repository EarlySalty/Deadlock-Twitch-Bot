//! Zugriff auf `twitch_live_state` — die „Wer ist gerade live"-Wahrheit.
//!
//! Vertrag wie Python (`_load_live_state_snapshot` / `_persist_live_state_rows`):
//!
//! - **Conflict-Key ist `twitch_user_id`** (PK), nicht der Login.
//! - **DELETE-before-UPSERT gegen user_id-Drift:** Existiert derselbe Login
//!   unter einer anderen user_id, wird die alte Row vorher entfernt.
//! - Rows ohne user_id oder Login werden übersprungen (Invariante 1,
//!   Plan-Doc Schritt 4) — hier mit Debug-Log statt komplett stumm.
//! - Timestamps in dieser Tabelle sind **TEXT** (ISO, Sekunden-Präzision) —
//!   anders als `twitch_stream_sessions` (timestamptz). Prod-verifiziert.

use std::collections::{BTreeSet, HashMap};

use sqlx::PgPool;

/// Vollständige Lesesicht einer `twitch_live_state`-Row (Spalten wie Prod).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LiveStateRow {
    pub twitch_user_id: String,
    pub streamer_login: String,
    pub last_stream_id: Option<String>,
    pub last_started_at: Option<String>,
    pub last_title: Option<String>,
    pub last_game_id: Option<String>,
    pub last_discord_message_id: Option<String>,
    pub last_notified_at: Option<String>,
    pub is_live: Option<i32>,
    pub last_seen_at: Option<String>,
    pub last_game: Option<String>,
    pub last_viewer_count: Option<i32>,
    pub last_tracking_token: Option<String>,
    pub active_session_id: Option<i64>,
    pub had_deadlock_in_session: Option<i32>,
    pub last_deadlock_seen_at: Option<String>,
}

/// Schreibsicht — exakt die 14 Spalten, die der Python-Upsert setzt.
#[derive(Debug, Clone)]
pub struct LiveStateUpsert {
    pub twitch_user_id: String,
    pub streamer_login: String,
    pub is_live: i32,
    pub last_seen_at: String,
    pub last_title: Option<String>,
    pub last_game: Option<String>,
    pub last_viewer_count: i32,
    pub last_discord_message_id: Option<String>,
    pub last_tracking_token: Option<String>,
    pub last_stream_id: Option<String>,
    pub last_started_at: Option<String>,
    pub had_deadlock_in_session: i32,
    pub active_session_id: Option<i64>,
    pub last_deadlock_seen_at: Option<String>,
}

/// Snapshot-Eintrag pro getracktem Login. `state == None` heißt: noch keine
/// Live-State-Row, aber aktiver Partner (Fallback-Eintrag wie in Python).
#[derive(Debug, Clone)]
pub struct SnapshotEntry {
    pub streamer_login: String,
    pub twitch_user_id: Option<String>,
    pub partner_raid_bot_enabled: i32,
    pub state: Option<LiveStateRow>,
}

/// Getrackter Streamer als Snapshot-Input.
#[derive(Debug, Clone)]
pub struct TrackedStreamer {
    pub login: String,
    pub twitch_user_id: Option<String>,
}

/// Minimaler Zustand fürs Session-Finalize.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct FinalizeState {
    pub twitch_user_id: Option<String>,
    pub last_game: Option<String>,
    pub had_deadlock_in_session: Option<i32>,
}

#[derive(sqlx::FromRow)]
struct SnapshotJoinRow {
    #[sqlx(flatten)]
    state: LiveStateRow,
    partner_raid_bot_enabled: i32,
}

#[derive(Clone)]
pub struct LiveStateStore {
    pool: PgPool,
}

impl LiveStateStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Lädt den Live-State aller getrackten Logins inkl. Partner-Raid-Flag
    /// (aktivster `twitch_partners`-Eintrag). Tracked Logins ohne Live-State-Row
    /// bekommen einen Fallback-Eintrag, sofern sie aktiver Partner sind.
    pub async fn load_snapshot(
        &self,
        tracked: &[TrackedStreamer],
    ) -> Result<HashMap<String, SnapshotEntry>, sqlx::Error> {
        let mut logins: Vec<String> = Vec::new();
        let mut login_to_user_id: HashMap<String, String> = HashMap::new();
        for entry in tracked {
            let login = entry.login.trim().to_lowercase();
            if login.is_empty() || logins.contains(&login) {
                continue;
            }
            if let Some(user_id) = entry
                .twitch_user_id
                .as_deref()
                .map(str::trim)
                .filter(|u| !u.is_empty())
            {
                login_to_user_id.insert(login.clone(), user_id.to_string());
            }
            logins.push(login);
        }
        if logins.is_empty() {
            return Ok(HashMap::new());
        }

        let rows: Vec<SnapshotJoinRow> = sqlx::query_as(
            r#"
            SELECT ls.twitch_user_id, ls.streamer_login, ls.last_stream_id,
                   ls.last_started_at, ls.last_title, ls.last_game_id,
                   ls.last_discord_message_id, ls.last_notified_at, ls.is_live,
                   ls.last_seen_at, ls.last_game, ls.last_viewer_count,
                   ls.last_tracking_token, ls.active_session_id,
                   ls.had_deadlock_in_session, ls.last_deadlock_seen_at,
                   COALESCE(p.raid_bot_enabled, 0) AS partner_raid_bot_enabled
            FROM twitch_live_state ls
            LEFT JOIN LATERAL (
                SELECT p.raid_bot_enabled
                FROM twitch_partners p
                WHERE p.status = 'active'
                  AND p.twitch_user_id = ls.twitch_user_id
                ORDER BY p.id DESC
                LIMIT 1
            ) p ON TRUE
            WHERE LOWER(ls.streamer_login) = ANY($1)
            "#,
        )
        .bind(&logins)
        .fetch_all(&self.pool)
        .await?;

        let mut snapshot: HashMap<String, SnapshotEntry> = HashMap::new();
        for row in rows {
            let key = row.state.streamer_login.trim().to_lowercase();
            if key.is_empty() {
                continue;
            }
            snapshot.insert(
                key,
                SnapshotEntry {
                    streamer_login: row.state.streamer_login.clone(),
                    twitch_user_id: Some(row.state.twitch_user_id.clone()),
                    partner_raid_bot_enabled: row.partner_raid_bot_enabled,
                    state: Some(row.state),
                },
            );
        }

        // Fallback: getrackte Logins ohne Live-State-Row, aber aktiver Partner.
        let missing_user_ids: Vec<String> = {
            let mut ids: Vec<String> = login_to_user_id
                .iter()
                .filter(|(login, _)| !snapshot.contains_key(*login))
                .map(|(_, user_id)| user_id.clone())
                .collect();
            ids.sort();
            ids.dedup();
            ids
        };
        let mut partner_flags: HashMap<String, i32> = HashMap::new();
        if !missing_user_ids.is_empty() {
            let rows: Vec<(String, i32)> = sqlx::query_as(
                r#"
                SELECT DISTINCT ON (p.twitch_user_id)
                       p.twitch_user_id,
                       COALESCE(p.raid_bot_enabled, 0) AS partner_raid_bot_enabled
                FROM twitch_partners p
                WHERE p.status = 'active'
                  AND p.twitch_user_id = ANY($1)
                ORDER BY p.twitch_user_id, p.id DESC
                "#,
            )
            .bind(&missing_user_ids)
            .fetch_all(&self.pool)
            .await?;
            partner_flags.extend(rows);
        }
        for (login, user_id) in &login_to_user_id {
            if snapshot.contains_key(login) {
                continue;
            }
            let Some(flag) = partner_flags.get(user_id) else {
                continue;
            };
            snapshot.insert(
                login.clone(),
                SnapshotEntry {
                    streamer_login: login.clone(),
                    twitch_user_id: Some(user_id.clone()),
                    partner_raid_bot_enabled: *flag,
                    state: None,
                },
            );
        }
        Ok(snapshot)
    }

    /// Batch-Persist in einer Transaktion: erst Drift-Cleanup, dann Upserts.
    pub async fn persist(&self, rows: &[LiveStateUpsert]) -> Result<(), sqlx::Error> {
        let valid: Vec<&LiveStateUpsert> = rows
            .iter()
            .filter(|r| {
                let ok = !r.twitch_user_id.trim().is_empty() && !r.streamer_login.trim().is_empty();
                if !ok {
                    tracing::debug!(
                        login = %r.streamer_login,
                        "Live-State-Row ohne user_id/login übersprungen"
                    );
                }
                ok
            })
            .collect();
        if valid.is_empty() {
            return Ok(());
        }

        // Drift-Cleanup-Paare dedupliziert und sortiert (stabile Lock-Reihenfolge).
        let cleanup: BTreeSet<(String, String)> = valid
            .iter()
            .map(|r| {
                (
                    r.streamer_login.trim().to_string(),
                    r.twitch_user_id.trim().to_string(),
                )
            })
            .collect();

        let mut tx = self.pool.begin().await?;
        for (login, user_id) in &cleanup {
            sqlx::query(
                r#"
                DELETE FROM twitch_live_state
                 WHERE LOWER(streamer_login) = LOWER($1)
                   AND LOWER(COALESCE(twitch_user_id, '')) <> LOWER($2)
                "#,
            )
            .bind(login)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        }
        for row in &valid {
            sqlx::query(
                r#"
                INSERT INTO twitch_live_state (
                    twitch_user_id, streamer_login, is_live, last_seen_at, last_title,
                    last_game, last_viewer_count, last_discord_message_id,
                    last_tracking_token, last_stream_id, last_started_at,
                    had_deadlock_in_session, active_session_id, last_deadlock_seen_at
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
                ON CONFLICT (twitch_user_id) DO UPDATE SET
                    streamer_login = EXCLUDED.streamer_login,
                    is_live = EXCLUDED.is_live,
                    last_seen_at = EXCLUDED.last_seen_at,
                    last_title = EXCLUDED.last_title,
                    last_game = EXCLUDED.last_game,
                    last_viewer_count = EXCLUDED.last_viewer_count,
                    last_discord_message_id = EXCLUDED.last_discord_message_id,
                    last_tracking_token = EXCLUDED.last_tracking_token,
                    last_stream_id = EXCLUDED.last_stream_id,
                    last_started_at = EXCLUDED.last_started_at,
                    had_deadlock_in_session = EXCLUDED.had_deadlock_in_session,
                    active_session_id = EXCLUDED.active_session_id,
                    last_deadlock_seen_at = EXCLUDED.last_deadlock_seen_at
                "#,
            )
            .bind(&row.twitch_user_id)
            .bind(&row.streamer_login)
            .bind(row.is_live)
            .bind(&row.last_seen_at)
            .bind(&row.last_title)
            .bind(&row.last_game)
            .bind(row.last_viewer_count)
            .bind(&row.last_discord_message_id)
            .bind(&row.last_tracking_token)
            .bind(&row.last_stream_id)
            .bind(&row.last_started_at)
            .bind(row.had_deadlock_in_session)
            .bind(row.active_session_id)
            .bind(&row.last_deadlock_seen_at)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Liest den Finalize-relevanten Zustand eines Logins.
    pub async fn finalize_state(&self, login: &str) -> Result<Option<FinalizeState>, sqlx::Error> {
        sqlx::query_as(
            "SELECT twitch_user_id, last_game, had_deadlock_in_session
               FROM twitch_live_state WHERE streamer_login = $1",
        )
        .bind(login)
        .fetch_optional(&self.pool)
        .await
    }
}
