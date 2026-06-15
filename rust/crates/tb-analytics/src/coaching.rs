//! Coaching-Engine (`/twitch/api/v2/coaching`).
//!
//! Port von `bot/analytics/coaching_engine.py` (1632 Z., regelbasiert/kein AI).
//! `get_coaching_data` ruft ~12 self-contained Analyse-Funktionen + einen
//! Recommendations-Builder. Wird in verifizierten Teil-Slices portiert — je
//! Analyse eine `pub fn`. **Teil 1: `_efficiency`** (Viewer-Stunden/Stream-Stunde
//! + Wachstum/10h, je mit Kategorie-Schnitt [Top-15 % gefiltert] + Perzentil).
//! **Teil 2: `_title_analysis` + `_extract_keywords`** (eigene vs. Kategorie-
//! Titel, Keyword-Muster, Titel-Varianz vs. Peers).

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use chrono::{DateTime, Utc};
use regex::Regex;
use serde_json::{json, Value};
use sqlx::PgPool;

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// p85-Schwelle (Python `sorted[max(0, int(len*0.85)-1)]`).
fn p85_threshold(sorted: &[f64]) -> f64 {
    let idx = ((sorted.len() as f64 * 0.85) as usize).saturating_sub(1);
    sorted[idx]
}

fn empty_efficiency() -> Value {
    json!({
        "viewerHoursPerStreamHour": 0,
        "categoryAvg": 0,
        "topPerformers": [],
        "percentile": 0,
        "totalStreamHours": 0,
        "totalViewerHours": 0,
        "growthPer10Hours": 0,
        "growthCategoryAvg": 0,
        "growthTopPerformers": [],
        "growthPercentile": 0,
    })
}

/// Effizienz-Analyse (Python `_efficiency`).
pub async fn efficiency(pool: &PgPool, streamer: &str, since: DateTime<Utc>) -> Result<Value, sqlx::Error> {
    // 1) Viewer-Stunden / Stream-Stunden je Streamer.
    let rows: Vec<(String, Option<f64>, Option<f64>, Option<f64>)> = sqlx::query_as(
        "SELECT s.streamer_login, \
                SUM(s.avg_viewers * s.duration_seconds / 3600.0)::float8, \
                SUM(s.duration_seconds / 3600.0)::float8, \
                (SUM(s.avg_viewers * s.duration_seconds / 3600.0) / NULLIF(SUM(s.duration_seconds / 3600.0), 0))::float8 \
           FROM twitch_stream_sessions s \
          WHERE s.started_at >= $1 AND s.duration_seconds > 300 \
          GROUP BY s.streamer_login \
         HAVING SUM(s.duration_seconds) / 3600.0 > 1 \
          ORDER BY 4 DESC",
    )
    .bind(since)
    .fetch_all(pool)
    .await?;

    let mut ratios: Vec<(String, f64)> = Vec::new();
    let mut your_ratio = 0.0;
    let mut your_vh = 0.0;
    let mut your_sh = 0.0;
    for (login, vh, sh, ratio) in &rows {
        let ratio = ratio.unwrap_or(0.0);
        ratios.push((login.clone(), ratio));
        if login == streamer {
            your_ratio = ratio;
            your_vh = vh.unwrap_or(0.0);
            your_sh = sh.unwrap_or(0.0);
        }
    }

    if ratios.is_empty() {
        return Ok(empty_efficiency());
    }

    let all_ratios: Vec<f64> = ratios.iter().map(|(_, v)| *v).collect();
    let mut sorted_ratios = all_ratios.clone();
    sorted_ratios.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let threshold = p85_threshold(&sorted_ratios);
    let filtered: Vec<&(String, f64)> = ratios.iter().filter(|(_, v)| *v <= threshold).collect();
    let cat_avg = if !filtered.is_empty() {
        filtered.iter().map(|(_, v)| *v).sum::<f64>() / filtered.len() as f64
    } else {
        all_ratios.iter().sum::<f64>() / all_ratios.len() as f64
    };
    let below = all_ratios.iter().filter(|r| **r < your_ratio).count();
    let percentile = (below as f64 / all_ratios.len() as f64 * 100.0) as i64;
    let top_performers: Vec<Value> = filtered
        .iter()
        .take(5)
        .map(|(login, v)| json!({ "streamer": login, "ratio": round1(*v) }))
        .collect();

    // 2) Wachstum: gewonnene Follower je 10 Stream-Stunden.
    let growth_rows: Vec<(String, Option<f64>)> = sqlx::query_as(
        "SELECT s.streamer_login, \
                (SUM(CASE WHEN s.follower_delta > 0 THEN s.follower_delta ELSE 0 END) / NULLIF(SUM(s.duration_seconds / 3600.0), 0) * 10.0)::float8 \
           FROM twitch_stream_sessions s \
          WHERE s.started_at >= $1 AND s.duration_seconds > 300 \
          GROUP BY s.streamer_login \
         HAVING SUM(s.duration_seconds) / 3600.0 > 1 \
          ORDER BY 2 DESC",
    )
    .bind(since)
    .fetch_all(pool)
    .await?;

    let mut your_growth = 0.0;
    let mut growth_ratios: Vec<(String, f64)> = Vec::new();
    for (login, g) in &growth_rows {
        let g = g.unwrap_or(0.0);
        growth_ratios.push((login.clone(), g));
        if login == streamer {
            your_growth = g;
        }
    }
    let all_growth: Vec<f64> = growth_ratios.iter().map(|(_, g)| *g).collect();
    let (growth_cat_avg, growth_top, growth_percentile): (f64, Vec<Value>, i64) = if !all_growth.is_empty() {
        let mut sorted_g = all_growth.clone();
        sorted_g.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let gt = p85_threshold(&sorted_g);
        let fg: Vec<&(String, f64)> = growth_ratios.iter().filter(|(_, g)| *g <= gt).collect();
        let avg = if !fg.is_empty() {
            fg.iter().map(|(_, g)| *g).sum::<f64>() / fg.len() as f64
        } else {
            0.0
        };
        let top: Vec<Value> = fg.iter().take(5).map(|(login, g)| json!({ "streamer": login, "value": round1(*g) })).collect();
        let below_g = all_growth.iter().filter(|g| **g < your_growth).count();
        (avg, top, (below_g as f64 / all_growth.len() as f64 * 100.0) as i64)
    } else {
        (0.0, Vec::new(), 0)
    };

    Ok(json!({
        "viewerHoursPerStreamHour": round1(your_ratio),
        "categoryAvg": round1(cat_avg),
        "topPerformers": top_performers,
        "percentile": percentile,
        "totalStreamHours": round1(your_sh),
        "totalViewerHours": round1(your_vh),
        "growthPer10Hours": round1(your_growth),
        "growthCategoryAvg": round1(growth_cat_avg),
        "growthTopPerformers": growth_top,
        "growthPercentile": growth_percentile,
    }))
}

/// Stopwords für `extract_keywords` (1:1 aus `coaching_engine.py`).
const KEYWORD_STOPWORDS: &[&str] = &[
    "der", "die", "das", "und", "oder", "mit", "in", "auf", "an", "von", "the", "and", "or",
    "with", "for", "to", "a", "is", "on", "at", "|", "-", "!", "?", "#", ":", "~", "//", ">>",
];

/// Aussagekräftige Keywords aus Stream-Titeln (Python `_extract_keywords`).
///
/// Token = `[A-Za-z0-9äöüÄÖÜß]+` auf dem kleingeschriebenen Titel, gefiltert auf
/// Länge ≥ 3 und Nicht-Stopword. Rückgabe = die 20 häufigsten Wörter. Bei
/// Häufigkeits-Gleichstand entscheidet die Erst-Vorkommens-Reihenfolge (exakt
/// wie `collections.Counter.most_common`: absteigende Anzahl, dann früheste
/// Einfügung) — hier über einen stabilen Sort auf der Insertion-Order.
fn extract_keywords(titles: &[String]) -> Vec<String> {
    static WORD_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"[A-Za-z0-9äöüÄÖÜß]+").unwrap());

    let mut order: Vec<String> = Vec::new();
    let mut counts: HashMap<String, usize> = HashMap::new();
    for title in titles {
        let lower = title.to_lowercase();
        for m in WORD_RE.find_iter(&lower) {
            let w = m.as_str();
            if w.chars().count() >= 3 && !KEYWORD_STOPWORDS.contains(&w) {
                let count = counts.entry(w.to_string()).or_insert_with(|| {
                    order.push(w.to_string());
                    0
                });
                *count += 1;
            }
        }
    }
    // Stable-Sort auf Insertion-Order → Ties behalten Erst-Vorkommen.
    order.sort_by(|a, b| counts[b].cmp(&counts[a]));
    order.truncate(20);
    order
}

/// Titel-Analyse (Python `_title_analysis`): eigene Titel-Performance, Top-Titel
/// der Kategorie, Keyword-Muster (fehlende/Top) und Titel-Varianz vs. Peers.
pub async fn title_analysis(
    pool: &PgPool,
    streamer: &str,
    since: DateTime<Utc>,
) -> Result<Value, sqlx::Error> {
    // 1) Eigene Titel, aggregiert.
    let your_titles: Vec<(String, Option<f64>, Option<i32>, Option<f64>, i64)> = sqlx::query_as(
        "SELECT s.stream_title, \
                AVG(s.avg_viewers)::float8, \
                MAX(s.peak_viewers), \
                AVG(s.unique_chatters)::float8, \
                COUNT(*)::bigint \
           FROM twitch_stream_sessions s \
          WHERE s.streamer_login = $1 AND s.started_at >= $2 \
            AND s.stream_title IS NOT NULL AND s.stream_title != '' \
          GROUP BY s.stream_title \
          ORDER BY 2 DESC",
    )
    .bind(streamer)
    .bind(since)
    .fetch_all(pool)
    .await?;

    let your_list: Vec<Value> = your_titles
        .iter()
        .map(|(title, avg_v, peak, chatters, count)| {
            json!({
                "title": title,
                "avgViewers": round1(avg_v.unwrap_or(0.0)),
                "peakViewers": peak.unwrap_or(0),
                "chatters": round1(chatters.unwrap_or(0.0)),
                "usageCount": count,
            })
        })
        .collect();

    // 2) Top-Titel der Kategorie (andere Streamer).
    let cat_titles: Vec<(String, String, Option<f64>)> = sqlx::query_as(
        "SELECT s.stream_title, s.streamer_login, AVG(s.avg_viewers)::float8 \
           FROM twitch_stream_sessions s \
          WHERE s.started_at >= $1 \
            AND s.stream_title IS NOT NULL AND s.stream_title != '' \
            AND s.streamer_login != $2 \
          GROUP BY s.stream_title, s.streamer_login \
         HAVING COUNT(*) >= 2 \
          ORDER BY 3 DESC \
          LIMIT 10",
    )
    .bind(since)
    .bind(streamer)
    .fetch_all(pool)
    .await?;

    let cat_list: Vec<Value> = cat_titles
        .iter()
        .map(|(title, login, avg_v)| {
            json!({ "title": title, "streamer": login, "avgViewers": round1(avg_v.unwrap_or(0.0)) })
        })
        .collect();

    // 3) Keyword-Muster.
    let your_words = extract_keywords(&your_titles.iter().map(|r| r.0.clone()).collect::<Vec<_>>());
    let top_words = extract_keywords(&cat_titles.iter().map(|r| r.0.clone()).collect::<Vec<_>>());
    let your_set: HashSet<&String> = your_words.iter().collect();
    let missing: Vec<&String> = top_words.iter().filter(|w| !your_set.contains(w)).take(10).collect();
    let top_patterns: Vec<&String> = top_words.iter().take(10).collect();

    // 4) Titel-Varianz vs. Peers.
    let own_total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM twitch_stream_sessions \
          WHERE streamer_login = $1 AND started_at >= $2 AND duration_seconds > 300",
    )
    .bind(streamer)
    .bind(since)
    .fetch_one(pool)
    .await?;

    let own_unique = your_titles.len() as i64;
    let own_variety_pct: Value = if own_total > 0 {
        json!(round1(own_unique as f64 / own_total as f64 * 100.0))
    } else {
        json!(0)
    };

    let peer_variety: Vec<(String, i64, i64, Option<f64>)> = sqlx::query_as(
        "SELECT s.streamer_login, \
                COUNT(DISTINCT s.stream_title)::bigint, \
                COUNT(*)::bigint, \
                ROUND(COUNT(DISTINCT s.stream_title) * 100.0 / COUNT(*), 1)::float8 \
           FROM twitch_stream_sessions s \
          WHERE s.started_at >= $1 AND s.duration_seconds > 300 \
            AND s.streamer_login != $2 \
            AND s.stream_title IS NOT NULL AND s.stream_title != '' \
          GROUP BY s.streamer_login \
         HAVING COUNT(*) >= 3 \
          ORDER BY 4 DESC",
    )
    .bind(since)
    .bind(streamer)
    .fetch_all(pool)
    .await?;

    let peer_pcts: Vec<f64> = peer_variety.iter().map(|r| r.3.unwrap_or(0.0)).collect();
    let avg_peer_variety: Value = if !peer_pcts.is_empty() {
        json!(round1(peer_pcts.iter().sum::<f64>() / peer_pcts.len() as f64))
    } else {
        json!(0)
    };
    let peer_variety_list: Vec<Value> = peer_variety
        .iter()
        .take(10)
        .map(|(login, unique_t, total_s, variety)| {
            json!({
                "streamer": login,
                "uniqueTitles": unique_t,
                "totalSessions": total_s,
                "varietyPct": variety.unwrap_or(0.0),
            })
        })
        .collect();

    Ok(json!({
        "yourTitles": your_list,
        "categoryTopTitles": cat_list,
        "yourMissingPatterns": missing,
        "topPerformerPatterns": top_patterns,
        "varietyPct": own_variety_pct,
        "uniqueTitleCount": own_unique,
        "totalSessionCount": own_total,
        "avgPeerVarietyPct": avg_peer_variety,
        "peerVariety": peer_variety_list,
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
        sqlx::query("CREATE TABLE twitch_stream_sessions (id BIGSERIAL PRIMARY KEY, streamer_login TEXT, started_at TIMESTAMPTZ, duration_seconds INTEGER, avg_viewers REAL, follower_delta INTEGER, stream_title TEXT, peak_viewers INTEGER, unique_chatters INTEGER)")
            .execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn efficiency_leer() {
        let Some(pool) = make_pool("t_coach_eff_empty").await else { return };
        let v = efficiency(&pool, "nani", Utc::now() - chrono::Duration::days(30)).await.unwrap();
        assert_eq!(v["viewerHoursPerStreamHour"], 0);
        assert_eq!(v["topPerformers"], json!([]));
    }

    #[tokio::test]
    async fn efficiency_berechnet() {
        let Some(pool) = make_pool("t_coach_eff").await else { return };
        // nani: 2h Stream, avg 50 → viewer_hours 100, ratio 50. other: 2h, avg 10 → ratio 10.
        sqlx::query("INSERT INTO twitch_stream_sessions (streamer_login, started_at, duration_seconds, avg_viewers, follower_delta) VALUES \
            ('nani', NOW()-INTERVAL '1 day', 7200, 50, 20), \
            ('other', NOW()-INTERVAL '1 day', 7200, 10, 5)")
            .execute(&pool).await.unwrap();
        let v = efficiency(&pool, "nani", Utc::now() - chrono::Duration::days(30)).await.unwrap();
        assert_eq!(v["viewerHoursPerStreamHour"], 50.0);
        assert_eq!(v["totalStreamHours"], 2.0);
        assert_eq!(v["totalViewerHours"], 100.0);
        // nani ratio 50 > other 10 → percentile 50 (1 von 2 darunter).
        assert_eq!(v["percentile"], 50);
        // Wachstum: nani 20 Follower / 2h *10 = 100/h... 20/2*10=100.
        assert_eq!(v["growthPer10Hours"], 100.0);
    }

    #[test]
    fn extract_keywords_haeufigkeit_und_reihenfolge() {
        // "deadlock" 2x → vorn; Rest count 1 in Erst-Vorkommens-Reihenfolge.
        let titles = vec![
            "Deadlock Ranked Grind!".to_string(),
            "Deadlock Chill Stream".to_string(),
        ];
        let kw = extract_keywords(&titles);
        assert_eq!(kw[0], "deadlock");
        assert_eq!(kw, vec!["deadlock", "ranked", "grind", "chill", "stream"]);
    }

    #[test]
    fn extract_keywords_stopwords_und_minlaenge() {
        // Stopwords raus, Tokens < 3 Zeichen raus, ä/ö/ü zählen.
        assert_eq!(extract_keywords(&["der die und".to_string()]), Vec::<String>::new());
        assert_eq!(extract_keywords(&["ab cd xyz".to_string()]), vec!["xyz"]);
        assert_eq!(extract_keywords(&["Übung Spaß".to_string()]), vec!["übung", "spaß"]);
    }

    #[tokio::test]
    async fn title_analysis_berechnet() {
        let Some(pool) = make_pool("t_coach_title").await else { return };
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (streamer_login, started_at, duration_seconds, avg_viewers, follower_delta, stream_title, peak_viewers, unique_chatters) VALUES \
            ('nani',  NOW()-INTERVAL '1 day', 7200, 50, 0, 'Deadlock Grind', 80, 10), \
            ('nani',  NOW()-INTERVAL '1 day', 7200, 60, 0, 'Deadlock Grind', 90, 12), \
            ('nani',  NOW()-INTERVAL '1 day', 7200, 30, 0, 'Chill Stream',   40,  5), \
            ('other', NOW()-INTERVAL '1 day', 7200, 100,0, 'Pro Gameplay',  150, 20), \
            ('other', NOW()-INTERVAL '1 day', 7200, 110,0, 'Pro Gameplay',  160, 22), \
            ('other', NOW()-INTERVAL '1 day', 7200, 90, 0, 'Other Title',   120, 18)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let v = title_analysis(&pool, "nani", Utc::now() - chrono::Duration::days(30))
            .await
            .unwrap();

        // Eigene Titel: "Deadlock Grind" (avg 55, peak 90, usage 2) vor "Chill Stream" (avg 30).
        assert_eq!(v["yourTitles"][0]["title"], "Deadlock Grind");
        assert_eq!(v["yourTitles"][0]["avgViewers"], 55.0);
        assert_eq!(v["yourTitles"][0]["peakViewers"], 90);
        assert_eq!(v["yourTitles"][0]["usageCount"], 2);

        // Varianz: 2 unique / 3 Sessions = 66.7 %.
        assert_eq!(v["uniqueTitleCount"], 2);
        assert_eq!(v["totalSessionCount"], 3);
        assert_eq!(v["varietyPct"], 66.7);

        // Kategorie-Top: nur "Pro Gameplay" (other) erfüllt COUNT>=2.
        assert_eq!(v["categoryTopTitles"][0]["title"], "Pro Gameplay");
        assert_eq!(v["categoryTopTitles"][0]["streamer"], "other");

        // Peer-Varianz: other hat 2 unique / 3 Sessions = 66.7.
        assert_eq!(v["peerVariety"][0]["varietyPct"], 66.7);
        assert_eq!(v["avgPeerVarietyPct"], 66.7);

        // Keyword-Muster: "pro"/"gameplay" fehlen nani.
        let missing = v["yourMissingPatterns"].as_array().unwrap();
        assert!(missing.iter().any(|w| w == "pro"));
        assert!(missing.iter().any(|w| w == "gameplay"));
    }
}
