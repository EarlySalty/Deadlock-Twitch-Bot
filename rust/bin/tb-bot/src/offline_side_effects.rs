//! Offline-Seiteneffekte außerhalb des Raid-Subsystems (Cutover-Kopplung 5
//! aus `04-cutover-plan.md`): beim `stream.offline`-Hook ausgeführt, damit
//! diese Trigger den Python-Monitoring-Abschied überleben.
//!
//! - **Engagement-Auto-Off**: Engagement-Layer wird an Stream-Leben gekoppelt
//!   (Python `bot/engagement/auto_off.py`, idempotentes UPDATE).
//! - **Global-Ban-Sweep**: plant den Offline-Sweep 1 h nach Stream-Ende
//!   (Python `storage/pg.py` `schedule_global_ban_sweep`); der Wartungs-Loop
//!   im Python-Worker arbeitet fällige Sweeps weiterhin ab.
//!
//! Bewusst NICHT portiert (laufen DB-getrieben im Python-Worker weiter):
//! Post-Stream-Analyse (Backfill-/Retry-Job) und Re-Auth-Reminder.

use sqlx::PgPool;

/// Sweep-Verzögerung nach Stream-Ende (Python: 3600 s).
const GLOBAL_BAN_SWEEP_DELAY_SECONDS: i32 = 3600;

pub struct OfflineSideEffects {
    pool: PgPool,
}

impl OfflineSideEffects {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Engagement-Auto-Off (Python `eventsub_mixin.py`:1861): koppelt den
    /// Engagement-Layer ans Stream-Leben. Läuft VOR dem Offline-Throttle, damit
    /// das Engagement auch bei einem gedrosselten Duplikat-Offline ausgeht.
    ///
    /// Case-sensitiv, KEIN Lowercasing — Python bindet `channel_login` roh ins
    /// `WHERE` (siehe tb_engagement::auto_off).
    pub async fn run_engagement_auto_off(&self, login: &str) {
        let login = login.trim();
        if login.is_empty() {
            return;
        }
        match tb_engagement::auto_off::auto_disable_on_offline(&self.pool, login).await {
            Ok(changed) if changed > 0 => {
                tracing::info!(streamer = %login, "Engagement auto-deaktiviert (Stream offline)");
            }
            Ok(_) => {}
            Err(error) => {
                tracing::error!(%error, streamer = %login, "Engagement-Auto-Off fehlgeschlagen");
            }
        }
    }

    /// Global-Ban-Sweep einplanen (Python `eventsub_mixin.py`:1908): läuft NACH
    /// dem bestandenen Throttle, VOR der State-Finalisierung. Lowercased Login
    /// (Python `login_lower`) als Schlüssel auf `broadcaster_login`.
    pub async fn run_global_ban_sweep(&self, broadcaster_id: &str, login: &str) {
        let login = login.trim();
        let broadcaster_id = broadcaster_id.trim();
        if login.is_empty() || broadcaster_id.is_empty() {
            return;
        }
        let login_lower = login.to_lowercase();
        if let Err(error) = self
            .schedule_global_ban_sweep(&login_lower, broadcaster_id)
            .await
        {
            tracing::error!(%error, streamer = %login_lower, "Global-Ban-Sweep nicht planbar");
        }
    }

    /// Python `schedule_global_ban_sweep`: UPSERT auf `broadcaster_login`.
    async fn schedule_global_ban_sweep(
        &self,
        login: &str,
        broadcaster_id: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO twitch_global_ban_sweep_due (broadcaster_login, broadcaster_id, run_after)
             VALUES ($1, $2, NOW() + ($3 * INTERVAL '1 second'))
             ON CONFLICT (broadcaster_login) DO UPDATE SET
                 broadcaster_id = EXCLUDED.broadcaster_id,
                 run_after      = EXCLUDED.run_after,
                 scheduled_at   = NOW()",
        )
        .bind(login)
        .bind(broadcaster_id)
        .bind(GLOBAL_BAN_SWEEP_DELAY_SECONDS)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(all(test, feature = "integration"))]
mod tests {
    use super::*;
    use std::str::FromStr;

    async fn setup(schema: &str) -> PgPool {
        let url = std::env::var("TB_TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:tbtest@127.0.0.1:5434/postgres".to_string());
        let admin = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
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
        let opts = sqlx::postgres::PgConnectOptions::from_str(&url)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE twitch_engagement_settings (
                channel_login TEXT PRIMARY KEY, enabled BOOLEAN DEFAULT FALSE,
                updated_at TIMESTAMPTZ DEFAULT NOW() )",
            "CREATE TABLE twitch_global_ban_sweep_due (
                broadcaster_login TEXT PRIMARY KEY, broadcaster_id TEXT,
                run_after TIMESTAMPTZ, scheduled_at TIMESTAMPTZ DEFAULT NOW() )",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        pool
    }

    #[tokio::test]
    async fn engagement_wird_deaktiviert_und_sweep_geplant() {
        let pool = setup("t6_offline_fx").await;
        // Engagement-Off ist case-sensitiv: der eingetragene Login muss exakt
        // dem an `run` übergebenen entsprechen.
        sqlx::query(
            "INSERT INTO twitch_engagement_settings (channel_login, enabled) VALUES ('Drag', TRUE)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let fx = OfflineSideEffects::new(pool.clone());
        fx.run_engagement_auto_off("Drag").await;
        fx.run_global_ban_sweep("42", "Drag").await;

        let enabled: bool = sqlx::query_scalar(
            "SELECT enabled FROM twitch_engagement_settings WHERE channel_login='Drag'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(!enabled, "Engagement nach offline deaktiviert");

        // Der Sweep-Schlüssel ist gelowercased (Python `login_lower`).
        let (bid, in_future): (String, bool) = sqlx::query_as(
            "SELECT broadcaster_id, run_after > NOW() + INTERVAL '50 minutes'
               FROM twitch_global_ban_sweep_due WHERE broadcaster_login='drag'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(bid, "42");
        assert!(in_future, "Sweep ~1h in der Zukunft");

        // Zweiter Offline-Trigger: UPSERT statt Fehler, Engagement bleibt aus.
        fx.run_engagement_auto_off("drag").await;
        fx.run_global_ban_sweep("42", "drag").await;
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_global_ban_sweep_due")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    /// Regression: Offline mit abweichender Groß-/Kleinschreibung deaktiviert
    /// Engagement NICHT (case-sensitiv), der exakte Login schon. Der
    /// Sweep-Schlüssel ist davon unabhängig immer gelowercased.
    #[tokio::test]
    async fn engagement_off_ist_case_sensitiv() {
        let pool = setup("t6_offline_fx_case").await;
        sqlx::query(
            "INSERT INTO twitch_engagement_settings (channel_login, enabled) VALUES ('MixedCase', TRUE)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let fx = OfflineSideEffects::new(pool.clone());

        // Abweichende Schreibweise → Engagement-Zeile bleibt aktiv.
        fx.run_engagement_auto_off("mixedcase").await;
        fx.run_global_ban_sweep("7", "mixedcase").await;
        let still_enabled: bool = sqlx::query_scalar(
            "SELECT enabled FROM twitch_engagement_settings WHERE channel_login='MixedCase'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(still_enabled, "gelowercaster Login trifft die exakte Zeile nicht");
        // Sweep wurde dennoch (gelowercased) geplant.
        let sweep_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM twitch_global_ban_sweep_due WHERE broadcaster_login='mixedcase')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(sweep_exists, "Sweep unter gelowercastem Login geplant");

        // Exakter Login → Engagement wird deaktiviert.
        fx.run_engagement_auto_off("MixedCase").await;
        let now_disabled: bool = sqlx::query_scalar(
            "SELECT NOT enabled FROM twitch_engagement_settings WHERE channel_login='MixedCase'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(now_disabled, "exakter Login deaktiviert Engagement");
    }
}
