//! Report-Dispatcher (Port von
//! `bot/social_media/analytics/report_dispatcher.py`).
//!
//! Generiert periodisch den wöchentlichen Admin-Report und merkt sich per
//! Setting, für welchen Zeitraum er schon erzeugt wurde (Idempotenz). Der
//! **Discord-DM-Versand an den Admin** (Admin-User-Auflösung, Message-Chunking,
//! `send`) ist **B10 (Discord-DMs, von Nani ausgeschlossen)** und nicht
//! portiert — der Report wird dennoch erzeugt und in `social_media_reports`
//! persistiert (im Dashboard sichtbar). An/Aus 1:1: dauerhaft an, Intervall 6h.

use std::time::Duration;

use chrono::{DateTime, Datelike, Duration as ChronoDuration, Utc};
use serde_json::Value;
use sqlx::PgPool;

use crate::report_writer::SocialMediaReportWriter;
use crate::settings::{get_setting, set_setting};

const KEY_ADMIN_WEEKLY_REPORT_SENT: &str = "admin_weekly_report_last_sent_period_end";
const INTERVAL_SECS: u64 = 6 * 60 * 60;
const INITIAL_DELAY_SECS: u64 = 120;

/// Montag-0-Uhr-Anker der aktuellen Woche (Python `_weekly_anchor`).
fn weekly_anchor(now: DateTime<Utc>) -> DateTime<Utc> {
    let days = now.weekday().num_days_from_monday() as i64;
    let monday = (now - ChronoDuration::days(days)).date_naive();
    monday.and_hms_opt(0, 0, 0).unwrap().and_utc()
}

/// Dispatcher für den wöchentlichen Admin-Report.
pub struct ReportDispatcher {
    pool: PgPool,
    writer: SocialMediaReportWriter,
    interval: Duration,
}

impl ReportDispatcher {
    pub fn new(pool: PgPool) -> Self {
        Self {
            writer: SocialMediaReportWriter::new(pool.clone()),
            pool,
            interval: Duration::from_secs(INTERVAL_SECS),
        }
    }

    /// Erzeugt den Wochenreport für die aktuelle Woche (sofern noch nicht
    /// geschehen). Liefert `true`, wenn diesmal generiert wurde.
    pub async fn dispatch_weekly_admin_report(&self) -> bool {
        let period_end = weekly_anchor(Utc::now());
        let period_start = period_end - ChronoDuration::days(7);
        let marker = Value::String(period_end.to_rfc3339());
        if get_setting(&self.pool, KEY_ADMIN_WEEKLY_REPORT_SENT)
            .await
            .as_ref()
            == Some(&marker)
        {
            return false;
        }

        // B10: Discord-DM an den Admin entfällt. Der Report wird trotzdem
        // generiert + persistiert (über das Dashboard abrufbar).
        if let Err(error) = self
            .writer
            .write_admin_weekly_report(Some(period_start), Some(period_end), false)
            .await
        {
            tracing::warn!(%error, "Social-Media-Report: Wochenreport konnte nicht geschrieben werden");
            return false;
        }
        if let Err(error) = set_setting(
            &self.pool,
            KEY_ADMIN_WEEKLY_REPORT_SENT,
            &marker,
            Some("social_media_report_dispatcher"),
        )
        .await
        {
            tracing::warn!(
                %error,
                marker = ?marker,
                "Social-Media-Report: Sent-Marker konnte nicht gespeichert werden"
            );
        }
        true
    }

    /// Hintergrund-Loop (120s Initial-Delay + 6h-Intervall). Noch nicht in
    /// tb-bot gespawnt (Wiring = Cutover-Slice).
    pub async fn run(&self) {
        tokio::time::sleep(Duration::from_secs(INITIAL_DELAY_SECS)).await;
        loop {
            self.dispatch_weekly_admin_report().await;
            tokio::time::sleep(self.interval).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    #[test]
    fn weekly_anchor_ist_montag_mitternacht() {
        // Mittwoch 2026-06-17 15:30 → Montag 2026-06-15 00:00.
        let now = DateTime::parse_from_rfc3339("2026-06-17T15:30:00+00:00")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            weekly_anchor(now),
            DateTime::parse_from_rfc3339("2026-06-15T00:00:00+00:00").unwrap()
        );
        // Montag bleibt Montag.
        let mon = DateTime::parse_from_rfc3339("2026-06-15T08:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            weekly_anchor(mon),
            DateTime::parse_from_rfc3339("2026-06-15T00:00:00+00:00").unwrap()
        );
    }

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
        for ddl in [
            "CREATE TABLE social_media_settings (key TEXT PRIMARY KEY, value JSONB, updated_at TIMESTAMPTZ, updated_by TEXT)",
            "CREATE TABLE social_media_reports (id SERIAL PRIMARY KEY, kind TEXT NOT NULL, streamer_login TEXT, period_start TIMESTAMPTZ NOT NULL, period_end TIMESTAMPTZ NOT NULL, content_md TEXT NOT NULL, model TEXT, created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP)",
            "CREATE TABLE twitch_clips_social_media (id SERIAL PRIMARY KEY, streamer_login TEXT, clip_title TEXT, clip_url TEXT, created_at TIMESTAMPTZ DEFAULT NOW(), game_name TEXT, discarded_at TIMESTAMPTZ)",
            "CREATE TABLE twitch_clips_social_analytics (id SERIAL PRIMARY KEY, clip_id INTEGER, platform TEXT, bucket TEXT, views INTEGER, likes INTEGER, comments INTEGER, shares INTEGER, watch_time_seconds INTEGER, ctr_percent NUMERIC(5,2), engagement_rate NUMERIC(5,2), provider TEXT, synced_at TIMESTAMPTZ)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn dispatch_generiert_einmal_pro_woche() {
        let Some(pool) = make_pool("t_sm_report_dispatch").await else {
            return;
        };
        std::env::set_var("OLLAMA_HOST", "127.0.0.1:59999"); // LLM → Fallback
        let dispatcher = ReportDispatcher::new(pool.clone());

        // Erster Lauf generiert den Admin-Report + setzt den Marker.
        assert!(dispatcher.dispatch_weekly_admin_report().await);
        let n: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM social_media_reports WHERE kind = 'admin'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(n, 1);
        let marker: Option<String> =
            sqlx::query_scalar("SELECT value::text FROM social_media_settings WHERE key = $1")
                .bind(KEY_ADMIN_WEEKLY_REPORT_SENT)
                .fetch_optional(&pool)
                .await
                .unwrap()
                .flatten();
        assert!(marker.is_some());

        // Zweiter Lauf in derselben Woche → kein neuer Report (Idempotenz).
        assert!(!dispatcher.dispatch_weekly_admin_report().await);
        let n2: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM social_media_reports WHERE kind = 'admin'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(n2, 1);
    }
}
