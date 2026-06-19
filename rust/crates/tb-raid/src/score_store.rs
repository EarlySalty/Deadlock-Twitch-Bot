//! DB-Store für `twitch_partner_raid_scores`.
//!
//! Port der Lese- und Schreibmethoden aus `PartnerRaidScoreService` in
//! `bot/raid/partner_scores.py`. Nur DB-Zugriff — die reine Score-Berechnung
//! liegt in [`crate::scoring`].
//!
//! Schema-Vertrag (Prod, read-only verifiziert):
//!
//! | Spalte                        | Typ                | Rust-Typ            |
//! |-------------------------------|--------------------|---------------------|
//! | twitch_user_id                | TEXT PK            | String              |
//! | twitch_login                  | TEXT               | String              |
//! | avg_duration_sec              | INTEGER            | i32                 |
//! | time_pattern_score_base       | DOUBLE PRECISION   | f64                 |
//! | received_successful_raids_total | INTEGER          | i32                 |
//! | is_new_partner_preferred      | INTEGER (0/1)      | i32                 |
//! | new_partner_multiplier        | DOUBLE PRECISION   | f64                 |
//! | raid_boost_multiplier         | DOUBLE PRECISION   | f64                 |
//! | is_live                       | INTEGER (0/1)      | i32                 |
//! | current_started_at            | TEXT               | Option\<String\>    |
//! | current_uptime_sec            | INTEGER            | i32                 |
//! | duration_score                | DOUBLE PRECISION   | f64                 |
//! | time_pattern_score            | DOUBLE PRECISION   | f64                 |
//! | base_score                    | DOUBLE PRECISION   | f64                 |
//! | final_score                   | DOUBLE PRECISION   | f64                 |
//! | today_received_raids          | INTEGER            | i32                 |
//! | last_computed_at              | TEXT               | String              |
//! | readiness_score               | DOUBLE PRECISION   | f64                 |
//! | fairness_score                | DOUBLE PRECISION   | f64                 |
//! | internal_sent_raids_30d       | INTEGER            | i32                 |
//! | internal_received_raids_7d    | INTEGER            | i32                 |
//! | internal_received_raids_30d   | INTEGER            | i32                 |
//!
//! Flags (`is_live`, `is_new_partner_preferred`) sind **INTEGER**, nicht BOOLEAN.
//! Timestamps (`current_started_at`, `last_computed_at`) sind **TEXT**.

use sqlx::PgPool;

/// Eine Zeile aus `twitch_partner_raid_scores` (prod-verifizierte Typen).
///
/// Flags als i32 (0 oder 1), Timestamps als TEXT — exakt das Prod-Schema.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PartnerRaidScoreRow {
    pub twitch_user_id: String,
    pub twitch_login: String,
    pub avg_duration_sec: i32,
    pub time_pattern_score_base: f64,
    pub received_successful_raids_total: i32,
    /// INTEGER-Flag (0 oder 1), kein BOOLEAN.
    pub is_new_partner_preferred: i32,
    pub new_partner_multiplier: f64,
    pub raid_boost_multiplier: f64,
    /// INTEGER-Flag (0 oder 1), kein BOOLEAN.
    pub is_live: i32,
    /// TEXT-Timestamp (ISO-8601), nullable.
    pub current_started_at: Option<String>,
    pub current_uptime_sec: i32,
    pub duration_score: f64,
    pub time_pattern_score: f64,
    pub base_score: f64,
    pub final_score: f64,
    pub today_received_raids: i32,
    /// TEXT-Timestamp (ISO-8601), immer gesetzt.
    pub last_computed_at: String,
    pub readiness_score: f64,
    pub fairness_score: f64,
    pub internal_sent_raids_30d: i32,
    pub internal_received_raids_7d: i32,
    pub internal_received_raids_30d: i32,
}

/// Schreibeingabe für `upsert` — alle Felder einer vollständig berechneten Zeile.
///
/// Entspricht `_PreparedScore.as_db_tuple()` in `partner_scores.py`.
#[derive(Debug, Clone)]
pub struct PartnerRaidScoreUpsert {
    pub twitch_user_id: String,
    pub twitch_login: String,
    pub avg_duration_sec: i32,
    pub time_pattern_score_base: f64,
    pub received_successful_raids_total: i32,
    /// INTEGER-Flag (0 oder 1).
    pub is_new_partner_preferred: i32,
    pub new_partner_multiplier: f64,
    pub raid_boost_multiplier: f64,
    /// INTEGER-Flag (0 oder 1).
    pub is_live: i32,
    pub current_started_at: Option<String>,
    pub current_uptime_sec: i32,
    pub duration_score: f64,
    pub time_pattern_score: f64,
    pub readiness_score: f64,
    pub fairness_score: f64,
    pub base_score: f64,
    pub final_score: f64,
    pub internal_sent_raids_30d: i32,
    pub internal_received_raids_30d: i32,
    pub internal_received_raids_7d: i32,
    pub today_received_raids: i32,
    pub last_computed_at: String,
}

/// Lesezugriff auf `twitch_partner_raid_scores`.
#[derive(Clone)]
pub struct ScoreStore {
    pool: PgPool,
}

impl ScoreStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Lädt eine einzelne Score-Zeile nach `twitch_user_id`.
    ///
    /// Port von `PartnerRaidScoreService._load_cached_rows_by_id` +
    /// `_load_cached_rows` in `partner_scores.py`.
    pub async fn load(
        &self,
        twitch_user_id: &str,
    ) -> Result<Option<PartnerRaidScoreRow>, sqlx::Error> {
        sqlx::query_as::<_, PartnerRaidScoreRow>(
            r#"
            SELECT twitch_user_id, twitch_login, avg_duration_sec,
                   time_pattern_score_base, received_successful_raids_total,
                   is_new_partner_preferred, new_partner_multiplier,
                   raid_boost_multiplier, is_live, current_started_at,
                   current_uptime_sec, duration_score, time_pattern_score,
                   base_score, final_score, today_received_raids,
                   last_computed_at, readiness_score, fairness_score,
                   internal_sent_raids_30d, internal_received_raids_7d,
                   internal_received_raids_30d
            FROM twitch_partner_raid_scores
            WHERE twitch_user_id = $1
            "#,
        )
        .bind(twitch_user_id)
        .fetch_optional(&self.pool)
        .await
    }

    /// Lädt alle Score-Zeilen einer Liste von `twitch_user_id`s.
    ///
    /// Port von `PartnerRaidScoreService._load_cached_rows` in `partner_scores.py`.
    pub async fn load_many(
        &self,
        twitch_user_ids: &[&str],
    ) -> Result<Vec<PartnerRaidScoreRow>, sqlx::Error> {
        if twitch_user_ids.is_empty() {
            return Ok(vec![]);
        }
        sqlx::query_as::<_, PartnerRaidScoreRow>(
            r#"
            SELECT twitch_user_id, twitch_login, avg_duration_sec,
                   time_pattern_score_base, received_successful_raids_total,
                   is_new_partner_preferred, new_partner_multiplier,
                   raid_boost_multiplier, is_live, current_started_at,
                   current_uptime_sec, duration_score, time_pattern_score,
                   base_score, final_score, today_received_raids,
                   last_computed_at, readiness_score, fairness_score,
                   internal_sent_raids_30d, internal_received_raids_7d,
                   internal_received_raids_30d
            FROM twitch_partner_raid_scores
            WHERE twitch_user_id = ANY($1)
            "#,
        )
        .bind(twitch_user_ids)
        .fetch_all(&self.pool)
        .await
    }

    /// Lädt nur live Partner-Scores (is_live = 1).
    ///
    /// Port von `_load_cached_rows(..., live_only=True)` in `partner_scores.py`.
    pub async fn load_many_live_only(
        &self,
        twitch_user_ids: &[&str],
    ) -> Result<Vec<PartnerRaidScoreRow>, sqlx::Error> {
        if twitch_user_ids.is_empty() {
            return Ok(vec![]);
        }
        sqlx::query_as::<_, PartnerRaidScoreRow>(
            r#"
            SELECT twitch_user_id, twitch_login, avg_duration_sec,
                   time_pattern_score_base, received_successful_raids_total,
                   is_new_partner_preferred, new_partner_multiplier,
                   raid_boost_multiplier, is_live, current_started_at,
                   current_uptime_sec, duration_score, time_pattern_score,
                   base_score, final_score, today_received_raids,
                   last_computed_at, readiness_score, fairness_score,
                   internal_sent_raids_30d, internal_received_raids_7d,
                   internal_received_raids_30d
            FROM twitch_partner_raid_scores
            WHERE twitch_user_id = ANY($1) AND COALESCE(is_live, 0) = 1
            "#,
        )
        .bind(twitch_user_ids)
        .fetch_all(&self.pool)
        .await
    }

    /// Schreibt oder überschreibt eine Score-Zeile (Upsert auf `twitch_user_id`).
    ///
    /// Port von `PartnerRaidScoreService._upsert_scores` in `partner_scores.py`
    /// (INSERT … ON CONFLICT DO UPDATE, Z. 804–853).
    pub async fn upsert(&self, row: &PartnerRaidScoreUpsert) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO twitch_partner_raid_scores (
                twitch_user_id,
                twitch_login,
                avg_duration_sec,
                time_pattern_score_base,
                received_successful_raids_total,
                is_new_partner_preferred,
                new_partner_multiplier,
                raid_boost_multiplier,
                is_live,
                current_started_at,
                current_uptime_sec,
                duration_score,
                time_pattern_score,
                readiness_score,
                fairness_score,
                base_score,
                final_score,
                internal_sent_raids_30d,
                internal_received_raids_30d,
                internal_received_raids_7d,
                today_received_raids,
                last_computed_at
            ) VALUES (
                $1,  $2,  $3,  $4,  $5,
                $6,  $7,  $8,  $9,  $10,
                $11, $12, $13, $14, $15,
                $16, $17, $18, $19, $20,
                $21, $22
            )
            ON CONFLICT (twitch_user_id) DO UPDATE SET
                twitch_login                    = EXCLUDED.twitch_login,
                avg_duration_sec                = EXCLUDED.avg_duration_sec,
                time_pattern_score_base         = EXCLUDED.time_pattern_score_base,
                received_successful_raids_total = EXCLUDED.received_successful_raids_total,
                is_new_partner_preferred        = EXCLUDED.is_new_partner_preferred,
                new_partner_multiplier          = EXCLUDED.new_partner_multiplier,
                raid_boost_multiplier           = EXCLUDED.raid_boost_multiplier,
                is_live                         = EXCLUDED.is_live,
                current_started_at              = EXCLUDED.current_started_at,
                current_uptime_sec              = EXCLUDED.current_uptime_sec,
                duration_score                  = EXCLUDED.duration_score,
                time_pattern_score              = EXCLUDED.time_pattern_score,
                readiness_score                 = EXCLUDED.readiness_score,
                fairness_score                  = EXCLUDED.fairness_score,
                base_score                      = EXCLUDED.base_score,
                final_score                     = EXCLUDED.final_score,
                internal_sent_raids_30d         = EXCLUDED.internal_sent_raids_30d,
                internal_received_raids_30d     = EXCLUDED.internal_received_raids_30d,
                internal_received_raids_7d      = EXCLUDED.internal_received_raids_7d,
                today_received_raids            = EXCLUDED.today_received_raids,
                last_computed_at                = EXCLUDED.last_computed_at
            "#,
        )
        .bind(&row.twitch_user_id)
        .bind(&row.twitch_login)
        .bind(row.avg_duration_sec)
        .bind(row.time_pattern_score_base)
        .bind(row.received_successful_raids_total)
        .bind(row.is_new_partner_preferred)
        .bind(row.new_partner_multiplier)
        .bind(row.raid_boost_multiplier)
        .bind(row.is_live)
        .bind(&row.current_started_at)
        .bind(row.current_uptime_sec)
        .bind(row.duration_score)
        .bind(row.time_pattern_score)
        .bind(row.readiness_score)
        .bind(row.fairness_score)
        .bind(row.base_score)
        .bind(row.final_score)
        .bind(row.internal_sent_raids_30d)
        .bind(row.internal_received_raids_30d)
        .bind(row.internal_received_raids_7d)
        .bind(row.today_received_raids)
        .bind(&row.last_computed_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Erstellt ein isoliertes PostgreSQL-Schema mit der Prod-DDL für
    /// `twitch_partner_raid_scores` und gibt einen Pool zurück, dessen
    /// `search_path` auf dieses Schema zeigt.
    ///
    /// Muster: schema-pro-Test (wie in `tb-monitoring/tests/hermetic.rs`).
    /// Parallele Tests kollidieren nicht, da jedes Schema einmalig benannt ist.
    async fn setup_db(schema: &str) -> sqlx::PgPool {
        let url = std::env::var("TB_TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:tbtest@127.0.0.1:5434/postgres".to_string());

        // Zunächst ohne search_path verbinden, um das Schema anzulegen.
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
        admin.close().await;

        // Neuer Pool mit search_path auf das frische Schema.
        let schema_owned = schema.to_string();
        let pool = sqlx::postgres::PgPoolOptions::new()
            .after_connect(move |conn, _| {
                let schema = schema_owned.clone();
                Box::pin(async move {
                    sqlx::query(&format!("SET search_path TO {schema}"))
                        .execute(conn)
                        .await?;
                    Ok(())
                })
            })
            .connect(&url)
            .await
            .expect("Pool mit search_path fehlgeschlagen");

        // Prod-DDL anlegen.
        sqlx::query(
            r#"
            CREATE TABLE twitch_partner_raid_scores (
                twitch_user_id                  TEXT PRIMARY KEY,
                twitch_login                    TEXT NOT NULL,
                avg_duration_sec                INTEGER NOT NULL DEFAULT 0,
                time_pattern_score_base         DOUBLE PRECISION NOT NULL DEFAULT 0.5,
                received_successful_raids_total INTEGER NOT NULL DEFAULT 0,
                is_new_partner_preferred        INTEGER NOT NULL DEFAULT 0,
                new_partner_multiplier          DOUBLE PRECISION NOT NULL DEFAULT 1.0,
                raid_boost_multiplier           DOUBLE PRECISION NOT NULL DEFAULT 1.0,
                is_live                         INTEGER NOT NULL DEFAULT 0,
                current_started_at              TEXT,
                current_uptime_sec              INTEGER NOT NULL DEFAULT 0,
                duration_score                  DOUBLE PRECISION NOT NULL DEFAULT 0.5,
                time_pattern_score              DOUBLE PRECISION NOT NULL DEFAULT 0.5,
                base_score                      DOUBLE PRECISION NOT NULL DEFAULT 0.5,
                final_score                     DOUBLE PRECISION NOT NULL DEFAULT 0.5,
                today_received_raids            INTEGER NOT NULL DEFAULT 0,
                last_computed_at                TEXT NOT NULL,
                readiness_score                 DOUBLE PRECISION NOT NULL DEFAULT 0.5,
                fairness_score                  DOUBLE PRECISION NOT NULL DEFAULT 0.5,
                internal_sent_raids_30d         INTEGER NOT NULL DEFAULT 0,
                internal_received_raids_7d      INTEGER NOT NULL DEFAULT 0,
                internal_received_raids_30d     INTEGER NOT NULL DEFAULT 0
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    fn sample_upsert(user_id: &str) -> PartnerRaidScoreUpsert {
        PartnerRaidScoreUpsert {
            twitch_user_id: user_id.to_string(),
            twitch_login: "teststreamer".to_string(),
            avg_duration_sec: 7200,
            time_pattern_score_base: 0.75,
            received_successful_raids_total: 5,
            is_new_partner_preferred: 1,
            new_partner_multiplier: 1.125,
            raid_boost_multiplier: 1.0,
            is_live: 1,
            current_started_at: Some("2026-06-09T18:00:00+00:00".to_string()),
            current_uptime_sec: 3600,
            duration_score: 0.5,
            time_pattern_score: 0.75,
            readiness_score: 0.6,
            fairness_score: 0.68,
            base_score: 0.628,
            final_score: 0.7065,
            internal_sent_raids_30d: 3,
            internal_received_raids_30d: 1,
            internal_received_raids_7d: 2,
            today_received_raids: 0,
            last_computed_at: "2026-06-09T20:00:00+00:00".to_string(),
        }
    }

    #[tokio::test]
    async fn upsert_und_load_roundtrip() {
        let pool = setup_db("sc6c_upsert_load").await;
        let store = ScoreStore::new(pool);
        let input = sample_upsert("uid_001");

        store.upsert(&input).await.unwrap();

        let loaded = store.load("uid_001").await.unwrap().unwrap();
        assert_eq!(loaded.twitch_user_id, "uid_001");
        assert_eq!(loaded.twitch_login, "teststreamer");
        assert_eq!(loaded.avg_duration_sec, 7200);
        assert!((loaded.time_pattern_score_base - 0.75).abs() < 1e-9);
        assert_eq!(loaded.received_successful_raids_total, 5);
        assert_eq!(loaded.is_new_partner_preferred, 1);
        assert!((loaded.new_partner_multiplier - 1.125).abs() < 1e-9);
        assert!((loaded.raid_boost_multiplier - 1.0).abs() < 1e-9);
        assert_eq!(loaded.is_live, 1);
        assert_eq!(
            loaded.current_started_at.as_deref(),
            Some("2026-06-09T18:00:00+00:00")
        );
        assert_eq!(loaded.current_uptime_sec, 3600);
        assert!((loaded.duration_score - 0.5).abs() < 1e-9);
        assert!((loaded.time_pattern_score - 0.75).abs() < 1e-9);
        assert!((loaded.readiness_score - 0.6).abs() < 1e-9);
        assert!((loaded.fairness_score - 0.68).abs() < 1e-9);
        assert!((loaded.base_score - 0.628).abs() < 1e-9);
        assert!((loaded.final_score - 0.7065).abs() < 1e-9);
        assert_eq!(loaded.today_received_raids, 0);
        assert_eq!(loaded.last_computed_at, "2026-06-09T20:00:00+00:00");
        assert_eq!(loaded.internal_sent_raids_30d, 3);
        assert_eq!(loaded.internal_received_raids_30d, 1);
        assert_eq!(loaded.internal_received_raids_7d, 2);
    }

    #[tokio::test]
    async fn upsert_ueberschreibt_vorhandene_zeile() {
        let pool = setup_db("sc6c_upsert_overwrite").await;
        let store = ScoreStore::new(pool);

        let v1 = sample_upsert("uid_002");
        store.upsert(&v1).await.unwrap();

        // Zweiter Upsert mit geänderten Werten.
        let v2 = PartnerRaidScoreUpsert {
            twitch_login: "updated_login".to_string(),
            final_score: 0.9,
            is_live: 0,
            current_started_at: None,
            last_computed_at: "2026-06-09T22:00:00+00:00".to_string(),
            ..sample_upsert("uid_002")
        };
        store.upsert(&v2).await.unwrap();

        let loaded = store.load("uid_002").await.unwrap().unwrap();
        assert_eq!(loaded.twitch_login, "updated_login");
        assert!((loaded.final_score - 0.9).abs() < 1e-9);
        assert_eq!(loaded.is_live, 0);
        assert_eq!(loaded.current_started_at, None);
        assert_eq!(loaded.last_computed_at, "2026-06-09T22:00:00+00:00");
    }

    #[tokio::test]
    async fn load_unbekannte_id_liefert_none() {
        let pool = setup_db("sc6c_load_none").await;
        let store = ScoreStore::new(pool);
        let result = store.load("does_not_exist").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn load_many_mehrere_zeilen() {
        let pool = setup_db("sc6c_load_many").await;
        let store = ScoreStore::new(pool);

        store.upsert(&sample_upsert("uid_m1")).await.unwrap();
        store.upsert(&sample_upsert("uid_m2")).await.unwrap();

        let rows = store
            .load_many(&["uid_m1", "uid_m2", "uid_missing"])
            .await
            .unwrap();
        assert_eq!(rows.len(), 2);
        let ids: Vec<&str> = rows.iter().map(|r| r.twitch_user_id.as_str()).collect();
        assert!(ids.contains(&"uid_m1"));
        assert!(ids.contains(&"uid_m2"));
    }

    #[tokio::test]
    async fn load_many_live_only_filtert_offline() {
        let pool = setup_db("sc6c_live_only").await;
        let store = ScoreStore::new(pool);

        // live (is_live=1)
        store.upsert(&sample_upsert("uid_live")).await.unwrap();

        // offline (is_live=0)
        let offline = PartnerRaidScoreUpsert {
            is_live: 0,
            ..sample_upsert("uid_offline")
        };
        store.upsert(&offline).await.unwrap();

        let live_rows = store
            .load_many_live_only(&["uid_live", "uid_offline"])
            .await
            .unwrap();
        assert_eq!(live_rows.len(), 1);
        assert_eq!(live_rows[0].twitch_user_id, "uid_live");
    }

    #[tokio::test]
    async fn load_many_leere_liste_liefert_leeren_vec() {
        let pool = setup_db("sc6c_empty_list").await;
        let store = ScoreStore::new(pool);
        let rows = store.load_many(&[]).await.unwrap();
        assert!(rows.is_empty());
    }
}
