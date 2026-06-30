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

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn round0(v: f64) -> f64 {
    v.round()
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Pearson-Korrelation (Python `_pearson`, Populations-Std mit `/n`).
/// Liefert 0.0 bei < 3 Werten oder konstanter Reihe (Std = 0).
fn pearson(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len();
    if n < 3 {
        return 0.0;
    }
    let nf = n as f64;
    let mx = x.iter().sum::<f64>() / nf;
    let my = y.iter().sum::<f64>() / nf;
    let sx = (x.iter().map(|xi| (xi - mx).powi(2)).sum::<f64>() / nf).sqrt();
    let sy = (y.iter().map(|yi| (yi - my).powi(2)).sum::<f64>() / nf).sqrt();
    if sx == 0.0 || sy == 0.0 {
        return 0.0;
    }
    let cov = x
        .iter()
        .zip(y.iter())
        .map(|(xi, yi)| (xi - mx) * (yi - my))
        .sum::<f64>()
        / nf;
    cov / (sx * sy)
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
pub async fn efficiency(
    pool: &PgPool,
    streamer: &str,
    since: DateTime<Utc>,
) -> Result<Value, sqlx::Error> {
    // 1) Viewer-Stunden / Stream-Stunden je Streamer.
    let rows: Vec<(String, Option<f64>, Option<f64>, Option<f64>)> = sqlx::query!(
        r#"
        SELECT s.streamer_login AS "streamer_login!",
               SUM(s.avg_viewers * s.duration_seconds / 3600.0)::float8 AS viewer_hours,
               SUM(s.duration_seconds / 3600.0)::float8 AS stream_hours,
               (SUM(s.avg_viewers * s.duration_seconds / 3600.0) / NULLIF(SUM(s.duration_seconds / 3600.0), 0))::float8 AS ratio
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1
          AND s.duration_seconds > 300
        GROUP BY s.streamer_login
        HAVING SUM(s.duration_seconds) / 3600.0 > 1
        ORDER BY 4 DESC
        "#,
        since
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.streamer_login, row.viewer_hours, row.stream_hours, row.ratio))
    .collect();

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
    let growth_rows: Vec<(String, Option<f64>)> = sqlx::query!(
        r#"
        SELECT s.streamer_login AS "streamer_login!",
               (SUM(CASE WHEN s.follower_delta > 0 THEN s.follower_delta ELSE 0 END) / NULLIF(SUM(s.duration_seconds / 3600.0), 0) * 10.0)::float8 AS growth
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1
          AND s.duration_seconds > 300
        GROUP BY s.streamer_login
        HAVING SUM(s.duration_seconds) / 3600.0 > 1
        ORDER BY 2 DESC
        "#,
        since
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.streamer_login, row.growth))
    .collect();

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
    let (growth_cat_avg, growth_top, growth_percentile): (f64, Vec<Value>, i64) =
        if !all_growth.is_empty() {
            let mut sorted_g = all_growth.clone();
            sorted_g.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let gt = p85_threshold(&sorted_g);
            let fg: Vec<&(String, f64)> = growth_ratios.iter().filter(|(_, g)| *g <= gt).collect();
            let avg = if !fg.is_empty() {
                fg.iter().map(|(_, g)| *g).sum::<f64>() / fg.len() as f64
            } else {
                0.0
            };
            let top: Vec<Value> = fg
                .iter()
                .take(5)
                .map(|(login, g)| json!({ "streamer": login, "value": round1(*g) }))
                .collect();
            let below_g = all_growth.iter().filter(|g| **g < your_growth).count();
            (
                avg,
                top,
                (below_g as f64 / all_growth.len() as f64 * 100.0) as i64,
            )
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
    static WORD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z0-9äöüÄÖÜß]+").unwrap());

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
    let your_titles: Vec<(String, Option<f64>, Option<i32>, Option<f64>, i64)> = sqlx::query!(
        r#"
        SELECT s.stream_title AS "stream_title!",
               AVG(s.avg_viewers)::float8 AS avg_viewers,
               MAX(s.peak_viewers) AS peak_viewers,
               AVG(s.unique_chatters)::float8 AS avg_chatters,
               COUNT(*)::bigint AS "usage_count!"
        FROM twitch_stream_sessions s
        WHERE s.streamer_login = $1
          AND s.started_at >= $2
          AND s.stream_title IS NOT NULL
          AND s.stream_title != ''
        GROUP BY s.stream_title
        ORDER BY 2 DESC
        "#,
        streamer,
        since
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        (
            row.stream_title,
            row.avg_viewers,
            row.peak_viewers,
            row.avg_chatters,
            row.usage_count,
        )
    })
    .collect();

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
    let cat_titles: Vec<(String, String, Option<f64>)> = sqlx::query!(
        r#"
        SELECT s.stream_title AS "stream_title!",
               s.streamer_login AS "streamer_login!",
               AVG(s.avg_viewers)::float8 AS avg_viewers
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1
          AND s.stream_title IS NOT NULL
          AND s.stream_title != ''
          AND s.streamer_login != $2
        GROUP BY s.stream_title, s.streamer_login
        HAVING COUNT(*) >= 2
        ORDER BY 3 DESC
        LIMIT 10
        "#,
        since,
        streamer
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.stream_title, row.streamer_login, row.avg_viewers))
    .collect();

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
    let missing: Vec<&String> = top_words
        .iter()
        .filter(|w| !your_set.contains(w))
        .take(10)
        .collect();
    let top_patterns: Vec<&String> = top_words.iter().take(10).collect();

    // 4) Titel-Varianz vs. Peers.
    let own_total: i64 = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*)::bigint AS "count!"
        FROM twitch_stream_sessions
        WHERE streamer_login = $1
          AND started_at >= $2
          AND duration_seconds > 300
        "#,
        streamer,
        since
    )
    .fetch_one(pool)
    .await?;

    let own_unique = your_titles.len() as i64;
    let own_variety_pct: Value = if own_total > 0 {
        json!(round1(own_unique as f64 / own_total as f64 * 100.0))
    } else {
        json!(0)
    };

    let peer_variety: Vec<(String, i64, i64, Option<f64>)> = sqlx::query!(
        r#"
        SELECT s.streamer_login AS "streamer_login!",
               COUNT(DISTINCT s.stream_title)::bigint AS "unique_titles!",
               COUNT(*)::bigint AS "total_sessions!",
               ROUND(COUNT(DISTINCT s.stream_title) * 100.0 / COUNT(*), 1)::float8 AS variety
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1
          AND s.duration_seconds > 300
          AND s.streamer_login != $2
          AND s.stream_title IS NOT NULL
          AND s.stream_title != ''
        GROUP BY s.streamer_login
        HAVING COUNT(*) >= 3
        ORDER BY 4 DESC
        "#,
        since,
        streamer
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        (
            row.streamer_login,
            row.unique_titles,
            row.total_sessions,
            row.variety,
        )
    })
    .collect();

    let peer_pcts: Vec<f64> = peer_variety.iter().map(|r| r.3.unwrap_or(0.0)).collect();
    let avg_peer_variety: Value = if !peer_pcts.is_empty() {
        json!(round1(
            peer_pcts.iter().sum::<f64>() / peer_pcts.len() as f64
        ))
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

/// Schedule-Optimizer (Python `_schedule_optimizer`): Konkurrenz-Heatmap der
/// Kategorie (Wochentag×Stunde), eigene Slots und „Sweet Spots" (viel
/// Kategorie-Publikum bei wenig Konkurrenz).
pub async fn schedule_optimizer(
    pool: &PgPool,
    streamer: &str,
    since: DateTime<Utc>,
) -> Result<Value, sqlx::Error> {
    // 1) Konkurrenz-Heatmap je Wochentag/Stunde (UTC).
    let competition: Vec<(i32, i32, i64, Option<f64>)> = sqlx::query!(
        r#"
        SELECT EXTRACT(DOW FROM (ts_utc AT TIME ZONE 'UTC'))::int AS "weekday!",
               EXTRACT(HOUR FROM (ts_utc AT TIME ZONE 'UTC'))::int AS "hour!",
               COUNT(DISTINCT streamer)::bigint AS "competitors!",
               AVG(viewer_count)::float8 AS avg_viewers
        FROM twitch_stats_category
        WHERE ts_utc >= $1
        GROUP BY 1, 2
        "#,
        since
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.weekday, row.hour, row.competitors, row.avg_viewers))
    .collect();

    // Zelle = (weekday, hour, competitors, categoryViewers[gerundet]). Die
    // Rundung passiert VOR der Opportunity-Rechnung — exakt wie Python, das
    // `cell["categoryViewers"]` (bereits gerundet) weiterverwendet.
    let cells: Vec<(i32, i32, i64, f64)> = competition
        .iter()
        .map(|(w, h, c, v)| (*w, *h, *c, round1(v.unwrap_or(0.0))))
        .collect();

    let heatmap: Vec<Value> = cells
        .iter()
        .map(|(w, h, c, v)| {
            json!({ "weekday": w, "hour": h, "competitors": c, "categoryViewers": v })
        })
        .collect();

    // 2) Eigene Stream-Slots.
    let your_slots: Vec<(i32, i32, i64)> = sqlx::query!(
        r#"
        SELECT EXTRACT(DOW FROM (started_at AT TIME ZONE 'UTC'))::int AS "weekday!",
               EXTRACT(HOUR FROM (started_at AT TIME ZONE 'UTC'))::int AS "hour!",
               COUNT(*)::bigint AS "count!"
        FROM twitch_stream_sessions
        WHERE streamer_login = $1
          AND started_at >= $2
        GROUP BY 1, 2
        ORDER BY 3 DESC
        "#,
        streamer,
        since
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.weekday, row.hour, row.count))
    .collect();

    let current_slots: Vec<Value> = your_slots
        .iter()
        .map(|(w, h, cnt)| json!({ "weekday": w, "hour": h, "count": cnt }))
        .collect();

    // 3) Sweet Spots: viel Kategorie-Publikum pro Konkurrent.
    let mut sweet: Vec<(i32, i32, i64, f64, f64)> = cells
        .iter()
        .map(|(w, h, c, v)| {
            let opportunity = if *c > 0 { v / *c as f64 } else { *v };
            (*w, *h, *c, *v, round1(opportunity))
        })
        .collect();
    // Stabiler Sort (wie Pythons Timsort) absteigend nach opportunityScore.
    sweet.sort_by(|a, b| b.4.partial_cmp(&a.4).unwrap_or(std::cmp::Ordering::Equal));
    let sweet_spots: Vec<Value> = sweet
        .iter()
        .take(15)
        .map(|(w, h, c, v, o)| {
            json!({
                "weekday": w,
                "hour": h,
                "categoryViewers": v,
                "competitors": c,
                "opportunityScore": o,
            })
        })
        .collect();

    Ok(json!({
        "sweetSpots": sweet_spots,
        "yourCurrentSlots": current_slots,
        "competitionHeatmap": heatmap,
    }))
}

/// Dauer-Buckets (Python `buckets_def`): Label + [lo, hi) Sekunden.
const DURATION_BUCKETS: &[(&str, i32, i32)] = &[
    ("< 1h", 0, 3600),
    ("1-2h", 3600, 7200),
    ("2-3h", 7200, 10800),
    ("3-4h", 10800, 14400),
    ("4-5h", 14400, 18000),
    ("5h+", 18000, 999999),
];

/// Dauer-Analyse (Python `_duration_analysis`): Stream-Performance je
/// Längen-Bucket, optimale Länge (höchste Ø-Viewer bei ≥ 2 Streams) und
/// Korrelation Dauer↔Viewer.
pub async fn duration_analysis(
    pool: &PgPool,
    streamer: &str,
    since: DateTime<Utc>,
) -> Result<Value, sqlx::Error> {
    let rows: Vec<(i32, Option<f64>, Option<i32>, Option<f64>)> = sqlx::query!(
        r#"
        SELECT s.duration_seconds AS "duration_seconds!",
               s.avg_viewers::float8 AS avg_viewers,
               s.unique_chatters,
               s.retention_5m::float8 AS retention_5m
        FROM twitch_stream_sessions s
        WHERE s.streamer_login = $1
          AND s.started_at >= $2
          AND s.duration_seconds > 300
        "#,
        streamer,
        since
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        (
            row.duration_seconds,
            row.avg_viewers,
            row.unique_chatters,
            row.retention_5m,
        )
    })
    .collect();

    if rows.is_empty() {
        return Ok(json!({
            "buckets": [],
            "optimalLabel": "",
            "currentAvgHours": 0,
            "correlation": 0,
        }));
    }

    let mut buckets: Vec<Value> = Vec::new();
    // (label, streamCount, gerundete avgViewers) — Basis für die Optimal-Wahl.
    let mut meta: Vec<(&str, usize, f64)> = Vec::new();
    for (label, lo, hi) in DURATION_BUCKETS {
        let subset: Vec<&(i32, Option<f64>, Option<i32>, Option<f64>)> =
            rows.iter().filter(|r| *lo <= r.0 && r.0 < *hi).collect();
        if subset.is_empty() {
            buckets.push(json!({
                "label": label,
                "streamCount": 0,
                "avgViewers": 0,
                "avgChatters": 0,
                "avgRetention5m": 0,
                "efficiencyRatio": 0,
            }));
            meta.push((label, 0, 0.0));
            continue;
        }
        let len = subset.len() as f64;
        let avg_v = subset.iter().map(|r| r.1.unwrap_or(0.0)).sum::<f64>() / len;
        let avg_c = subset.iter().map(|r| r.2.unwrap_or(0) as f64).sum::<f64>() / len;
        let ret_vals: Vec<f64> = subset.iter().filter_map(|r| r.3).collect();
        let avg_ret = if !ret_vals.is_empty() {
            ret_vals.iter().sum::<f64>() / ret_vals.len() as f64
        } else {
            0.0
        };
        let avg_dur = subset.iter().map(|r| r.0 as f64).sum::<f64>() / len;
        // 1:1 Pythons Literal-Ausdruck (avg_dur kürzt sich raus, fp-identisch).
        let eff = if avg_dur > 0.0 {
            (avg_v * avg_dur / 3600.0) / (avg_dur / 3600.0)
        } else {
            0.0
        };
        let av_rounded = round1(avg_v);
        buckets.push(json!({
            "label": label,
            "streamCount": subset.len(),
            "avgViewers": av_rounded,
            "avgChatters": round1(avg_c),
            "avgRetention5m": round1(avg_ret),
            "efficiencyRatio": round1(eff),
        }));
        meta.push((label, subset.len(), av_rounded));
    }

    // Optimaler Bucket: höchste (gerundete) avgViewers bei ≥ 2 Streams.
    // Python `max(..., key=...)` nimmt das ERSTE Maximum → nur bei striktem
    // Größer ersetzen, Bucket-Reihenfolge bleibt erhalten.
    let mut optimal = "";
    let mut best = f64::NEG_INFINITY;
    for (label, count, av) in &meta {
        if *count >= 2 && *av > best {
            best = *av;
            optimal = label;
        }
    }

    let total_dur: i64 = rows.iter().map(|r| r.0 as i64).sum();
    let current_avg = total_dur as f64 / rows.len() as f64 / 3600.0;

    let durations: Vec<f64> = rows.iter().map(|r| r.0 as f64).collect();
    let viewers: Vec<f64> = rows.iter().map(|r| r.1.unwrap_or(0.0)).collect();
    let correlation = pearson(&durations, &viewers);

    Ok(json!({
        "buckets": buckets,
        "optimalLabel": optimal,
        "currentAvgHours": round1(current_avg),
        "correlation": round3(correlation),
    }))
}

/// Cross-Community-Analyse (Python `_cross_community`): geteilte Chatter mit
/// anderen Streamern, Anteil „isolierter" (nur eigener Channel) Zuschauer und
/// eine Ökosystem-Einordnung.
pub async fn cross_community(
    pool: &PgPool,
    streamer: &str,
    since: DateTime<Utc>,
) -> Result<Value, sqlx::Error> {
    // 1) Eigene Unique-Chatter.
    let total_unique: i64 = sqlx::query_scalar!(
        r#"
        SELECT COUNT(DISTINCT chatter_login)::bigint AS "count!"
        FROM twitch_chatter_rollup
        WHERE streamer_login = $1 AND last_seen_at >= $2
        "#,
        streamer,
        since
    )
    .fetch_one(pool)
    .await?;

    if total_unique == 0 {
        return Ok(json!({
            "totalUniqueChatters": 0,
            "chatterSources": [],
            "isolatedChatters": 0,
            "isolatedPercentage": 0,
            "ecosystemSummary": "Keine Chatter-Daten verfuegbar.",
        }));
    }

    // 2) Geteilte Chatter je anderem Streamer ($1/$2 mehrfach referenziert).
    let shared: Vec<(String, i64)> = sqlx::query!(
        r#"
        SELECT c2.streamer_login AS "streamer_login!",
               COUNT(DISTINCT c1.chatter_login)::bigint AS "shared_count!"
        FROM twitch_chatter_rollup c1
        JOIN twitch_chatter_rollup c2
          ON c1.chatter_login = c2.chatter_login
         AND c2.streamer_login != $1
         AND c2.last_seen_at >= $2
        WHERE c1.streamer_login = $1
          AND c1.last_seen_at >= $2
        GROUP BY c2.streamer_login
        ORDER BY 2 DESC
        LIMIT 15
        "#,
        streamer,
        since
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.streamer_login, row.shared_count))
    .collect();

    let sources: Vec<Value> = shared
        .iter()
        .map(|(source, count)| {
            json!({
                "sourceStreamer": source,
                "sharedChatters": count,
                "percentage": round1(*count as f64 / total_unique as f64 * 100.0),
            })
        })
        .collect();

    // 3) Chatter, die auch anderswo auftauchen → isoliert = Rest.
    let shared_count: i64 = sqlx::query_scalar!(
        r#"
        SELECT COUNT(DISTINCT c1.chatter_login)::bigint AS "count!"
        FROM twitch_chatter_rollup c1
        WHERE c1.streamer_login = $1
          AND c1.last_seen_at >= $2
          AND EXISTS (
              SELECT 1
              FROM twitch_chatter_rollup c2
              WHERE c2.chatter_login = c1.chatter_login
                AND c2.streamer_login != $1
                AND c2.last_seen_at >= $2
          )
        "#,
        streamer,
        since
    )
    .fetch_one(pool)
    .await?;

    let isolated = total_unique - shared_count;
    let isolated_pct = round1(isolated as f64 / total_unique as f64 * 100.0);

    // Schwellen-Vergleich auf dem GERUNDETEN Wert (1:1 Python).
    let summary = if isolated_pct > 60.0 {
        "Deine Community ist stark eigenstaendig - die meisten Chatter sind nur in deinem Channel aktiv."
    } else if isolated_pct > 30.0 {
        "Gute Mischung: Ein Teil deiner Zuschauer kommt aus der Deadlock-Community, viele sind aber deine eigenen."
    } else {
        "Dein Channel profitiert stark vom Community-Oekoystem. Viele Zuschauer kennst du aus anderen Channels."
    };

    Ok(json!({
        "totalUniqueChatters": total_unique,
        "chatterSources": sources,
        "isolatedChatters": isolated,
        "isolatedPercentage": isolated_pct,
        "ecosystemSummary": summary,
    }))
}

/// Einzel-Tags aus gruppierten Tag-Strings (Python `_split_tags_from_rows`).
/// Split auf `,` `;` `|`, getrimmt + lowercase, leere raus → Set.
fn split_tags_from_rows(tag_strings: &[String]) -> HashSet<String> {
    let mut tags = HashSet::new();
    for raw in tag_strings {
        for part in raw.split(|c| c == ',' || c == ';' || c == '|') {
            let cleaned = part.trim().to_lowercase();
            if !cleaned.is_empty() {
                tags.insert(cleaned);
            }
        }
    }
    tags
}

/// Tag-Optimierung (Python `_tag_optimization`): eigene Tag-Kombinationen,
/// beste Kategorie-Tags, fehlende High-Performer und eigene Underperformer.
pub async fn tag_optimization(
    pool: &PgPool,
    streamer: &str,
    since: DateTime<Utc>,
) -> Result<Value, sqlx::Error> {
    // 1) Eigene Tag-Kombinationen.
    let your_rows: Vec<(String, Option<f64>, i64)> = sqlx::query!(
        r#"
        SELECT s.tags AS "tags!",
               AVG(s.avg_viewers)::float8 AS avg_viewers,
               COUNT(*)::bigint AS "usage_count!"
        FROM twitch_stream_sessions s
        WHERE s.streamer_login = $1
          AND s.started_at >= $2
          AND s.tags IS NOT NULL
          AND s.tags != ''
        GROUP BY s.tags
        ORDER BY 2 DESC
        "#,
        streamer,
        since
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.tags, row.avg_viewers, row.usage_count))
    .collect();

    // (tags, gerundete avgViewers, usageCount) — Basis für Output + Avg/Underperf.
    let your_data: Vec<(String, f64, i64)> = your_rows
        .iter()
        .map(|(tags, avg, cnt)| (tags.clone(), round1(avg.unwrap_or(0.0)), *cnt))
        .collect();
    let your_tags: Vec<Value> = your_data
        .iter()
        .map(|(t, a, c)| json!({ "tags": t, "avgViewers": a, "usageCount": c }))
        .collect();
    let your_individual =
        split_tags_from_rows(&your_rows.iter().map(|r| r.0.clone()).collect::<Vec<_>>());

    // 2) Beste Kategorie-Tags (alle Streamer, COUNT≥3).
    let cat_rows: Vec<(String, Option<f64>, i64)> = sqlx::query!(
        r#"
        SELECT s.tags AS "tags!",
               AVG(s.avg_viewers)::float8 AS avg_viewers,
               COUNT(DISTINCT s.streamer_login)::bigint AS "streamer_count!"
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1
          AND s.tags IS NOT NULL
          AND s.tags != ''
        GROUP BY s.tags
        HAVING COUNT(*) >= 3
        ORDER BY 2 DESC
        LIMIT 15
        "#,
        since
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.tags, row.avg_viewers, row.streamer_count))
    .collect();

    let cat_tags: Vec<Value> = cat_rows
        .iter()
        .map(|(tags, avg, scnt)| {
            json!({ "tags": tags, "avgViewers": round1(avg.unwrap_or(0.0)), "streamerCount": scnt })
        })
        .collect();
    let cat_individual =
        split_tags_from_rows(&cat_rows.iter().map(|r| r.0.clone()).collect::<Vec<_>>());

    // 3) Fehlende High-Performer (Set-Iteration → Reihenfolge frei wie Python).
    let missing: Vec<&String> = cat_individual
        .iter()
        .filter(|t| !your_individual.contains(*t))
        .take(10)
        .collect();

    // 4) Underperformer: eigene Tags unter 80 % des eigenen Schnitts.
    let underperforming: Vec<&String> = if !your_data.is_empty() {
        let your_avg = your_data.iter().map(|(_, a, _)| *a).sum::<f64>() / your_data.len() as f64;
        your_data
            .iter()
            .filter(|(_, a, _)| *a < your_avg * 0.8)
            .map(|(t, _, _)| t)
            .take(5)
            .collect()
    } else {
        Vec::new()
    };

    Ok(json!({
        "yourTags": your_tags,
        "categoryBestTags": cat_tags,
        "missingHighPerformers": missing,
        "underperformingTags": underperforming,
    }))
}

/// Normalisierte Viewer-Kurve (Python `_build_viewer_curve`): pro 5-Minuten-
/// Marke der Ø-Anteil `viewer_count / peak * 100` über die Sessions. Rückgabe =
/// `(minute, gerundeter Prozentwert)` für 0,5,…,60.
async fn build_viewer_curve(
    pool: &PgPool,
    session_ids: &[i64],
    peak_viewers: &[i64],
) -> Result<Vec<(i32, f64)>, sqlx::Error> {
    if session_ids.is_empty() {
        return Ok(Vec::new());
    }
    let pairs: Vec<(i64, i64)> = session_ids
        .iter()
        .zip(peak_viewers.iter())
        .map(|(s, p)| (*s, *p))
        .collect();
    let ids: Vec<i64> = pairs.iter().map(|(s, _)| *s).collect();

    let rows: Vec<(i64, Option<i32>, i32)> = sqlx::query!(
        r#"
        SELECT session_id AS "session_id!",
               minutes_from_start,
               viewer_count AS "viewer_count!"
        FROM twitch_session_viewers
        WHERE session_id = ANY($1)
          AND minutes_from_start <= 60
        ORDER BY session_id, minutes_from_start
        "#,
        &ids
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.session_id, row.minutes_from_start, row.viewer_count))
    .collect();

    let peak_map: HashMap<i64, i64> = pairs.into_iter().collect();
    let mut by_minute: HashMap<i32, Vec<f64>> = HashMap::new();
    for (sid, minute, vc) in &rows {
        // NULL-Minuten landen in Python in by_minute[None] → nie gelesen (curve
        // fragt nur 0..60 ab); hier überspringen = identische Kurve.
        let Some(minute) = minute else { continue };
        let peak = peak_map.get(sid).copied().unwrap_or(1);
        if peak > 0 {
            by_minute
                .entry(*minute)
                .or_default()
                .push(*vc as f64 / peak as f64 * 100.0);
        }
    }

    let mut curve: Vec<(i32, f64)> = Vec::new();
    let mut m = 0;
    while m <= 60 {
        let avg = match by_minute.get(&m) {
            Some(vals) if !vals.is_empty() => vals.iter().sum::<f64>() / vals.len() as f64,
            _ => 0.0,
        };
        curve.push((m, round1(avg)));
        m += 5;
    }
    Ok(curve)
}

fn curve_to_json(curve: &[(i32, f64)]) -> Vec<Value> {
    curve
        .iter()
        .map(|(m, pct)| json!({ "minute": m, "avgViewerPct": pct }))
        .collect()
}

/// `round(avg, 1) if avg else 0` — Pythons Truthiness (None ODER 0.0 → Int-0).
fn retention_value(avg: Option<f64>) -> Value {
    match avg {
        Some(v) if v != 0.0 => json!(round1(v)),
        _ => json!(0),
    }
}

/// Retention-Coaching (Python `_retention_coaching`): 5-Min-Retention vs.
/// Kategorie, eigene + Top-Performer-Viewer-Kurve und die kritische Abfall-
/// Minute (erster 5-Min-Schritt mit > 10 % Verlust).
pub async fn retention_coaching(
    pool: &PgPool,
    streamer: &str,
    since: DateTime<Utc>,
) -> Result<Value, sqlx::Error> {
    let your_avg: Option<f64> = sqlx::query_scalar!(
        r#"
        SELECT AVG(retention_5m)::float8 AS avg_retention
        FROM twitch_stream_sessions
        WHERE streamer_login = $1
          AND started_at >= $2
          AND retention_5m IS NOT NULL
        "#,
        streamer,
        since
    )
    .fetch_one(pool)
    .await?;

    let cat_avg: Option<f64> = sqlx::query_scalar!(
        r#"
        SELECT AVG(retention_5m)::float8 AS avg_retention
        FROM twitch_stream_sessions
        WHERE started_at >= $1
          AND retention_5m IS NOT NULL
        "#,
        since
    )
    .fetch_one(pool)
    .await?;

    // Eigene Kurve: jüngste 20 Sessions mit peak > 0.
    let your_sessions: Vec<(i64, i32)> = sqlx::query!(
        r#"
        SELECT s.id AS "id!",
               s.peak_viewers AS "peak_viewers!"
        FROM twitch_stream_sessions s
        WHERE s.streamer_login = $1
          AND s.started_at >= $2
          AND s.peak_viewers > 0
        ORDER BY s.started_at DESC
        LIMIT 20
        "#,
        streamer,
        since
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.id, row.peak_viewers))
    .collect();
    let your_ids: Vec<i64> = your_sessions.iter().map(|(id, _)| *id).collect();
    let your_peaks: Vec<i64> = your_sessions.iter().map(|(_, p)| i64::from(*p)).collect();
    let your_curve = build_viewer_curve(pool, &your_ids, &your_peaks).await?;

    // Top-Performer-Kurve: beste 20 fremder Sessions nach avg_viewers.
    let top_sessions: Vec<(i64, i32)> = sqlx::query!(
        r#"
        SELECT s.id AS "id!",
               s.peak_viewers AS "peak_viewers!"
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1
          AND s.streamer_login != $2
          AND s.peak_viewers > 0
        ORDER BY s.avg_viewers DESC
        LIMIT 20
        "#,
        since,
        streamer
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.id, row.peak_viewers))
    .collect();
    let top_ids: Vec<i64> = top_sessions.iter().map(|(id, _)| *id).collect();
    let top_peaks: Vec<i64> = top_sessions.iter().map(|(_, p)| i64::from(*p)).collect();
    let top_curve = build_viewer_curve(pool, &top_ids, &top_peaks).await?;

    // Kritische Abfall-Minute: erster Schritt unter 90 % des Vorwerts.
    let mut critical_minute = 0;
    for i in 1..your_curve.len() {
        if your_curve[i].1 < your_curve[i - 1].1 * 0.9 {
            critical_minute = your_curve[i].0;
            break;
        }
    }

    Ok(json!({
        "your5mRetention": retention_value(your_avg),
        "category5mRetention": retention_value(cat_avg),
        "yourViewerCurve": curve_to_json(&your_curve),
        "topPerformerCurve": curve_to_json(&top_curve),
        "criticalDropoffMinute": critical_minute,
    }))
}

/// Doppel-Stream-Erkennung (Python `_double_stream_detection`): Tage mit
/// mehreren Sessions, plus Vergleich der Tages-Ø-Viewer von Einzel- vs.
/// Doppel-Stream-Tagen. `date` als ISO-String (= `_sanitize_coaching_payload`,
/// das `date.isoformat()` anwendet).
pub async fn double_stream_detection(
    pool: &PgPool,
    streamer: &str,
    since: DateTime<Utc>,
) -> Result<Value, sqlx::Error> {
    // 1) Doppel-Stream-Tage.
    let rows: Vec<(chrono::NaiveDate, i64, Option<f64>)> = sqlx::query!(
        r#"
        SELECT DATE(started_at) AS "date!",
               COUNT(*)::bigint AS "count!",
               AVG(avg_viewers)::float8 AS avg_viewers
        FROM twitch_stream_sessions
        WHERE streamer_login = $1
          AND started_at >= $2
          AND duration_seconds > 300
        GROUP BY 1
        HAVING COUNT(*) > 1
        ORDER BY 1 DESC
        "#,
        streamer,
        since
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.date, row.count, row.avg_viewers))
    .collect();

    let occurrences: Vec<Value> = rows
        .iter()
        .map(|(date, count, avg)| {
            json!({
                "date": date.to_string(),
                "sessionCount": count,
                "avgViewers": round1(avg.unwrap_or(0.0)),
            })
        })
        .collect();

    // 2) Tages-Ø der Einzel-Stream-Tage.
    let single_avg: Option<f64> = sqlx::query_scalar!(
        r#"
        SELECT AVG(day_avg)::float8 AS avg_viewers
        FROM (
            SELECT DATE(started_at) AS d, AVG(avg_viewers) AS day_avg
            FROM twitch_stream_sessions
            WHERE streamer_login = $1
              AND started_at >= $2
              AND duration_seconds > 300
            GROUP BY d
            HAVING COUNT(*) = 1
        ) sub
        "#,
        streamer,
        since
    )
    .fetch_one(pool)
    .await?;

    // 3) Tages-Ø der Doppel-Stream-Tage.
    let double_avg: Option<f64> = sqlx::query_scalar!(
        r#"
        SELECT AVG(day_avg)::float8 AS avg_viewers
        FROM (
            SELECT DATE(started_at) AS d, AVG(avg_viewers) AS day_avg
            FROM twitch_stream_sessions
            WHERE streamer_login = $1
              AND started_at >= $2
              AND duration_seconds > 300
            GROUP BY d
            HAVING COUNT(*) > 1
        ) sub
        "#,
        streamer,
        since
    )
    .fetch_one(pool)
    .await?;

    let count = occurrences.len();
    let shown: Vec<&Value> = occurrences.iter().take(10).collect();

    Ok(json!({
        "detected": count > 0,
        "count": count,
        "occurrences": shown,
        "singleDayAvg": retention_value(single_avg),
        "doubleDayAvg": retention_value(double_avg),
    }))
}

/// Chat-Konzentration & Loyalität (Python `_chat_concentration`): Loyalty-
/// Buckets, Top-Chatter mit kumulativem Anteil, HHI-Konzentrationsindex und
/// One-Timer-Quote vs. Peers.
pub async fn chat_concentration(
    pool: &PgPool,
    streamer: &str,
    since: DateTime<Utc>,
) -> Result<Value, sqlx::Error> {
    // 1) Loyalty-Buckets nach Session-Anzahl.
    let buckets_raw: Vec<(String, i64, Option<i64>)> = sqlx::query!(
        r#"
        SELECT CASE
                 WHEN total_sessions = 1 THEN 'oneTimer'
                 WHEN total_sessions BETWEEN 2 AND 3 THEN 'casual'
                 WHEN total_sessions BETWEEN 4 AND 10 THEN 'regular'
                 ELSE 'loyal'
               END AS "bucket!",
               COUNT(*)::bigint AS "count!",
               SUM(total_messages)::bigint AS total_messages
        FROM twitch_chatter_rollup
        WHERE streamer_login = $1
          AND last_seen_at >= $2
        GROUP BY 1
        "#,
        streamer,
        since
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.bucket, row.count, row.total_messages))
    .collect();

    let total_chatters = {
        let s: i64 = buckets_raw.iter().map(|r| r.1).sum();
        if s == 0 {
            1
        } else {
            s
        }
    };
    let total_msgs = {
        let s: i64 = buckets_raw.iter().map(|r| r.2.unwrap_or(0)).sum();
        if s == 0 {
            1
        } else {
            s
        }
    };

    let mut buckets = serde_json::Map::new();
    let mut own_one_timer_pct: Value = json!(0);
    for (bucket, cnt, msgs) in &buckets_raw {
        let pct = round1(*cnt as f64 / total_chatters as f64 * 100.0);
        buckets.insert(
            bucket.clone(),
            json!({ "count": cnt, "pct": pct, "messages": msgs.unwrap_or(0) }),
        );
        if bucket == "oneTimer" {
            own_one_timer_pct = json!(pct);
        }
    }

    // 2) Top-Chatter + kumulativer Anteil.
    let top: Vec<(String, i64, i64)> = sqlx::query!(
        r#"
        SELECT chatter_login AS "chatter_login!",
               COALESCE(total_messages, 0)::bigint AS "total_messages!",
               COALESCE(total_sessions, 0)::bigint AS "total_sessions!"
        FROM twitch_chatter_rollup
        WHERE streamer_login = $1
          AND last_seen_at >= $2
        ORDER BY total_messages DESC
        LIMIT 15
        "#,
        streamer,
        since
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.chatter_login, row.total_messages, row.total_sessions))
    .collect();

    // (login, messages, sessions, sharePct, cumulativePct) — share gerundet,
    // cumulative summiert die GERUNDETEN shares (1:1 Python).
    let mut top_chatters: Vec<(String, i64, i64, f64, f64)> = Vec::new();
    let mut cumulative = 0.0;
    for (login, messages, sessions) in &top {
        let share = round1(*messages as f64 / total_msgs as f64 * 100.0);
        cumulative += share;
        top_chatters.push((
            login.clone(),
            *messages,
            *sessions,
            share,
            round1(cumulative),
        ));
    }

    // HHI: leeres top → Integer-0 (Pythons leere sum() = int), sonst float.
    let concentration_index: Value = if top.is_empty() {
        json!(0)
    } else {
        let hhi: f64 = top
            .iter()
            .map(|(_, m, _)| {
                let x = *m as f64 / total_msgs as f64;
                x * x
            })
            .sum::<f64>()
            * 10000.0;
        json!(round0(hhi))
    };

    let top1_pct: Value = top_chatters.first().map_or(json!(0), |c| json!(c.3));
    let top3_pct: Value = if top_chatters.len() >= 3 {
        json!(top_chatters[2].4)
    } else {
        top1_pct.clone()
    };

    // 3) Peer-Vergleich: One-Timer-Quote.
    let peer_loyalty: Vec<(String, i64, Option<i64>)> = sqlx::query!(
        r#"
        SELECT streamer_login AS "streamer_login!",
               COUNT(*)::bigint AS "count!",
               SUM(CASE WHEN total_sessions = 1 THEN 1 ELSE 0 END)::bigint AS one_timers
        FROM twitch_chatter_rollup
        WHERE last_seen_at >= $1
        GROUP BY streamer_login
        HAVING COUNT(*) >= 5
        "#,
        since
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.streamer_login, row.count, row.one_timers))
    .collect();

    let peer_pcts: Vec<f64> = peer_loyalty
        .iter()
        .filter(|r| r.0 != streamer && r.1 > 0)
        .map(|r| round1(r.2.unwrap_or(0) as f64 / r.1 as f64 * 100.0))
        .collect();
    let avg_peer_one_timer: Value = if !peer_pcts.is_empty() {
        json!(round1(
            peer_pcts.iter().sum::<f64>() / peer_pcts.len() as f64
        ))
    } else {
        json!(0)
    };

    let top_chatters_json: Vec<Value> = top_chatters
        .iter()
        .take(10)
        .map(|(l, m, s, sh, cu)| {
            json!({
                "login": l,
                "messages": m,
                "sessions": s,
                "sharePct": sh,
                "cumulativePct": cu,
            })
        })
        .collect();

    Ok(json!({
        "totalChatters": total_chatters,
        "totalMessages": total_msgs,
        "msgsPerChatter": round1(total_msgs as f64 / total_chatters as f64),
        "loyaltyBuckets": Value::Object(buckets),
        "topChatters": top_chatters_json,
        "concentrationIndex": concentration_index,
        "top1Pct": top1_pct,
        "top3Pct": top3_pct,
        "ownOneTimerPct": own_one_timer_pct,
        "avgPeerOneTimerPct": avg_peer_one_timer,
    }))
}

/// Raid-Netzwerk (Python `_raid_network`): Sende-/Empfangs-Bilanz der Raids,
/// Partner-Reziprozität (mutual/sentOnly/receivedOnly) und Aggregate.
pub async fn raid_network(
    pool: &PgPool,
    streamer: &str,
    since: DateTime<Utc>,
) -> Result<Value, sqlx::Error> {
    // Gesendete Raids je Ziel.
    let sent: Vec<(String, i64, Option<f64>, Option<i64>)> = sqlx::query!(
        r#"
        SELECT LOWER(to_broadcaster_login) AS "login!",
               COUNT(*)::bigint AS "count!",
               AVG(viewer_count)::float8 AS avg_viewers,
               SUM(viewer_count)::bigint AS total_viewers
        FROM twitch_raid_history
        WHERE LOWER(from_broadcaster_login) = $1
          AND executed_at >= $2
          AND COALESCE(success, FALSE) IS TRUE
        GROUP BY LOWER(to_broadcaster_login)
        ORDER BY COUNT(*) DESC
        "#,
        streamer,
        since
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.login, row.count, row.avg_viewers, row.total_viewers))
    .collect();

    // Empfangene Raids je Quelle.
    let received: Vec<(String, i64, Option<f64>, Option<i64>)> = sqlx::query!(
        r#"
        SELECT LOWER(from_broadcaster_login) AS "login!",
               COUNT(*)::bigint AS "count!",
               AVG(viewer_count)::float8 AS avg_viewers,
               SUM(viewer_count)::bigint AS total_viewers
        FROM twitch_raid_history
        WHERE LOWER(to_broadcaster_login) = $1
          AND executed_at >= $2
          AND COALESCE(success, FALSE) IS TRUE
        GROUP BY LOWER(from_broadcaster_login)
        ORDER BY COUNT(*) DESC
        "#,
        streamer,
        since
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.login, row.count, row.avg_viewers, row.total_viewers))
    .collect();

    // (count, avgViewers[Value: round1 ODER Int-0 via Truthiness], totalViewers).
    let to_map =
        |rows: &[(String, i64, Option<f64>, Option<i64>)]| -> HashMap<String, (i64, Value, i64)> {
            rows.iter()
                .map(|(login, count, avg, total)| {
                    (
                        login.clone(),
                        (*count, retention_value(*avg), total.unwrap_or(0)),
                    )
                })
                .collect()
        };
    let sent_map = to_map(&sent);
    let recv_map = to_map(&received);

    let mut all_partners: HashSet<&String> = HashSet::new();
    all_partners.extend(sent_map.keys());
    all_partners.extend(recv_map.keys());

    // (sortkey = sent+recv, reciprocity, Partner-JSON).
    let mut entries: Vec<(i64, &'static str, Value)> = Vec::new();
    for p in &all_partners {
        let s = sent_map.get(*p);
        let r = recv_map.get(*p);
        let s_count = s.map_or(0, |x| x.0);
        let r_count = r.map_or(0, |x| x.0);
        let s_avg = s.map_or(json!(0), |x| x.1.clone());
        let r_avg = r.map_or(json!(0), |x| x.1.clone());
        let reciprocity = if s_count > 0 && r_count > 0 {
            "mutual"
        } else if s_count > 0 {
            "sentOnly"
        } else {
            "receivedOnly"
        };
        entries.push((
            s_count + r_count,
            reciprocity,
            json!({
                "login": p,
                "sentCount": s_count,
                "sentAvgViewers": s_avg,
                "receivedCount": r_count,
                "receivedAvgViewers": r_avg,
                "reciprocity": reciprocity,
                "balance": r_count - s_count,
            }),
        ));
    }
    // Stabiler Sort absteigend nach Gesamt-Raids (Ties = Set-Order, frei wie Python).
    entries.sort_by(|a, b| b.0.cmp(&a.0));

    let total_sent: i64 = sent_map.values().map(|x| x.0).sum();
    let total_recv: i64 = recv_map.values().map(|x| x.0).sum();
    let total_sent_v: i64 = sent_map.values().map(|x| x.2).sum();
    let total_recv_v: i64 = recv_map.values().map(|x| x.2).sum();
    let mutual = entries.iter().filter(|e| e.1 == "mutual").count();
    let total_partners = entries.len();
    let partners: Vec<&Value> = entries.iter().take(15).map(|e| &e.2).collect();

    let avg_sent = if total_sent > 0 {
        json!(round1(total_sent_v as f64 / total_sent as f64))
    } else {
        json!(0)
    };
    let avg_recv = if total_recv > 0 {
        json!(round1(total_recv_v as f64 / total_recv as f64))
    } else {
        json!(0)
    };
    let reciprocity_ratio = if total_sent > 0 {
        json!(round2(total_recv as f64 / total_sent as f64))
    } else {
        json!(0)
    };

    Ok(json!({
        "totalSent": total_sent,
        "totalReceived": total_recv,
        "totalSentViewers": total_sent_v,
        "totalReceivedViewers": total_recv_v,
        "avgSentViewers": avg_sent,
        "avgReceivedViewers": avg_recv,
        "reciprocityRatio": reciprocity_ratio,
        "mutualPartners": mutual,
        "totalPartners": total_partners,
        "partners": partners,
    }))
}

/// Peer-Vergleich (Python `_peer_comparison`): Multi-Metrik-Tabelle aller
/// Streamer mit ≥ 3 Sessions, eigener Rang je Metrik, ähnliche vs.
/// aspirationale Peers und Abstand zum nächsthöheren Streamer.
pub async fn peer_comparison(
    pool: &PgPool,
    streamer: &str,
    since: DateTime<Utc>,
) -> Result<Value, sqlx::Error> {
    // Numerische Felder (gerundet) für Logik + vorgebaute JSON (Int-0-Truthiness).
    struct Peer {
        login: String,
        entry: Value,
        avg_viewers: f64,
        avg_chatters: f64,
        retention5m: f64,
        title_variety: f64,
        max_peak: i64,
        sessions: i64,
    }
    fn metric_value(p: &Peer, metric: &str) -> f64 {
        match metric {
            "avgViewers" => p.avg_viewers,
            "maxPeak" => p.max_peak as f64,
            "avgChatters" => p.avg_chatters,
            "retention5m" => p.retention5m,
            "titleVariety" => p.title_variety,
            "sessions" => p.sessions as f64,
            _ => 0.0,
        }
    }

    #[allow(clippy::type_complexity)]
    let all_rows: Vec<(
        String,
        i64,
        Option<f64>,
        Option<i32>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<i64>,
        i64,
    )> = sqlx::query!(
        r#"
        SELECT s.streamer_login AS "streamer_login!",
               COUNT(*)::bigint AS "session_count!",
               AVG(s.avg_viewers)::float8 AS avg_viewers,
               MAX(s.peak_viewers) AS max_peak,
               AVG(s.duration_seconds / 3600.0)::float8 AS avg_hours,
               AVG(s.unique_chatters)::float8 AS avg_chatters,
               AVG(s.retention_5m)::float8 AS retention_5m,
               SUM(s.duration_seconds / 3600.0)::float8 AS total_hours,
               SUM(CASE WHEN s.follower_delta > 0 THEN s.follower_delta ELSE 0 END)::bigint AS "follows_gained!",
               COUNT(DISTINCT s.stream_title)::bigint AS "unique_titles!"
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1
          AND s.duration_seconds > 300
          AND s.ended_at IS NOT NULL
        GROUP BY s.streamer_login
        HAVING COUNT(*) >= 3
        ORDER BY 3 DESC
        "#,
        since
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        (
            row.streamer_login,
            row.session_count,
            row.avg_viewers,
            row.max_peak,
            row.avg_hours,
            row.avg_chatters,
            row.retention_5m,
            row.total_hours,
            Some(row.follows_gained),
            row.unique_titles,
        )
    })
    .collect();

    let mut peers: Vec<Peer> = Vec::new();
    let mut own_index: Option<usize> = None;
    for (i, r) in all_rows.iter().enumerate() {
        let (
            login,
            sessions,
            avg_v,
            max_peak,
            avg_hours,
            avg_chat,
            avg_ret,
            total_hours,
            follows,
            unique_titles,
        ) = r;
        let sessions = *sessions;
        let unique_t = *unique_titles;

        let avg_viewers = round1(avg_v.unwrap_or(0.0));
        let avg_chatters = round1(avg_chat.unwrap_or(0.0));
        let retention5m = round1(avg_ret.unwrap_or(0.0) * 100.0);
        let max_peak_i = max_peak.unwrap_or(0) as i64;
        let title_variety = if sessions > 0 && unique_t != 0 {
            round1(unique_t as f64 / sessions as f64 * 100.0)
        } else {
            0.0
        };

        // JSON-Werte: Int-0 bei Falsy (= Pythons `round(x or 0, 1)`).
        let retention5m_json = match avg_ret {
            Some(v) if *v != 0.0 => json!(round1(v * 100.0)),
            _ => json!(0),
        };
        let title_variety_json = if sessions > 0 && unique_t != 0 {
            json!(title_variety)
        } else {
            json!(0)
        };
        let entry = json!({
            "login": login,
            "sessions": sessions,
            "avgViewers": retention_value(*avg_v),
            "maxPeak": max_peak_i,
            "avgHours": retention_value(*avg_hours),
            "avgChatters": retention_value(*avg_chat),
            "retention5m": retention5m_json,
            "totalHours": retention_value(*total_hours),
            "followsGained": follows.unwrap_or(0),
            "uniqueTitles": unique_t,
            "titleVariety": title_variety_json,
        });

        if login == streamer {
            own_index = Some(i);
        }
        peers.push(Peer {
            login: login.clone(),
            entry,
            avg_viewers,
            avg_chatters,
            retention5m,
            title_variety,
            max_peak: max_peak_i,
            sessions,
        });
    }

    let total = peers.len();

    // Pro-Metrik-Rang (nur wenn eigener Streamer dabei).
    let mut metrics_ranked = serde_json::Map::new();
    if own_index.is_some() {
        for metric in [
            "avgViewers",
            "maxPeak",
            "avgChatters",
            "retention5m",
            "titleVariety",
            "sessions",
        ] {
            let mut ordered: Vec<&Peer> = peers.iter().collect();
            ordered.sort_by(|a, b| {
                metric_value(b, metric)
                    .partial_cmp(&metric_value(a, metric))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            for (j, p) in ordered.iter().enumerate() {
                if p.login == streamer {
                    metrics_ranked.insert(
                        metric.to_string(),
                        json!({ "rank": j + 1, "total": total, "value": p.entry[metric].clone() }),
                    );
                    break;
                }
            }
        }
    }

    // Ähnliche (±50 %) vs. aspirationale (1.5×–4×) Peers.
    let mut similar: Vec<&Value> = Vec::new();
    let mut aspirational: Vec<&Value> = Vec::new();
    if let Some(oi) = own_index {
        let own_avg = peers[oi].avg_viewers;
        for p in &peers {
            if p.login == streamer {
                continue;
            }
            let pv = p.avg_viewers;
            if own_avg * 0.5 <= pv && pv <= own_avg * 1.5 {
                similar.push(&p.entry);
            } else if own_avg * 1.5 < pv && pv <= own_avg * 4.0 {
                aspirational.push(&p.entry);
            }
        }
    }

    // Abstand zum nächsthöheren (peers[own_rank-2] = peers[own_index-1]).
    let gap: Value = match own_index {
        Some(oi) if oi >= 1 => {
            let nxt = &peers[oi - 1];
            let own = &peers[oi];
            json!({
                "login": nxt.login,
                "avgViewersDiff": round1(nxt.avg_viewers - own.avg_viewers),
                "chatDiff": round1(nxt.avg_chatters - own.avg_chatters),
                "retentionDiff": round1(nxt.retention5m - own.retention5m),
            })
        }
        _ => Value::Null,
    };

    let own_data = own_index.map_or(Value::Null, |oi| peers[oi].entry.clone());
    let own_rank = own_index.map_or(0, |oi| oi + 1);
    let similar_out: Vec<&Value> = similar.into_iter().take(5).collect();
    let aspirational_out: Vec<&Value> = aspirational.into_iter().take(5).collect();

    Ok(json!({
        "ownData": own_data,
        "ownRank": own_rank,
        "totalStreamers": total,
        "similarPeers": similar_out,
        "aspirationalPeers": aspirational_out,
        "metricsRanked": Value::Object(metrics_ranked),
        "gapToNext": gap,
    }))
}

/// Wochentags-Kürzel (Python `weekday_names`, Index = EXTRACT(DOW), So=0).
const WEEKDAY_NAMES: &[&str] = &["So", "Mo", "Di", "Mi", "Do", "Fr", "Sa"];

/// Konkurrenz-Dichte (Python `_competition_density`): aktive Streamer +
/// Kategorie-Ø je Stunde/Wochentag, eigene Performance als Overlay, plus die
/// besten Gelegenheits-Stunden (Ø-Viewer pro Konkurrent).
pub async fn competition_density(
    pool: &PgPool,
    streamer: &str,
    since: DateTime<Utc>,
) -> Result<Value, sqlx::Error> {
    // 1) Stündliche Dichte (alle Streamer, abgeschlossene Sessions).
    let density: Vec<(i32, i64, Option<f64>, Option<f64>)> = sqlx::query!(
        r#"
        SELECT EXTRACT(HOUR FROM (s.started_at AT TIME ZONE 'UTC'))::int AS "hour!",
               COUNT(DISTINCT s.streamer_login)::bigint AS "streamers!",
               AVG(s.avg_viewers)::float8 AS avg_viewers,
               AVG(s.peak_viewers)::float8 AS avg_peak
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1
          AND s.duration_seconds > 300
          AND s.ended_at IS NOT NULL
        GROUP BY 1
        ORDER BY 1
        "#,
        since
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.hour, row.streamers, row.avg_viewers, row.avg_peak))
    .collect();

    // 2) Eigene Performance je Stunde (KEIN ended_at-Filter, 1:1 Python).
    let own_hours: Vec<(i32, i64, Option<f64>, Option<f64>, Option<f64>)> = sqlx::query!(
        r#"
        SELECT EXTRACT(HOUR FROM (s.started_at AT TIME ZONE 'UTC'))::int AS "hour!",
               COUNT(*)::bigint AS "count!",
               AVG(s.avg_viewers)::float8 AS avg_viewers,
               AVG(s.peak_viewers)::float8 AS avg_peak,
               AVG(s.unique_chatters)::float8 AS avg_chatters
        FROM twitch_stream_sessions s
        WHERE s.streamer_login = $1
          AND s.started_at >= $2
          AND s.duration_seconds > 300
        GROUP BY 1
        ORDER BY 1
        "#,
        streamer,
        since
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        (
            row.hour,
            row.count,
            row.avg_viewers,
            row.avg_peak,
            row.avg_chatters,
        )
    })
    .collect();

    let mut own_map: HashMap<i32, Value> = HashMap::new();
    for (hour, cnt, avg_v, avg_peak, avg_chat) in &own_hours {
        own_map.insert(
            *hour,
            json!({
                "count": cnt,
                "avgViewers": retention_value(*avg_v),
                "avgPeak": retention_value(*avg_peak),
                "avgChatters": retention_value(*avg_chat),
            }),
        );
    }

    let mut hourly: Vec<Value> = Vec::new();
    let mut opp_values: Vec<f64> = Vec::new();
    for (hour, streamers, avg_v_opt, avg_p_opt) in &density {
        // opp nutzt den GERUNDETEN avg_v (1:1 Python).
        let avg_v_num = round1(avg_v_opt.unwrap_or(0.0));
        let (opp_num, opp_json) = if *streamers > 0 {
            let o = round2(avg_v_num / *streamers as f64);
            (o, json!(o))
        } else {
            (0.0, json!(0))
        };
        hourly.push(json!({
            "hour": hour,
            "activeStreamers": streamers,
            "avgViewers": retention_value(*avg_v_opt),
            "avgPeak": retention_value(*avg_p_opt),
            "opportunityScore": opp_json,
            "yourData": own_map.get(hour).cloned().unwrap_or(Value::Null),
        }));
        opp_values.push(opp_num);
    }

    // 3) Dichte je Wochentag.
    let weekday_density: Vec<(i32, i64, Option<f64>)> = sqlx::query!(
        r#"
        SELECT EXTRACT(DOW FROM (s.started_at AT TIME ZONE 'UTC'))::int AS "weekday!",
               COUNT(DISTINCT s.streamer_login)::bigint AS "streamers!",
               AVG(s.avg_viewers)::float8 AS avg_viewers
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1
          AND s.duration_seconds > 300
          AND s.ended_at IS NOT NULL
        GROUP BY 1
        ORDER BY 1
        "#,
        since
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.weekday, row.streamers, row.avg_viewers))
    .collect();

    // 4) Eigene Performance je Wochentag.
    let own_weekdays: Vec<(i32, i64, Option<f64>, Option<f64>)> = sqlx::query!(
        r#"
        SELECT EXTRACT(DOW FROM (s.started_at AT TIME ZONE 'UTC'))::int AS "weekday!",
               COUNT(*)::bigint AS "count!",
               AVG(s.avg_viewers)::float8 AS avg_viewers,
               AVG(s.peak_viewers)::float8 AS avg_peak
        FROM twitch_stream_sessions s
        WHERE s.streamer_login = $1
          AND s.started_at >= $2
          AND s.duration_seconds > 300
        GROUP BY 1
        "#,
        streamer,
        since
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.weekday, row.count, row.avg_viewers, row.avg_peak))
    .collect();

    let mut own_wd_map: HashMap<i32, Value> = HashMap::new();
    for (wd, cnt, avg_v, avg_peak) in &own_weekdays {
        own_wd_map.insert(
            *wd,
            json!({
                "count": cnt,
                "avgViewers": retention_value(*avg_v),
                "avgPeak": retention_value(*avg_peak),
            }),
        );
    }

    let weekly: Vec<Value> = weekday_density
        .iter()
        .map(|(wd, streamers, avg_v)| {
            json!({
                "weekday": wd,
                "weekdayLabel": WEEKDAY_NAMES.get(*wd as usize).copied().unwrap_or(""),
                "activeStreamers": streamers,
                "avgViewers": retention_value(*avg_v),
                "yourData": own_wd_map.get(wd).cloned().unwrap_or(Value::Null),
            })
        })
        .collect();

    // Sweet Spots: stabiler Sort nach opportunityScore DESC, Top 5.
    let mut idx: Vec<usize> = (0..hourly.len()).collect();
    idx.sort_by(|&a, &b| {
        opp_values[b]
            .partial_cmp(&opp_values[a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let sweet_spots: Vec<Value> = idx.iter().take(5).map(|&i| hourly[i].clone()).collect();

    Ok(json!({
        "hourly": hourly,
        "weekly": weekly,
        "sweetSpots": sweet_spots,
    }))
}

// --- Recommendations-Engine (Python `_build_recommendations`) ---------------

fn num(obj: &Value, key: &str) -> f64 {
    obj.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}
fn int_of(obj: &Value, key: &str) -> i64 {
    obj.get(key).and_then(Value::as_i64).unwrap_or(0)
}
fn text_of(obj: &Value, key: &str) -> String {
    obj.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}
fn value_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map_or(false, |f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}
fn truthy(obj: &Value, key: &str) -> bool {
    obj.get(key).map_or(false, value_truthy)
}
fn present_non_null(obj: &Value, key: &str) -> bool {
    obj.get(key).map_or(false, |v| !v.is_null())
}
/// Formatiert einen Wert wie Pythons `f"{value}"`: Float → kürzeste
/// Round-Trip-Repr (`{:?}` = "50.0"/"0.2", wie CPython-`repr`), Integer als
/// Ganzzahl ("0"/"10"), String unverändert.
fn pyfmt(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "None".to_string(),
        Some(Value::Bool(b)) => if *b { "True" } else { "False" }.to_string(),
        Some(Value::Number(n)) => {
            if n.is_f64() {
                format!("{:?}", n.as_f64().unwrap())
            } else {
                n.to_string()
            }
        }
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}
fn prio_rank(r: &Value) -> u8 {
    match r.get("priority").and_then(Value::as_str) {
        Some("critical") => 0,
        Some("high") => 1,
        Some("medium") => 2,
        Some("low") => 3,
        _ => 4,
    }
}

/// Regelbasierte Empfehlungs-Engine (Python `_build_recommendations`).
/// Konsumiert die 12 zusammengesetzten Analyse-Resultate (`data`) und erzeugt
/// priorisierte Empfehlungen, sortiert nach Priorität (critical→low).
pub fn build_recommendations(data: &Value) -> Vec<Value> {
    let null = Value::Null;
    let mut recs: Vec<Value> = Vec::new();

    // Doppel-Streams erkannt.
    let ds = data.get("doubleStreamDetection").unwrap_or(&null);
    if truthy(ds, "detected") && int_of(ds, "count") > 0 {
        let mut impact = String::new();
        if truthy(ds, "singleDayAvg") && truthy(ds, "doubleDayAvg") {
            let diff = num(ds, "singleDayAvg") - num(ds, "doubleDayAvg");
            if diff > 0.0 {
                impact = format!("An Single-Stream-Tagen hast du {:.0} mehr Ø Viewer.", diff);
            }
        }
        let count = int_of(ds, "count");
        let est = if impact.is_empty() {
            "Konsistenter Schedule = bessere Zuschauerbindung".to_string()
        } else {
            impact
        };
        recs.push(json!({
            "priority": "critical",
            "category": "Schedule",
            "title": "Doppel-Streams erkannt",
            "description": format!("{}x hast du an einem Tag mehrfach gestreamt. Das kann dein Wachstum bremsen, weil Viewer nicht wissen, wann du ON bist.", count),
            "estimatedImpact": est,
            "evidence": format!("{} Tage mit mehreren Sessions", count),
            "icon": "AlertTriangle",
        }));
    }

    // Effizienz unter 25. Perzentil.
    let eff = data.get("efficiency").unwrap_or(&null);
    if eff
        .get("percentile")
        .and_then(Value::as_f64)
        .unwrap_or(50.0)
        < 25.0
        && num(eff, "totalStreamHours") > 5.0
    {
        recs.push(json!({
            "priority": "critical",
            "category": "Effizienz",
            "title": "Unterdurchschnittliche Effizienz",
            "description": format!("Deine Viewer-Hours pro Stream-Hour ({}) liegen unter dem 25. Perzentil. Der Kategorie-Durchschnitt liegt bei {}.", pyfmt(eff.get("viewerHoursPerStreamHour")), pyfmt(eff.get("categoryAvg"))),
            "estimatedImpact": "Kuerzere, fokussiertere Streams koennten deine Effizienz deutlich steigern",
            "evidence": format!("Perzentil {}% | Du: {} vs Kat: {}", pyfmt(eff.get("percentile")), pyfmt(eff.get("viewerHoursPerStreamHour")), pyfmt(eff.get("categoryAvg"))),
            "icon": "TrendingDown",
        }));
    }

    // Retention unter 50 % des Kategorie-Schnitts.
    let ret = data.get("retentionCoaching").unwrap_or(&null);
    if truthy(ret, "category5mRetention") && truthy(ret, "your5mRetention") {
        let yours = num(ret, "your5mRetention");
        let cat = num(ret, "category5mRetention");
        if yours < cat * 0.5 && cat > 0.0 {
            recs.push(json!({
                "priority": "high",
                "category": "Retention",
                "title": "Niedrige 5-Minuten-Retention",
                "description": format!("Deine 5-Min-Retention ({}%) liegt deutlich unter dem Kategorie-Schnitt ({}%). Viewer springen frueh ab.", pyfmt(ret.get("your5mRetention")), pyfmt(ret.get("category5mRetention"))),
                "estimatedImpact": "Bessere Intros und fruehe Interaktion koennen die Retention verdoppeln",
                "evidence": format!("Du: {}% vs Kategorie: {}%", pyfmt(ret.get("your5mRetention")), pyfmt(ret.get("category5mRetention"))),
                "icon": "UserMinus",
            }));
        }
    }

    // Dauer über 150 % des Optimums.
    let dur = data.get("durationAnalysis").unwrap_or(&null);
    if truthy(dur, "optimalLabel") && truthy(dur, "currentAvgHours") {
        let optimal_mid = match text_of(dur, "optimalLabel").as_str() {
            "< 1h" => 0.5,
            "1-2h" => 1.5,
            "2-3h" => 2.5,
            "3-4h" => 3.5,
            "4-5h" => 4.5,
            "5h+" => 6.0,
            _ => 3.0,
        };
        let cur = num(dur, "currentAvgHours");
        if cur > optimal_mid * 1.5 && cur > 2.0 {
            recs.push(json!({
                "priority": "high",
                "category": "Dauer",
                "title": "Streams zu lang",
                "description": format!("Dein Ø Stream dauert {:.1}h, aber dein Sweet-Spot liegt bei {}. Laengere Streams verwassern deine Metriken.", cur, text_of(dur, "optimalLabel")),
                "estimatedImpact": format!("Kuerze auf {} fuer bessere Viewer-Zahlen", text_of(dur, "optimalLabel")),
                "evidence": format!("Optimaler Bucket: {} | Aktuell: {:.1}h", text_of(dur, "optimalLabel"), cur),
                "icon": "Clock",
            }));
        }
    }

    // Schedule-Mismatch (zu viel Konkurrenz in eigenen Slots).
    let sched = data.get("scheduleOptimizer").unwrap_or(&null);
    if truthy(sched, "yourCurrentSlots") && truthy(sched, "competitionHeatmap") {
        let slots = sched
            .get("yourCurrentSlots")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let your_slots_set: HashSet<(i64, i64)> = slots
            .iter()
            .map(|s| (int_of(s, "weekday"), int_of(s, "hour")))
            .collect();
        let mut comp = sched
            .get("competitionHeatmap")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        comp.sort_by(|a, b| int_of(b, "competitors").cmp(&int_of(a, "competitors")));
        let take = comp.len() / 4;
        let high_comp_set: HashSet<(i64, i64)> = comp[..take]
            .iter()
            .map(|s| (int_of(s, "weekday"), int_of(s, "hour")))
            .collect();
        if !your_slots_set.is_empty() && !high_comp_set.is_empty() {
            let overlap = your_slots_set.intersection(&high_comp_set).count();
            let overlap_pct = overlap as f64 / your_slots_set.len() as f64 * 100.0;
            if overlap_pct > 70.0 {
                recs.push(json!({
                    "priority": "high",
                    "category": "Schedule",
                    "title": "Zu viel Konkurrenz in deinen Slots",
                    "description": format!("{:.0}% deiner Streams laufen in den konkurrenzstaerksten Zeitfenstern. Verschiebe einige Streams in Sweet-Spots.", overlap_pct),
                    "estimatedImpact": "Weniger Konkurrenz = mehr Discovery durch Browse-Tab",
                    "evidence": format!("{}/{} Slots in Top-25% Konkurrenz", overlap, your_slots_set.len()),
                    "icon": "Calendar",
                }));
            }
        }
    }

    // Fehlende Top-Tags.
    let tags = data.get("tagOptimization").unwrap_or(&null);
    if truthy(tags, "missingHighPerformers") {
        let full = tags.get("missingHighPerformers").and_then(Value::as_array);
        let missing: Vec<String> = full
            .map(|a| {
                a.iter()
                    .take(3)
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        recs.push(json!({
            "priority": "high",
            "category": "Tags",
            "title": "Erfolgreiche Tags fehlen",
            "description": format!("Dir fehlen Tags, die in der Kategorie gut performen: {}. Tags beeinflussen die Sichtbarkeit im Browse-Tab.", missing.join(", ")),
            "estimatedImpact": "Bessere Tags = mehr Discovery ueber Twitch Browse",
            "evidence": format!("{} fehlende High-Performer Tags", full.map_or(0, |a| a.len())),
            "icon": "Tag",
        }));
    }

    // Titel zu oft wiederholt (> 5x).
    let titles = data.get("titleAnalysis").unwrap_or(&null);
    if truthy(titles, "yourTitles") {
        let your_titles = titles.get("yourTitles").and_then(Value::as_array);
        let max_reuse = your_titles.map_or(0, |a| {
            a.iter().map(|t| int_of(t, "usageCount")).max().unwrap_or(0)
        });
        if max_reuse > 5 {
            let reused: Vec<&Value> = your_titles
                .map(|a| a.iter().filter(|t| int_of(t, "usageCount") > 5).collect())
                .unwrap_or_default();
            let evidence = if let Some(first) = reused.first() {
                let t40: String = text_of(first, "title").chars().take(40).collect();
                format!("'{}...' wurde {}x benutzt", t40, max_reuse)
            } else {
                String::new()
            };
            recs.push(json!({
                "priority": "medium",
                "category": "Titel",
                "title": "Titel-Wiederholung",
                "description": format!("Du nutzt denselben Titel zu oft ({}x). Variiere deine Titel, um im Browse-Tab aufzufallen.", max_reuse),
                "estimatedImpact": "Einzigartige Titel erhoehen die Klickrate",
                "evidence": evidence,
                "icon": "Type",
            }));
        }
    }

    // Community-Abhängigkeit (> 80 % extern).
    let comm = data.get("crossCommunity").unwrap_or(&null);
    if present_non_null(comm, "isolatedPercentage")
        && num(comm, "isolatedPercentage") < 20.0
        && int_of(comm, "totalUniqueChatters") > 10
    {
        recs.push(json!({
            "priority": "medium",
            "category": "Community",
            "title": "Hohe Abhaengigkeit vom Oekosystem",
            "description": format!("Nur {:.0}% deiner Chatter sind exklusiv in deinem Channel. Du bist stark vom Deadlock-Oekosystem abhaengig.", num(comm, "isolatedPercentage")),
            "estimatedImpact": "Eigene Community aufbauen = nachhaltiges Wachstum",
            "evidence": format!("{} von {} Chattern nur bei dir", int_of(comm, "isolatedChatters"), int_of(comm, "totalUniqueChatters")),
            "icon": "Users",
        }));
    }

    // Fehlende Titel-Keywords.
    if truthy(titles, "yourMissingPatterns") {
        let full = titles.get("yourMissingPatterns").and_then(Value::as_array);
        let keywords: Vec<String> = full
            .map(|a| {
                a.iter()
                    .take(5)
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        recs.push(json!({
            "priority": "low",
            "category": "Titel",
            "title": "Fehlende Titel-Keywords",
            "description": format!("Erfolgreiche Streamer nutzen Keywords, die du nicht verwendest: {}.", keywords.join(", ")),
            "estimatedImpact": "Kleine Optimierung fuer bessere Discoverability",
            "evidence": format!("{} fehlende Patterns", full.map_or(0, |a| a.len())),
            "icon": "Search",
        }));
    }

    // Negative Korrelation Dauer↔Viewer.
    if present_non_null(dur, "correlation") && num(dur, "correlation") < -0.3 {
        recs.push(json!({
            "priority": "medium",
            "category": "Dauer",
            "title": "Negative Korrelation: Laenge vs Viewer",
            "description": format!("Je laenger du streamst, desto weniger Viewer hast du (r={:.2}). Das deutet auf Ermuedung hin.", num(dur, "correlation")),
            "estimatedImpact": "Kuerzere Streams koennen den Viewer-Schnitt steigern",
            "evidence": format!("Pearson r = {:.2}", num(dur, "correlation")),
            "icon": "TrendingDown",
        }));
    }

    // Titel-Vielfalt extrem gering.
    if present_non_null(titles, "varietyPct") && truthy(titles, "avgPeerVarietyPct") {
        let own_var = num(titles, "varietyPct");
        let peer_var = num(titles, "avgPeerVarietyPct");
        if own_var < peer_var * 0.5 && int_of(titles, "totalSessionCount") >= 5 {
            recs.push(json!({
                "priority": "critical",
                "category": "Titel",
                "title": "Extrem geringe Titel-Vielfalt",
                "description": format!("Nur {}% einzigartige Titel vs {}% Peer-Durchschnitt. {} verschiedene bei {} Sessions.", pyfmt(titles.get("varietyPct")), pyfmt(titles.get("avgPeerVarietyPct")), int_of(titles, "uniqueTitleCount"), int_of(titles, "totalSessionCount")),
                "estimatedImpact": "Verschiedene Titel locken verschiedene Zielgruppen in der Browse-Page an",
                "evidence": format!("Du: {}% | Peers: {}%", pyfmt(titles.get("varietyPct")), pyfmt(titles.get("avgPeerVarietyPct"))),
                "icon": "Type",
            }));
        }
    }

    // Chat-Konzentration.
    let chat = data.get("chatConcentration").unwrap_or(&null);
    if num(chat, "top1Pct") > 50.0 && num(chat, "totalMessages") > 50.0 {
        let evidence = if truthy(chat, "topChatters") {
            let login = chat
                .get("topChatters")
                .and_then(|a| a.get(0))
                .map(|t| text_of(t, "login"))
                .unwrap_or_default();
            format!("Top-Chatter: {} ({}%)", login, pyfmt(chat.get("top1Pct")))
        } else {
            String::new()
        };
        recs.push(json!({
            "priority": "high",
            "category": "Community",
            "title": "Chat abhaengig von einer Person",
            "description": format!("Ein einzelner Chatter macht {}% aller Nachrichten aus. Wenn diese Person wegfaellt, stirbt der Chat.", pyfmt(chat.get("top1Pct"))),
            "estimatedImpact": "Neue Chatter aktiv einbinden, Fragen stellen, namentlich ansprechen",
            "evidence": evidence,
            "icon": "Users",
        }));
    } else if num(chat, "top3Pct") > 70.0 && num(chat, "totalMessages") > 50.0 {
        recs.push(json!({
            "priority": "high",
            "category": "Community",
            "title": "Chat von Top-3 dominiert",
            "description": format!("Top 3 Chatter machen {}% aller Nachrichten. Dein Chat ist fragil - diversifiziere die Beteiligung.", pyfmt(chat.get("top3Pct"))),
            "estimatedImpact": "Interaktive Formate (Coaching, Q&A) bringen neue Stimmen",
            "evidence": format!("HHI-Index: {:.0} (>2500 = hochkonzentriert)", num(chat, "concentrationIndex")),
            "icon": "Users",
        }));
    }

    // One-Timer-Rate über Peer-Schnitt +10.
    if num(chat, "ownOneTimerPct") > num(chat, "avgPeerOneTimerPct") + 10.0
        && int_of(chat, "totalChatters") >= 5
    {
        recs.push(json!({
            "priority": "medium",
            "category": "Community",
            "title": "Zu viele Einmal-Chatter",
            "description": format!("{}% deiner Chatter kommen nur einmal (Peers: {}%). Follow-up Strategien einsetzen: Discord, Social Media, persoenliche Begruessung.", pyfmt(chat.get("ownOneTimerPct")), pyfmt(chat.get("avgPeerOneTimerPct"))),
            "estimatedImpact": "Senkung der One-Timer-Rate um 10% = deutlich stabilerer Chat",
            "evidence": format!("Du: {}% | Peers: {}%", pyfmt(chat.get("ownOneTimerPct")), pyfmt(chat.get("avgPeerOneTimerPct"))),
            "icon": "UserMinus",
        }));
    }

    // Einseitiges Raid-Netzwerk.
    let raids = data.get("raidNetwork").unwrap_or(&null);
    if num(raids, "totalSent") > 5.0 && num(raids, "reciprocityRatio") < 0.3 {
        recs.push(json!({
            "priority": "medium",
            "category": "Netzwerk",
            "title": "Einseitiges Raid-Netzwerk",
            "description": format!("Du sendest {} Raids, erhaeltst aber nur {}. Reziprozitaet: {}x. Aktiv gegenseitige Raid-Partnerschaften aufbauen.", int_of(raids, "totalSent"), int_of(raids, "totalReceived"), pyfmt(raids.get("reciprocityRatio"))),
            "estimatedImpact": "Gegenseitige Raids bringen neue Viewer in deinen Channel",
            "evidence": format!("Gesendet: {} | Erhalten: {} | Mutual: {}", int_of(raids, "totalSent"), int_of(raids, "totalReceived"), int_of(raids, "mutualPartners")),
            "icon": "Users",
        }));
    }

    // Ungenutzter Sweet-Spot.
    let comp = data.get("competitionDensity").unwrap_or(&null);
    let sweet_non_empty = comp
        .get("sweetSpots")
        .and_then(Value::as_array)
        .map_or(false, |a| !a.is_empty());
    let hourly_non_empty = comp
        .get("hourly")
        .and_then(Value::as_array)
        .map_or(false, |a| !a.is_empty());
    if sweet_non_empty && hourly_non_empty {
        let best = &comp.get("sweetSpots").and_then(Value::as_array).unwrap()[0];
        if !truthy(best, "yourData") {
            let hour = int_of(best, "hour");
            recs.push(json!({
                "priority": "high",
                "category": "Schedule",
                "title": format!("Ungenutzter Sweet-Spot: {:02}:00 UTC", hour),
                "description": format!("Um {:02}:00 UTC gibt es nur {} aktive Streamer bei {} Ø Viewern. Du streamst dort nicht.", hour, int_of(best, "activeStreamers"), pyfmt(best.get("avgViewers"))),
                "estimatedImpact": format!("Opportunity-Score {} - hoechstes Viewer/Konkurrenz-Verhaeltnis", pyfmt(best.get("opportunityScore"))),
                "evidence": format!("{} Streamer | {} Ø Viewer", int_of(best, "activeStreamers"), pyfmt(best.get("avgViewers"))),
                "icon": "Calendar",
            }));
        }
    }

    // Nach Priorität sortieren (stabil → Regel-Reihenfolge bleibt bei Gleichstand).
    recs.sort_by_key(prio_rank);
    recs
}

/// Vollständige Coaching-Daten (Python `CoachingEngine.get_coaching_data`):
/// Session-Count-Guard, dann alle 12 Analysen + Empfehlungen + aiSummary.
/// `streamer` wird im Output original (nur getrimmt) echot, intern für Queries
/// kleingeschrieben.
pub async fn get_coaching_data(
    pool: &PgPool,
    streamer: &str,
    days: i64,
) -> Result<Value, sqlx::Error> {
    let streamer_login = streamer.to_lowercase();
    let since = Utc::now() - chrono::Duration::days(days);

    let count: i64 = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*)::bigint AS "count!"
        FROM twitch_stream_sessions
        WHERE streamer_login = $1
          AND started_at >= $2
        "#,
        &streamer_login,
        since
    )
    .fetch_one(pool)
    .await?;

    if count == 0 {
        return Ok(json!({
            "streamer": streamer,
            "days": days,
            "empty": true,
            "aiSummary": null,
        }));
    }

    let efficiency_v = efficiency(pool, &streamer_login, since).await?;
    let title_v = title_analysis(pool, &streamer_login, since).await?;
    let schedule_v = schedule_optimizer(pool, &streamer_login, since).await?;
    let duration_v = duration_analysis(pool, &streamer_login, since).await?;
    let cross_v = cross_community(pool, &streamer_login, since).await?;
    let tag_v = tag_optimization(pool, &streamer_login, since).await?;
    let retention_v = retention_coaching(pool, &streamer_login, since).await?;
    let double_v = double_stream_detection(pool, &streamer_login, since).await?;
    let concentration_v = chat_concentration(pool, &streamer_login, since).await?;
    let raid_v = raid_network(pool, &streamer_login, since).await?;
    let peer_v = peer_comparison(pool, &streamer_login, since).await?;
    let competition_v = competition_density(pool, &streamer_login, since).await?;

    let mut result = json!({
        "streamer": streamer,
        "days": days,
        "empty": false,
        "efficiency": efficiency_v,
        "titleAnalysis": title_v,
        "scheduleOptimizer": schedule_v,
        "durationAnalysis": duration_v,
        "crossCommunity": cross_v,
        "tagOptimization": tag_v,
        "retentionCoaching": retention_v,
        "doubleStreamDetection": double_v,
        "chatConcentration": concentration_v,
        "raidNetwork": raid_v,
        "peerComparison": peer_v,
        "competitionDensity": competition_v,
    });

    // recommendations konsumiert die 12 Analysen (ohne recommendations/aiSummary).
    let recommendations = build_recommendations(&result);
    result["recommendations"] = Value::Array(recommendations);
    result["aiSummary"] = Value::Null;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

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
            .options([("search_path", schema), ("timezone", "UTC")]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE twitch_stream_sessions (id BIGSERIAL PRIMARY KEY, streamer_login TEXT, started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ, duration_seconds INTEGER, avg_viewers REAL, follower_delta INTEGER, stream_title TEXT, peak_viewers INTEGER, unique_chatters INTEGER, retention_5m REAL, tags TEXT)")
            .execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn efficiency_leer() {
        let Some(pool) = make_pool("t_coach_eff_empty").await else {
            return;
        };
        let v = efficiency(&pool, "nani", Utc::now() - chrono::Duration::days(30))
            .await
            .unwrap();
        assert_eq!(v["viewerHoursPerStreamHour"], 0);
        assert_eq!(v["topPerformers"], json!([]));
    }

    #[tokio::test]
    async fn efficiency_berechnet() {
        let Some(pool) = make_pool("t_coach_eff").await else {
            return;
        };
        // nani: 2h Stream, avg 50 → viewer_hours 100, ratio 50. other: 2h, avg 10 → ratio 10.
        sqlx::query("INSERT INTO twitch_stream_sessions (streamer_login, started_at, duration_seconds, avg_viewers, follower_delta) VALUES \
            ('nani', NOW()-INTERVAL '1 day', 7200, 50, 20), \
            ('other', NOW()-INTERVAL '1 day', 7200, 10, 5)")
            .execute(&pool).await.unwrap();
        let v = efficiency(&pool, "nani", Utc::now() - chrono::Duration::days(30))
            .await
            .unwrap();
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
        assert_eq!(
            extract_keywords(&["der die und".to_string()]),
            Vec::<String>::new()
        );
        assert_eq!(extract_keywords(&["ab cd xyz".to_string()]), vec!["xyz"]);
        assert_eq!(
            extract_keywords(&["Übung Spaß".to_string()]),
            vec!["übung", "spaß"]
        );
    }

    #[tokio::test]
    async fn title_analysis_berechnet() {
        let Some(pool) = make_pool("t_coach_title").await else {
            return;
        };
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

    #[tokio::test]
    async fn schedule_optimizer_berechnet() {
        let Some(pool) = make_pool("t_coach_sched").await else {
            return;
        };
        sqlx::query("CREATE TABLE twitch_stats_category (id BIGSERIAL PRIMARY KEY, ts_utc TIMESTAMPTZ, streamer TEXT, viewer_count INTEGER)")
            .execute(&pool).await.unwrap();
        // Slot A (Stunde 14): 2 Konkurrenten Ø(100,200)=150 → opportunity 75.
        // Slot B (Stunde 20): 1 Konkurrent 500 → opportunity 500 (= bester).
        sqlx::query(
            "INSERT INTO twitch_stats_category (ts_utc, streamer, viewer_count) VALUES \
            (TIMESTAMPTZ '2026-06-08 14:00:00+00', 'sx', 100), \
            (TIMESTAMPTZ '2026-06-08 14:00:00+00', 'sy', 200), \
            (TIMESTAMPTZ '2026-06-09 20:00:00+00', 'sx', 500)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (streamer_login, started_at, duration_seconds, avg_viewers, follower_delta) VALUES \
            ('nani', TIMESTAMPTZ '2026-06-08 14:30:00+00', 7200, 40, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let v = schedule_optimizer(&pool, "nani", Utc::now() - chrono::Duration::days(365))
            .await
            .unwrap();

        // Heatmap: 2 Zellen.
        assert_eq!(v["competitionHeatmap"].as_array().unwrap().len(), 2);
        // Bester Sweet Spot = Slot B (Stunde 20, opportunity 500).
        assert_eq!(v["sweetSpots"][0]["hour"], 20);
        assert_eq!(v["sweetSpots"][0]["opportunityScore"], 500.0);
        assert_eq!(v["sweetSpots"][0]["competitors"], 1);
        // Slot A in Heatmap: categoryViewers 150, 2 Konkurrenten.
        let slot_a = v["competitionHeatmap"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["hour"] == 14)
            .unwrap();
        assert_eq!(slot_a["categoryViewers"], 150.0);
        assert_eq!(slot_a["competitors"], 2);
        // Eigener Slot: Stunde 14, 1 Stream.
        assert_eq!(v["yourCurrentSlots"][0]["hour"], 14);
        assert_eq!(v["yourCurrentSlots"][0]["count"], 1);
    }

    #[test]
    fn pearson_perfekt_und_zu_klein() {
        // Perfekt positiv korreliert → 1.0.
        let r = pearson(&[1.0, 2.0, 3.0, 4.0], &[2.0, 4.0, 6.0, 8.0]);
        assert!((r - 1.0).abs() < 1e-9, "r = {r}");
        // < 3 Werte → 0.0.
        assert_eq!(pearson(&[1.0, 2.0], &[1.0, 2.0]), 0.0);
        // Konstante Reihe (Std 0) → 0.0.
        assert_eq!(pearson(&[5.0, 5.0, 5.0], &[1.0, 2.0, 3.0]), 0.0);
    }

    #[tokio::test]
    async fn duration_analysis_berechnet() {
        let Some(pool) = make_pool("t_coach_dur").await else {
            return;
        };
        // 3 Streams im "1-2h"-Bucket (5400s): avg_v 40/50/60→50, chatters→12,
        // retention 80/NULL/90→85. Konstante Dauer → Korrelation 0.
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (streamer_login, started_at, duration_seconds, avg_viewers, unique_chatters, retention_5m) VALUES \
            ('nani', NOW()-INTERVAL '1 day', 5400, 40, 10, 80), \
            ('nani', NOW()-INTERVAL '1 day', 5400, 50, 12, NULL), \
            ('nani', NOW()-INTERVAL '1 day', 5400, 60, 14, 90)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let v = duration_analysis(&pool, "nani", Utc::now() - chrono::Duration::days(30))
            .await
            .unwrap();

        let bucket = v["buckets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["label"] == "1-2h")
            .unwrap();
        assert_eq!(bucket["streamCount"], 3);
        assert_eq!(bucket["avgViewers"], 50.0);
        assert_eq!(bucket["avgChatters"], 12.0);
        assert_eq!(bucket["avgRetention5m"], 85.0); // NULL ignoriert
        assert_eq!(bucket["efficiencyRatio"], 50.0);

        assert_eq!(v["optimalLabel"], "1-2h");
        assert_eq!(v["currentAvgHours"], 1.5);
        // Konstante Dauer → sx=0 → Korrelation 0.
        assert_eq!(v["correlation"], 0.0);
    }

    async fn make_rollup_pool(schema: &str) -> Option<PgPool> {
        let pool = make_pool(schema).await?;
        sqlx::query("CREATE TABLE twitch_chatter_rollup (streamer_login TEXT NOT NULL, chatter_login TEXT NOT NULL, first_seen_at TIMESTAMPTZ NOT NULL, last_seen_at TIMESTAMPTZ NOT NULL, total_messages INTEGER DEFAULT 0, total_sessions INTEGER DEFAULT 0, PRIMARY KEY (streamer_login, chatter_login))")
            .execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn cross_community_berechnet() {
        let Some(pool) = make_rollup_pool("t_coach_cross").await else {
            return;
        };
        // nani: a,b,c,d (4 unique). rivalx teilt a,b. rivaly teilt a.
        // → shared_count {a,b}=2, isolated=2 → 50 % → "Gute Mischung".
        sqlx::query(
            "INSERT INTO twitch_chatter_rollup (streamer_login, chatter_login, first_seen_at, last_seen_at) VALUES \
            ('nani','a', NOW()-INTERVAL '2 day', NOW()-INTERVAL '1 day'), \
            ('nani','b', NOW()-INTERVAL '2 day', NOW()-INTERVAL '1 day'), \
            ('nani','c', NOW()-INTERVAL '2 day', NOW()-INTERVAL '1 day'), \
            ('nani','d', NOW()-INTERVAL '2 day', NOW()-INTERVAL '1 day'), \
            ('rivalx','a', NOW()-INTERVAL '2 day', NOW()-INTERVAL '1 day'), \
            ('rivalx','b', NOW()-INTERVAL '2 day', NOW()-INTERVAL '1 day'), \
            ('rivaly','a', NOW()-INTERVAL '2 day', NOW()-INTERVAL '1 day')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let v = cross_community(&pool, "nani", Utc::now() - chrono::Duration::days(30))
            .await
            .unwrap();

        assert_eq!(v["totalUniqueChatters"], 4);
        // Quellen nach geteilten Chattern DESC: rivalx (2) vor rivaly (1).
        assert_eq!(v["chatterSources"].as_array().unwrap().len(), 2);
        assert_eq!(v["chatterSources"][0]["sourceStreamer"], "rivalx");
        assert_eq!(v["chatterSources"][0]["sharedChatters"], 2);
        assert_eq!(v["chatterSources"][0]["percentage"], 50.0);
        assert_eq!(v["chatterSources"][1]["sharedChatters"], 1);
        assert_eq!(v["chatterSources"][1]["percentage"], 25.0);
        assert_eq!(v["isolatedChatters"], 2);
        assert_eq!(v["isolatedPercentage"], 50.0);
        assert_eq!(
            v["ecosystemSummary"],
            "Gute Mischung: Ein Teil deiner Zuschauer kommt aus der Deadlock-Community, viele sind aber deine eigenen."
        );
    }

    #[tokio::test]
    async fn cross_community_leer() {
        let Some(pool) = make_rollup_pool("t_coach_cross_empty").await else {
            return;
        };
        let v = cross_community(&pool, "nani", Utc::now() - chrono::Duration::days(30))
            .await
            .unwrap();
        assert_eq!(v["totalUniqueChatters"], 0);
        assert_eq!(v["chatterSources"], json!([]));
        assert_eq!(v["ecosystemSummary"], "Keine Chatter-Daten verfuegbar.");
    }

    #[test]
    fn split_tags_from_rows_trennt_und_normalisiert() {
        let set = split_tags_from_rows(&[
            "Deadlock, German ;Chill".to_string(),
            "deadlock|PvP".to_string(),
            "".to_string(),
        ]);
        // dedup case-insensitiv, getrimmt, leere weg.
        assert!(set.contains("deadlock"));
        assert!(set.contains("german"));
        assert!(set.contains("chill"));
        assert!(set.contains("pvp"));
        assert_eq!(set.len(), 4);
    }

    #[tokio::test]
    async fn tag_optimization_berechnet() {
        let Some(pool) = make_pool("t_coach_tags").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (streamer_login, started_at, duration_seconds, avg_viewers, tags) VALUES \
            ('nani',  NOW()-INTERVAL '1 day', 7200, 90,  'Deadlock,German'), \
            ('nani',  NOW()-INTERVAL '1 day', 7200, 110, 'Deadlock,German'), \
            ('nani',  NOW()-INTERVAL '1 day', 7200, 20,  'Deadlock,Chill'), \
            ('other', NOW()-INTERVAL '1 day', 7200, 200, 'Deadlock,Pro'), \
            ('other', NOW()-INTERVAL '1 day', 7200, 200, 'Deadlock,Pro'), \
            ('other', NOW()-INTERVAL '1 day', 7200, 200, 'Deadlock,Pro')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let v = tag_optimization(&pool, "nani", Utc::now() - chrono::Duration::days(30))
            .await
            .unwrap();

        // Eigene Tags nach avg DESC: "Deadlock,German" (100) vor "Deadlock,Chill" (20).
        assert_eq!(v["yourTags"][0]["tags"], "Deadlock,German");
        assert_eq!(v["yourTags"][0]["avgViewers"], 100.0);
        assert_eq!(v["yourTags"][0]["usageCount"], 2);
        assert_eq!(v["yourTags"][1]["tags"], "Deadlock,Chill");

        // Kategorie: nur "Deadlock,Pro" (other, COUNT 3) erfüllt HAVING>=3.
        assert_eq!(v["categoryBestTags"][0]["tags"], "Deadlock,Pro");
        assert_eq!(v["categoryBestTags"][0]["streamerCount"], 1);

        // Fehlend: "pro" (deadlock kennt nani schon). Einziges Element → deterministisch.
        assert_eq!(v["missingHighPerformers"], json!(["pro"]));

        // Underperformer: your_avg=(100+20)/2=60, Schwelle 48 → nur "Deadlock,Chill".
        assert_eq!(v["underperformingTags"], json!(["Deadlock,Chill"]));
    }

    #[tokio::test]
    async fn retention_coaching_berechnet() {
        let Some(pool) = make_pool("t_coach_ret").await else {
            return;
        };
        sqlx::query("CREATE TABLE twitch_session_viewers (session_id BIGINT NOT NULL, ts_utc TIMESTAMPTZ NOT NULL, minutes_from_start INTEGER, viewer_count INTEGER NOT NULL, PRIMARY KEY(session_id, ts_utc))")
            .execute(&pool).await.unwrap();

        // nani: retention 80 (peak 100) + 90 (peak 0) → AVG 85.
        let nani_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_stream_sessions (streamer_login, started_at, retention_5m, peak_viewers, avg_viewers) \
             VALUES ('nani', NOW()-INTERVAL '1 day', 80, 100, 40) RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (streamer_login, started_at, retention_5m, peak_viewers, avg_viewers) \
             VALUES ('nani', NOW()-INTERVAL '2 day', 90, 0, 30)",
        )
        .execute(&pool)
        .await
        .unwrap();
        // other: retention 70 (peak 200) → cat AVG (80+90+70)/3 = 80.
        let other_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_stream_sessions (streamer_login, started_at, retention_5m, peak_viewers, avg_viewers) \
             VALUES ('other', NOW()-INTERVAL '1 day', 70, 200, 500) RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        // Viewer-Timeline: nani fällt min0=100 % → min5=50 % (kritischer Drop).
        sqlx::query(
            "INSERT INTO twitch_session_viewers (session_id, ts_utc, minutes_from_start, viewer_count) VALUES \
            ($1, TIMESTAMPTZ '2026-06-14 12:00:00+00', 0, 100), \
            ($1, TIMESTAMPTZ '2026-06-14 12:05:00+00', 5, 50), \
            ($2, TIMESTAMPTZ '2026-06-14 12:00:00+00', 0, 200)",
        )
        .bind(nani_id)
        .bind(other_id)
        .execute(&pool)
        .await
        .unwrap();

        let v = retention_coaching(&pool, "nani", Utc::now() - chrono::Duration::days(30))
            .await
            .unwrap();

        assert_eq!(v["your5mRetention"], 85.0);
        assert_eq!(v["category5mRetention"], 80.0);
        // Kurve: 13 Marken (0,5,..,60).
        assert_eq!(v["yourViewerCurve"].as_array().unwrap().len(), 13);
        assert_eq!(
            v["yourViewerCurve"][0],
            json!({ "minute": 0, "avgViewerPct": 100.0 })
        );
        assert_eq!(
            v["yourViewerCurve"][1],
            json!({ "minute": 5, "avgViewerPct": 50.0 })
        );
        // 50 < 100*0.9=90 → kritischer Drop bei Minute 5.
        assert_eq!(v["criticalDropoffMinute"], 5);
        // Top-Performer = other: 200/200 = 100 % bei Minute 0.
        assert_eq!(
            v["topPerformerCurve"][0],
            json!({ "minute": 0, "avgViewerPct": 100.0 })
        );
    }

    #[tokio::test]
    async fn double_stream_detection_berechnet() {
        let Some(pool) = make_pool("t_coach_double").await else {
            return;
        };
        // 2026-06-10: 2 Sessions (avg 40/60 → Tages-Ø 50) = Doppel-Stream-Tag.
        // 2026-06-11: 1 Session (avg 30) = Einzel-Tag.
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (streamer_login, started_at, duration_seconds, avg_viewers) VALUES \
            ('nani', TIMESTAMPTZ '2026-06-10 12:00:00+00', 7200, 40), \
            ('nani', TIMESTAMPTZ '2026-06-10 20:00:00+00', 7200, 60), \
            ('nani', TIMESTAMPTZ '2026-06-11 18:00:00+00', 7200, 30)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let v = double_stream_detection(&pool, "nani", Utc::now() - chrono::Duration::days(365))
            .await
            .unwrap();

        assert_eq!(v["detected"], true);
        assert_eq!(v["count"], 1);
        assert_eq!(v["occurrences"][0]["date"], "2026-06-10");
        assert_eq!(v["occurrences"][0]["sessionCount"], 2);
        assert_eq!(v["occurrences"][0]["avgViewers"], 50.0);
        // Tages-Ø: Einzel-Tag 30, Doppel-Tag 50.
        assert_eq!(v["singleDayAvg"], 30.0);
        assert_eq!(v["doubleDayAvg"], 50.0);
    }

    #[tokio::test]
    async fn chat_concentration_berechnet() {
        let Some(pool) = make_rollup_pool("t_coach_conc").await else {
            return;
        };
        // nani: 4 Chatter, je 1 pro Bucket. msgs 100/50/30/20 = 200.
        // rival: 5 Chatter, 3 One-Timer → 60 % (Peer mit COUNT>=5).
        sqlx::query(
            "INSERT INTO twitch_chatter_rollup (streamer_login, chatter_login, first_seen_at, last_seen_at, total_messages, total_sessions) VALUES \
            ('nani','a', NOW()-INTERVAL '2 day', NOW()-INTERVAL '1 day', 100, 1), \
            ('nani','b', NOW()-INTERVAL '2 day', NOW()-INTERVAL '1 day', 50,  2), \
            ('nani','c', NOW()-INTERVAL '2 day', NOW()-INTERVAL '1 day', 30,  5), \
            ('nani','d', NOW()-INTERVAL '2 day', NOW()-INTERVAL '1 day', 20,  15), \
            ('rival','r1', NOW()-INTERVAL '2 day', NOW()-INTERVAL '1 day', 10, 1), \
            ('rival','r2', NOW()-INTERVAL '2 day', NOW()-INTERVAL '1 day', 10, 1), \
            ('rival','r3', NOW()-INTERVAL '2 day', NOW()-INTERVAL '1 day', 10, 1), \
            ('rival','r4', NOW()-INTERVAL '2 day', NOW()-INTERVAL '1 day', 10, 2), \
            ('rival','r5', NOW()-INTERVAL '2 day', NOW()-INTERVAL '1 day', 10, 4)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let v = chat_concentration(&pool, "nani", Utc::now() - chrono::Duration::days(30))
            .await
            .unwrap();

        assert_eq!(v["totalChatters"], 4);
        assert_eq!(v["totalMessages"], 200);
        assert_eq!(v["msgsPerChatter"], 50.0);
        // Buckets: je 1 Chatter = 25 %.
        assert_eq!(v["loyaltyBuckets"]["oneTimer"]["count"], 1);
        assert_eq!(v["loyaltyBuckets"]["oneTimer"]["pct"], 25.0);
        assert_eq!(v["loyaltyBuckets"]["oneTimer"]["messages"], 100);
        // Top-Chatter nach Nachrichten: a(100,50%) b(50,25%) c(30,15%) d(20,10%).
        assert_eq!(v["topChatters"][0]["login"], "a");
        assert_eq!(v["topChatters"][0]["sharePct"], 50.0);
        assert_eq!(v["topChatters"][0]["cumulativePct"], 50.0);
        assert_eq!(v["topChatters"][2]["cumulativePct"], 90.0);
        // HHI = (0.25+0.0625+0.0225+0.01)*10000 = 3450.
        assert_eq!(v["concentrationIndex"], 3450.0);
        assert_eq!(v["top1Pct"], 50.0);
        assert_eq!(v["top3Pct"], 90.0);
        assert_eq!(v["ownOneTimerPct"], 25.0);
        // Peer rival: 3/5 One-Timer = 60 %.
        assert_eq!(v["avgPeerOneTimerPct"], 60.0);
    }

    #[tokio::test]
    async fn raid_network_berechnet() {
        let Some(pool) = make_pool("t_coach_raid").await else {
            return;
        };
        sqlx::query("CREATE TABLE twitch_raid_history (from_broadcaster_login TEXT, to_broadcaster_login TEXT, viewer_count INTEGER, executed_at TIMESTAMPTZ, success BOOLEAN)")
            .execute(&pool).await.unwrap();
        // nani → partnera ×2 (100,200), → partnerb ×1 (50). nani ← partnera (80), ← partnerc (40).
        // Plus ein success=FALSE-Raid → muss gefiltert werden.
        sqlx::query(
            "INSERT INTO twitch_raid_history (from_broadcaster_login, to_broadcaster_login, viewer_count, executed_at, success) VALUES \
            ('nani','partnera', 100, NOW()-INTERVAL '1 day', TRUE), \
            ('nani','partnera', 200, NOW()-INTERVAL '1 day', TRUE), \
            ('nani','partnerb', 50,  NOW()-INTERVAL '1 day', TRUE), \
            ('partnera','nani', 80,  NOW()-INTERVAL '1 day', TRUE), \
            ('partnerc','nani', 40,  NOW()-INTERVAL '1 day', TRUE), \
            ('nani','partnerx', 999, NOW()-INTERVAL '1 day', FALSE)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let v = raid_network(&pool, "nani", Utc::now() - chrono::Duration::days(30))
            .await
            .unwrap();

        // success=FALSE-Raid raus → totalSent 3, nicht 4.
        assert_eq!(v["totalSent"], 3);
        assert_eq!(v["totalReceived"], 2);
        assert_eq!(v["totalSentViewers"], 350);
        assert_eq!(v["totalReceivedViewers"], 120);
        assert_eq!(v["avgSentViewers"], 116.7); // 350/3
        assert_eq!(v["avgReceivedViewers"], 60.0);
        assert_eq!(v["reciprocityRatio"], 0.67); // 2/3
        assert_eq!(v["mutualPartners"], 1);
        assert_eq!(v["totalPartners"], 3);

        // partnera = höchste Gesamt-Raids (3) → erster, mutual.
        assert_eq!(v["partners"][0]["login"], "partnera");
        assert_eq!(v["partners"][0]["reciprocity"], "mutual");
        assert_eq!(v["partners"][0]["sentCount"], 2);
        assert_eq!(v["partners"][0]["receivedCount"], 1);
        assert_eq!(v["partners"][0]["balance"], -1);
        assert_eq!(v["partners"][0]["sentAvgViewers"], 150.0);
        assert_eq!(v["partners"][0]["receivedAvgViewers"], 80.0);

        // partnerb (sentOnly) / partnerc (receivedOnly) — Tie, daher per login finden.
        let arr = v["partners"].as_array().unwrap();
        let pb = arr.iter().find(|p| p["login"] == "partnerb").unwrap();
        assert_eq!(pb["reciprocity"], "sentOnly");
        assert_eq!(pb["sentAvgViewers"], 50.0);
        assert_eq!(pb["receivedAvgViewers"], 0); // fehlt → Int-0
        let pc = arr.iter().find(|p| p["login"] == "partnerc").unwrap();
        assert_eq!(pc["reciprocity"], "receivedOnly");
        assert_eq!(pc["sentAvgViewers"], 0); // fehlt → Int-0
        assert_eq!(pc["receivedAvgViewers"], 40.0);
        // partnerx (success=FALSE) nicht vorhanden.
        assert!(arr.iter().all(|p| p["login"] != "partnerx"));
    }

    #[tokio::test]
    async fn peer_comparison_berechnet() {
        let Some(pool) = make_pool("t_coach_peer").await else {
            return;
        };
        // 4 Streamer je 3 Sessions (HAVING>=3). avg_v: alpha150, gamma60, nani50, beta20.
        // nani 3 distinct titles → titleVariety 100; andere 1 → 33.3.
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (streamer_login, started_at, ended_at, duration_seconds, avg_viewers, peak_viewers, unique_chatters, retention_5m, follower_delta, stream_title) VALUES \
            ('nani',  NOW()-INTERVAL '1 day', NOW(), 7200, 50,  100, 10, 0.5,  5, 't1'), \
            ('nani',  NOW()-INTERVAL '2 day', NOW(), 7200, 50,  100, 10, 0.5,  5, 't2'), \
            ('nani',  NOW()-INTERVAL '3 day', NOW(), 7200, 50,  100, 10, 0.5,  5, 't3'), \
            ('alpha', NOW()-INTERVAL '1 day', NOW(), 7200, 150, 300, 30, 0.6, 10, 'a'), \
            ('alpha', NOW()-INTERVAL '2 day', NOW(), 7200, 150, 300, 30, 0.6, 10, 'a'), \
            ('alpha', NOW()-INTERVAL '3 day', NOW(), 7200, 150, 300, 30, 0.6, 10, 'a'), \
            ('gamma', NOW()-INTERVAL '1 day', NOW(), 7200, 60,  120, 12, 0.55, 6, 'g'), \
            ('gamma', NOW()-INTERVAL '2 day', NOW(), 7200, 60,  120, 12, 0.55, 6, 'g'), \
            ('gamma', NOW()-INTERVAL '3 day', NOW(), 7200, 60,  120, 12, 0.55, 6, 'g'), \
            ('beta',  NOW()-INTERVAL '1 day', NOW(), 7200, 20,  40,  5,  0.4,  2, 'b'), \
            ('beta',  NOW()-INTERVAL '2 day', NOW(), 7200, 20,  40,  5,  0.4,  2, 'b'), \
            ('beta',  NOW()-INTERVAL '3 day', NOW(), 7200, 20,  40,  5,  0.4,  2, 'b')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let v = peer_comparison(&pool, "nani", Utc::now() - chrono::Duration::days(30))
            .await
            .unwrap();

        // Sortiert avg_v DESC: alpha, gamma, nani, beta → nani Rang 3 von 4.
        assert_eq!(v["ownRank"], 3);
        assert_eq!(v["totalStreamers"], 4);
        assert_eq!(v["ownData"]["login"], "nani");
        assert_eq!(v["ownData"]["avgViewers"], 50.0);
        assert_eq!(v["ownData"]["retention5m"], 50.0); // 0.5*100
        assert_eq!(v["ownData"]["titleVariety"], 100.0); // 3/3
        assert_eq!(v["ownData"]["maxPeak"], 100);
        assert_eq!(v["ownData"]["uniqueTitles"], 3);

        // Ähnlich (25..75): gamma(60). Aspirational (75..200): alpha(150).
        assert_eq!(v["similarPeers"].as_array().unwrap().len(), 1);
        assert_eq!(v["similarPeers"][0]["login"], "gamma");
        assert_eq!(v["aspirationalPeers"].as_array().unwrap().len(), 1);
        assert_eq!(v["aspirationalPeers"][0]["login"], "alpha");

        // Gap zum nächsthöheren (gamma, Rang 2).
        assert_eq!(v["gapToNext"]["login"], "gamma");
        assert_eq!(v["gapToNext"]["avgViewersDiff"], 10.0); // 60-50
        assert_eq!(v["gapToNext"]["chatDiff"], 2.0); // 12-10
        assert_eq!(v["gapToNext"]["retentionDiff"], 5.0); // 55-50

        // Pro-Metrik-Rang.
        assert_eq!(v["metricsRanked"]["avgViewers"]["rank"], 3);
        assert_eq!(v["metricsRanked"]["avgViewers"]["value"], 50.0);
        assert_eq!(v["metricsRanked"]["avgViewers"]["total"], 4);
        // titleVariety: nani 100 ist Höchstwert → Rang 1.
        assert_eq!(v["metricsRanked"]["titleVariety"]["rank"], 1);
    }

    #[tokio::test]
    async fn competition_density_berechnet() {
        let Some(pool) = make_pool("t_coach_compdens").await else {
            return;
        };
        // Alle am 2026-06-10. Stunde 14: nani(50)+other(100) → 2 Streamer, Ø75.
        // Stunde 20: nur nani(80) → 1 Streamer, Ø80 (bester Sweet Spot).
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (streamer_login, started_at, ended_at, duration_seconds, avg_viewers, peak_viewers, unique_chatters) VALUES \
            ('nani',  TIMESTAMPTZ '2026-06-10 14:00:00+00', NOW(), 7200, 50,  100, 10), \
            ('other', TIMESTAMPTZ '2026-06-10 14:00:00+00', NOW(), 7200, 100, 200, 20), \
            ('nani',  TIMESTAMPTZ '2026-06-10 20:00:00+00', NOW(), 7200, 80,  150, 12)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let v = competition_density(&pool, "nani", Utc::now() - chrono::Duration::days(365))
            .await
            .unwrap();

        // Stündlich: 2 Buckets (14, 20), nach Stunde sortiert.
        let hourly = v["hourly"].as_array().unwrap();
        assert_eq!(hourly.len(), 2);
        assert_eq!(hourly[0]["hour"], 14);
        assert_eq!(hourly[0]["activeStreamers"], 2);
        assert_eq!(hourly[0]["avgViewers"], 75.0); // (50+100)/2
        assert_eq!(hourly[0]["opportunityScore"], 37.5); // 75/2
        assert_eq!(hourly[0]["yourData"]["count"], 1);
        assert_eq!(hourly[0]["yourData"]["avgViewers"], 50.0);
        assert_eq!(hourly[1]["hour"], 20);
        assert_eq!(hourly[1]["opportunityScore"], 80.0); // 80/1

        // Sweet Spots: Stunde 20 (80) vor Stunde 14 (37.5).
        assert_eq!(v["sweetSpots"][0]["hour"], 20);
        assert_eq!(v["sweetSpots"][1]["hour"], 14);

        // Wochentag: ein Bucket (2026-06-10), 2 aktive Streamer.
        let weekly = v["weekly"].as_array().unwrap();
        assert_eq!(weekly.len(), 1);
        assert_eq!(weekly[0]["activeStreamers"], 2);
        assert_eq!(weekly[0]["avgViewers"], 76.7); // (50+100+80)/3
                                                   // Label = WEEKDAY_NAMES[DOW(2026-06-10)] (So=0).
        use chrono::Datelike;
        let dow = chrono::NaiveDate::from_ymd_opt(2026, 6, 10)
            .unwrap()
            .weekday()
            .num_days_from_sunday();
        assert_eq!(weekly[0]["weekday"], dow);
        assert_eq!(weekly[0]["weekdayLabel"], WEEKDAY_NAMES[dow as usize]);
        // yourData: nani 2 Sessions an dem Tag, Ø(50,80)=65.
        assert_eq!(weekly[0]["yourData"]["count"], 2);
        assert_eq!(weekly[0]["yourData"]["avgViewers"], 65.0);
    }

    #[test]
    fn pyfmt_float_vs_int() {
        // Float → Python-repr ("50.0"/"0.2"), Integer → ganzzahlig ("0"/"10").
        assert_eq!(pyfmt(Some(&json!(50.0))), "50.0");
        assert_eq!(pyfmt(Some(&json!(0.2))), "0.2");
        assert_eq!(pyfmt(Some(&json!(133.3))), "133.3");
        assert_eq!(pyfmt(Some(&json!(10))), "10");
        assert_eq!(pyfmt(Some(&json!(0))), "0");
        assert_eq!(pyfmt(Some(&json!("abc"))), "abc");
        assert_eq!(pyfmt(None), "None");
    }

    #[test]
    fn build_recommendations_regeln_und_sortierung() {
        let data = json!({
            "doubleStreamDetection": {"detected": true, "count": 3, "singleDayAvg": 50.0, "doubleDayAvg": 30.0},
            "efficiency": {"percentile": 10, "totalStreamHours": 20.0, "viewerHoursPerStreamHour": 50.0, "categoryAvg": 133.3},
            "raidNetwork": {"totalSent": 10, "totalReceived": 2, "reciprocityRatio": 0.2, "mutualPartners": 1},
        });
        let recs = build_recommendations(&data);

        // 2 critical (double, efficiency) + 1 medium (raid), critical zuerst.
        assert_eq!(recs.len(), 3);
        assert_eq!(recs[0]["priority"], "critical");
        assert_eq!(recs[1]["priority"], "critical");
        assert_eq!(recs[2]["priority"], "medium");

        // Regel-Reihenfolge innerhalb critical: double vor efficiency.
        assert_eq!(recs[0]["title"], "Doppel-Streams erkannt");
        assert_eq!(
            recs[0]["estimatedImpact"],
            "An Single-Stream-Tagen hast du 20 mehr Ø Viewer."
        );
        assert_eq!(recs[0]["evidence"], "3 Tage mit mehreren Sessions");

        assert_eq!(recs[1]["title"], "Unterdurchschnittliche Effizienz");
        // Float-Repr im Text: 50.0 / 133.3 (nicht "50"/"133").
        assert_eq!(
            recs[1]["evidence"],
            "Perzentil 10% | Du: 50.0 vs Kat: 133.3"
        );

        assert_eq!(recs[2]["title"], "Einseitiges Raid-Netzwerk");
        assert_eq!(
            recs[2]["description"],
            "Du sendest 10 Raids, erhaeltst aber nur 2. Reziprozitaet: 0.2x. Aktiv gegenseitige Raid-Partnerschaften aufbauen."
        );
        assert_eq!(
            recs[2]["evidence"],
            "Gesendet: 10 | Erhalten: 2 | Mutual: 1"
        );
    }

    #[test]
    fn build_recommendations_leer() {
        // Leere Daten → keine Regel feuert.
        assert!(build_recommendations(&json!({})).is_empty());
    }

    /// Pool mit ALLEN Tabellen, die get_coaching_data anfasst.
    async fn make_full_pool(schema: &str) -> Option<PgPool> {
        let pool = make_pool(schema).await?;
        sqlx::query("CREATE TABLE twitch_stats_category (id BIGSERIAL PRIMARY KEY, ts_utc TIMESTAMPTZ, streamer TEXT, viewer_count INTEGER)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_chatter_rollup (streamer_login TEXT NOT NULL, chatter_login TEXT NOT NULL, first_seen_at TIMESTAMPTZ NOT NULL, last_seen_at TIMESTAMPTZ NOT NULL, total_messages INTEGER DEFAULT 0, total_sessions INTEGER DEFAULT 0, PRIMARY KEY (streamer_login, chatter_login))").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_session_viewers (session_id BIGINT NOT NULL, ts_utc TIMESTAMPTZ NOT NULL, minutes_from_start INTEGER, viewer_count INTEGER NOT NULL, PRIMARY KEY(session_id, ts_utc))").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_raid_history (from_broadcaster_login TEXT, to_broadcaster_login TEXT, viewer_count INTEGER, executed_at TIMESTAMPTZ, success BOOLEAN)").execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn get_coaching_data_leer() {
        let Some(pool) = make_pool("t_coach_assembly_empty").await else {
            return;
        };
        // Keine Sessions → empty-Guard.
        let v = get_coaching_data(&pool, "Nani", 30).await.unwrap();
        assert_eq!(v["streamer"], "Nani"); // Original-Case echot
        assert_eq!(v["days"], 30);
        assert_eq!(v["empty"], true);
        assert_eq!(v["aiSummary"], Value::Null);
        // Keine Analyse-Keys im empty-Fall.
        assert!(v.get("efficiency").is_none());
    }

    #[tokio::test]
    async fn get_coaching_data_voll() {
        let Some(pool) = make_full_pool("t_coach_assembly_full").await else {
            return;
        };
        // Eine nani-Session (lowercase in DB) → count>0, empty:false.
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (streamer_login, started_at, ended_at, duration_seconds, avg_viewers, peak_viewers, retention_5m) VALUES \
            ('nani', NOW()-INTERVAL '1 day', NOW(), 7200, 50, 100, 0.5)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Eingabe in Original-Case "Nani" → intern lowercase-Match.
        let v = get_coaching_data(&pool, "Nani", 30).await.unwrap();
        assert_eq!(v["streamer"], "Nani");
        assert_eq!(v["empty"], false);
        // Alle 12 Analyse-Keys + recommendations + aiSummary vorhanden.
        for key in [
            "efficiency",
            "titleAnalysis",
            "scheduleOptimizer",
            "durationAnalysis",
            "crossCommunity",
            "tagOptimization",
            "retentionCoaching",
            "doubleStreamDetection",
            "chatConcentration",
            "raidNetwork",
            "peerComparison",
            "competitionDensity",
        ] {
            assert!(v.get(key).is_some(), "Key {key} fehlt");
        }
        assert!(v["recommendations"].is_array());
        assert_eq!(v["aiSummary"], Value::Null);
    }
}
