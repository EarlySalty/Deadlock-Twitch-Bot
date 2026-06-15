//! Monetization- & Hype-Train-Übersicht (`/twitch/api/v2/monetization`).
//!
//! Port von `bot/analytics/insights_monetization_loader.py:load_monetization_payload`.
//! Aggregiert über das Zeitfenster: Ad-Breaks (Anzahl/auto/manuell/Dauer +
//! Viewer-Drop-Analyse je Ad), Hype-Trains, Bits und Subs.
//!
//! **1:1-Hinweis:** Pythons bits/subs-Queries filtern über eine direkte Spalte
//! `streamer_login`, die in keiner Schema-Variante (Migration, Legacy, Rust-Writer)
//! existiert — plus subs vergleicht `is_gift=1` gegen eine BOOLEAN-Spalte. Beide
//! Queries scheitern daher und liefern via try/except den Default 0. Das wird hier
//! wortgleich gespiegelt (catch-all → Default): heute 0, und falls die Spalte je
//! ergänzt wird, leuchten Python und Rust identisch auf. ads + hype_train filtern
//! dagegen über JOIN auf `twitch_stream_sessions.streamer_login` und funktionieren.
//!
//! **Teil 1: Aggregate (ads/hype/bits/subs) + leere Drop-Strukturen** (= Pythons
//! Output ohne Ad-Viewer-Daten). Das per-Ad-Viewer-Drop-Windowing folgt als Teil 2.

use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// Die immer im Output vorhandenen, von der Ad-Drop-Analyse abgeleiteten Felder.
struct DropAnalysis {
    avg_viewer_drop_pct: Value,
    worst_ads: Vec<Value>,
    position_impact: Value,
    duration_impact: Value,
    auto_vs_manual: Value,
    best_ad_time: Value,
    avg_recovery_min: Value,
    recovery_by_duration: Value,
    recommendations: Vec<String>,
}

const POSITION_BUCKETS: [&str; 4] = ["early_0_30m", "mid_30_60m", "late_60_90m", "endgame_90m"];
const DURATION_BUCKETS: [&str; 4] = ["30s", "60s", "90s", "120s_plus"];

fn empty_impact(buckets: &[&str], value_key: &str) -> Value {
    let mut m = serde_json::Map::new();
    for b in buckets {
        m.insert(b.to_string(), json!({ value_key: Value::Null, "count": 0 }));
    }
    Value::Object(m)
}

impl DropAnalysis {
    /// Leere Analyse — entspricht Pythons Output ohne Ad-Viewer-Daten.
    fn empty() -> Self {
        DropAnalysis {
            avg_viewer_drop_pct: Value::Null,
            worst_ads: Vec::new(),
            position_impact: empty_impact(&POSITION_BUCKETS, "avg_drop"),
            duration_impact: empty_impact(&DURATION_BUCKETS, "avg_drop"),
            auto_vs_manual: json!({
                "auto_avg_drop": Value::Null,
                "manual_avg_drop": Value::Null,
                "auto_count": 0,
                "manual_count": 0,
            }),
            best_ad_time: Value::Null,
            avg_recovery_min: Value::Null,
            recovery_by_duration: empty_impact(&DURATION_BUCKETS, "avg_recovery_min"),
            recommendations: Vec::new(),
        }
    }
}

/// Lädt die Monetization-Übersicht (Python `load_monetization_payload`).
/// `streamer` ist der bereits getrimmte/kleingeschriebene Query-Wert (`""` = ohne Filter).
pub async fn load_monetization_payload(
    pool: &PgPool,
    streamer: &str,
    days: i64,
) -> Result<Value, sqlx::Error> {
    let cutoff: DateTime<Utc> = Utc::now() - Duration::days(days);

    // --- Ad-Break-Aggregat (filtert über JOIN s.streamer_login → funktioniert). ---
    let (total_ads, auto_ads, avg_duration, sessions_with_ads): (i64, i64, Option<f64>, i64) =
        sqlx::query_as(
            "SELECT COUNT(*)::bigint, \
                    COALESCE(SUM(CASE WHEN a.is_automatic IS TRUE THEN 1 ELSE 0 END), 0)::bigint, \
                    AVG(a.duration_seconds)::float8, \
                    COUNT(DISTINCT a.session_id)::bigint \
               FROM twitch_ad_break_events a \
               LEFT JOIN twitch_stream_sessions s ON s.id = a.session_id \
              WHERE a.started_at >= $1 AND ($2 = '' OR LOWER(s.streamer_login) = $2)",
        )
        .bind(cutoff)
        .bind(streamer)
        .fetch_one(pool)
        .await?;

    let analysis = DropAnalysis::empty();

    let ads = json!({
        "total": total_ads,
        "auto": auto_ads,
        "manual": total_ads - auto_ads,
        "sessions_with_ads": sessions_with_ads,
        "avg_duration_s": round1(avg_duration.unwrap_or(0.0)),
        "avg_viewer_drop_pct": analysis.avg_viewer_drop_pct,
        "worst_ads": analysis.worst_ads,
        "position_impact": analysis.position_impact,
        "duration_impact": analysis.duration_impact,
        "auto_vs_manual": analysis.auto_vs_manual,
        "best_ad_time": analysis.best_ad_time,
        "avg_recovery_min": analysis.avg_recovery_min,
        "recovery_by_duration": analysis.recovery_by_duration,
        "recommendations": analysis.recommendations,
    });

    // --- Hype-Train (filtert über JOIN; catch-all → Default wie Pythons try/except). ---
    let hype_train = match sqlx::query_as::<_, (i64, Option<f64>, Option<i32>, Option<f64>)>(
        "SELECT COUNT(*)::bigint, AVG(h.level)::float8, MAX(h.level)::int, AVG(h.duration_seconds)::float8 \
           FROM twitch_hype_train_events h \
           LEFT JOIN twitch_stream_sessions s ON s.id = h.session_id \
          WHERE h.started_at >= $1 AND h.ended_at IS NOT NULL \
            AND ($2 = '' OR LOWER(s.streamer_login) = $2)",
    )
    .bind(cutoff)
    .bind(streamer)
    .fetch_one(pool)
    .await
    {
        Ok((total, avg_level, max_level, avg_dur)) => json!({
            "total": total,
            "avg_level": round1(avg_level.unwrap_or(0.0)),
            "max_level": max_level.unwrap_or(0),
            "avg_duration_s": avg_dur.unwrap_or(0.0).round(),
        }),
        Err(e) => {
            tracing::debug!("Hype train query failed: {e}");
            json!({ "total": 0, "avg_level": 0.0, "max_level": 0, "avg_duration_s": 0.0 })
        }
    };

    // --- Bits (Spalte streamer_login fehlt → Query scheitert → Default 0; 1:1 zu Python). ---
    let bits = match sqlx::query_as::<_, (Option<i64>, i64)>(
        "SELECT SUM(amount)::bigint, COUNT(*)::bigint \
           FROM twitch_bits_events \
          WHERE received_at >= $1 AND ($2 = '' OR LOWER(streamer_login) = $2)",
    )
    .bind(cutoff)
    .bind(streamer)
    .fetch_one(pool)
    .await
    {
        Ok((total, events)) => json!({ "total": total.unwrap_or(0), "cheer_events": events }),
        Err(e) => {
            tracing::debug!("Bits query failed: {e}");
            json!({ "total": 0, "cheer_events": 0 })
        }
    };

    // --- Subs (streamer_login fehlt + is_gift=1 gegen BOOLEAN → scheitert → Default 0; 1:1). ---
    let subs = match sqlx::query_as::<_, (i64, Option<i64>)>(
        "SELECT COUNT(*)::bigint, SUM(CASE WHEN is_gift=1 THEN 1 ELSE 0 END)::bigint \
           FROM twitch_subscription_events \
          WHERE received_at >= $1 AND ($2 = '' OR LOWER(streamer_login) = $2)",
    )
    .bind(cutoff)
    .bind(streamer)
    .fetch_one(pool)
    .await
    {
        Ok((total, gifted)) => json!({ "total_events": total, "gifted": gifted.unwrap_or(0) }),
        Err(e) => {
            tracing::debug!("Subs query failed: {e}");
            json!({ "total_events": 0, "gifted": 0 })
        }
    };

    Ok(json!({
        "ads": ads,
        "hype_train": hype_train,
        "bits": bits,
        "subs": subs,
        "window_days": days,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        sqlx::query("CREATE TABLE twitch_stream_sessions (id BIGSERIAL PRIMARY KEY, streamer_login TEXT, started_at TIMESTAMPTZ)")
            .execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_ad_break_events (id BIGSERIAL PRIMARY KEY, session_id BIGINT, duration_seconds INTEGER, is_automatic BOOLEAN DEFAULT FALSE, started_at TIMESTAMPTZ)")
            .execute(&pool).await.unwrap();
        // Live-Schema: bits/subs OHNE streamer_login, subs.is_gift BOOLEAN.
        sqlx::query("CREATE TABLE twitch_bits_events (id BIGSERIAL PRIMARY KEY, session_id BIGINT, amount INTEGER, received_at TIMESTAMPTZ)")
            .execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_subscription_events (id BIGSERIAL PRIMARY KEY, session_id BIGINT, is_gift BOOLEAN DEFAULT FALSE, received_at TIMESTAMPTZ)")
            .execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_hype_train_events (id BIGSERIAL PRIMARY KEY, session_id BIGINT, level INTEGER, duration_seconds INTEGER, started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ)")
            .execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn ads_aggregat_und_graceful_defaults() {
        let Some(pool) = make_pool("t_mon").await else { return };
        sqlx::query("INSERT INTO twitch_stream_sessions (streamer_login, started_at) VALUES ('nani', NOW() - INTERVAL '2 hours')")
            .execute(&pool).await.unwrap();
        // 3 Ads: 2 auto (60s/60s), 1 manuell (30s).
        sqlx::query("INSERT INTO twitch_ad_break_events (session_id, duration_seconds, is_automatic, started_at) VALUES (1,60,TRUE,NOW()-INTERVAL '1 hour'),(1,60,TRUE,NOW()-INTERVAL '50 min'),(1,30,FALSE,NOW()-INTERVAL '40 min')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_hype_train_events (session_id, level, duration_seconds, started_at, ended_at) VALUES (1,3,300,NOW()-INTERVAL '1 hour',NOW()-INTERVAL '55 min'),(1,5,600,NOW()-INTERVAL '30 min',NOW()-INTERVAL '20 min')")
            .execute(&pool).await.unwrap();

        let v = load_monetization_payload(&pool, "nani", 30).await.unwrap();
        assert_eq!(v["ads"]["total"], 3);
        assert_eq!(v["ads"]["auto"], 2);
        assert_eq!(v["ads"]["manual"], 1);
        assert_eq!(v["ads"]["sessions_with_ads"], 1);
        assert_eq!(v["ads"]["avg_duration_s"], 50.0); // (60+60+30)/3
        // Leere Drop-Strukturen (Teil 1).
        assert!(v["ads"]["avg_viewer_drop_pct"].is_null());
        assert_eq!(v["ads"]["worst_ads"], json!([]));
        assert_eq!(v["ads"]["position_impact"]["early_0_30m"]["count"], 0);
        assert!(v["ads"]["best_ad_time"].is_null());
        assert_eq!(v["ads"]["recommendations"], json!([]));
        // Hype-Train aggregiert.
        assert_eq!(v["hype_train"]["total"], 2);
        assert_eq!(v["hype_train"]["avg_level"], 4.0); // (3+5)/2
        assert_eq!(v["hype_train"]["max_level"], 5);
        assert_eq!(v["hype_train"]["avg_duration_s"], 450.0); // (300+600)/2
        // bits/subs: streamer_login fehlt → graceful Default 0.
        assert_eq!(v["bits"], json!({ "total": 0, "cheer_events": 0 }));
        assert_eq!(v["subs"], json!({ "total_events": 0, "gifted": 0 }));
        assert_eq!(v["window_days"], 30);
    }

    #[tokio::test]
    async fn leer_ohne_streamer() {
        let Some(pool) = make_pool("t_mon_empty").await else { return };
        let v = load_monetization_payload(&pool, "", 7).await.unwrap();
        assert_eq!(v["ads"]["total"], 0);
        assert_eq!(v["ads"]["avg_duration_s"], 0.0);
        assert_eq!(v["hype_train"]["total"], 0);
        assert_eq!(v["window_days"], 7);
    }
}
