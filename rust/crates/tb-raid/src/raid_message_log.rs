//! Abgleich der Raid-Begrüßung gegen das persistierte Chatlog
//! (`twitch_chat_messages`).
//!
//! ## Warum
//!
//! Die Live-Beobachtung der Begrüßung hat zwei Blindstellen: der EventSub-Strom
//! sieht eine Nachricht nur, wenn das Pending zum Zeitpunkt des Eintreffens
//! schon registriert war, und die anonyme IRC-Beobachtung joint den Zielkanal
//! erst nach dem Raid-Start. Beides verpasst genau das kurze „Hallo" direkt zu
//! Beginn und wirft dem Raider dann fälschlich Schweigen vor.
//!
//! Das Chatlog kennt diese Rennen nicht: jede Nachricht steht mit ihrem echten
//! Zeitstempel in der Tabelle, sobald der Zielkanal mitgeschrieben wird. Der
//! Abgleich mappt den Raid-Zeitpunkt auf den Chatverlauf und zählt nach, ob der
//! Raider (inklusive seiner Zweit-Accounts) tatsächlich geschrieben hat.

use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};

/// Ergebnis des Chatlog-Abgleichs für einen Raider im Zielchat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GreetingObservation {
    /// Nachrichten des Raiders im Fenster.
    pub count: u32,
    /// Abstand zwischen erster und letzter Nachricht.
    pub span: Duration,
}

/// Liest das persistierte Chatlog für den Begrüßungs-Abgleich.
#[derive(Clone)]
pub struct RaidMessageLog {
    pool: PgPool,
}

impl RaidMessageLog {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Zählt die Nachrichten des Raiders im Zielchat ab `since`.
    ///
    /// `raider_logins` und `raider_ids` enthalten den Raider und seine bekannten
    /// Zweit-Accounts; eine Nachricht von einem davon zählt als seine.
    ///
    /// `Ok(None)` heißt: der Zielkanal wurde im Fenster gar nicht mitgeschrieben
    /// (keine einzige Nachricht persistiert). Daraus darf **nicht** auf Schweigen
    /// geschlossen werden — die Begrüßung ist schlicht nicht messbar.
    ///
    /// `Ok(Some(_))` ist belastbar: der Kanal wurde erfasst, die Zählung ist echt.
    pub async fn observe(
        &self,
        target_login: &str,
        raider_logins: &[String],
        raider_ids: &[String],
        since: DateTime<Utc>,
    ) -> Result<Option<GreetingObservation>, sqlx::Error> {
        let target = target_login.trim().trim_start_matches('#').to_lowercase();
        if target.is_empty() {
            return Ok(None);
        }
        let logins: Vec<String> = raider_logins
            .iter()
            .map(|login| login.trim().to_lowercase())
            .filter(|login| !login.is_empty())
            .collect();
        let ids: Vec<String> = raider_ids
            .iter()
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
            .collect();

        // Ein Scan über das Kanal-Fenster: `total` deckt die Erfassung ab, die
        // gefilterten Aggregate liefern die Zählung des Raiders. Der Index
        // (streamer_login, message_ts) trägt den WHERE-Teil.
        let row = sqlx::query(
            "SELECT \
                 COUNT(*) FILTER (WHERE LOWER(chatter_login) = ANY($3) \
                                     OR chatter_id = ANY($4)) AS raider_count, \
                 MIN(message_ts) FILTER (WHERE LOWER(chatter_login) = ANY($3) \
                                           OR chatter_id = ANY($4)) AS first_ts, \
                 MAX(message_ts) FILTER (WHERE LOWER(chatter_login) = ANY($3) \
                                           OR chatter_id = ANY($4)) AS last_ts, \
                 COUNT(*) AS total \
             FROM twitch_chat_messages \
             WHERE LOWER(streamer_login) = $1 \
               AND message_ts >= $2",
        )
        .bind(&target)
        .bind(since)
        .bind(&logins)
        .bind(&ids)
        .fetch_one(&self.pool)
        .await?;

        let total: i64 = row.try_get("total")?;
        if total == 0 {
            // Kanal im Fenster nicht mitgeschrieben: nicht messbar, kein Schweigen.
            return Ok(None);
        }

        let raider_count: i64 = row.try_get("raider_count")?;
        let first_ts: Option<DateTime<Utc>> = row.try_get("first_ts")?;
        let last_ts: Option<DateTime<Utc>> = row.try_get("last_ts")?;
        let span = match (first_ts, last_ts) {
            (Some(first), Some(last)) => (last - first).to_std().unwrap_or(Duration::ZERO),
            _ => Duration::ZERO,
        };

        Ok(Some(GreetingObservation {
            count: raider_count.max(0) as u32,
            span,
        }))
    }
}
