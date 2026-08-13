//! DB-Store für `twitch_raid_courtesy_events` — Datenschicht der Raid-Etikette.
//!
//! Schreibt je ausgeführtem Raid eine Beobachtung und liest sie für den
//! Score-Aufbau und die Whisper-Drosselung zurück. Die Einstufungs- und
//! Score-Logik liegt in [`crate::courtesy`], dieser Store rechnet nichts.

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;

use crate::courtesy::{
    summarize, CourtesyClass, CourtesyOutcome, CourtesySummary, COURTESY_LOOKBACK_DAYS,
};

/// Woher die Beobachtung stammt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationSource {
    /// Nur über die EventSub-Chat-Events des Bots gesehen.
    EventSub,
    /// Nur über die anonyme IRC-Beobachtung des Zielkanals.
    IrcProbe,
    /// Beide Quellen hatten etwas.
    Both,
}

impl ObservationSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EventSub => "eventsub",
            Self::IrcProbe => "irc_probe",
            Self::Both => "both",
        }
    }
}

/// Eine fertige Beobachtung, bereit zum Schreiben.
#[derive(Debug, Clone)]
pub struct CourtesyEvent {
    pub raid_history_id: Option<i64>,
    pub from_broadcaster_id: String,
    pub from_broadcaster_login: String,
    pub to_broadcaster_id: String,
    pub to_broadcaster_login: String,
    /// Beginn des Beobachtungsfensters.
    pub observed_from: DateTime<Utc>,
    pub outcome: CourtesyOutcome,
    pub message_count: i32,
    pub message_span_sec: i32,
    pub observation_source: Option<ObservationSource>,
    /// Bei [`CourtesyOutcome::Unknown`]: warum nicht messbar.
    pub unknown_reason: Option<String>,
    pub whisper_sent: bool,
}

#[derive(Clone)]
pub struct CourtesyStore {
    pool: PgPool,
}

impl CourtesyStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Schreibt eine Beobachtung. Ein bereits bewerteter Raid (gleiche
    /// `raid_history_id`) wird nicht doppelt eingetragen.
    pub async fn record(&self, event: &CourtesyEvent) -> Result<Option<i64>, sqlx::Error> {
        let id = sqlx::query_scalar!(
            r#"
            INSERT INTO twitch_raid_courtesy_events (
                raid_history_id, from_broadcaster_id, from_broadcaster_login,
                to_broadcaster_id, to_broadcaster_login, observed_from,
                courtesy_class, message_count, message_span_sec,
                observation_source, unknown_reason, whisper_sent
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            ON CONFLICT (raid_history_id) WHERE raid_history_id IS NOT NULL
            DO NOTHING
            RETURNING id
            "#,
            event.raid_history_id,
            event.from_broadcaster_id.trim(),
            event.from_broadcaster_login.trim().to_lowercase(),
            event.to_broadcaster_id.trim(),
            event.to_broadcaster_login.trim().to_lowercase(),
            event.observed_from,
            event.outcome.as_str(),
            event.message_count,
            event.message_span_sec,
            event.observation_source.map(ObservationSource::as_str),
            event.unknown_reason.as_deref(),
            event.whisper_sent,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(id)
    }

    /// Fasst die Historie eines Streamers über [`COURTESY_LOOKBACK_DAYS`]
    /// zusammen. `unknown` wird von [`summarize`] ignoriert.
    pub async fn summary_for(
        &self,
        from_broadcaster_id: &str,
        now: DateTime<Utc>,
    ) -> Result<CourtesySummary, sqlx::Error> {
        let cutoff = now - Duration::days(COURTESY_LOOKBACK_DAYS);
        let classes = sqlx::query_scalar!(
            r#"
            SELECT courtesy_class AS "courtesy_class!"
            FROM twitch_raid_courtesy_events
            WHERE from_broadcaster_id = $1
              AND observed_at >= $2
            ORDER BY observed_at DESC
            "#,
            from_broadcaster_id.trim(),
            cutoff,
        )
        .fetch_all(&self.pool)
        .await?;

        let outcomes: Vec<CourtesyOutcome> = classes
            .iter()
            .map(|class| CourtesyOutcome::from_db(class))
            .collect();
        Ok(summarize(&outcomes))
    }

    /// Wie [`CourtesyStore::summary_for`], aber für viele Streamer auf einmal —
    /// der Score-Refresh läuft über den gesamten Partner-Roster.
    ///
    /// Streamer ohne Zeilen fehlen im Ergebnis; der Aufrufer setzt für sie den
    /// Default ein (voller Wert, keine Klasse).
    pub async fn summaries_for(
        &self,
        from_broadcaster_ids: &[String],
        now: DateTime<Utc>,
    ) -> Result<std::collections::HashMap<String, CourtesySummary>, sqlx::Error> {
        use std::collections::HashMap;

        if from_broadcaster_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let cutoff = now - Duration::days(COURTESY_LOOKBACK_DAYS);
        let rows = sqlx::query!(
            r#"
            SELECT from_broadcaster_id AS "from_broadcaster_id!",
                   courtesy_class AS "courtesy_class!"
            FROM twitch_raid_courtesy_events
            WHERE from_broadcaster_id = ANY($1)
              AND observed_at >= $2
            ORDER BY observed_at DESC
            "#,
            from_broadcaster_ids,
            cutoff,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut grouped: HashMap<String, Vec<CourtesyOutcome>> = HashMap::new();
        for row in rows {
            grouped
                .entry(row.from_broadcaster_id)
                .or_default()
                .push(CourtesyOutcome::from_db(&row.courtesy_class));
        }
        Ok(grouped
            .into_iter()
            .map(|(id, outcomes)| (id, summarize(&outcomes)))
            .collect())
    }

    /// Zeitpunkt der letzten versendeten Whisper an diesen Streamer.
    /// `None` = noch nie eine bekommen.
    pub async fn last_whisper_at(
        &self,
        from_broadcaster_id: &str,
    ) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
        sqlx::query_scalar!(
            r#"
            SELECT MAX(observed_at) AS "last_at?"
            FROM twitch_raid_courtesy_events
            WHERE from_broadcaster_id = $1
              AND whisper_sent
            "#,
            from_broadcaster_id.trim(),
        )
        .fetch_one(&self.pool)
        .await
    }

    /// Die letzten Einstufungen eines Streamers, neueste zuerst. Für Logs und
    /// die Dashboard-Ansicht.
    pub async fn recent_classes(
        &self,
        from_broadcaster_id: &str,
        limit: i64,
    ) -> Result<Vec<CourtesyOutcome>, sqlx::Error> {
        let classes = sqlx::query_scalar!(
            r#"
            SELECT courtesy_class AS "courtesy_class!"
            FROM twitch_raid_courtesy_events
            WHERE from_broadcaster_id = $1
            ORDER BY observed_at DESC
            LIMIT $2
            "#,
            from_broadcaster_id.trim(),
            limit.clamp(1, 500),
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(classes
            .iter()
            .map(|class| CourtesyOutcome::from_db(class))
            .collect())
    }
}

/// Matching-Klassen aus dem Score-Cache, wie sie die Kandidaten-Auswahl braucht.
///
/// Getrennt vom Ereignis-Store, weil die Auswahl den fertigen Aggregatwert aus
/// `twitch_partner_raid_scores` liest statt jedes Mal die Historie zu falten.
pub fn parse_class(value: Option<&str>) -> Option<CourtesyClass> {
    value.and_then(CourtesyClass::from_db)
}
