//! Persistenz für die externe-Recruitment-Blacklist-Pipeline (B3 Arrival-Sinks).
//!
//! Faithful-Port von `bot/raid/raid_metrics_store.py` +
//! `bot/raid/services/raid_blacklist.py`. Schreibt gegen die bereits in Prod
//! bestehenden Tabellen (Python legt sie via `ensure_schema` an — hier KEINE
//! Schema-Anlage, nur Zugriff):
//!   - `twitch_confirmed_external_recruitment_raids`
//!   - `twitch_external_recruitment_blacklist_pending`
//!   - `twitch_external_bot_ban_check_pending`
//!
//! Der Store ist reine Datenzugriffsschicht; die Entscheidung, WANN persistiert
//! oder geplant wird, trifft der Arrival-Klassifikator (`arrival_confirmation`).

use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// Legacy-Schwelle für die alte „zu oft extern geraidet"-Blacklist.
/// Der Trust-Leiter läuft endlos; echte Ban-/Raid-Fehler-Blacklists bleiben
/// davon unberührt.
pub const EXTERNAL_RECRUITMENT_RAID_LIMIT: i64 = i64::MAX;

/// 48h Karenz, bevor ein wiederholt rekrutierendes Ziel tatsächlich auf die
/// Blacklist wandert. Python: `external_recruitment_blacklist_grace_seconds`.
pub const EXTERNAL_RECRUITMENT_BLACKLIST_GRACE_SECONDS: i64 = 172_800;

/// Eingabe für einen bestätigten externen Recruitment-Raid.
#[derive(Debug, Clone)]
pub struct ConfirmedExternalRecruitmentRaid {
    pub raid_flow_id: Option<String>,
    pub from_broadcaster_id: Option<String>,
    pub from_broadcaster_login: String,
    pub to_broadcaster_id: String,
    pub to_broadcaster_login: String,
    pub viewer_count: i32,
    pub confirmation_signal: Option<String>,
}

/// Fällige verzögerte Blacklist-Eintragung (`blacklist_after <= now`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueBlacklistPending {
    pub target_id: String,
    pub target_login: String,
    pub confirmed_raid_count: i32,
    pub threshold_reached_at: DateTime<Utc>,
}

/// Fälliger Bot-Ban-Check (`run_after <= now`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueBotBanCheck {
    pub target_id: String,
    pub target_login: String,
    pub source: String,
}

/// Aktion für die verzögerte Blacklist nach einem bestätigten externen
/// Recruitment-Raid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlacklistScheduleAction {
    /// Schwelle nicht erreicht → nichts tun.
    None,
    /// Ziel ist (wieder) Partner → vorhandenes Pending entfernen (Partner dürfen
    /// nie auf die Blacklist).
    Delete,
    /// Schwelle erreicht, kein Partner → verzögert einplanen.
    Schedule,
}

/// Entscheidet die Blacklist-Aktion nach einem bestätigten externen
/// Recruitment-Raid. Python: `maybe_schedule_external_recruitment_blacklist_pending`
/// (raid_blacklist.py:240) — unter der Schwelle passiert nichts; ein Partner-Ziel
/// führt zum Löschen eines evtl. vorhandenen Pendings; sonst wird eingeplant.
pub fn decide_blacklist_action(
    confirmed_raid_count: i64,
    target_is_partner: bool,
) -> BlacklistScheduleAction {
    if confirmed_raid_count < EXTERNAL_RECRUITMENT_RAID_LIMIT {
        BlacklistScheduleAction::None
    } else if target_is_partner {
        BlacklistScheduleAction::Delete
    } else {
        BlacklistScheduleAction::Schedule
    }
}

/// Datenzugriff für die externe-Recruitment-Blacklist-Pipeline.
pub struct ExternalRecruitmentStore {
    pool: PgPool,
}

impl ExternalRecruitmentStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Persistiert einen bestätigten externen Recruitment-Raid (idempotent über
    /// `raid_flow_id`) und liefert die Gesamtzahl bestätigter Raids auf dieses
    /// Ziel. Python: `record_confirmed_external_recruitment_raid` — INSERT +
    /// COUNT in einer Transaktion, damit der Count den Insert sieht.
    pub async fn record_confirmed_raid(
        &self,
        raid: &ConfirmedExternalRecruitmentRaid,
    ) -> Result<i64, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO twitch_confirmed_external_recruitment_raids (
                raid_flow_id,
                from_broadcaster_id,
                from_broadcaster_login,
                to_broadcaster_id,
                to_broadcaster_login,
                viewer_count,
                confirmation_signal
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (raid_flow_id) DO NOTHING
            "#,
        )
        .bind(raid.raid_flow_id.as_deref())
        .bind(raid.from_broadcaster_id.as_deref())
        .bind(&raid.from_broadcaster_login)
        .bind(&raid.to_broadcaster_id)
        .bind(&raid.to_broadcaster_login)
        .bind(raid.viewer_count)
        .bind(raid.confirmation_signal.as_deref())
        .execute(&mut *tx)
        .await?;

        let count: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*)
            FROM twitch_confirmed_external_recruitment_raids
            WHERE to_broadcaster_id = $1
            "#,
        )
        .bind(&raid.to_broadcaster_id)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(count.0)
    }

    /// Anzahl bestätigter externer Recruitment-Raids auf ein Ziel.
    pub async fn count_confirmed_raids(&self, to_broadcaster_id: &str) -> Result<i64, sqlx::Error> {
        let row: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*)
            FROM twitch_confirmed_external_recruitment_raids
            WHERE to_broadcaster_id = $1
            "#,
        )
        .bind(to_broadcaster_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    /// UPSERT der verzögerten Blacklist (`blacklist_after = now + grace`).
    /// `confirmed_raid_count` wird per `GREATEST` monoton hochgezogen. Python:
    /// `_schedule_external_recruitment_blacklist_pending`.
    pub async fn schedule_blacklist_pending(
        &self,
        target_id: &str,
        target_login: &str,
        confirmed_raid_count: i32,
        raid_flow_id: Option<&str>,
        grace_seconds: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO twitch_external_recruitment_blacklist_pending (
                target_id,
                target_login,
                confirmed_raid_count,
                threshold_reached_at,
                blacklist_after,
                last_raid_flow_id,
                updated_at
            )
            VALUES (
                $1,
                $2,
                $3,
                CURRENT_TIMESTAMP,
                CURRENT_TIMESTAMP + ($4::double precision * INTERVAL '1 second'),
                $5,
                CURRENT_TIMESTAMP
            )
            ON CONFLICT (target_id) DO UPDATE SET
                target_login = EXCLUDED.target_login,
                confirmed_raid_count = GREATEST(
                    twitch_external_recruitment_blacklist_pending.confirmed_raid_count,
                    EXCLUDED.confirmed_raid_count
                ),
                last_raid_flow_id = EXCLUDED.last_raid_flow_id,
                updated_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(target_id)
        .bind(target_login)
        .bind(confirmed_raid_count)
        .bind(grace_seconds)
        .bind(raid_flow_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Entfernt die verzögerte Blacklist (z. B. wenn das Ziel jetzt Partner ist).
    pub async fn delete_blacklist_pending(&self, target_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "DELETE FROM twitch_external_recruitment_blacklist_pending WHERE target_id = $1",
        )
        .bind(target_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Lädt alle fälligen verzögerten Blacklist-Einträge (`blacklist_after <= now`).
    pub async fn load_due_blacklist_pending(
        &self,
    ) -> Result<Vec<DueBlacklistPending>, sqlx::Error> {
        let rows: Vec<(String, String, i32, DateTime<Utc>)> = sqlx::query_as(
            r#"
            SELECT target_id, target_login, confirmed_raid_count, threshold_reached_at
            FROM twitch_external_recruitment_blacklist_pending
            WHERE blacklist_after <= NOW()
            ORDER BY blacklist_after ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(target_id, target_login, confirmed_raid_count, threshold_reached_at)| {
                    DueBlacklistPending {
                        target_id,
                        target_login,
                        confirmed_raid_count,
                        threshold_reached_at,
                    }
                },
            )
            .collect())
    }

    /// UPSERT eines verzögerten Bot-Ban-Checks (`run_after = now + delay`).
    /// Python: `_schedule_external_target_ban_check`.
    pub async fn schedule_bot_ban_check(
        &self,
        target_id: &str,
        target_login: &str,
        source: &str,
        delay_seconds: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO twitch_external_bot_ban_check_pending (
                target_id,
                target_login,
                source,
                run_after,
                updated_at
            )
            VALUES (
                $1,
                $2,
                $3,
                CURRENT_TIMESTAMP + ($4::double precision * INTERVAL '1 second'),
                CURRENT_TIMESTAMP
            )
            ON CONFLICT (target_id) DO UPDATE SET
                target_login = EXCLUDED.target_login,
                source = EXCLUDED.source,
                run_after = EXCLUDED.run_after,
                updated_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(target_id)
        .bind(target_login)
        .bind(source)
        .bind(delay_seconds)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Entfernt einen verzögerten Bot-Ban-Check.
    pub async fn delete_bot_ban_check(&self, target_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM twitch_external_bot_ban_check_pending WHERE target_id = $1")
            .bind(target_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Verschiebt einen Bot-Ban-Check (mind. 60s in die Zukunft, wie Python).
    pub async fn reschedule_bot_ban_check(
        &self,
        target_id: &str,
        delay_seconds: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE twitch_external_bot_ban_check_pending
            SET run_after = CURRENT_TIMESTAMP + ($1::double precision * INTERVAL '1 second'),
                updated_at = CURRENT_TIMESTAMP
            WHERE target_id = $2
            "#,
        )
        .bind(delay_seconds.max(60))
        .bind(target_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Lädt fällige Bot-Ban-Checks (`run_after <= now`, max. 25, wie Python).
    pub async fn load_due_bot_ban_checks(&self) -> Result<Vec<DueBotBanCheck>, sqlx::Error> {
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            r#"
            SELECT target_id, target_login, source
            FROM twitch_external_bot_ban_check_pending
            WHERE run_after <= NOW()
            ORDER BY run_after ASC
            LIMIT 25
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(target_id, target_login, source)| DueBotBanCheck {
                target_id,
                target_login,
                source,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frisches Schema + die 3 Prod-Tabellen (1:1 aus Pythons ensure_schema);
    /// jeder Test isoliert über eigenes Schema + search_path.
    async fn setup_db(schema: &str) -> PgPool {
        let url =
            std::env::var("TB_TEST_DATABASE_URL").expect("TB_TEST_DATABASE_URL muss gesetzt sein");
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
            CREATE TABLE twitch_confirmed_external_recruitment_raids (
                id                     BIGSERIAL PRIMARY KEY,
                raid_flow_id           TEXT UNIQUE,
                from_broadcaster_id    TEXT,
                from_broadcaster_login TEXT NOT NULL,
                to_broadcaster_id      TEXT NOT NULL,
                to_broadcaster_login   TEXT NOT NULL,
                viewer_count           INTEGER DEFAULT 0,
                confirmation_signal    TEXT,
                confirmed_at           TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE twitch_external_recruitment_blacklist_pending (
                target_id            TEXT PRIMARY KEY,
                target_login         TEXT NOT NULL,
                confirmed_raid_count INTEGER NOT NULL,
                threshold_reached_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
                blacklist_after      TIMESTAMPTZ NOT NULL,
                last_raid_flow_id    TEXT,
                updated_at           TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE twitch_external_bot_ban_check_pending (
                target_id    TEXT PRIMARY KEY,
                target_login TEXT NOT NULL,
                source       TEXT NOT NULL,
                run_after    TIMESTAMPTZ NOT NULL,
                scheduled_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at   TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    fn raid(flow: &str, target: &str) -> ConfirmedExternalRecruitmentRaid {
        ConfirmedExternalRecruitmentRaid {
            raid_flow_id: Some(flow.to_string()),
            from_broadcaster_id: Some("from_1".to_string()),
            from_broadcaster_login: "raider".to_string(),
            to_broadcaster_id: target.to_string(),
            to_broadcaster_login: "victim".to_string(),
            viewer_count: 12,
            confirmation_signal: Some("shoutout".to_string()),
        }
    }

    macro_rules! skip_without_db {
        () => {
            if std::env::var("TB_TEST_DATABASE_URL").is_err() {
                eprintln!(
                    "SKIP: TB_TEST_DATABASE_URL nicht gesetzt — `rust/scripts/test_db.sh up`"
                );
                return;
            }
        };
    }

    #[test]
    fn blacklist_action_threshold_and_partner() {
        // Die alte „zu oft geraidet"-Blacklist ist für die Trust-Leiter praktisch deaktiviert.
        assert_eq!(
            decide_blacklist_action(0, false),
            BlacklistScheduleAction::None
        );
        assert_eq!(
            decide_blacklist_action(3, false),
            BlacklistScheduleAction::None
        );
        assert_eq!(
            decide_blacklist_action(3, true),
            BlacklistScheduleAction::None
        );
        assert_eq!(
            decide_blacklist_action(4, false),
            BlacklistScheduleAction::None
        );
        assert_eq!(
            decide_blacklist_action(9, false),
            BlacklistScheduleAction::None
        );
        assert_eq!(
            decide_blacklist_action(50_000, false),
            BlacklistScheduleAction::None
        );
    }

    #[tokio::test]
    async fn record_is_idempotent_per_flow_and_counts_per_target() {
        skip_without_db!();
        let pool = setup_db("er_record").await;
        let store = ExternalRecruitmentStore::new(pool);

        // Gleicher flow_id → dedup, Count bleibt 1.
        assert_eq!(
            store
                .record_confirmed_raid(&raid("f1", "t1"))
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            store
                .record_confirmed_raid(&raid("f1", "t1"))
                .await
                .unwrap(),
            1
        );
        // Neuer flow_id, gleiches Ziel → Count 2.
        assert_eq!(
            store
                .record_confirmed_raid(&raid("f2", "t1"))
                .await
                .unwrap(),
            2
        );
        // Anderes Ziel → eigener Count.
        assert_eq!(
            store
                .record_confirmed_raid(&raid("f3", "t2"))
                .await
                .unwrap(),
            1
        );
        assert_eq!(store.count_confirmed_raids("t1").await.unwrap(), 2);
        assert_eq!(store.count_confirmed_raids("unknown").await.unwrap(), 0);
    }

    #[tokio::test]
    async fn blacklist_pending_upsert_due_and_delete() {
        skip_without_db!();
        let pool = setup_db("er_pending").await;
        let store = ExternalRecruitmentStore::new(pool);

        // Zukunft (grace 3600s) → nicht fällig.
        store
            .schedule_blacklist_pending("t1", "victim", 4, Some("f1"), 3600)
            .await
            .unwrap();
        assert!(store.load_due_blacklist_pending().await.unwrap().is_empty());

        // GREATEST: niedrigerer Count darf nicht herunterzählen.
        store
            .schedule_blacklist_pending("t1", "victim", 2, Some("f2"), 3600)
            .await
            .unwrap();

        // Vergangenheit (grace -10s) für ein zweites Ziel → fällig.
        store
            .schedule_blacklist_pending("t2", "other", 5, None, -10)
            .await
            .unwrap();
        let due = store.load_due_blacklist_pending().await.unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].target_id, "t2");
        assert_eq!(due[0].confirmed_raid_count, 5);

        store.delete_blacklist_pending("t2").await.unwrap();
        assert!(store.load_due_blacklist_pending().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn bot_ban_check_schedule_reschedule_due_delete() {
        skip_without_db!();
        let pool = setup_db("er_ban").await;
        let store = ExternalRecruitmentStore::new(pool);

        // Fällig (Vergangenheit).
        store
            .schedule_bot_ban_check("t1", "victim", "external_recruitment", -5)
            .await
            .unwrap();
        let due = store.load_due_bot_ban_checks().await.unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].source, "external_recruitment");

        // Reschedule in die Zukunft → nicht mehr fällig.
        store.reschedule_bot_ban_check("t1", 3600).await.unwrap();
        assert!(store.load_due_bot_ban_checks().await.unwrap().is_empty());

        store.delete_bot_ban_check("t1").await.unwrap();
        store.reschedule_bot_ban_check("t1", 3600).await.unwrap(); // No-op auf leerer Zeile
        assert!(store.load_due_bot_ban_checks().await.unwrap().is_empty());
    }
}
