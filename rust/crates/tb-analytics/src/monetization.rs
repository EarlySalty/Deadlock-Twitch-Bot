//! Monetization- & Hype-Train-Übersicht (`/twitch/api/v2/monetization`).
//!
//! Port von `bot/analytics/insights_monetization_loader.py:load_monetization_payload`.
//! Aggregiert über das Zeitfenster: Ad-Breaks (Anzahl/auto/manuell/Dauer +
//! Viewer-Drop-Analyse je Ad), Hype-Trains, Bits und Subs.
//!
//! **Migrations-Fix (bits/subs):** Pythons bits/subs-Queries filtern über eine
//! direkte Spalte `streamer_login`, die durch die Schema-Umstellung des Rust-
//! Cutovers nicht mehr existiert — in Python scheitern sie still und liefern immer 0.
//! Die beabsichtigte Funktion ist eindeutig (Bits/Subs je Streamer im Fenster), daher
//! hier korrigiert statt 1:1 den Defekt zu spiegeln: Filter über JOIN auf
//! `twitch_stream_sessions.streamer_login` per `session_id` — genau wie ads +
//! hype_train im selben Loader. Der catch-all bleibt defensiver Fallback (Tabelle fehlt).

use std::collections::{HashMap, HashSet};

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
// Labels für best_ad_time bzw. die position-Empfehlung (unterschiedlicher Wortlaut!).
const POSITION_LABELS_SLOT: [&str; 4] = ["ersten 30 Min", "Min 30-60", "Min 60-90", "nach Min 90"];
const POSITION_LABELS_RECO: [&str; 4] =
    ["in den ersten 30 Min", "zwischen Min 30-60", "zwischen Min 60-90", "nach Min 90"];
const DURATION_LABELS: [&str; 4] = ["30s", "60s", "90s", "120s+"];

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

/// Python `_avg`: round1(mean) oder null bei leer.
fn avg_round1(values: &[f64]) -> Value {
    if values.is_empty() {
        Value::Null
    } else {
        json!(round1(mean(values)))
    }
}

/// {bucket: {value_key, count}}-Map (leerer Bucket → value_key null, count 0).
fn impact_map(names: &[&str], data: &[Vec<f64>; 4], value_key: &str) -> Value {
    let mut m = serde_json::Map::new();
    for (i, name) in names.iter().enumerate() {
        m.insert(
            name.to_string(),
            json!({ value_key: avg_round1(&data[i]), "count": data[i].len() }),
        );
    }
    Value::Object(m)
}

/// Index des nicht-leeren Buckets mit kleinstem Mittelwert (Gleichstand → niedrigster Index).
fn min_mean_index(data: &[Vec<f64>; 4]) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, d) in data.iter().enumerate() {
        if d.is_empty() {
            continue;
        }
        let m = mean(d);
        if best.map_or(true, |(_, bm)| m < bm) {
            best = Some((i, m));
        }
    }
    best.map(|(i, _)| i)
}

/// Index des nicht-leeren Buckets mit größtem Mittelwert (Gleichstand → niedrigster Index).
fn max_mean_index(data: &[Vec<f64>; 4]) -> Option<usize> {
    let mut worst: Option<(usize, f64)> = None;
    for (i, d) in data.iter().enumerate() {
        if d.is_empty() {
            continue;
        }
        let m = mean(d);
        if worst.map_or(true, |(_, wm)| m > wm) {
            worst = Some((i, m));
        }
    }
    worst.map(|(i, _)| i)
}

/// Per-Ad-Viewer-Drop-Analyse: lädt die letzten 200 Ad-Breaks + die Viewer-Timeline
/// ihrer Sessions und berechnet je Ad den Viewer-Drop (Ø 3 Min vor Ad vs. 2 Min nach
/// Ad-Ende), Recovery-Zeit, Positions-/Dauer-Buckets + abgeleitete Empfehlungen.
/// Ohne Ad-/Timeline-Daten ergeben sich automatisch die leeren Default-Strukturen.
async fn compute_drop_analysis(
    pool: &PgPool,
    streamer: &str,
    cutoff: DateTime<Utc>,
) -> Result<DropAnalysis, sqlx::Error> {
    type AdRow = (i64, i64, DateTime<Utc>, Option<i32>, Option<bool>, Option<DateTime<Utc>>);
    let ad_rows: Vec<AdRow> = sqlx::query_as(
        "SELECT a.id::bigint, a.session_id::bigint, a.started_at, a.duration_seconds, a.is_automatic, s.started_at \
           FROM twitch_ad_break_events a \
           JOIN twitch_stream_sessions s ON s.id = a.session_id \
          WHERE a.started_at >= $1 AND a.session_id IS NOT NULL AND ($2 = '' OR LOWER(s.streamer_login) = $2) \
          ORDER BY a.started_at DESC LIMIT 200",
    )
    .bind(cutoff)
    .bind(streamer)
    .fetch_all(pool)
    .await?;

    // Viewer-Timeline je Session (nur wenn Ads vorhanden — wie Pythons `if ad_rows`).
    let mut timeline_map: HashMap<i64, Vec<(f64, f64)>> = HashMap::new();
    if !ad_rows.is_empty() {
        let mut seen: HashSet<i64> = HashSet::new();
        let mut session_ids: Vec<i64> = Vec::new();
        for r in &ad_rows {
            if seen.insert(r.1) {
                session_ids.push(r.1);
            }
        }
        if !session_ids.is_empty() {
            let viewer_rows: Vec<(i64, Option<i32>, i32)> = sqlx::query_as(
                "SELECT session_id::bigint, minutes_from_start, viewer_count \
                   FROM twitch_session_viewers WHERE session_id = ANY($1) \
                  ORDER BY session_id, minutes_from_start",
            )
            .bind(&session_ids)
            .fetch_all(pool)
            .await?;
            for (sid, minute, vc) in viewer_rows {
                timeline_map.entry(sid).or_default().push((minute.unwrap_or(0) as f64, vc as f64));
            }
        }
    }

    let mut drop_pcts: Vec<f64> = Vec::new();
    let mut worst_ads: Vec<(f64, Value)> = Vec::new();
    let mut position: [Vec<f64>; 4] = Default::default();
    let mut duration: [Vec<f64>; 4] = Default::default();
    let mut auto_drops: Vec<f64> = Vec::new();
    let mut manual_drops: Vec<f64> = Vec::new();
    let mut recovery_times: Vec<f64> = Vec::new();
    let mut duration_recovery: [Vec<f64>; 4] = Default::default();

    for (_id, session_id, started_at, duration_seconds, is_automatic, session_start) in &ad_rows {
        // Python `float(ad.duration_seconds or 30)`: None/0 → 30.
        let duration_seconds =
            duration_seconds.map(|d| d as f64).filter(|d| *d != 0.0).unwrap_or(30.0);
        // str(None)→fromisoformat-Fehler→continue.
        let Some(session_start) = session_start else { continue };
        let minutes_into = (*started_at - *session_start).num_milliseconds() as f64 / 60_000.0;
        let Some(timeline) = timeline_map.get(session_id) else { continue };
        if timeline.is_empty() {
            continue;
        }
        let duration_minutes = duration_seconds / 60.0;
        let pre: Vec<f64> = timeline
            .iter()
            .filter(|(m, _)| (minutes_into - 3.0) <= *m && *m < minutes_into)
            .map(|(_, v)| *v)
            .collect();
        let post_start = minutes_into + duration_minutes;
        let post: Vec<f64> = timeline
            .iter()
            .filter(|(m, _)| post_start <= *m && *m < post_start + 2.0)
            .map(|(_, v)| *v)
            .collect();
        if pre.is_empty() || post.is_empty() {
            continue;
        }
        let pre_avg = mean(&pre);
        if pre_avg <= 0.0 {
            continue;
        }
        let drop = (pre_avg - mean(&post)) / pre_avg * 100.0;
        drop_pcts.push(drop);

        // Recovery: erstes Timeline-Sample nach Ad-Ende mit >= 95 % des Pre-Schnitts.
        let mut sorted_tl = timeline.clone();
        sorted_tl.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut recovery_min: Option<f64> = None;
        for (minute, viewers) in &sorted_tl {
            if *minute > post_start && *viewers >= pre_avg * 0.95 {
                recovery_min = Some(round1(minute - post_start));
                break;
            }
        }
        let di = if duration_seconds <= 35.0 {
            0
        } else if duration_seconds <= 65.0 {
            1
        } else if duration_seconds <= 100.0 {
            2
        } else {
            3
        };
        if let Some(rm) = recovery_min {
            recovery_times.push(rm);
            duration_recovery[di].push(rm);
        }

        let is_auto = is_automatic.unwrap_or(false);
        let drop_pct_round = round1(drop);
        worst_ads.push((
            drop_pct_round,
            json!({
                "started_at": started_at.format("%Y-%m-%d %H:%M").to_string(),
                "duration_s": duration_seconds as i64,
                "drop_pct": drop_pct_round,
                "is_automatic": is_auto,
                "min_into_stream": round1(minutes_into),
                "recovery_min": recovery_min.map(|v| json!(v)).unwrap_or(Value::Null),
            }),
        ));

        let pi = if minutes_into < 30.0 {
            0
        } else if minutes_into < 60.0 {
            1
        } else if minutes_into < 90.0 {
            2
        } else {
            3
        };
        position[pi].push(drop);
        duration[di].push(drop);
        if is_auto {
            auto_drops.push(drop);
        } else {
            manual_drops.push(drop);
        }
    }

    let avg_viewer_drop_pct =
        if drop_pcts.is_empty() { Value::Null } else { json!(round1(mean(&drop_pcts))) };

    // Top-5 nach drop_pct absteigend (stabil → Gleichstand behält Ad-Reihenfolge).
    worst_ads.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let worst_ads_json: Vec<Value> = worst_ads.into_iter().take(5).map(|(_, j)| j).collect();

    let auto_vs_manual = json!({
        "auto_avg_drop": avg_round1(&auto_drops),
        "manual_avg_drop": avg_round1(&manual_drops),
        "auto_count": auto_drops.len(),
        "manual_count": manual_drops.len(),
    });

    let best_ad_time = match (min_mean_index(&position), max_mean_index(&position)) {
        (Some(best), Some(worst)) => Value::String(format!(
            "Nach {} (Ø -{:.1}% statt -{:.1}% {})",
            POSITION_LABELS_SLOT[best],
            mean(&position[best]),
            mean(&position[worst]),
            POSITION_LABELS_SLOT[worst]
        )),
        _ => Value::Null,
    };

    let avg_recovery_min =
        if recovery_times.is_empty() { Value::Null } else { json!(round1(mean(&recovery_times))) };

    let mut recommendations: Vec<String> = Vec::new();
    if let Some(best) = min_mean_index(&duration) {
        recommendations.push(format!(
            "{}-Ads verursachen den geringsten Drop (Ø {:.1}%)",
            DURATION_LABELS[best],
            mean(&duration[best])
        ));
    }
    if !auto_drops.is_empty() && !manual_drops.is_empty() {
        let auto_avg = mean(&auto_drops);
        let manual_avg = mean(&manual_drops);
        if manual_avg < auto_avg * 0.7 {
            recommendations.push(format!(
                "Manuelle Ads verlieren {:.0}% weniger Viewer als automatische",
                (auto_avg - manual_avg) / auto_avg * 100.0
            ));
        }
    }
    if let Some(best) = min_mean_index(&position) {
        recommendations.push(format!(
            "Beste Ad-Zeit: {} (Ø {:.1}% Drop)",
            POSITION_LABELS_RECO[best],
            mean(&position[best])
        ));
    }
    if !recovery_times.is_empty() {
        recommendations.push(format!(
            "Ø Recovery-Zeit: {:.1} Minuten nach Ad-Ende",
            mean(&recovery_times)
        ));
    }

    Ok(DropAnalysis {
        avg_viewer_drop_pct,
        worst_ads: worst_ads_json,
        position_impact: impact_map(&POSITION_BUCKETS, &position, "avg_drop"),
        duration_impact: impact_map(&DURATION_BUCKETS, &duration, "avg_drop"),
        auto_vs_manual,
        best_ad_time,
        avg_recovery_min,
        recovery_by_duration: impact_map(&DURATION_BUCKETS, &duration_recovery, "avg_recovery_min"),
        recommendations,
    })
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
                    COALESCE(SUM(CASE WHEN COALESCE(a.is_automatic, false) THEN 1 ELSE 0 END), 0)::bigint, \
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

    let analysis = compute_drop_analysis(pool, streamer, cutoff).await?;

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

    // --- Bits (Migrations-Fix: Streamer-Filter via session_id-JOIN statt fehlender Spalte). ---
    let bits = match sqlx::query_as::<_, (Option<i64>, i64)>(
        "SELECT SUM(b.amount)::bigint, COUNT(*)::bigint \
           FROM twitch_bits_events b \
           LEFT JOIN twitch_stream_sessions s ON s.id = b.session_id \
          WHERE b.received_at >= $1 AND ($2 = '' OR LOWER(s.streamer_login) = $2)",
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

    // --- Subs (Migrations-Fix: Streamer-Filter via session_id-JOIN). ---
    let subs = match sqlx::query_as::<_, (i64, Option<i64>)>(
        "SELECT COUNT(*)::bigint, SUM(CASE WHEN COALESCE(su.is_gift, false) THEN 1 ELSE 0 END)::bigint \
           FROM twitch_subscription_events su \
           LEFT JOIN twitch_stream_sessions s ON s.id = su.session_id \
          WHERE su.received_at >= $1 AND ($2 = '' OR LOWER(s.streamer_login) = $2)",
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
        // Live-Schema: bits/subs OHNE streamer_login.
        sqlx::query("CREATE TABLE twitch_bits_events (id BIGSERIAL PRIMARY KEY, session_id BIGINT, amount INTEGER, received_at TIMESTAMPTZ)")
            .execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_subscription_events (id BIGSERIAL PRIMARY KEY, session_id BIGINT, is_gift BOOLEAN DEFAULT FALSE, received_at TIMESTAMPTZ)")
            .execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_hype_train_events (id BIGSERIAL PRIMARY KEY, session_id BIGINT, level INTEGER, duration_seconds INTEGER, started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ)")
            .execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_session_viewers (session_id BIGINT, ts_utc TIMESTAMPTZ, minutes_from_start INTEGER, viewer_count INTEGER)")
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
        // keine bits/subs-Zeilen → 0.
        assert_eq!(v["bits"], json!({ "total": 0, "cheer_events": 0 }));
        assert_eq!(v["subs"], json!({ "total_events": 0, "gifted": 0 }));
        assert_eq!(v["window_days"], 30);
    }

    #[tokio::test]
    async fn bits_subs_zaehlen_via_session() {
        let Some(pool) = make_pool("t_mon_bitssubs").await else { return };
        // Migrations-Fix: Filter läuft über session_id → s.streamer_login (wie ads/hype).
        sqlx::query("INSERT INTO twitch_stream_sessions (id, streamer_login, started_at) VALUES (1,'nani',NOW()-INTERVAL '2 hours'),(2,'other',NOW()-INTERVAL '2 hours')")
            .execute(&pool).await.unwrap();
        // nani: 2 Bits (100+50), 3 Subs (1 gift); other: 1 Bit (999) — darf NICHT zählen.
        sqlx::query("INSERT INTO twitch_bits_events (session_id, amount, received_at) VALUES (1,100,NOW()-INTERVAL '1 hour'),(1,50,NOW()-INTERVAL '30 min'),(2,999,NOW()-INTERVAL '1 hour')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_subscription_events (session_id, is_gift, received_at) VALUES (1,FALSE,NOW()-INTERVAL '1 hour'),(1,TRUE,NOW()-INTERVAL '50 min'),(1,FALSE,NOW()-INTERVAL '40 min'),(2,TRUE,NOW()-INTERVAL '1 hour')")
            .execute(&pool).await.unwrap();

        // Mit Streamer-Filter: nur nani.
        let v = load_monetization_payload(&pool, "nani", 30).await.unwrap();
        assert_eq!(v["bits"], json!({ "total": 150, "cheer_events": 2 }));
        assert_eq!(v["subs"], json!({ "total_events": 3, "gifted": 1 }));

        // Ohne Filter: alles.
        let all = load_monetization_payload(&pool, "", 30).await.unwrap();
        assert_eq!(all["bits"], json!({ "total": 1149, "cheer_events": 3 }));
        assert_eq!(all["subs"], json!({ "total_events": 4, "gifted": 2 }));
    }

    #[tokio::test]
    async fn drop_analysis_berechnet() {
        let Some(pool) = make_pool("t_mon_drop").await else { return };
        // Session + 1 manueller 60s-Ad, 10 Min in den Stream (feste Timestamps → minutes_into=10.0).
        sqlx::query("INSERT INTO twitch_stream_sessions (id, streamer_login, started_at) VALUES (1,'nani','2026-06-14 12:00:00+00')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_ad_break_events (session_id, duration_seconds, is_automatic, started_at) VALUES (1,60,FALSE,'2026-06-14 12:10:00+00')")
            .execute(&pool).await.unwrap();
        // Timeline: pre (Min 7-9)=100, während/post (Min 10-12)=50, Recovery bei Min 13=95.
        for (m, vc) in [(7, 100), (8, 100), (9, 100), (10, 50), (11, 50), (12, 50), (13, 95)] {
            sqlx::query("INSERT INTO twitch_session_viewers (session_id, ts_utc, minutes_from_start, viewer_count) VALUES (1, NOW(), $1, $2)")
                .bind(m as i32).bind(vc as i32).execute(&pool).await.unwrap();
        }

        let v = load_monetization_payload(&pool, "nani", 3650).await.unwrap();
        let ads = &v["ads"];
        // drop = (100-50)/100*100 = 50.0
        assert_eq!(ads["avg_viewer_drop_pct"], 50.0);
        let w = &ads["worst_ads"][0];
        assert_eq!(w["started_at"], "2026-06-14 12:10");
        assert_eq!(w["duration_s"], 60);
        assert_eq!(w["drop_pct"], 50.0);
        assert_eq!(w["is_automatic"], false);
        assert_eq!(w["min_into_stream"], 10.0);
        assert_eq!(w["recovery_min"], 2.0); // Min 13 − post_start 11
        // Buckets: Position early, Dauer 60s.
        assert_eq!(ads["position_impact"]["early_0_30m"], json!({ "avg_drop": 50.0, "count": 1 }));
        assert_eq!(ads["position_impact"]["mid_30_60m"], json!({ "avg_drop": Value::Null, "count": 0 }));
        assert_eq!(ads["duration_impact"]["60s"], json!({ "avg_drop": 50.0, "count": 1 }));
        assert_eq!(ads["auto_vs_manual"], json!({ "auto_avg_drop": Value::Null, "manual_avg_drop": 50.0, "auto_count": 0, "manual_count": 1 }));
        assert_eq!(ads["avg_recovery_min"], 2.0);
        assert_eq!(ads["recovery_by_duration"]["60s"], json!({ "avg_recovery_min": 2.0, "count": 1 }));
        assert_eq!(ads["best_ad_time"], "Nach ersten 30 Min (Ø -50.0% statt -50.0% ersten 30 Min)");
        assert_eq!(ads["recommendations"], json!([
            "60s-Ads verursachen den geringsten Drop (Ø 50.0%)",
            "Beste Ad-Zeit: in den ersten 30 Min (Ø 50.0% Drop)",
            "Ø Recovery-Zeit: 2.0 Minuten nach Ad-Ende"
        ]));
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
