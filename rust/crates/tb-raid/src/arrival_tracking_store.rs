//! DB-Store für `twitch_raid_arrival_tracking` — Arrival-Datenschicht.
//!
//! Port von `bot/raid/services/partner_arrival_tracking.py` — nur INSERT/UPDATE,
//! keine Klassifikations-/Korrelations-Logik.
//!
//! Prod-Schema (verifiziert):
//!
//! | Spalte                    | Typ          | Rust-Typ                  |
//! |---------------------------|--------------|---------------------------|
//! | id                        | int (int4)   | i64 via `RETURNING id::bigint` |
//! | detected_at               | timestamptz  | serverseitig NOW()        |
//! | last_signal_at            | timestamptz  | serverseitig NOW()        |
//! | from_broadcaster_id       | text         | Option<String>            |
//! | from_broadcaster_login    | text         | String                    |
//! | to_broadcaster_id         | text         | String                    |
//! | to_broadcaster_login      | text         | String                    |
//! | viewer_count              | int          | i32                       |
//! | classification            | text         | String                    |
//! | confirmation_signals      | text         | String (CSV/kommasep.)    |
//! | primary_signal            | text         | String                    |
//! | correlation_status        | text         | String                    |
//! | correlation_detail        | text         | Option<String>            |
//! | source_resolution         | text         | String                    |
//! | raid_history_id           | bigint       | Option<i64>               |
//! | raid_history_executed_at  | timestamptz  | Option<DateTime<Utc>>     |
//! | unraid_seen               | boolean      | bool                      |
//! | last_unraid_at            | timestamptz  | Option<DateTime<Utc>>     |

use chrono::{DateTime, Utc};
use sqlx::PgPool;

// ---------------------------------------------------------------------------
// Eingabe-Strukturen
// ---------------------------------------------------------------------------

/// Eingabe für [`ArrivalTrackingStore::record_arrival`].
///
/// Port von `PartnerArrivalTrackingService.store_partner_raid_arrival`
/// (partner_arrival_tracking.py Z. 193–210).
///
/// `classification`, `correlation_status`, `source_resolution` und
/// `primary_signal` kommen als Parameter rein — die Algorithmus-Logik,
/// die sie berechnet, liegt separat (Signal-Korrelation, kommt in Schritt 6f).
#[derive(Debug, Clone)]
pub struct RecordArrivalInput {
    /// Broadcaster-ID der Quelle (kann unbekannt sein).
    pub from_broadcaster_id: Option<String>,
    /// Login-Name der Quelle (normalisiert).
    pub from_broadcaster_login: String,
    /// Broadcaster-ID des Ziels.
    pub to_broadcaster_id: String,
    /// Login-Name des Ziels.
    pub to_broadcaster_login: String,
    /// Viewer-Anzahl beim Raid.
    pub viewer_count: i32,
    /// Klassifikation, z. B. "partner_raid" — kommt von der Korrelations-Schicht.
    pub classification: String,
    /// Kommaseparierte Liste der Bestätigungs-Signale (bereits serialisiert).
    /// Python serialisiert via `serialize_confirmation_signals` → sorted, join.
    pub confirmation_signals: String,
    /// Primäres Erkennungs-Signal.
    pub primary_signal: String,
    /// Korrelations-Status, z. B. "confirmed" / "unconfirmed".
    pub correlation_status: String,
    /// Optionales Detail zum Korrelations-Status.
    pub correlation_detail: Option<String>,
    /// Herkunfts-Auflösung, z. B. "twitch_api" / "pending_raid".
    pub source_resolution: String,
    /// Optionale Referenz auf einen `twitch_raid_history`-Eintrag.
    pub raid_history_id: Option<i64>,
    /// Ausführungs-Zeitpunkt des referenzierten Raid-History-Eintrags.
    pub raid_history_executed_at: Option<DateTime<Utc>>,
    /// Ob direkt beim Eintragen schon ein Unraid beobachtet wurde.
    pub unraid_seen: bool,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

/// Schreib-Zugriff auf `twitch_raid_arrival_tracking`.
#[derive(Clone)]
pub struct ArrivalTrackingStore {
    pool: PgPool,
}

impl ArrivalTrackingStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Schreibt einen neuen Arrival-Eintrag und gibt die neue `id` zurück.
    ///
    /// `detected_at` und `last_signal_at` werden serverseitig auf `NOW()` gesetzt.
    /// `last_unraid_at` wird auf `NOW()` gesetzt wenn `unraid_seen = true`,
    /// sonst `NULL` — exakt wie Python Z. 250.
    ///
    /// Port von `PartnerArrivalTrackingService.store_partner_raid_arrival`
    /// (partner_arrival_tracking.py Z. 193–255).
    pub async fn record_arrival(&self, input: &RecordArrivalInput) -> Result<i64, sqlx::Error> {
        // Prod-Spalte `id` ist int4 (SERIAL). Ohne den expliziten `::bigint`-Cast
        // lehnt sqlx das Tupel-Decode in `(i64,)` strikt ab → jeder Insert schlägt fehl.
        let row: (i64,) = sqlx::query_as(
            r#"
            INSERT INTO twitch_raid_arrival_tracking (
                from_broadcaster_id,
                from_broadcaster_login,
                to_broadcaster_id,
                to_broadcaster_login,
                viewer_count,
                classification,
                confirmation_signals,
                primary_signal,
                correlation_status,
                correlation_detail,
                source_resolution,
                raid_history_id,
                raid_history_executed_at,
                unraid_seen,
                last_unraid_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                CASE WHEN $14 THEN NOW() ELSE NULL END
            )
            RETURNING id::bigint
            "#,
        )
        .bind(&input.from_broadcaster_id)
        .bind(&input.from_broadcaster_login)
        .bind(&input.to_broadcaster_id)
        .bind(&input.to_broadcaster_login)
        .bind(input.viewer_count)
        .bind(&input.classification)
        .bind(&input.confirmation_signals)
        .bind(&input.primary_signal)
        .bind(&input.correlation_status)
        .bind(&input.correlation_detail)
        .bind(&input.source_resolution)
        .bind(input.raid_history_id)
        .bind(input.raid_history_executed_at)
        .bind(input.unraid_seen)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0)
    }

    /// Aktualisiert `confirmation_signals`, `last_signal_at` und optional `unraid_seen` / `last_unraid_at`.
    ///
    /// `unraid_seen` wird nur auf `TRUE` gesetzt, nie zurückgesetzt (CASE WHEN).
    /// `last_unraid_at` wird auf `NOW()` gesetzt wenn `unraid_seen = true`,
    /// sonst unverändert — exakt wie Python Z. 279–286.
    ///
    /// Port von `PartnerArrivalTrackingService.update_partner_raid_arrival`
    /// (partner_arrival_tracking.py Z. 265–292).
    pub async fn update_arrival(
        &self,
        arrival_tracking_id: i64,
        confirmation_signals: &str,
        unraid_seen: bool,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE twitch_raid_arrival_tracking
            SET confirmation_signals = $1,
                last_signal_at       = NOW(),
                unraid_seen          = CASE WHEN $2 THEN TRUE ELSE unraid_seen END,
                last_unraid_at       = CASE WHEN $2 THEN NOW() ELSE last_unraid_at END
            WHERE id = $3
            "#,
        )
        .bind(confirmation_signals)
        .bind(unraid_seen)
        .bind(arrival_tracking_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Markiert einen Arrival-Eintrag als "Unraid gesehen":
    /// setzt `unraid_seen = TRUE` und `last_unraid_at = NOW()`.
    ///
    /// Wird separat von `update_arrival` angeboten, da ein reines Unraid-Signal
    /// keine neuen `confirmation_signals` mitbringt.
    pub async fn mark_unraid(&self, arrival_tracking_id: i64) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE twitch_raid_arrival_tracking
            SET unraid_seen    = TRUE,
                last_unraid_at = NOW(),
                last_signal_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(arrival_tracking_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Findet die jüngste Arrival-Zeile für ein (Ziel, Quelle)-Paar innerhalb
    /// des Recent-Fensters und gibt `(id, confirmation_signals)` zurück.
    ///
    /// DB-Pendant zu Pythons In-Memory-Cache `lookup_recent_raid_arrival`
    /// (`raid_state_store.py:137-155`, TTL `recent_raid_arrival_ttl_seconds
    /// = 600`): Sekundär-Signale aktualisieren nur Arrivals, die jünger als
    /// das Fenster sind — ältere gelten als eigenständige Vorgänge.
    pub async fn find_recent_arrival(
        &self,
        to_broadcaster_id: &str,
        from_broadcaster_login: &str,
        max_age_seconds: i64,
    ) -> Result<Option<(i64, String)>, sqlx::Error> {
        let row: Option<(i64, Option<String>)> = sqlx::query_as(
            r#"
            SELECT id::bigint, confirmation_signals
            FROM twitch_raid_arrival_tracking
            WHERE to_broadcaster_id = $1
              AND LOWER(from_broadcaster_login) = LOWER($2)
              AND detected_at > NOW() - ($3 * INTERVAL '1 second')
            ORDER BY detected_at DESC
            LIMIT 1
            "#,
        )
        .bind(to_broadcaster_id)
        .bind(from_broadcaster_login)
        .bind(max_age_seconds)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(id, signals)| (id, signals.unwrap_or_default())))
    }
}

/// Serialisiert Bestätigungs-Signale wie Python
/// `serialize_confirmation_signals` (`partner_arrival_tracking.py:390-393`):
/// trim, dedupe, sortiert, kommasepariert.
pub fn serialize_confirmation_signals<I, S>(signals: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let set: std::collections::BTreeSet<String> = signals
        .into_iter()
        .map(|s| s.as_ref().trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    set.into_iter().collect::<Vec<_>>().join(",")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Legt ein frisches Schema + Tabelle an; gibt einen Pool zurück, der
    /// `search_path` auf dieses Schema setzt.
    ///
    /// Hermetisch: jeder Test bekommt ein eigenes Schema, kein geteilter Zustand.
    async fn setup_db(schema: &str) -> PgPool {
        let url = std::env::var("TB_TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:tbtest@127.0.0.1:5434/postgres".to_string());

        let admin = sqlx::PgPool::connect(&url)
            .await
            .expect("Test-DB-Verbindung fehlgeschlagen");

        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();

        let pool = sqlx::PgPool::connect(&format!("{url}?options=-c%20search_path%3D{schema}"))
            .await
            .expect("Pool mit Schema fehlgeschlagen");

        sqlx::query(
            r#"
            CREATE TABLE twitch_raid_arrival_tracking (
                id                        SERIAL PRIMARY KEY,
                detected_at               TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                last_signal_at            TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                from_broadcaster_id       TEXT,
                from_broadcaster_login    TEXT NOT NULL,
                to_broadcaster_id         TEXT NOT NULL,
                to_broadcaster_login      TEXT NOT NULL,
                viewer_count              INTEGER NOT NULL DEFAULT 0,
                classification            TEXT NOT NULL DEFAULT '',
                confirmation_signals      TEXT NOT NULL DEFAULT '',
                primary_signal            TEXT NOT NULL DEFAULT '',
                correlation_status        TEXT NOT NULL DEFAULT '',
                correlation_detail        TEXT,
                source_resolution         TEXT NOT NULL DEFAULT '',
                raid_history_id           BIGINT,
                raid_history_executed_at  TIMESTAMPTZ,
                unraid_seen               BOOLEAN NOT NULL DEFAULT FALSE,
                last_unraid_at            TIMESTAMPTZ
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        pool
    }

    fn sample_input() -> RecordArrivalInput {
        RecordArrivalInput {
            from_broadcaster_id: Some("from_001".to_string()),
            from_broadcaster_login: "streamer_a".to_string(),
            to_broadcaster_id: "to_001".to_string(),
            to_broadcaster_login: "streamer_b".to_string(),
            viewer_count: 42,
            classification: "partner_raid".to_string(),
            confirmation_signals: "chat_raid,viewer_wave".to_string(),
            primary_signal: "chat_raid".to_string(),
            correlation_status: "confirmed".to_string(),
            correlation_detail: None,
            source_resolution: "pending_raid".to_string(),
            raid_history_id: None,
            raid_history_executed_at: None,
            unraid_seen: false,
        }
    }

    #[tokio::test]
    async fn record_arrival_gibt_id_zurueck() {
        let pool = setup_db("ats_record").await;
        let store = ArrivalTrackingStore::new(pool);
        let id = store.record_arrival(&sample_input()).await.unwrap();
        assert!(id > 0, "id muss positiv sein, war: {id}");
    }

    #[tokio::test]
    async fn record_arrival_schreibt_felder_korrekt() {
        let pool = setup_db("ats_fields").await;
        let store = ArrivalTrackingStore::new(pool.clone());
        let id = store.record_arrival(&sample_input()).await.unwrap();

        let row: (String, String, i32, String, bool) = sqlx::query_as(
            "SELECT from_broadcaster_login, classification, viewer_count, correlation_status, unraid_seen
             FROM twitch_raid_arrival_tracking WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(row.0, "streamer_a");
        assert_eq!(row.1, "partner_raid");
        assert_eq!(row.2, 42);
        assert_eq!(row.3, "confirmed");
        assert!(!row.4);
    }

    #[tokio::test]
    async fn record_arrival_unraid_seen_setzt_last_unraid_at() {
        let pool = setup_db("ats_unraid_insert").await;
        let store = ArrivalTrackingStore::new(pool.clone());
        let mut input = sample_input();
        input.unraid_seen = true;
        let id = store.record_arrival(&input).await.unwrap();

        let row: (bool, Option<DateTime<Utc>>) = sqlx::query_as(
            "SELECT unraid_seen, last_unraid_at FROM twitch_raid_arrival_tracking WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert!(row.0, "unraid_seen muss TRUE sein");
        assert!(
            row.1.is_some(),
            "last_unraid_at muss gesetzt sein wenn unraid_seen=true"
        );
    }

    #[tokio::test]
    async fn record_arrival_unraid_false_last_unraid_at_null() {
        let pool = setup_db("ats_unraid_null").await;
        let store = ArrivalTrackingStore::new(pool.clone());
        let id = store.record_arrival(&sample_input()).await.unwrap();

        let row: (bool, Option<DateTime<Utc>>) = sqlx::query_as(
            "SELECT unraid_seen, last_unraid_at FROM twitch_raid_arrival_tracking WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert!(!row.0);
        assert!(
            row.1.is_none(),
            "last_unraid_at muss NULL sein wenn unraid_seen=false"
        );
    }

    #[tokio::test]
    async fn update_arrival_aktualisiert_signals() {
        let pool = setup_db("ats_update").await;
        let store = ArrivalTrackingStore::new(pool.clone());
        let id = store.record_arrival(&sample_input()).await.unwrap();

        store
            .update_arrival(id, "chat_raid,channel_points,viewer_wave", false)
            .await
            .unwrap();

        let row: (String, bool) = sqlx::query_as(
            "SELECT confirmation_signals, unraid_seen FROM twitch_raid_arrival_tracking WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(row.0, "chat_raid,channel_points,viewer_wave");
        assert!(!row.1);
    }

    #[tokio::test]
    async fn update_arrival_setzt_unraid_seen_nicht_zurueck() {
        let pool = setup_db("ats_unraid_no_reset").await;
        let store = ArrivalTrackingStore::new(pool.clone());
        let mut input = sample_input();
        input.unraid_seen = true;
        let id = store.record_arrival(&input).await.unwrap();

        // Update mit unraid_seen=false darf unraid_seen NICHT zurücksetzen
        store.update_arrival(id, "chat_raid", false).await.unwrap();

        let row: (bool,) =
            sqlx::query_as("SELECT unraid_seen FROM twitch_raid_arrival_tracking WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();

        assert!(
            row.0,
            "unraid_seen darf nach Update nicht auf FALSE zurückgesetzt werden"
        );
    }

    #[tokio::test]
    async fn mark_unraid_setzt_felder() {
        let pool = setup_db("ats_mark_unraid").await;
        let store = ArrivalTrackingStore::new(pool.clone());
        let id = store.record_arrival(&sample_input()).await.unwrap();

        store.mark_unraid(id).await.unwrap();

        let row: (bool, Option<DateTime<Utc>>) = sqlx::query_as(
            "SELECT unraid_seen, last_unraid_at FROM twitch_raid_arrival_tracking WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert!(row.0, "unraid_seen muss TRUE sein nach mark_unraid");
        assert!(
            row.1.is_some(),
            "last_unraid_at muss gesetzt sein nach mark_unraid"
        );
    }

    #[tokio::test]
    async fn record_arrival_ohne_from_broadcaster_id() {
        let pool = setup_db("ats_null_from_id").await;
        let store = ArrivalTrackingStore::new(pool.clone());
        let mut input = sample_input();
        input.from_broadcaster_id = None;
        let id = store.record_arrival(&input).await.unwrap();

        let row: (Option<String>,) = sqlx::query_as(
            "SELECT from_broadcaster_id FROM twitch_raid_arrival_tracking WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert!(row.0.is_none(), "from_broadcaster_id muss NULL sein");
    }
}
