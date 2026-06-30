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

#[path = "inbox_store/retry.rs"]
mod write_retry;
use write_retry::{with_write_retry, RetryPolicy};

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

/// Restzustand der letzten Session (Quelle für den Auto-Raid-Trigger).
/// Python: `previous_state` im Offline-Handler.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OfflineSourceState {
    /// INTEGER-Flag (0/1) — für den Manual-Raid-DB-Fallback.
    pub is_live: Option<i32>,
    pub last_game: Option<String>,
    pub had_deadlock_in_session: Option<i32>,
    pub last_deadlock_seen_at: Option<String>,
    pub last_viewer_count: Option<i32>,
    pub last_started_at: Option<String>,
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

        let rows = sqlx::query!(
            r#"
            SELECT ls.twitch_user_id, ls.streamer_login, ls.last_stream_id,
                   ls.last_started_at, ls.last_title, ls.last_game_id,
                   ls.last_discord_message_id, ls.last_notified_at, ls.is_live,
                   ls.last_seen_at, ls.last_game, ls.last_viewer_count,
                   ls.last_tracking_token, ls.active_session_id,
                   ls.had_deadlock_in_session, ls.last_deadlock_seen_at,
                   COALESCE(p.raid_bot_enabled, 0) AS "partner_raid_bot_enabled!"
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
            &logins,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut snapshot: HashMap<String, SnapshotEntry> = HashMap::new();
        for row in rows {
            let state = LiveStateRow {
                twitch_user_id: row.twitch_user_id,
                streamer_login: row.streamer_login,
                last_stream_id: row.last_stream_id,
                last_started_at: row.last_started_at,
                last_title: row.last_title,
                last_game_id: row.last_game_id,
                last_discord_message_id: row.last_discord_message_id,
                last_notified_at: row.last_notified_at,
                is_live: row.is_live,
                last_seen_at: row.last_seen_at,
                last_game: row.last_game,
                last_viewer_count: row.last_viewer_count,
                last_tracking_token: row.last_tracking_token,
                active_session_id: row.active_session_id,
                had_deadlock_in_session: row.had_deadlock_in_session,
                last_deadlock_seen_at: row.last_deadlock_seen_at,
            };
            let key = state.streamer_login.trim().to_lowercase();
            if key.is_empty() {
                continue;
            }
            snapshot.insert(
                key,
                SnapshotEntry {
                    streamer_login: state.streamer_login.clone(),
                    twitch_user_id: Some(state.twitch_user_id.clone()),
                    partner_raid_bot_enabled: row.partner_raid_bot_enabled,
                    state: Some(state),
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
            let rows = sqlx::query!(
                r#"
                SELECT DISTINCT ON (p.twitch_user_id)
                       p.twitch_user_id,
                       COALESCE(p.raid_bot_enabled, 0) AS "partner_raid_bot_enabled!"
                FROM twitch_partners p
                WHERE p.status = 'active'
                  AND p.twitch_user_id = ANY($1)
                ORDER BY p.twitch_user_id, p.id DESC
                "#,
                &missing_user_ids,
            )
            .fetch_all(&self.pool)
            .await?;
            partner_flags.extend(
                rows.into_iter()
                    .map(|row| (row.twitch_user_id, row.partner_raid_bot_enabled)),
            );
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

        with_write_retry(RetryPolicy::from_env(), || {
            let pool = self.pool.clone();
            let cleanup = &cleanup;
            let valid = &valid;
            async move {
                let mut tx = pool.begin().await?;
                for (login, user_id) in cleanup {
                    sqlx::query!(
                        r#"
                        DELETE FROM twitch_live_state
                         WHERE LOWER(streamer_login) = LOWER($1)
                           AND LOWER(COALESCE(twitch_user_id, '')) <> LOWER($2)
                        "#,
                        login,
                        user_id,
                    )
                    .execute(&mut *tx)
                    .await?;
                }
                for row in valid.iter().copied() {
                    sqlx::query!(
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
                        &row.twitch_user_id,
                        &row.streamer_login,
                        row.is_live,
                        &row.last_seen_at,
                        row.last_title.as_deref(),
                        row.last_game.as_deref(),
                        row.last_viewer_count,
                        row.last_discord_message_id.as_deref(),
                        row.last_tracking_token.as_deref(),
                        row.last_stream_id.as_deref(),
                        row.last_started_at.as_deref(),
                        row.had_deadlock_in_session,
                        row.active_session_id,
                        row.last_deadlock_seen_at.as_deref(),
                    )
                    .execute(&mut *tx)
                    .await?;
                }
                tx.commit().await?;
                Ok(())
            }
        })
        .await
    }

    /// EventSub stream.online: Drift-Cleanup + minimaler Upsert. Bestehende
    /// Felder bleiben erhalten (COALESCE-Semantik wie Python
    /// `_handle_stream_online`) — der Poll-Tick füllt den Rest nach.
    pub async fn apply_stream_online(
        &self,
        broadcaster_user_id: &str,
        login_lower: &str,
        stream_id: Option<&str>,
        started_at: Option<&str>,
        now_iso: &str,
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        if !login_lower.is_empty() {
            sqlx::query!(
                r#"
                DELETE FROM twitch_live_state
                 WHERE LOWER(streamer_login) = LOWER($1)
                   AND LOWER(COALESCE(twitch_user_id, '')) <> LOWER($2)
                "#,
                login_lower,
                broadcaster_user_id,
            )
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query!(
            r#"
            INSERT INTO twitch_live_state (
                twitch_user_id, streamer_login, is_live, last_seen_at, last_stream_id, last_started_at
            )
            VALUES ($1, $2, 1, $3, $4, $5)
            ON CONFLICT (twitch_user_id) DO UPDATE
                SET streamer_login = COALESCE(NULLIF(EXCLUDED.streamer_login, ''), twitch_live_state.streamer_login),
                    is_live = 1,
                    last_seen_at = EXCLUDED.last_seen_at,
                    last_stream_id = COALESCE(EXCLUDED.last_stream_id, twitch_live_state.last_stream_id),
                    last_started_at = COALESCE(EXCLUDED.last_started_at, twitch_live_state.last_started_at)
            "#,
            broadcaster_user_id,
            if login_lower.is_empty() {
                broadcaster_user_id
            } else {
                login_lower
            },
            now_iso,
            stream_id,
            started_at,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Go-Live-Enrichment: Titel/Kategorie aus dem gezielten `/channels`-Lookup
    /// in den Live-State übernehmen (COALESCE-Semantik wie channel.update,
    /// aber ohne Protokoll-Insert). Greift nur solange der Kanal live ist.
    pub async fn apply_channel_info(
        &self,
        broadcaster_user_id: &str,
        title: Option<&str>,
        game_name: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        if title.is_none() && game_name.is_none() {
            return Ok(());
        }
        sqlx::query!(
            "UPDATE twitch_live_state
                SET last_title = COALESCE($1, last_title),
                    last_game  = COALESCE($2, last_game)
              WHERE twitch_user_id = $3 AND is_live = 1",
            title,
            game_name,
            broadcaster_user_id,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// EventSub stream.offline: Live-State sofort auf offline setzen.
    pub async fn apply_stream_offline(
        &self,
        broadcaster_user_id: &str,
        now_iso: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "UPDATE twitch_live_state
                SET is_live = 0, last_seen_at = $1, active_session_id = NULL
              WHERE twitch_user_id = $2",
            now_iso,
            broadcaster_user_id,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Heilt verwaiste Live-State-Rows, deren TEXT-Timestamp länger nicht
    /// aktualisiert wurde. Wichtig: `last_seen_at` ist TEXT und muss für die
    /// Alterungsprüfung explizit als `timestamptz` interpretiert werden.
    pub async fn sweep_stale_live(&self, max_age_secs: i64) -> Result<u64, sqlx::Error> {
        let max_age_secs = max_age_secs.max(0).to_string();
        let result = sqlx::query!(
            "UPDATE twitch_live_state
                SET is_live = 0, active_session_id = NULL
              WHERE is_live = 1
                AND last_seen_at::timestamptz < now() - ($1 || ' seconds')::interval",
            max_age_secs,
        )
        .execute(&self.pool)
        .await?;
        let healed = result.rows_affected();
        if healed > 0 {
            tracing::info!(healed, "Stale Live-State-Markierungen bereinigt");
        }
        Ok(healed)
    }

    /// Session-Restzustand mehrerer Logins als Map (Python
    /// `load_partner_live_state_map`) — Eingabe für den
    /// Kandidaten-Eligibility-Filter des Auto-Raids.
    pub async fn source_states_by_logins(
        &self,
        logins: &[String],
    ) -> Result<std::collections::HashMap<String, OfflineSourceState>, sqlx::Error> {
        if logins.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let rows = sqlx::query!(
            "SELECT streamer_login, is_live, last_game, had_deadlock_in_session,
                    last_deadlock_seen_at, last_viewer_count, last_started_at
               FROM twitch_live_state WHERE streamer_login = ANY($1)",
            logins,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| {
                (
                    row.streamer_login.trim().to_lowercase(),
                    OfflineSourceState {
                        is_live: row.is_live,
                        last_game: row.last_game,
                        had_deadlock_in_session: row.had_deadlock_in_session,
                        last_deadlock_seen_at: row.last_deadlock_seen_at,
                        last_viewer_count: row.last_viewer_count,
                        last_started_at: row.last_started_at,
                    },
                )
            })
            .collect())
    }

    /// Restzustand der letzten Session für den Auto-Raid-Trigger.
    /// `apply_stream_offline` leert diese Spalten bewusst NICHT — sie
    /// beschreiben, was vor dem Offline-Gehen lief (Python `previous_state`).
    pub async fn offline_source_state(
        &self,
        broadcaster_user_id: &str,
    ) -> Result<Option<OfflineSourceState>, sqlx::Error> {
        sqlx::query_as!(
            OfflineSourceState,
            "SELECT is_live, last_game, had_deadlock_in_session, last_deadlock_seen_at,
                    last_viewer_count, last_started_at
               FROM twitch_live_state WHERE twitch_user_id = $1",
            broadcaster_user_id,
        )
        .fetch_optional(&self.pool)
        .await
    }

    /// Login zu einer user_id (Python `_resolve_eventsub_broadcaster_login`).
    pub async fn login_for_user_id(&self, user_id: &str) -> Result<Option<String>, sqlx::Error> {
        let login: Option<String> = sqlx::query_scalar!(
            "SELECT streamer_login
               FROM twitch_live_state
              WHERE twitch_user_id = $1
              ORDER BY last_seen_at DESC NULLS LAST
              LIMIT 1",
            user_id,
        )
        .fetch_optional(&self.pool)
        .await?;
        let login = login
            .map(|l| l.trim().to_lowercase())
            .filter(|l| !l.is_empty());
        if login.is_some() {
            return Ok(login);
        }

        let identity_login: Option<String> = sqlx::query_scalar!(
            "SELECT twitch_login
               FROM twitch_streamer_identities
              WHERE twitch_user_id = $1
                AND COALESCE(BTRIM(twitch_login), '') <> ''
              LIMIT 1",
            user_id,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(identity_login
            .map(|l| l.trim().to_lowercase())
            .filter(|l| !l.is_empty()))
    }

    /// Liest den Finalize-relevanten Zustand eines Logins.
    pub async fn finalize_state(&self, login: &str) -> Result<Option<FinalizeState>, sqlx::Error> {
        sqlx::query_as!(
            FinalizeState,
            r#"
            SELECT twitch_user_id AS "twitch_user_id?", last_game, had_deadlock_in_session
              FROM twitch_live_state WHERE streamer_login = $1
            "#,
            login,
        )
        .fetch_optional(&self.pool)
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(3)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_live_state (
                twitch_user_id TEXT PRIMARY KEY,
                streamer_login TEXT NOT NULL,
                last_stream_id TEXT, last_started_at TEXT, last_title TEXT, last_game_id TEXT,
                last_discord_message_id TEXT, last_notified_at TEXT,
                is_live INTEGER DEFAULT 0, last_seen_at TEXT, last_game TEXT,
                last_viewer_count INTEGER DEFAULT 0, last_tracking_token TEXT,
                active_session_id BIGINT, had_deadlock_in_session INTEGER DEFAULT 0,
                last_deadlock_seen_at TEXT
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_streamer_identities (
                twitch_user_id TEXT PRIMARY KEY,
                twitch_login TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    fn upsert_row(user_id: &str, login: &str) -> LiveStateUpsert {
        LiveStateUpsert {
            twitch_user_id: user_id.to_string(),
            streamer_login: login.to_string(),
            is_live: 1,
            last_seen_at: "2026-06-22T10:00:00+00:00".to_string(),
            last_title: None,
            last_game: None,
            last_viewer_count: 0,
            last_discord_message_id: None,
            last_tracking_token: None,
            last_stream_id: None,
            last_started_at: None,
            had_deadlock_in_session: 0,
            active_session_id: None,
            last_deadlock_seen_at: None,
        }
    }

    #[tokio::test]
    async fn login_for_user_id_falls_back_to_streamer_identity() {
        let Some(pool) = make_pool("t_live_state_login_identity").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login)
             VALUES ('42', 'NaniLogin')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let store = LiveStateStore::new(pool);
        let login = store.login_for_user_id("42").await.unwrap();
        assert_eq!(login.as_deref(), Some("nanilogin"));
    }

    #[tokio::test]
    async fn persist_retries_transient_connection_error() {
        let Some(pool) = make_pool("t_live_state_persist_retry").await else {
            return;
        };
        sqlx::query("CREATE SEQUENCE live_state_fail_once_seq")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            r#"
            CREATE OR REPLACE FUNCTION fail_live_state_insert_once()
            RETURNS trigger LANGUAGE plpgsql AS $$
            BEGIN
                IF nextval('live_state_fail_once_seq') = 1 THEN
                    RAISE SQLSTATE '08006';
                END IF;
                RETURN NEW;
            END $$;
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TRIGGER live_state_fail_once
             BEFORE INSERT ON twitch_live_state
             FOR EACH ROW EXECUTE FUNCTION fail_live_state_insert_once()",
        )
        .execute(&pool)
        .await
        .unwrap();

        let store = LiveStateStore::new(pool.clone());
        store.persist(&[upsert_row("42", "nani")]).await.unwrap();

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_live_state WHERE twitch_user_id = '42'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 1);
    }
}
