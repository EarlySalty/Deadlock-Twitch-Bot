//! Durable Work-Queue der EventSub-Verarbeitung
//! (`twitch_eventsub_processing_inbox` + `twitch_eventsub_processing_dead_letter`).
//!
//! Empfang und Verarbeitung sind entkoppelt: Transporte legen Aufträge per
//! [`ProcessingInboxStore::enqueue`] ab, der Worker ([`crate::inbox_runtime`])
//! leased fällige Einträge via `FOR UPDATE SKIP LOCKED` und retryt mit
//! Backoff bis zum Dead-Letter. Vertrag identisch zur
//! Python-`EventSubProcessingInboxStore`; beide Prozesse können während der
//! Migration sicher gleichzeitig leasen (Row-Lock + Skip).

use sqlx::PgPool;
use uuid::Uuid;

/// Fehlertexte werden wie in Python auf 500 Zeichen gekürzt.
const MAX_ERROR_LEN: usize = 500;

/// Ein geleaster Arbeitsauftrag (Ergebnis von [`ProcessingInboxStore::lease_due`]).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LeasedWork {
    pub work_id: String,
    pub work_type: String,
    pub message_id: Option<String>,
    pub payload_json: String,
    pub queued_at: f64,
    pub attempt_count: i32,
}

/// Offener Inbox-Eintrag (Admin-/Snapshot-Sicht).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PendingEntry {
    pub work_id: String,
    pub work_type: String,
    pub message_id: Option<String>,
    pub payload_json: String,
    pub queued_at: f64,
    pub next_attempt_at: f64,
    pub attempt_count: i32,
    pub last_error: Option<String>,
}

/// Dead-Letter-Eintrag (Admin-/Snapshot-Sicht).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DeadLetterEntry {
    pub work_id: String,
    pub work_type: String,
    pub message_id: Option<String>,
    pub payload_json: String,
    pub queued_at: f64,
    pub dead_lettered_at: f64,
    pub attempt_count: i32,
    pub last_error: Option<String>,
}

/// Zugriff auf die Inbox-Tabellen. Zeit ist expliziter Parameter
/// (Epoch-Sekunden) — deterministisch testbar.
#[derive(Clone)]
pub struct ProcessingInboxStore {
    pool: PgPool,
}

impl ProcessingInboxStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Legt einen Auftrag an, sofort fällig (`next_attempt_at = now`).
    /// `work_id` = UUIDv4 ohne Bindestriche (Python-`uuid4().hex`-Format).
    pub async fn enqueue(
        &self,
        work_type: &str,
        payload: &serde_json::Value,
        message_id: Option<&str>,
        now: f64,
    ) -> Result<String, sqlx::Error> {
        let work_id = Uuid::new_v4().simple().to_string();
        let message_id = message_id.map(str::trim).filter(|m| !m.is_empty());
        sqlx::query(
            r#"
            INSERT INTO twitch_eventsub_processing_inbox (
                work_id, work_type, message_id, payload_json,
                queued_at, next_attempt_at, attempt_count, last_error
            )
            VALUES ($1, $2, $3, $4, $5, $5, 0, NULL)
            "#,
        )
        .bind(&work_id)
        .bind(work_type.trim())
        .bind(message_id)
        .bind(payload.to_string())
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(work_id)
    }

    /// Leased bis zu `limit` fällige Aufträge für `lease_seconds`:
    /// `next_attempt_at` wird auf `now + lease` geschoben, sodass kein anderer
    /// Worker (auch nicht der Python-Prozess) sie parallel zieht.
    pub async fn lease_due(
        &self,
        now: f64,
        lease_seconds: f64,
        limit: i64,
    ) -> Result<Vec<LeasedWork>, sqlx::Error> {
        let lease_until = now + lease_seconds.max(1.0);
        sqlx::query_as::<_, LeasedWork>(
            r#"
            WITH due AS (
                SELECT work_id
                  FROM twitch_eventsub_processing_inbox
                 WHERE next_attempt_at <= $1
                 ORDER BY queued_at ASC
                 LIMIT $2
                 FOR UPDATE SKIP LOCKED
            )
            UPDATE twitch_eventsub_processing_inbox AS inbox
               SET next_attempt_at = $3
              FROM due
             WHERE inbox.work_id = due.work_id
            RETURNING inbox.work_id, inbox.work_type, inbox.message_id,
                      inbox.payload_json, inbox.queued_at, inbox.attempt_count
            "#,
        )
        .bind(now)
        .bind(limit.max(1))
        .bind(lease_until)
        .fetch_all(&self.pool)
        .await
    }

    /// Erfolgreich verarbeitet → Auftrag löschen.
    pub async fn mark_delivered(&self, work_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM twitch_eventsub_processing_inbox WHERE work_id = $1")
            .bind(work_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Fehlversuch → Versuchszähler + nächste Fälligkeit + Fehlertext setzen.
    pub async fn mark_retry(
        &self,
        work_id: &str,
        attempt_count: i32,
        error_message: &str,
        next_attempt_at: f64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE twitch_eventsub_processing_inbox
               SET attempt_count = $1,
                   next_attempt_at = $2,
                   last_error = $3
             WHERE work_id = $4
            "#,
        )
        .bind(attempt_count.max(1))
        .bind(next_attempt_at)
        .bind(truncate_error(error_message))
        .bind(work_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Endgültig gescheitert → atomar in die Dead-Letter-Tabelle verschieben.
    pub async fn mark_dead_letter(
        &self,
        work: &LeasedWork,
        attempt_count: i32,
        error_message: &str,
        dead_lettered_at: f64,
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO twitch_eventsub_processing_dead_letter (
                work_id, work_type, message_id, payload_json,
                queued_at, dead_lettered_at, attempt_count, last_error
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (work_id) DO UPDATE
               SET work_type = EXCLUDED.work_type,
                   message_id = EXCLUDED.message_id,
                   payload_json = EXCLUDED.payload_json,
                   queued_at = EXCLUDED.queued_at,
                   dead_lettered_at = EXCLUDED.dead_lettered_at,
                   attempt_count = EXCLUDED.attempt_count,
                   last_error = EXCLUDED.last_error
            "#,
        )
        .bind(&work.work_id)
        .bind(&work.work_type)
        .bind(work.message_id.as_deref().map(str::trim).filter(|m| !m.is_empty()))
        .bind(&work.payload_json)
        .bind(work.queued_at)
        .bind(dead_lettered_at)
        .bind(attempt_count.max(1))
        .bind(truncate_error(error_message))
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM twitch_eventsub_processing_inbox WHERE work_id = $1")
            .bind(&work.work_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Offene Aufträge, älteste zuerst (Admin-Snapshot).
    pub async fn list_pending(&self, limit: i64) -> Result<Vec<PendingEntry>, sqlx::Error> {
        sqlx::query_as::<_, PendingEntry>(
            r#"
            SELECT work_id, work_type, message_id, payload_json,
                   queued_at, next_attempt_at, attempt_count, last_error
              FROM twitch_eventsub_processing_inbox
             ORDER BY queued_at ASC
             LIMIT $1
            "#,
        )
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await
    }

    /// Dead-Letters, neueste zuerst (Admin-Snapshot).
    pub async fn list_dead_letters(&self, limit: i64) -> Result<Vec<DeadLetterEntry>, sqlx::Error> {
        sqlx::query_as::<_, DeadLetterEntry>(
            r#"
            SELECT work_id, work_type, message_id, payload_json,
                   queued_at, dead_lettered_at, attempt_count, last_error
              FROM twitch_eventsub_processing_dead_letter
             ORDER BY dead_lettered_at DESC
             LIMIT $1
            "#,
        )
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await
    }

    /// Holt einen Dead-Letter atomar zurück in die Inbox (Versuchszähler 0,
    /// sofort fällig). `false` = work_id unbekannt.
    pub async fn requeue_dead_letter(&self, work_id: &str, now: f64) -> Result<bool, sqlx::Error> {
        let work_id = work_id.trim();
        if work_id.is_empty() {
            return Ok(false);
        }
        let mut tx = self.pool.begin().await?;
        let row: Option<(String, String, Option<String>, String)> = sqlx::query_as(
            r#"
            DELETE FROM twitch_eventsub_processing_dead_letter
             WHERE work_id = $1
         RETURNING work_id, work_type, message_id, payload_json
            "#,
        )
        .bind(work_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((work_id, work_type, message_id, payload_json)) = row else {
            return Ok(false);
        };
        sqlx::query(
            r#"
            INSERT INTO twitch_eventsub_processing_inbox (
                work_id, work_type, message_id, payload_json,
                queued_at, next_attempt_at, attempt_count, last_error
            )
            VALUES ($1, $2, $3, $4, $5, $5, 0, NULL)
            "#,
        )
        .bind(&work_id)
        .bind(&work_type)
        .bind(&message_id)
        .bind(&payload_json)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }
}

/// Kürzt Fehlertexte UTF-8-sicher auf [`MAX_ERROR_LEN`] Zeichen; leer → NULL.
fn truncate_error(error_message: &str) -> Option<String> {
    let trimmed = error_message.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(MAX_ERROR_LEN).collect())
}
