//! Handler für `GET /twitch/api/v2/audience-demographics`.
//!
//! Port von `_api_v2_audience_demographics` (api_audience.py:1045).
//! Enthält die vollständige Engagement-Berechnung (engagement_metrics.py) und
//! `_compute_weighted_peak_hours` (api_audience.py:118) als reine Rust-Funktionen.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use std::collections::HashMap;

use crate::auth::level::DashboardAuthLevel;
use tb_analytics::engagement_metrics::{calculate_engagement, quantile, EngagementInputs};

const PEAK_SESSION_WINDOW: i64 = 30;
const PEAK_HALF_LIFE_SESSIONS: f64 = 8.0;

use tb_analytics::bekannte_bots::KNOWN_CHAT_BOTS;

// Engagement-Berechnung + Quantile leben jetzt in tb_analytics::engagement_metrics
// (geteilt mit chat-analytics; siehe `use` oben).

// ─── Sprach-Label ────────────────────────────────────────────────────────────

fn lang_label(code: &str) -> String {
    let c = code.to_lowercase();
    if c.starts_with("de") {
        "German".into()
    } else if c.starts_with("en") {
        "English".into()
    } else if c.starts_with("fr") {
        "French".into()
    } else if c.starts_with("es") {
        "Spanish".into()
    } else if c.starts_with("pt") {
        "Portuguese".into()
    } else if c.starts_with("tr") {
        "Turkish".into()
    } else if c.starts_with("pl") {
        "Polish".into()
    } else if c.starts_with("ru") {
        "Russian".into()
    } else if c.starts_with("it") {
        "Italian".into()
    } else if c == "unknown" {
        "Unbekannt".into()
    } else {
        c
    }
}

// ─── Gewichtete Peak-Hours (Port von _compute_weighted_peak_hours) ───────────

struct PeakResult {
    peak_hours: Vec<i32>,
    session_count: usize,
    sessions_with_activity: usize,
    sample_count: i64,
    coverage: f64,
}

async fn compute_weighted_peak_hours(
    pool: &PgPool,
    streamer: &str,
    since: DateTime<Utc>,
    tz_name: &str,
) -> Result<PeakResult, sqlx::Error> {
    let empty = PeakResult {
        peak_hours: vec![],
        session_count: 0,
        sessions_with_activity: 0,
        sample_count: 0,
        coverage: 0.0,
    };
    let bots: Vec<String> = KNOWN_CHAT_BOTS.iter().map(|s| s.to_string()).collect();

    // Q3a: letzte N Sessions (neueste zuerst)
    let sess_rows = sqlx::query!(
        "SELECT id FROM twitch_stream_sessions
         WHERE started_at >= $1 AND LOWER(streamer_login) = $2 AND ended_at IS NOT NULL
         ORDER BY started_at DESC LIMIT $3",
        since,
        streamer,
        PEAK_SESSION_WINDOW
    )
    .fetch_all(pool)
    .await?;

    let session_ids: Vec<i64> = sess_rows.into_iter().map(|r| r.id).collect();
    if session_ids.is_empty() {
        return Ok(empty);
    }
    let n = session_ids.len();

    // Exponentielles Recency-Gewicht: idx=0 = neueste Session (Gewicht=1.0)
    let session_weights: HashMap<i64, f64> = session_ids
        .iter()
        .enumerate()
        .map(|(idx, &sid)| (sid, 0.5_f64.powf(idx as f64 / PEAK_HALF_LIFE_SESSIONS)))
        .collect();

    // Q3b: Chat-Messages aggregiert nach (session_id, hour_in_tz)
    let msg_rows = sqlx::query!(
        "SELECT cm.session_id AS \"session_id!\", EXTRACT(HOUR FROM (cm.message_ts AT TIME ZONE $1))::int AS \"hour!\", COUNT(*) AS \"cnt!\"
         FROM twitch_chat_messages cm
         WHERE cm.session_id = ANY($2::bigint[])
           AND (cm.chatter_login IS NULL OR cm.chatter_login = ''
                OR (LOWER(cm.chatter_login) <> ALL($3::text[]) AND LOWER(cm.chatter_login) !~ '^justinfan[0-9]+$'))
         GROUP BY cm.session_id, EXTRACT(HOUR FROM (cm.message_ts AT TIME ZONE $1))::int",
        tz_name,
        &session_ids[..],
        &bots[..]
    )
    .fetch_all(pool).await?;

    // per_session_hours: session_id → hour → count
    let mut per_session_hours: HashMap<i64, HashMap<i32, f64>> = HashMap::new();
    let mut total_samples: i64 = 0;
    for sid in &session_ids {
        per_session_hours.insert(*sid, HashMap::new());
    }

    for row in &msg_rows {
        let sid = row.session_id;
        let hour = row.hour;
        let cnt = row.cnt;
        if let Some(h) = per_session_hours.get_mut(&sid) {
            *h.entry(hour).or_insert(0.0) += cnt as f64;
            total_samples += cnt;
        }
    }

    let sessions_with_activity = session_ids
        .iter()
        .filter(|sid| {
            per_session_hours
                .get(*sid)
                .map(|h| !h.is_empty())
                .unwrap_or(false)
        })
        .count();
    let coverage = sessions_with_activity as f64 / n.max(1) as f64;

    // Winsorisierung bei p90 pro Stunde, dann gewichtete Summe
    let mut weighted_scores = [0.0_f64; 24];
    for hour in 0..24i32 {
        let mut hour_vals: Vec<f64> = session_ids
            .iter()
            .map(|sid| {
                per_session_hours
                    .get(sid)
                    .and_then(|h| h.get(&hour))
                    .copied()
                    .unwrap_or(0.0)
            })
            .collect();
        hour_vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let cap = quantile(&hour_vals, 0.90).max(1.0);

        for sid in &session_ids {
            let raw = per_session_hours
                .get(sid)
                .and_then(|h| h.get(&hour))
                .copied()
                .unwrap_or(0.0);
            weighted_scores[hour as usize] += session_weights[sid] * raw.min(cap);
        }
    }

    // Top-3 Stunden mit Score > 0, absteigend sortiert
    let mut hour_score_pairs: Vec<(i32, f64)> = (0..24i32)
        .map(|h| (h, weighted_scores[h as usize]))
        .filter(|(_, s)| *s > 0.0)
        .collect();
    hour_score_pairs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then(a.0.cmp(&b.0)));
    let peak_hours: Vec<i32> = hour_score_pairs
        .into_iter()
        .take(3)
        .map(|(h, _)| h)
        .collect();

    Ok(PeakResult {
        peak_hours,
        session_count: n,
        sessions_with_activity,
        sample_count: total_samples,
        coverage,
    })
}

// ─── Query-Params ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct DemoQuery {
    pub streamer: Option<String>,
    pub days: Option<i32>,
    pub timezone: Option<String>,
}

// ─── Handler ─────────────────────────────────────────────────────────────────

/// `GET /twitch/api/v2/audience-demographics?streamer=&days=30&timezone=UTC`
pub async fn audience_demographics_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<DemoQuery>,
) -> impl IntoResponse {
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }
    // IDOR-Guard: Partner auf den eigenen Login klemmen (Plan UND Owner).
    let streamer =
        match crate::auth::resolve_streamer_scope(&auth, params.streamer.as_deref(), true) {
            Ok(Some(s)) => s,
            Ok(None) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error":"Streamer required"})),
                )
                    .into_response()
            }
            Err(resp) => return resp,
        };
    let days = params.days.unwrap_or(30).clamp(7, 365) as i64;
    let since: DateTime<Utc> = Utc::now() - Duration::days(days);
    let tz_req = params.timezone.as_deref().unwrap_or("UTC");
    // Validate via chrono-tz; fallback to UTC
    let tz_name: &str = if tz_req.parse::<chrono_tz::Tz>().is_ok() {
        tz_req
    } else {
        "UTC"
    };

    // P2.97: jeder DB-Fehler wird zu 500 (wie Python analytics_internal_error_response),
    // statt ein zeroed-200 zurückzugeben das einen Ausfall maskiert.
    match compute_demographics(&pool, &streamer, since, tz_name).await {
        Ok(resp) => resp,
        Err(e) => {
            tracing::error!("audience-demographics DB-Fehler: {e}");
            crate::auth::analytics_request_failed_json().into_response()
        }
    }
}

async fn compute_demographics(
    pool: &PgPool,
    streamer: &str,
    since: DateTime<Utc>,
    tz_name: &str,
) -> Result<axum::response::Response, sqlx::Error> {
    let bots: Vec<String> = KNOWN_CHAT_BOTS.iter().map(|s| s.to_string()).collect();

    // ── Q1: Sprach-Mix ────────────────────────────────────────────────────────
    let lang_rows = sqlx::query!(
        "SELECT LOWER(language) AS \"lang!\",
                COUNT(*) AS \"sessions!\",
                AVG(avg_viewers) AS \"avg_v?\"
         FROM twitch_stream_sessions
         WHERE started_at >= $1 AND LOWER(streamer_login) = $2 AND ended_at IS NOT NULL
           AND COALESCE(language,'') <> ''
         GROUP BY 1 ORDER BY 2 DESC",
        since,
        streamer
    )
    .fetch_all(pool)
    .await?;

    let lang_session_total: i64 = lang_rows.iter().map(|r| r.sessions).sum();
    let primary_lang_code: String = lang_rows
        .first()
        .map(|r| r.lang.clone())
        .unwrap_or_else(|| "unknown".into());
    let primary_lang_count: i64 = lang_rows.first().map(|r| r.sessions).unwrap_or(0);
    let language_confidence: f64 = if lang_session_total > 0 {
        (primary_lang_count as f64 / lang_session_total as f64 * 1000.0).round() / 10.0
    } else {
        0.0
    };
    let primary_language_label = lang_label(&primary_lang_code);

    // ── Q2: Stündliche Schedule-Stats (für Regions-Scoring) ──────────────────
    let time_rows = sqlx::query!(
        "SELECT EXTRACT(HOUR FROM (started_at AT TIME ZONE 'UTC'))::int AS \"hour!\",
                AVG(avg_viewers) AS \"avg_v?\",
                COUNT(*) AS \"cnt!\"
         FROM twitch_stream_sessions
         WHERE started_at >= $1 AND LOWER(streamer_login) = $2 AND ended_at IS NOT NULL
         GROUP BY 1",
        since,
        streamer
    )
    .fetch_all(pool)
    .await?;

    // ── Q3: Gewichtete Peak-Hours ─────────────────────────────────────────────
    let peak_res = compute_weighted_peak_hours(pool, streamer, since, tz_name).await?;

    // ── Q4: Session-Stats ─────────────────────────────────────────────────────
    let sess_row = sqlx::query!(
        "SELECT COUNT(*) AS \"cnt!\",
                COALESCE(SUM(duration_seconds),0)::bigint AS \"total_dur!\",
                AVG(avg_viewers) AS \"avg_v?\",
                COALESCE(SUM(COALESCE(avg_viewers,0)*GREATEST(COALESCE(duration_seconds,0),0)/60.0),0) AS \"vm_fallback!\"
         FROM twitch_stream_sessions
         WHERE started_at >= $1 AND LOWER(streamer_login) = $2 AND ended_at IS NOT NULL",
        since,
        streamer
    ).fetch_optional(pool).await?;

    let session_count: i64 = sess_row.as_ref().map(|r| r.cnt).unwrap_or(0);
    let avg_viewers_val: f64 = sess_row.as_ref().and_then(|r| r.avg_v).unwrap_or(0.0);
    let vm_fallback: f64 = sess_row.as_ref().map(|r| r.vm_fallback).unwrap_or(0.0);

    // ── Q5: Viewer-Sample (twitch_session_viewers) ────────────────────────────
    let vsamp = sqlx::query!(
        "SELECT COUNT(*) AS \"cnt!\", COALESCE(SUM(GREATEST(sv.viewer_count,0)),0)::float8 AS \"vm!\"
         FROM twitch_session_viewers sv
         JOIN twitch_stream_sessions s ON s.id = sv.session_id
         WHERE s.started_at >= $1 AND LOWER(s.streamer_login) = $2 AND s.ended_at IS NOT NULL",
        since,
        streamer
    )
    .fetch_optional(pool)
    .await?;
    let viewer_sample_count: i64 = vsamp.as_ref().map(|r| r.cnt).unwrap_or(0);
    let vm_real: f64 = vsamp.as_ref().map(|r| r.vm).unwrap_or(0.0);
    let viewer_minutes = if viewer_sample_count > 0 {
        vm_real
    } else {
        vm_fallback
    };
    let vm_has_real = viewer_sample_count > 0;

    // ── Q6: Viewer-Kohorten (per_user + rollup) ───────────────────────────────
    // $1=since, $2=streamer, $3=bots[], — $1 wird auch für seen_before verwendet
    let vrows = sqlx::query!(r#"
        WITH per_user AS (
            SELECT
                COALESCE(NULLIF(sc.chatter_login,''), sc.chatter_id) AS user_id,
                NULLIF(sc.chatter_login,'') AS chatter_login,
                COUNT(DISTINCT sc.session_id) AS session_count,
                MAX(CASE WHEN sc.messages > 0 THEN 1 ELSE 0 END) AS active_flag,
                MAX(CASE WHEN sc.messages = 0 AND sc.seen_via_chatters_api IS TRUE THEN 1 ELSE 0 END) AS lurker_flag,
                MAX(CASE WHEN LOWER(COALESCE(CAST(sc.is_first_time_streamer AS TEXT),'0')) IN ('1','t','true') THEN 1 ELSE 0 END) AS first_time_flag,
                MAX(CASE WHEN sc.is_first_time_streamer IS NOT NULL THEN 1 ELSE 0 END) AS has_first_flag,
                MAX(CASE WHEN sc.seen_via_chatters_api IS TRUE THEN 1 ELSE 0 END) AS seen_flag
            FROM twitch_session_chatters sc
            JOIN twitch_stream_sessions s ON s.id = sc.session_id
            WHERE s.started_at >= $1 AND LOWER(s.streamer_login) = $2 AND s.ended_at IS NOT NULL
              AND COALESCE(NULLIF(sc.chatter_login,''), sc.chatter_id) IS NOT NULL
              AND (sc.chatter_login IS NULL OR sc.chatter_login = ''
                   OR (LOWER(sc.chatter_login) <> ALL($3::text[]) AND LOWER(sc.chatter_login) !~ '^justinfan[0-9]+$'))
            GROUP BY user_id, chatter_login
        ),
        rollup AS (
            SELECT LOWER(streamer_login) AS sl, LOWER(chatter_login) AS cl, first_seen_at
            FROM twitch_chatter_rollup
            WHERE LOWER(streamer_login) = $2
              AND (chatter_login IS NULL OR chatter_login = ''
                   OR (LOWER(chatter_login) <> ALL($3::text[]) AND LOWER(chatter_login) !~ '^justinfan[0-9]+$'))
        )
        SELECT
            pu.user_id AS "user_id?", pu.chatter_login AS "chatter_login?", pu.session_count::bigint AS "session_count!",
            pu.active_flag::int AS "active_flag!", pu.lurker_flag::int AS "lurker_flag!",
            pu.first_time_flag::int AS "first_time_flag!", pu.has_first_flag::int AS "has_first_flag!", pu.seen_flag::int AS "seen_flag!",
            CASE WHEN r.cl IS NOT NULL AND r.first_seen_at < $1 THEN 1 ELSE 0 END AS "seen_before!"
        FROM per_user pu
        LEFT JOIN rollup r ON r.cl = LOWER(pu.chatter_login)
    "#, since, streamer, &bots[..]).fetch_all(pool).await?;

    struct ViewerEntry {
        session_count: i64,
        active: bool,
        lurker: bool,
        first_flag: bool,
        has_first_flag: bool,
        seen_flag: bool,
        seen_before: bool,
        has_login: bool,
    }

    let mut viewer_entries: Vec<ViewerEntry> = Vec::with_capacity(vrows.len());
    let mut has_first_flag_data = false;
    for row in &vrows {
        let sc = row.session_count;
        let active = row.active_flag;
        let lurker = row.lurker_flag;
        let first_flag = row.first_time_flag;
        let has_ff = row.has_first_flag;
        let seen = row.seen_flag;
        let seen_before = row.seen_before;
        let login = row.chatter_login.as_ref();
        let hff = has_ff != 0;
        has_first_flag_data = has_first_flag_data || hff;
        viewer_entries.push(ViewerEntry {
            session_count: sc,
            active: active != 0,
            lurker: lurker != 0,
            first_flag: first_flag != 0,
            has_first_flag: hff,
            seen_flag: seen != 0,
            seen_before: seen_before != 0,
            has_login: login.is_some(),
        });
    }

    let total_viewers = viewer_entries.len();
    let loyalty_returning = viewer_entries
        .iter()
        .filter(|v| v.session_count >= 2)
        .count();
    let seen_before_count = viewer_entries.iter().filter(|v| v.seen_before).count();
    let cold_rollup = total_viewers > 0 && (seen_before_count as f64 / total_viewers as f64) < 0.1;
    let seen_via_chatters_count = viewer_entries.iter().filter(|v| v.seen_flag).count();

    let mut dedicated = 0usize;
    let mut regular = 0usize;
    let mut silent_regular = 0usize;
    let mut casual = 0usize;
    let mut new_visitors = 0usize;

    for v in &viewer_entries {
        let is_returning = if cold_rollup {
            v.session_count >= 2
        } else if has_first_flag_data && v.has_first_flag {
            let mut is_first = v.first_flag;
            if !is_first && v.lurker && !v.seen_before {
                is_first = true;
            }
            !is_first
        } else {
            if v.has_login {
                v.seen_before
            } else {
                false
            }
        };

        if is_returning {
            if v.active && v.session_count >= 3 {
                dedicated += 1;
            } else if v.active {
                regular += 1;
            } else if v.lurker || v.seen_flag {
                silent_regular += 1;
            }
        } else {
            if v.active {
                casual += 1;
            } else {
                new_visitors += 1;
            }
        }
    }

    let pct = |part: usize, whole: usize| -> f64 {
        if whole == 0 {
            0.0
        } else {
            (part as f64 / whole as f64 * 1000.0).round() / 10.0
        }
    };
    let active_viewers = viewer_entries.iter().filter(|v| v.active).count();

    // ── Q7: Total Messages ────────────────────────────────────────────────────
    let msg_count: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) AS \"cnt!\" FROM twitch_chat_messages cm
         WHERE cm.message_ts >= $1 AND LOWER(cm.streamer_login) = $2
           AND (cm.chatter_login IS NULL OR cm.chatter_login = ''
                OR (LOWER(cm.chatter_login) <> ALL($3::text[]) AND LOWER(cm.chatter_login) !~ '^justinfan[0-9]+$'))",
        since,
        streamer,
        &bots[..]
    )
    .fetch_optional(pool)
    .await?
    .unwrap_or(0);

    // ── Q8: Sessions with chat ────────────────────────────────────────────────
    let sessions_with_chat: i64 = sqlx::query_scalar!(
        "SELECT COUNT(DISTINCT sc.session_id) AS \"cnt!\"
         FROM twitch_session_chatters sc
         JOIN twitch_stream_sessions s ON s.id = sc.session_id
         WHERE s.started_at >= $1 AND LOWER(s.streamer_login) = $2 AND s.ended_at IS NOT NULL
           AND (sc.chatter_login IS NULL OR sc.chatter_login = ''
                OR (LOWER(sc.chatter_login) <> ALL($3::text[]) AND LOWER(sc.chatter_login) !~ '^justinfan[0-9]+$'))",
        since,
        streamer,
        &bots[..]
    )
    .fetch_optional(pool)
    .await?
    .unwrap_or(0);

    // ── Engagement ────────────────────────────────────────────────────────────
    let engagement = calculate_engagement(&EngagementInputs {
        total_messages: msg_count,
        active_chatters: active_viewers,
        tracked_chat_accounts: total_viewers,
        chatters_api_seen: seen_via_chatters_count,
        viewer_minutes,
        viewer_minutes_has_real_samples: vm_has_real,
        avg_viewers: avg_viewers_val,
        session_count,
        sessions_with_chat,
    });

    // ── Aktivitätsmuster (Schedule) ───────────────────────────────────────────
    let mut weekday_counts: [i64; 7] = [0; 7];
    let mut schedule_total: i64 = 0;
    // Schedule-DOW-Query für Aktivitätsmuster
    let dow_rows = sqlx::query!(
        "SELECT EXTRACT(DOW FROM (started_at AT TIME ZONE 'UTC'))::int AS \"dow!\", COUNT(*) AS \"cnt!\"
         FROM twitch_stream_sessions
         WHERE started_at >= $1 AND LOWER(streamer_login) = $2 AND ended_at IS NOT NULL
         GROUP BY 1",
        since,
        streamer
    )
    .fetch_all(pool)
    .await?;
    for row in &dow_rows {
        let dow = row.dow;
        let cnt = row.cnt;
        if (0..7).contains(&dow) {
            weekday_counts[dow as usize] += cnt;
            schedule_total += cnt;
        }
    }
    let weekend_streams: i64 = weekday_counts[0] + weekday_counts[6];
    let weekday_streams: i64 = weekday_counts[1..6].iter().sum();
    let activity_pattern = if weekend_streams > weekday_streams {
        "weekend-heavy"
    } else if weekday_streams > weekend_streams * 2 {
        "weekday-focused"
    } else {
        "balanced"
    };

    // ── Regions-Scoring ───────────────────────────────────────────────────────
    // (keine Ausgabe, nur interne Logik — Endpunkt gibt nur Peak-Hours zurück)
    let _ = {
        let mut scores: HashMap<&str, f64> =
            [("DACH", 0.0), ("Rest EU", 0.0), ("NA", 0.0), ("Other", 0.0)].into();
        let lh = primary_lang_code.to_lowercase();
        let dach: std::collections::HashSet<&str> =
            ["de", "de-de", "de-at", "de-ch", "ger", "german"].into();
        let eu: std::collections::HashSet<&str> = [
            "fr", "fr-fr", "es", "es-es", "it", "pl", "ru", "nl", "sv", "da", "fi", "tr",
        ]
        .into();
        if dach.contains(lh.as_str()) {
            *scores.entry("DACH").or_default() += 3.5;
            *scores.entry("Rest EU").or_default() += 2.0;
        } else if eu.contains(lh.as_str()) {
            *scores.entry("Rest EU").or_default() += 3.0;
            *scores.entry("Other").or_default() += 0.8;
        } else if lh.starts_with("en") {
            *scores.entry("NA").or_default() += 2.5;
            *scores.entry("Rest EU").or_default() += 2.0;
        } else if lh.starts_with("pt") || lh.starts_with("es") {
            *scores.entry("Other").or_default() += 2.5;
            *scores.entry("Rest EU").or_default() += 1.0;
        } else {
            *scores.entry("Other").or_default() += 2.0;
        }
        for row in &time_rows {
            let hour = row.hour;
            let avg_v = row.avg_v.unwrap_or(0.0).max(0.0);
            let cnt = row.cnt as f64;
            let score = avg_v.max(1.0) * cnt.max(1.0);
            if (17..=23).contains(&hour) {
                *scores.entry("Rest EU").or_default() += score;
                if dach.contains(lh.as_str()) {
                    *scores.entry("DACH").or_default() += score * 0.7;
                }
            }
            if (0..=5).contains(&hour) {
                *scores.entry("NA").or_default() += score;
            }
            if (6..=12).contains(&hour) {
                *scores.entry("Other").or_default() += score;
            }
            if (13..=16).contains(&hour) {
                *scores.entry("Rest EU").or_default() += score * 0.5;
            }
        }
        scores
    };

    // ── Peak-Hours Qualität ───────────────────────────────────────────────────
    let session_samples = {
        let ls = lang_session_total;
        let ss = schedule_total;
        let ps = peak_res.session_count as i64;
        ls.max(ss).max(ps)
    };
    let peak_sample_count = peak_res.sample_count;
    let peak_coverage = peak_res.coverage;
    let peak_sessions_with_activity = peak_res.sessions_with_activity;
    let peak_session_count = peak_res.session_count;
    let peak_quality_method: &str = if peak_sample_count <= 0 {
        "no_data"
    } else if peak_coverage < 0.20 || peak_sessions_with_activity < 3 {
        "low_coverage"
    } else {
        "real_samples"
    };
    let confidence: &str = if peak_quality_method == "real_samples" {
        if peak_sample_count >= 500 && peak_coverage >= 0.60 {
            "high"
        } else if peak_sample_count >= 150 && peak_coverage >= 0.35 {
            "medium"
        } else {
            "low"
        }
    } else if peak_quality_method == "low_coverage" {
        "low"
    } else {
        "very_low"
    };

    let peak_hours_response: &[i32] = if peak_quality_method == "real_samples" {
        &peak_res.peak_hours
    } else {
        &[]
    };

    let loyalty_score = pct(loyalty_returning, total_viewers);
    let peak_method_str = format!(
        "weighted_chat_activity_exp_decay_h{}_w{}_winsor_p90",
        PEAK_HALF_LIFE_SESSIONS as i32, PEAK_SESSION_WINDOW
    );

    Ok(Json(json!({
        "viewerTypes": [
            {"label": "Dedicated Fans",   "percentage": pct(dedicated, total_viewers)},
            {"label": "Regular Viewers",  "percentage": pct(regular, total_viewers)},
            {"label": "Silent Regulars",  "percentage": pct(silent_regular, total_viewers)},
            {"label": "Casual Viewers",   "percentage": pct(casual, total_viewers)},
            {"label": "New Visitors",     "percentage": pct(new_visitors, total_viewers)},
        ],
        "activityPattern": activity_pattern,
        "primaryLanguage": primary_language_label,
        "languageConfidence": language_confidence,
        "peakActivityHours": peak_hours_response,
        "peakHoursMethod": peak_method_str,
        "chatPenetrationPct": engagement.chat_penetration_pct,
        "chatPenetrationReliable": engagement.chat_penetration_reliable,
        "messagesPer100ViewerMinutes": engagement.messages_per_100_viewer_minutes,
        "viewerMinutes": engagement.viewer_minutes,
        "legacyInteractionActivePerAvgViewer": engagement.legacy_interaction_active_per_avg_viewer,
        "interactiveRate": engagement.chat_penetration_pct.unwrap_or(0.0),
        "interactionRateActivePerViewer": engagement.chat_penetration_pct.unwrap_or(0.0),
        "interactionRateActivePerAvgViewer": engagement.legacy_interaction_active_per_avg_viewer,
        "interactionRateReliable": engagement.chat_penetration_reliable,
        "loyaltyScore": loyalty_score,
        "timezone": tz_name,
        "dataQuality": {
            "confidence": confidence,
            "sessions": session_samples,
            "method": engagement.method,
            "peakMethod": peak_quality_method,
            "coverage": (peak_coverage * 1000.0).round() / 1000.0,
            "sampleCount": peak_sample_count,
            "peakSessionCount": peak_session_count,
            "peakSessionsWithActivity": peak_sessions_with_activity,
            "interactiveSampleCount": active_viewers,
            "interactionCoverage": engagement.chatters_coverage,
            "chattersCoverage": engagement.chatters_coverage,
            "chattersApiCoverage": engagement.chatters_coverage,
            "passiveViewerSamples": engagement.passive_viewer_samples,
            "viewerSampleCount": viewer_sample_count,
            "viewerMinutesSource": if vm_has_real { "real_samples" } else { "low_coverage" },
            "sessionsWithChat": sessions_with_chat,
            "chatSessionCoverage": (engagement.chat_session_coverage * 1000.0).round() / 10.0,
            "botFilterApplied": true,
        },
    }))
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn empty_schema_pool(schema: &str) -> Option<PgPool> {
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
        Some(
            PgPoolOptions::new()
                .max_connections(2)
                .connect_with(opts)
                .await
                .unwrap(),
        )
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let pool = empty_schema_pool(schema).await?;
        sqlx::query("CREATE TABLE twitch_stream_sessions (id BIGSERIAL PRIMARY KEY, streamer_login TEXT, started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ, language TEXT, avg_viewers DOUBLE PRECISION, duration_seconds BIGINT)")
            .execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_session_chatters (session_id BIGINT, chatter_login TEXT, chatter_id TEXT, messages INTEGER DEFAULT 0, seen_via_chatters_api BOOLEAN DEFAULT FALSE, is_first_time_streamer BOOLEAN)")
            .execute(&pool).await.unwrap();
        sqlx::query(
            "CREATE TABLE twitch_session_viewers (session_id BIGINT, viewer_count INTEGER)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("CREATE TABLE twitch_chat_messages (id BIGSERIAL PRIMARY KEY, session_id BIGINT, streamer_login TEXT, chatter_login TEXT, chatter_id TEXT, message_ts TIMESTAMPTZ)")
            .execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_chatter_rollup (streamer_login TEXT, chatter_login TEXT, first_seen_at TIMESTAMPTZ)")
            .execute(&pool).await.unwrap();
        Some(pool)
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn call(pool: PgPool) -> axum::response::Response {
        audience_demographics_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(DemoQuery {
                streamer: Some("nani".into()),
                days: Some(30),
                timezone: Some("UTC".into()),
            }),
        )
        .await
        .into_response()
    }

    fn partner(login: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: login.to_string(),
            twitch_user_id: "42".to_string(),
            display_name: login.to_string(),
        }
    }

    // IDOR-Guard: Partner mit fremdem ?streamer= → 403.
    #[tokio::test]
    async fn partner_fremder_streamer_403() {
        let Some(pool) = make_pool("t_demo_idor").await else {
            return;
        };
        let resp = audience_demographics_handler(
            partner("earlysalty"),
            State(pool),
            Query(DemoQuery {
                streamer: Some("ismile_e".into()),
                days: Some(30),
                timezone: Some("UTC".into()),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    // P2.95/P2.92/P2.96: Chat-Messages mit NULL chatter_login (anonym) müssen in
    // total_messages und Peak-Hours mitgezählt werden.
    #[tokio::test]
    async fn null_login_messages_counted() {
        let Some(pool) = make_pool("t_demo_nullmsg").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_stream_sessions (streamer_login, started_at, ended_at, avg_viewers, duration_seconds) VALUES ('nani', NOW()-INTERVAL '2 days', NOW()-INTERVAL '2 days'+INTERVAL '3 hours', 10.0, 10800)")
            .execute(&pool).await.unwrap();
        // 1x normaler Chatter, 1x anonym (NULL login), 1x Bot mit mixed case
        sqlx::query("INSERT INTO twitch_chat_messages (session_id, streamer_login, chatter_login, message_ts) VALUES (1,'nani','viewer', NOW()-INTERVAL '2 days')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_chat_messages (session_id, streamer_login, chatter_login, chatter_id, message_ts) VALUES (1,'nani',NULL,'anon', NOW()-INTERVAL '2 days')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_chat_messages (session_id, streamer_login, chatter_login, message_ts) VALUES (1,'nani','Nightbot', NOW()-INTERVAL '2 days')")
            .execute(&pool).await.unwrap();

        let resp = call(pool).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        // viewer + anon = 2, Nightbot (mixed case) gefiltert
        assert_eq!(
            v["dataQuality"]["sampleCount"], 2,
            "NULL-Login zählt, mixed-case Bot raus"
        );
    }

    // P2.97: Ein DB-Fehler (fehlende Tabelle) muss 500 ergeben, kein zeroed-200.
    #[tokio::test]
    async fn db_error_returns_500() {
        let Some(pool) = empty_schema_pool("t_demo_dberr").await else {
            return;
        };
        // Keine Tabellen angelegt → erste Query schlägt fehl.
        let resp = call(pool).await;
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
