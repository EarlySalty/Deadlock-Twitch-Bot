//! Report-Writer — Aggregations- und Formatierungsschicht (Port von
//! `bot/social_media/analytics/report_writer.py`, reiner Teil).
//!
//! Lädt Clip-Performance aus `twitch_clips_social_analytics`, aggregiert sie je
//! Streamer/Gesamt und baut die Prompt-Listen sowie die Fallback-Markdown-Reports.
//! Die LLM-gestützte Generierung + `write_*`-Orchestrierung folgt im nächsten
//! Slice.

use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use sqlx::{PgPool, Row};

/// Aggregierte Performance eines Clips über alle Plattformen.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipPerformance {
    pub clip_db_id: i32,
    pub title: String,
    pub streamer_login: String,
    pub clip_url: Option<String>,
    pub game_name: Option<String>,
    pub created_at: Option<String>,
    pub platforms: Vec<String>,
    pub views: i64,
    pub likes: i64,
    pub comments: i64,
    pub shares: i64,
    pub watch_time_seconds: i64,
    pub ctr_percent: Option<f64>,
    pub engagement_rate: Option<f64>,
    pub score: f64,
}

/// Aggregierte Performance eines Streamers.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamerPerformance {
    pub streamer_login: String,
    pub clip_count: i64,
    pub views: i64,
    pub likes: i64,
    pub comments: i64,
    pub shares: i64,
    pub watch_time_seconds: i64,
    pub engagement_rate: Option<f64>,
    pub top_clip_title: Option<String>,
}

/// Aggregierte Gesamtwerte (Python `_aggregate_totals`).
#[derive(Debug, Clone, PartialEq)]
pub struct Totals {
    pub views: i64,
    pub likes: i64,
    pub comments: i64,
    pub shares: i64,
    pub watch_time_seconds: i64,
    pub engagement_rate: Option<f64>,
}

/// Report-Zeitraum (Woche/Monat).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeriodKind {
    Week,
    Month,
}

/// Aufgelöster Zeitraum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Period {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

fn mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

/// Lädt + aggregiert Clip-Performance (Python `_load_clip_performance`).
pub async fn load_clip_performance(
    pool: &PgPool,
    bucket: &str,
    period_start: &str,
    period_end: &str,
    streamer_login: Option<&str>,
) -> Vec<ClipPerformance> {
    let rows = sqlx::query(
        "SELECT c.id AS clip_db_id, c.streamer_login, c.clip_title, c.clip_url, \
                c.created_at::text AS created_at, c.game_name, a.platform, \
                a.views, a.likes, a.comments, a.shares, a.watch_time_seconds, \
                a.ctr_percent::double precision AS ctr, a.engagement_rate::double precision AS eng \
           FROM twitch_clips_social_analytics a \
           JOIN twitch_clips_social_media c ON c.id = a.clip_id \
          WHERE a.bucket = $1 AND a.synced_at >= $2::timestamptz AND a.synced_at < $3::timestamptz \
            AND c.discarded_at IS NULL AND COALESCE(a.provider, '') NOT LIKE 'error:%' \
            AND ($4::text IS NULL OR LOWER(c.streamer_login) = LOWER($4)) \
          ORDER BY c.id ASC, a.platform ASC, a.synced_at DESC",
    )
    .bind(bucket)
    .bind(period_start)
    .bind(period_end)
    .bind(streamer_login)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    // Pro (Clip, Plattform) nur den neuesten Snapshot (Rows sind synced_at DESC).
    let mut seen: std::collections::HashSet<(i32, String)> = std::collections::HashSet::new();
    // Gruppiert je Clip, in Einfüge-Reihenfolge (Clip-ID asc, Plattform asc).
    let mut order: Vec<i32> = Vec::new();
    let mut grouped: std::collections::HashMap<i32, Vec<PlatformRow>> = std::collections::HashMap::new();
    for r in &rows {
        let clip_db_id: i32 = r.try_get("clip_db_id").unwrap_or(0);
        let platform: String = r.try_get("platform").unwrap_or_default();
        if !seen.insert((clip_db_id, platform.clone())) {
            continue;
        }
        let pr = PlatformRow {
            clip_db_id,
            streamer_login: r.try_get::<Option<String>, _>("streamer_login").unwrap_or(None).unwrap_or_default(),
            clip_title: r.try_get::<Option<String>, _>("clip_title").unwrap_or(None),
            clip_url: r.try_get::<Option<String>, _>("clip_url").unwrap_or(None),
            game_name: r.try_get::<Option<String>, _>("game_name").unwrap_or(None),
            created_at: r.try_get::<Option<String>, _>("created_at").unwrap_or(None),
            platform,
            views: r.try_get::<Option<i32>, _>("views").unwrap_or(None).unwrap_or(0) as i64,
            likes: r.try_get::<Option<i32>, _>("likes").unwrap_or(None).unwrap_or(0) as i64,
            comments: r.try_get::<Option<i32>, _>("comments").unwrap_or(None).unwrap_or(0) as i64,
            shares: r.try_get::<Option<i32>, _>("shares").unwrap_or(None).unwrap_or(0) as i64,
            watch_time_seconds: r.try_get::<Option<i32>, _>("watch_time_seconds").unwrap_or(None).unwrap_or(0) as i64,
            ctr_percent: r.try_get::<Option<f64>, _>("ctr").unwrap_or(None),
            engagement_rate: r.try_get::<Option<f64>, _>("eng").unwrap_or(None),
        };
        grouped.entry(clip_db_id).or_insert_with(|| {
            order.push(clip_db_id);
            Vec::new()
        });
        grouped.get_mut(&clip_db_id).unwrap().push(pr);
    }

    order.iter().map(|id| aggregate_clip(&grouped[id])).collect()
}

struct PlatformRow {
    clip_db_id: i32,
    streamer_login: String,
    clip_title: Option<String>,
    clip_url: Option<String>,
    game_name: Option<String>,
    created_at: Option<String>,
    platform: String,
    views: i64,
    likes: i64,
    comments: i64,
    shares: i64,
    watch_time_seconds: i64,
    ctr_percent: Option<f64>,
    engagement_rate: Option<f64>,
}

fn aggregate_clip(clip_rows: &[PlatformRow]) -> ClipPerformance {
    let first = &clip_rows[0];
    let views: i64 = clip_rows.iter().map(|r| r.views).sum();
    let likes: i64 = clip_rows.iter().map(|r| r.likes).sum();
    let comments: i64 = clip_rows.iter().map(|r| r.comments).sum();
    let shares: i64 = clip_rows.iter().map(|r| r.shares).sum();
    let watch_time_seconds: i64 = clip_rows.iter().map(|r| r.watch_time_seconds).sum();
    let ctr_values: Vec<f64> = clip_rows.iter().filter_map(|r| r.ctr_percent).collect();
    let engagement_values: Vec<f64> = clip_rows.iter().filter_map(|r| r.engagement_rate).collect();

    let mut score = views as f64 + likes as f64 * 4.0 + comments as f64 * 8.0 + shares as f64 * 10.0;
    if let Some(m) = mean(&engagement_values) {
        score += m * 15.0;
    }

    let mut platforms: Vec<String> = clip_rows.iter().map(|r| r.platform.clone()).collect();
    platforms.sort();

    ClipPerformance {
        clip_db_id: first.clip_db_id,
        title: first.clip_title.clone().filter(|t| !t.is_empty()).unwrap_or_else(|| format!("Clip {}", first.clip_db_id)),
        streamer_login: first.streamer_login.clone(),
        clip_url: first.clip_url.clone().filter(|s| !s.is_empty()),
        game_name: first.game_name.clone().filter(|s| !s.is_empty()),
        created_at: first.created_at.clone().filter(|s| !s.is_empty()),
        platforms,
        views,
        likes,
        comments,
        shares,
        watch_time_seconds,
        ctr_percent: mean(&ctr_values).map(round2),
        engagement_rate: mean(&engagement_values).map(round2),
        score: round2(score),
    }
}

/// Aggregiert je Streamer, sortiert nach (views, watch_time) absteigend.
pub fn aggregate_streamers(clips: &[ClipPerformance]) -> Vec<StreamerPerformance> {
    let mut order: Vec<String> = Vec::new();
    let mut grouped: std::collections::HashMap<String, Vec<&ClipPerformance>> = std::collections::HashMap::new();
    for clip in clips {
        grouped.entry(clip.streamer_login.clone()).or_insert_with(|| {
            order.push(clip.streamer_login.clone());
            Vec::new()
        });
        grouped.get_mut(&clip.streamer_login).unwrap().push(clip);
    }

    let mut items: Vec<StreamerPerformance> = order
        .iter()
        .map(|login| {
            let sc = &grouped[login];
            let top_clip = sc.iter().max_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal));
            let engagement_values: Vec<f64> = sc.iter().filter_map(|c| c.engagement_rate).collect();
            StreamerPerformance {
                streamer_login: login.clone(),
                clip_count: sc.len() as i64,
                views: sc.iter().map(|c| c.views).sum(),
                likes: sc.iter().map(|c| c.likes).sum(),
                comments: sc.iter().map(|c| c.comments).sum(),
                shares: sc.iter().map(|c| c.shares).sum(),
                watch_time_seconds: sc.iter().map(|c| c.watch_time_seconds).sum(),
                engagement_rate: mean(&engagement_values).map(round2),
                top_clip_title: top_clip.map(|c| c.title.clone()),
            }
        })
        .collect();
    items.sort_by(|a, b| (b.views, b.watch_time_seconds).cmp(&(a.views, a.watch_time_seconds)));
    items
}

/// Gesamtwerte über alle Clips.
pub fn aggregate_totals(clips: &[ClipPerformance]) -> Totals {
    let engagement_values: Vec<f64> = clips.iter().filter_map(|c| c.engagement_rate).collect();
    Totals {
        views: clips.iter().map(|c| c.views).sum(),
        likes: clips.iter().map(|c| c.likes).sum(),
        comments: clips.iter().map(|c| c.comments).sum(),
        shares: clips.iter().map(|c| c.shares).sum(),
        watch_time_seconds: clips.iter().map(|c| c.watch_time_seconds).sum(),
        engagement_rate: mean(&engagement_values).map(round2),
    }
}

fn midnight(dt: DateTime<Utc>) -> DateTime<Utc> {
    dt.with_hour(0).unwrap().with_minute(0).unwrap().with_second(0).unwrap().with_nanosecond(0).unwrap()
}

fn first_of_month(dt: DateTime<Utc>) -> DateTime<Utc> {
    midnight(dt).with_day(1).unwrap()
}

/// Löst den Report-Zeitraum auf (Python `_coerce_period`), `now` injizierbar.
pub fn coerce_period_at(
    now: DateTime<Utc>,
    period_start: Option<DateTime<Utc>>,
    period_end: Option<DateTime<Utc>>,
    default: PeriodKind,
) -> Period {
    match (period_start, period_end) {
        (_, None) => match default {
            PeriodKind::Month => {
                let anchor = first_of_month(now);
                let start = first_of_month(anchor - Duration::days(1));
                Period { start, end: anchor }
            }
            PeriodKind::Week => {
                let monday = midnight(now) - Duration::days(now.weekday().num_days_from_monday() as i64);
                Period { start: monday - Duration::days(7), end: monday }
            }
        },
        (None, Some(end)) => {
            let delta = if default == PeriodKind::Month { Duration::days(30) } else { Duration::days(7) };
            Period { start: end - delta, end }
        }
        (Some(start), Some(end)) => Period { start, end },
    }
}

/// `now`-basierter Wrapper (Prod).
pub fn coerce_period(period_start: Option<DateTime<Utc>>, period_end: Option<DateTime<Utc>>, default: PeriodKind) -> Period {
    coerce_period_at(Utc::now(), period_start, period_end, default)
}

/// "dd.mm.YYYY bis dd.mm.YYYY" (Endzeitpunkt minus 1s, Python `_format_period`).
pub fn format_period(period: &Period) -> String {
    format!("{} bis {}", period.start.format("%d.%m.%Y"), (period.end - Duration::seconds(1)).format("%d.%m.%Y"))
}

fn fmt_pct(value: Option<f64>) -> String {
    match value {
        None => "n/a".to_string(),
        Some(v) => format!("{v:.2}%"),
    }
}

/// Nummerierte Clip-Liste für den Prompt (Python `_format_clip_list`).
pub fn format_clip_list(clips: &[ClipPerformance]) -> String {
    if clips.is_empty() {
        return "- keine Clips".to_string();
    }
    clips
        .iter()
        .enumerate()
        .map(|(i, c)| {
            format!(
                "{}. {} | streamer={} | views={} | likes={} | comments={} | shares={} | er={} | plattformen={}",
                i + 1,
                c.title,
                c.streamer_login,
                c.views,
                c.likes,
                c.comments,
                c.shares,
                fmt_pct(c.engagement_rate),
                c.platforms.join(",")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Nummerierte Streamer-Liste für den Prompt (Python `_format_streamer_list`).
pub fn format_streamer_list(streamers: &[StreamerPerformance]) -> String {
    if streamers.is_empty() {
        return "- keine Streamer".to_string();
    }
    streamers
        .iter()
        .enumerate()
        .map(|(i, s)| {
            format!(
                "{}. @{} | clips={} | views={} | shares={} | er={} | top_clip={}",
                i + 1,
                s.streamer_login,
                s.clip_count,
                s.views,
                s.shares,
                fmt_pct(s.engagement_rate),
                s.top_clip_title.as_deref().unwrap_or("n/a")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Fallback-Markdown bei fehlenden Daten.
pub fn fallback_no_data_report(heading: &str, period: &Period, note: &str) -> String {
    format!("{heading}\n\nZeitraum: {}\n\n## Status\n{note}\n", format_period(period))
}

/// Fallback-Wochenreport eines Streamers.
pub fn fallback_streamer_report(
    streamer_login: &str,
    period: &Period,
    top: &[ClipPerformance],
    bottom: &[ClipPerformance],
    totals: &Totals,
) -> String {
    format!(
        "# Wochenreport · @{streamer_login}\n\nZeitraum: {period}\n\n## TL;DR\n\
         In der Woche kamen {views} Views und {shares} Shares zusammen. \
         Die durchschnittliche Engagement-Rate lag bei {er}.\n\n\
         ## Top 5\n{top_list}\n\n## Bottom 5\n{bottom_list}\n\n\
         ## Massnahmen naechste Woche\n\
         - Mehr Varianten des staerksten Clip-Musters in den ersten 3 Sekunden testen.\n\
         - Schwache Clips mit geringer Share-Quote auf Hook und Caption ueberarbeiten.\n\
         - Plattformen mit niedriger CTR im Dashboard gezielt gegen die Top-Clips vergleichen.\n",
        period = format_period(period),
        views = totals.views,
        shares = totals.shares,
        er = fmt_pct(totals.engagement_rate),
        top_list = format_clip_list(top),
        bottom_list = format_clip_list(bottom),
    )
}

/// Fallback-Monatsreport (Cross-Streamer).
pub fn fallback_cross_report(period: &Period, streamers: &[StreamerPerformance], clips: &[ClipPerformance]) -> String {
    let mut ranked: Vec<ClipPerformance> = clips.to_vec();
    ranked.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    let top_clips: Vec<ClipPerformance> = ranked.into_iter().take(8).collect();
    format!(
        "# Monatsreport · Cross-Streamer\n\nZeitraum: {period}\n\n## Gesamtbild\n\
         Mit Daten vertreten: {n} Streamer.\n\n## Streamer-Ranking\n{ranking}\n\n## Top Clips\n{top}\n\n\
         ## Naechste 30 Tage\n\
         - Erfolgreiche Hook-Muster der Top-Streamer uebertragen.\n\
         - Schwache Streamer zuerst auf Share- und Comment-Quote optimieren.\n\
         - 30d-Buckets gegen 7d-Buckets vergleichen, um Ausreisser schnell zu erkennen.\n",
        period = format_period(period),
        n = streamers.len(),
        ranking = format_streamer_list(streamers.iter().take(10).cloned().collect::<Vec<_>>().as_slice()),
        top = format_clip_list(&top_clips),
    )
}

/// Fallback-Admin-Wochenreport.
pub fn fallback_admin_report(period: &Period, streamers: &[StreamerPerformance], top_clips: &[ClipPerformance]) -> String {
    format!(
        "# Admin-Wochenreport · Social Media\n\nZeitraum: {period}\n\n## Executive Summary\n\
         Verwertbare Daten liegen fuer {n} Streamer vor.\n\n## Auffaellige Streamer\n{ranking}\n\n## Top Clips\n{top}\n\n\
         ## Admin-Aktionen\n\
         - Ausreisser mit hoher Engagement-Rate fuer Layout-/Title-Patterns markieren.\n\
         - Streamer mit wenig Views, aber hoher CTR auf mehr Upload-Volumen pushen.\n\
         - Fehlende Plattformdaten im Analytics-Tab pruefen und OAuth/API-Ausfaelle verfolgen.\n",
        period = format_period(period),
        n = streamers.len(),
        ranking = format_streamer_list(streamers.iter().take(10).cloned().collect::<Vec<_>>().as_slice()),
        top = format_clip_list(top_clips),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    fn clip(id: i32, login: &str, views: i64, score: f64, er: Option<f64>) -> ClipPerformance {
        ClipPerformance {
            clip_db_id: id,
            title: format!("Clip {id}"),
            streamer_login: login.into(),
            clip_url: None,
            game_name: None,
            created_at: None,
            platforms: vec!["tiktok".into()],
            views,
            likes: 0,
            comments: 0,
            shares: 0,
            watch_time_seconds: 0,
            ctr_percent: None,
            engagement_rate: er,
            score,
        }
    }

    #[test]
    fn coerce_period_week_und_month() {
        // Mittwoch 2026-06-17 12:00 UTC (weekday=2).
        let now = DateTime::parse_from_rfc3339("2026-06-17T12:00:00+00:00").unwrap().with_timezone(&Utc);
        let wk = coerce_period_at(now, None, None, PeriodKind::Week);
        assert_eq!(wk.end, DateTime::parse_from_rfc3339("2026-06-15T00:00:00+00:00").unwrap()); // Montag
        assert_eq!(wk.start, DateTime::parse_from_rfc3339("2026-06-08T00:00:00+00:00").unwrap());
        let mo = coerce_period_at(now, None, None, PeriodKind::Month);
        assert_eq!(mo.end, DateTime::parse_from_rfc3339("2026-06-01T00:00:00+00:00").unwrap());
        assert_eq!(mo.start, DateTime::parse_from_rfc3339("2026-05-01T00:00:00+00:00").unwrap());
        // end gegeben, start None → start = end - 7d.
        let end = DateTime::parse_from_rfc3339("2026-06-15T00:00:00+00:00").unwrap().with_timezone(&Utc);
        let p = coerce_period_at(now, None, Some(end), PeriodKind::Week);
        assert_eq!(p.start, DateTime::parse_from_rfc3339("2026-06-08T00:00:00+00:00").unwrap());
    }

    #[test]
    fn formatter_und_aggregation() {
        let period = Period {
            start: DateTime::parse_from_rfc3339("2026-06-08T00:00:00+00:00").unwrap().with_timezone(&Utc),
            end: DateTime::parse_from_rfc3339("2026-06-15T00:00:00+00:00").unwrap().with_timezone(&Utc),
        };
        assert_eq!(format_period(&period), "08.06.2026 bis 14.06.2026"); // end-1s
        assert_eq!(fmt_pct(Some(12.5)), "12.50%");
        assert_eq!(fmt_pct(None), "n/a");
        assert_eq!(format_clip_list(&[]), "- keine Clips");

        let clips = vec![clip(1, "nani", 100, 50.0, Some(10.0)), clip(2, "nani", 300, 80.0, Some(20.0)), clip(3, "other", 50, 5.0, None)];
        let totals = aggregate_totals(&clips);
        assert_eq!(totals.views, 450);
        assert_eq!(totals.engagement_rate, Some(15.0)); // mean(10,20)
        let streamers = aggregate_streamers(&clips);
        // nani (400 views) vor other (50).
        assert_eq!(streamers[0].streamer_login, "nani");
        assert_eq!(streamers[0].views, 400);
        assert_eq!(streamers[0].top_clip_title.as_deref(), Some("Clip 2")); // höchster score
        assert_eq!(streamers[1].streamer_login, "other");
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(3).connect_with(opts).await.unwrap();
        for ddl in [
            "CREATE TABLE twitch_clips_social_media (id SERIAL PRIMARY KEY, streamer_login TEXT, clip_title TEXT, clip_url TEXT, created_at TIMESTAMPTZ DEFAULT NOW(), game_name TEXT, discarded_at TIMESTAMPTZ)",
            "CREATE TABLE twitch_clips_social_analytics (id SERIAL PRIMARY KEY, clip_id INTEGER, platform TEXT, bucket TEXT, views INTEGER, likes INTEGER, comments INTEGER, shares INTEGER, watch_time_seconds INTEGER, ctr_percent NUMERIC(5,2), engagement_rate NUMERIC(5,2), provider TEXT, synced_at TIMESTAMPTZ)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn load_aggregiert_plattformen_und_latest() {
        let Some(pool) = make_pool("t_sm_report_load").await else { return };
        let c: i32 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (streamer_login, clip_title) VALUES ('nani', 'Mein Clip') RETURNING id").fetch_one(&pool).await.unwrap();
        // tiktok: alter + neuer Snapshot (nur neuer zählt).
        sqlx::query("INSERT INTO twitch_clips_social_analytics (clip_id, platform, bucket, views, likes, engagement_rate, provider, synced_at) VALUES ($1, 'tiktok', '7d', 50, 1, 5.0, 'ok', NOW() - INTERVAL '2 hours')").bind(c).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_clips_social_analytics (clip_id, platform, bucket, views, likes, engagement_rate, provider, synced_at) VALUES ($1, 'tiktok', '7d', 100, 10, 10.0, 'ok', NOW())").bind(c).execute(&pool).await.unwrap();
        // youtube: ein Snapshot.
        sqlx::query("INSERT INTO twitch_clips_social_analytics (clip_id, platform, bucket, views, likes, engagement_rate, provider, synced_at) VALUES ($1, 'youtube', '7d', 200, 20, 20.0, 'ok', NOW())").bind(c).execute(&pool).await.unwrap();
        // error-Provider → ausgeschlossen.
        sqlx::query("INSERT INTO twitch_clips_social_analytics (clip_id, platform, bucket, views, provider, synced_at) VALUES ($1, 'instagram', '7d', 999, 'error:instagram:api', NOW())").bind(c).execute(&pool).await.unwrap();

        let clips = load_clip_performance(&pool, "7d", "2000-01-01T00:00:00+00:00", "2100-01-01T00:00:00+00:00", None).await;
        assert_eq!(clips.len(), 1);
        let cp = &clips[0];
        // views = latest tiktok (100) + youtube (200) = 300, instagram-error nicht.
        assert_eq!(cp.views, 300);
        assert_eq!(cp.likes, 30);
        assert_eq!(cp.platforms, vec!["tiktok".to_string(), "youtube".to_string()]);
        assert_eq!(cp.engagement_rate, Some(15.0)); // mean(10,20)
        // score = 300 + 30*4 + 0 + 0 + mean(10,20)*15 = 300+120+225 = 645.
        assert_eq!(cp.score, 645.0);
        assert_eq!(cp.title, "Mein Clip");
    }
}
