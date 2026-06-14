//! Handler für `GET /twitch/api/v2/overview`.
//!
//! Admin-only. Kein Partner-Session-Auth (deferred, ADR 0003).

use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tb_analytics::overview::{
    overview_chat_per_100, overview_chatter_metrics, overview_metrics,
    overview_monetization_counts, overview_network_stats, overview_session_count,
    OverviewMonetization, OverviewNetworkStats,
};
use tb_http_core::{ApiError, AuthLevel};

#[derive(Deserialize)]
pub struct OverviewParams {
    pub streamer: Option<String>,
    /// Zeitraum in Tagen. Default 30, min 7, max 365.
    #[serde(default = "default_days")]
    pub days: i64,
}

fn default_days() -> i64 {
    30
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum OverviewResponse {
    Empty { empty: bool, error: &'static str },
    Data(OverviewData),
}

#[derive(Serialize)]
pub struct OverviewData {
    pub streamer: Option<String>,
    pub days: i64,
    pub summary: OverviewSummary,
    pub scores: HealthScores,
    pub findings: Vec<Finding>,
    pub actions: Vec<ActionItem>,
    pub network: OverviewNetwork,
}

/// Health-Scores (Python `_calculate_health_scores`), je 0–100.
#[derive(Serialize)]
pub struct HealthScores {
    pub total: i64,
    pub reach: i64,
    pub retention: i64,
    pub engagement: i64,
    pub growth: i64,
    pub monetization: i64,
    pub network: i64,
}

/// Berechnet die Health-Scores exakt nach Python `_calculate_health_scores`.
/// `int()`-Truncation = `as i64` (positive Werte); `min(100,..)`/`max(0,..)`
/// wie Python. `category_percentile=None` → avg_viewers/5-Fallback (Reach).
#[allow(clippy::too_many_arguments)]
fn calculate_health_scores(
    avg_viewers: f64,
    retention_10m_pct: f64,
    retention_sample_count: i64,
    engagement_rate: f64,
    chat_sample_count: i64,
    followers_per_hour: f64,
    session_count: i64,
    category_percentile: Option<f64>,
    mon: OverviewMonetization,
    net: OverviewNetworkStats,
) -> HealthScores {
    let reach = match category_percentile {
        Some(p) => ((20.0 + p * 80.0) as i64).min(100),
        None => ((avg_viewers / 5.0) as i64).min(100),
    };
    let retention = if retention_sample_count < 3 {
        50
    } else {
        ((retention_10m_pct * 1.5) as i64).min(100)
    };
    let engagement = if chat_sample_count < 3 {
        50
    } else {
        ((engagement_rate * 5.0) as i64).min(100)
    };
    let growth = ((followers_per_hour.max(0.0) * 20.0) as i64).min(100);
    let monetization = {
        let sc = session_count.max(1);
        let weighted = mon.sub_events * 3 + mon.bits_events + mon.hype_trains * 5;
        (((weighted as f64 / sc as f64) * 10.0) as i64).clamp(0, 100)
    };
    let network = {
        let total = net.sent + net.received;
        let reciprocity = net.sent.min(net.received) * 10;
        (total * 8 + reciprocity).clamp(0, 100)
    };
    let total = (reach as f64 * 0.2
        + retention as f64 * 0.25
        + engagement as f64 * 0.2
        + growth as f64 * 0.15
        + monetization as f64 * 0.1
        + network as f64 * 0.1) as i64;
    HealthScores {
        total,
        reach,
        retention,
        engagement,
        growth,
        monetization,
        network,
    }
}

const RETENTION_LOW: f64 = 40.0;
const RETENTION_HIGH: f64 = 65.0;
const CHAT_LOW: f64 = 5.0;
const CHAT_HIGH: f64 = 30.0;

/// Ein Insight/Finding (Python `_generate_insights`).
#[derive(Serialize)]
pub struct Finding {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub title: &'static str,
    pub text: String,
}

/// Eine Handlungsempfehlung (Python `_generate_actions`).
#[derive(Serialize)]
pub struct ActionItem {
    pub tag: &'static str,
    pub text: &'static str,
    pub priority: &'static str,
}

/// Findings exakt nach Python `_generate_insights` (Texte 1:1, inkl. der
/// ASCII-Schreibweise der Quelle — Byte-Parität zum proxied Python beim v2-Flip).
#[allow(clippy::too_many_arguments)]
fn generate_insights(
    ret_10m_pct: f64,
    retention_sample_count: i64,
    chat_100: f64,
    chat_sample_count: i64,
    followers_per_hour: f64,
    gained_followers_per_hour: f64,
    follower_valid_count: i64,
    total_followers: i64,
) -> Vec<Finding> {
    let mut out = Vec::new();
    // Retention
    if retention_sample_count < 3 {
        out.push(Finding { kind: "info", title: "Retention-Daten unzureichend",
            text: "Zu wenige Sessions mit >=3 Viewern fur aussagekraftige Retention-Werte.".into() });
    } else if ret_10m_pct < RETENTION_LOW {
        out.push(Finding { kind: "neg", title: "Niedrige Retention",
            text: format!("10-Min Retention bei {ret_10m_pct:.1}%. Verbessere den Stream-Einstieg.") });
    } else if ret_10m_pct > RETENTION_HIGH {
        out.push(Finding { kind: "pos", title: "Starke Retention",
            text: format!("Exzellente {ret_10m_pct:.1}% Retention. Dein Content fesselt!") });
    }
    // Chat
    if chat_sample_count < 3 {
        out.push(Finding { kind: "info", title: "Chat-Daten unzureichend",
            text: "Zu wenige Sessions mit >=3 Viewern fur aussagekraftige Chat-Metriken.".into() });
    } else if chat_100 < CHAT_LOW {
        out.push(Finding { kind: "warn", title: "Niedrige Chat-Aktivitat",
            text: format!("Nur {chat_100:.1} Chatter/100 Peak-Viewer (Proxy). Mehr Interaktion fordern!") });
    } else if chat_100 > CHAT_HIGH {
        out.push(Finding { kind: "pos", title: "Aktive Community",
            text: format!("{chat_100:.1} Chatter/100 Peak-Viewer (Proxy) - sehr engagiert!") });
    }
    // Followers
    if follower_valid_count > 0 {
        if followers_per_hour < 0.0 {
            out.push(Finding { kind: "neg", title: "Follower-Verlust",
                text: format!("Netto {followers_per_hour:.2} Follower/Stunde ({total_followers:+} gesamt). Gewonnen: {gained_followers_per_hour:.2}/h. Unfollows uberwiegen.") });
        } else if followers_per_hour < 0.5 {
            out.push(Finding { kind: "warn", title: "Langsames Follower-Wachstum",
                text: format!("Nur {followers_per_hour:.2} Follower/Stunde. Regelmaig an Follows erinnern!") });
        } else if followers_per_hour > 3.0 {
            out.push(Finding { kind: "pos", title: "Starkes Wachstum",
                text: format!("{followers_per_hour:.1} Follower/Stunde - ausgezeichnet!") });
        }
    }
    out
}

/// Actions exakt nach Python `_generate_actions`.
fn generate_actions(
    ret_10m_pct: f64,
    retention_sample_count: i64,
    chat_100: f64,
    chat_sample_count: i64,
    followers_per_hour: f64,
    follower_valid_count: i64,
) -> Vec<ActionItem> {
    let mut out = Vec::new();
    if retention_sample_count >= 3 && ret_10m_pct < RETENTION_LOW {
        out.push(ActionItem { tag: "Retention",
            text: "Starte mit einem starken Hook in den ersten 2 Minuten.", priority: "high" });
    }
    if chat_sample_count >= 3 && chat_100 < CHAT_LOW {
        out.push(ActionItem { tag: "Engagement",
            text: "Stelle alle 5-10 Minuten eine direkte Frage an den Chat.", priority: "medium" });
    }
    if follower_valid_count > 0 && followers_per_hour < 0.0 {
        out.push(ActionItem { tag: "Growth",
            text: "Follower-Verlust! Prufe ob Content-Wechsel oder lange Pausen Unfollows verursachen.", priority: "high" });
    } else if follower_valid_count > 0 && followers_per_hour < 1.0 {
        out.push(ActionItem { tag: "Growth",
            text: "Erinnere alle 20-30 Minuten an Follow mit konkretem Grund.", priority: "medium" });
    }
    out
}

/// Raid-Netzwerk-Kachel (Python `_get_network_stats`).
#[derive(Serialize)]
pub struct OverviewNetwork {
    pub sent: i64,
    #[serde(rename = "sentViewers")]
    pub sent_viewers: i64,
    pub received: i64,
}

#[derive(Serialize)]
pub struct OverviewSummary {
    #[serde(rename = "avgViewers")]
    pub avg_viewers: f64,
    #[serde(rename = "peakViewers")]
    pub peak_viewers: i64,
    #[serde(rename = "totalHoursWatched")]
    pub total_hours_watched: f64,
    #[serde(rename = "totalAirtime")]
    pub total_airtime: f64,
    #[serde(rename = "followersDelta")]
    pub followers_delta: i64,
    #[serde(rename = "totalSessions")]
    pub total_sessions: i64,
    // Session-abgeleitete Felder (Python _calculate_overview_metrics):
    #[serde(rename = "followersGained")]
    pub followers_gained: i64,
    #[serde(rename = "followersPerHour")]
    pub followers_per_hour: f64,
    #[serde(rename = "followersGainedPerHour")]
    pub followers_gained_per_hour: f64,
    #[serde(rename = "retention10m")]
    pub retention_10m: f64,
    #[serde(rename = "retentionReliable")]
    pub retention_reliable: bool,
    // Trends ggü. Vorperiode (None = kein verlässlicher Vergleich), Python calc_trend.
    #[serde(rename = "avgViewersTrend")]
    pub avg_viewers_trend: Option<f64>,
    #[serde(rename = "followersTrend")]
    pub followers_trend: Option<f64>,
    #[serde(rename = "retentionTrend")]
    pub retention_trend: Option<f64>,
    // Chatter-abgeleitete Felder (Bot-gefiltert, Python _calculate_overview_metrics).
    #[serde(rename = "uniqueChatters")]
    pub unique_chatters: i64,
    #[serde(rename = "activeChatters")]
    pub active_chatters: i64,
    #[serde(rename = "uniqueViewers")]
    pub unique_viewers: i64,
    #[serde(rename = "engagementRate")]
    pub engagement_rate: f64,
}

/// Python `calc_trend`: None wenn prev 0/fehlt, sonst gerundete Prozent-Differenz.
fn calc_trend(curr: f64, prev: f64) -> Option<f64> {
    if prev == 0.0 {
        return None;
    }
    Some(((curr - prev) / prev.abs() * 100.0 * 10.0).round() / 10.0)
}

/// `GET /twitch/api/v2/overview?streamer=<login>[&days=30]`
pub async fn overview_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<OverviewParams>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    // days: clip to [7, 365]
    let days = params.days.clamp(7, 365);
    let since = (Utc::now() - Duration::days(days)).to_rfc3339();
    let login = params
        .streamer
        .as_deref()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    let login_ref = login.as_deref();

    // Existenz-Check
    let count = overview_session_count(&pool, &since, login_ref)
        .await
        .map_err(|_| ApiError::internal())?;

    if count == 0 {
        return Ok(Json(OverviewResponse::Empty {
            empty: true,
            error: "Keine Daten für den Zeitraum",
        }));
    }

    let metrics = overview_metrics(&pool, &since, login_ref, None)
        .await
        .map_err(|_| ApiError::internal())?
        .ok_or_else(ApiError::internal)?;

    // Vorperiode [now-2*days, since) für Trend-Pfeile (Python _get_overview_data_sync).
    let prev_since = (Utc::now() - Duration::days(days * 2)).to_rfc3339();
    let prev = overview_metrics(&pool, &prev_since, login_ref, Some(&since))
        .await
        .map_err(|_| ApiError::internal())?;

    // Chatter-abgeleitete Metriken (Bot-gefiltert).
    let chatter = overview_chatter_metrics(&pool, &since, login_ref)
        .await
        .map_err(|_| ApiError::internal())?;

    // Raid-Netzwerk-Kachel.
    let net = overview_network_stats(&pool, &since, login_ref)
        .await
        .map_err(|_| ApiError::internal())?;

    // Monetarisierungs-Events (fehlende Tabellen → 0).
    let mon = overview_monetization_counts(&pool, &since, login_ref).await;

    // Chatter pro 100 Peak-Viewer (für Chat-Insights/Actions).
    let chat_per_100 = overview_chat_per_100(&pool, &since, login_ref)
        .await
        .map_err(|_| ApiError::internal())?;

    let airtime = metrics.total_airtime_hours.unwrap_or(0.0);
    let total_followers = metrics.total_followers.unwrap_or(0);
    let gained = metrics.gained_followers.unwrap_or(0);
    let curr_ret = metrics.avg_retention_10m.unwrap_or(0.0) * 100.0;
    let curr_ret_sample = metrics.retention_sample_count.unwrap_or(0);
    let per_hour = |n: i64| if airtime > 0.0 { n as f64 / airtime } else { 0.0 };

    let prev_avg = prev.as_ref().and_then(|p| p.avg_avg_viewers).unwrap_or(0.0);
    let prev_fol = prev.as_ref().and_then(|p| p.total_followers).unwrap_or(0);
    let prev_ret = prev.as_ref().and_then(|p| p.avg_retention_10m).unwrap_or(0.0) * 100.0;
    let prev_ret_sample = prev.as_ref().and_then(|p| p.retention_sample_count).unwrap_or(0);

    let avg_viewers_trend = calc_trend(metrics.avg_avg_viewers.unwrap_or(0.0), prev_avg);
    // Python: bei |curr|<5 UND |prev|<5 unterdrücken, sonst auf ±999 kappen.
    let followers_trend = if total_followers.abs() < 5 && prev_fol.abs() < 5 {
        None
    } else {
        calc_trend(total_followers as f64, prev_fol as f64).map(|v| v.clamp(-999.0, 999.0))
    };
    // Python: nur wenn beide Perioden ≥3 Retention-Samples und prev>0.
    let retention_trend = if curr_ret_sample >= 3 && prev_ret_sample >= 3 && prev_ret > 0.0 {
        calc_trend(curr_ret, prev_ret)
    } else {
        None
    };

    let scores = calculate_health_scores(
        metrics.avg_avg_viewers.unwrap_or(0.0),
        curr_ret,
        curr_ret_sample,
        chatter.engagement_rate,
        metrics.chat_sample_count.unwrap_or(0),
        per_hour(total_followers),
        metrics.session_count.unwrap_or(0),
        None, // category_percentile noch nicht erhoben → avg_viewers/5-Fallback
        mon,
        net,
    );

    let chat_sample = metrics.chat_sample_count.unwrap_or(0);
    let follower_valid = metrics.follower_valid_count.unwrap_or(0);
    let fph = per_hour(total_followers);
    let findings = generate_insights(
        curr_ret,
        curr_ret_sample,
        chat_per_100,
        chat_sample,
        fph,
        per_hour(gained),
        follower_valid,
        total_followers,
    );
    let actions = generate_actions(
        curr_ret,
        curr_ret_sample,
        chat_per_100,
        chat_sample,
        fph,
        follower_valid,
    );

    Ok(Json(OverviewResponse::Data(OverviewData {
        streamer: params.streamer,
        days,
        scores,
        findings,
        actions,
        summary: OverviewSummary {
            avg_viewers: metrics.avg_avg_viewers.unwrap_or(0.0),
            peak_viewers: metrics.max_peak_viewers.unwrap_or(0),
            total_hours_watched: metrics.total_hours_watched.unwrap_or(0.0),
            total_airtime: airtime,
            followers_delta: total_followers,
            total_sessions: metrics.session_count.unwrap_or(0),
            followers_gained: gained,
            followers_per_hour: per_hour(total_followers),
            followers_gained_per_hour: per_hour(gained),
            retention_10m: curr_ret,
            retention_reliable: curr_ret_sample >= 3,
            avg_viewers_trend,
            followers_trend,
            retention_trend,
            unique_chatters: chatter.unique_chatters,
            active_chatters: chatter.active_chatters,
            unique_viewers: chatter.unique_viewers,
            engagement_rate: chatter.engagement_rate,
        },
        network: OverviewNetwork {
            sent: net.sent,
            sent_viewers: net.sent_viewers,
            received: net.received,
        },
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::{Request, StatusCode},
        routing::get,
        Extension, Router,
    };
    use sqlx::postgres::PgPoolOptions;
    use std::net::SocketAddr;
    use tb_http_core::ExpectedToken;
    use tower::ServiceExt;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    /// Gibt die DSN zurück oder bricht den Test ab.
    /// Mit `TB_TEST_REQUIRE_DB=1` wird statt des stillen Skips ein panic ausgelöst.
    macro_rules! db_dsn_or_skip {
        () => {
            match test_dsn() {
                Some(d) => d,
                None => {
                    if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                        panic!(
                            "TB_TEST_REQUIRE_DB=1 ist gesetzt, aber TB_TEST_DATABASE_URL fehlt"
                        );
                    }
                    eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                    return;
                }
            }
        };
    }

    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect test-db");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&pool)
            .await
            .expect("Schema droppen");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .expect("Schema anlegen");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path setzen fehlgeschlagen");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_stream_sessions (
                id               BIGSERIAL PRIMARY KEY,
                streamer_login   TEXT NOT NULL,
                started_at       TIMESTAMPTZ NOT NULL,
                ended_at         TIMESTAMPTZ,
                avg_viewers      DOUBLE PRECISION,
                peak_viewers     BIGINT,
                duration_seconds BIGINT,
                follower_delta   BIGINT,
                followers_start  BIGINT,
                followers_end    BIGINT,
                retention_10m    REAL,
                unique_chatters  BIGINT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL fehlgeschlagen");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_session_chatters (
                session_id            BIGINT NOT NULL,
                chatter_login         TEXT,
                chatter_id            TEXT,
                messages              INTEGER DEFAULT 0,
                seen_via_chatters_api BOOLEAN DEFAULT FALSE
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL chatters fehlgeschlagen");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_raid_history (
                from_broadcaster_login TEXT,
                to_broadcaster_login   TEXT,
                viewer_count           BIGINT,
                success                BOOLEAN,
                executed_at            TIMESTAMPTZ
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL raid_history fehlgeschlagen");
        // Tabellen leeren damit Wiederholungsläufe nicht alte Daten sehen
        sqlx::query("TRUNCATE twitch_stream_sessions")
            .execute(&pool)
            .await
            .expect("TRUNCATE fehlgeschlagen");
        pool
    }

    fn make_router(pool: PgPool, token: &str) -> Router {
        Router::new()
            .route("/twitch/api/v2/overview", get(overview_handler))
            .with_state(pool)
            .layer(Extension(ExpectedToken(token.to_string())))
    }

    fn admin_req(token: &str, streamer: &str) -> Request<Body> {
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        Request::builder()
            .uri(format!("/twitch/api/v2/overview?streamer={streamer}"))
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", token)
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test]
    async fn returns_401_without_auth() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_overview_unauth").await;
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        let req = Request::builder()
            .uri("/twitch/api/v2/overview?streamer=x")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .body(Body::empty())
            .unwrap();
        let res = make_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn returns_empty_for_unknown_streamer() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_overview_empty").await;
        let res = make_router(pool, "tok")
            .oneshot(admin_req("tok", "nobody"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 256).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["empty"], true);
    }

    #[tokio::test]
    async fn returns_metrics_for_known_streamer() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_overview_mit_daten").await;
        sqlx::query(
            r#"
            INSERT INTO twitch_stream_sessions
                (id, streamer_login, started_at, ended_at, avg_viewers, peak_viewers,
                 duration_seconds, follower_delta, followers_start, followers_end, retention_10m)
            VALUES
                (1, 'streamer_x', NOW() - INTERVAL '1 day', NOW() - INTERVAL '23 hours',
                 100.0, 200, 3600, 5, 1000, 1005, 0.6)
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO twitch_session_chatters (session_id, chatter_login, chatter_id, messages, seen_via_chatters_api)
            VALUES (1, 'alice', 'a1', 3, FALSE), (1, 'nightbot', 'nb', 7, FALSE), (1, 'bob', 'b2', 0, TRUE)
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO twitch_raid_history (from_broadcaster_login, to_broadcaster_login, viewer_count, success, executed_at)
            VALUES ('streamer_x', 'p_a', 30, TRUE, NOW() - INTERVAL '1 hour'),
                   ('p_b', 'streamer_x', 5, TRUE, NOW() - INTERVAL '2 hours')
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let res = make_router(pool, "tok")
            .oneshot(admin_req("tok", "streamer_x"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert!((v["summary"]["avgViewers"].as_f64().unwrap() - 100.0).abs() < 0.001);
        assert_eq!(v["summary"]["totalSessions"], 1);
        // Neue session-abgeleitete Summary-Felder.
        assert_eq!(v["summary"]["followersGained"], 5);
        assert!((v["summary"]["retention10m"].as_f64().unwrap() - 60.0).abs() < 0.01);
        assert_eq!(v["summary"]["retentionReliable"], false); // nur 1 Sample (<3)
        // Chatter-Felder (nightbot=Bot raus, bob nur via API).
        assert_eq!(v["summary"]["activeChatters"], 1);
        assert_eq!(v["summary"]["uniqueViewers"], 2);
        assert_eq!(v["summary"]["uniqueChatters"], 1);
        assert!((v["summary"]["engagementRate"].as_f64().unwrap() - 50.0).abs() < 0.001);
        // Netzwerk-Kachel.
        assert_eq!(v["network"]["sent"], 1);
        assert_eq!(v["network"]["sentViewers"], 30);
        assert_eq!(v["network"]["received"], 1);
        // Health-Scores: reach=avg/5=20, retention/engagement=50 (sample<3),
        // growth=min(100, fph*20)=100 (5 Follower / 1h), monetization=0 (keine
        // Event-Tabellen), network: total=2*8 + recip=10 = 26.
        assert_eq!(v["scores"]["reach"], 20);
        assert_eq!(v["scores"]["retention"], 50);
        assert_eq!(v["scores"]["engagement"], 50);
        assert_eq!(v["scores"]["growth"], 100);
        assert_eq!(v["scores"]["monetization"], 0);
        assert_eq!(v["scores"]["network"], 26);
        assert_eq!(v["scores"]["total"], 44);
        // Findings: Retention-Sample<3 (info), Chat-Sample<3 (info), fph=5>3 (pos).
        assert_eq!(v["findings"].as_array().unwrap().len(), 3);
        assert_eq!(v["findings"][2]["type"], "pos");
        // Actions: keine (Samples <3, fph nicht <1).
        assert_eq!(v["actions"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn health_scores_formel_exakt() {
        // category_percentile gesetzt → reach = 20 + 0.5*80 = 60.
        let s = calculate_health_scores(
            100.0, 40.0, 5, 12.0, 5, 2.0, 4,
            Some(0.5),
            OverviewMonetization { sub_events: 2, bits_events: 0, hype_trains: 1 },
            OverviewNetworkStats { sent: 3, received: 1, sent_viewers: 0 },
        );
        assert_eq!(s.reach, 60);
        assert_eq!(s.retention, 60); // min(100, 40*1.5)
        assert_eq!(s.engagement, 60); // min(100, 12*5)
        assert_eq!(s.growth, 40); // min(100, 2*20)
        // weighted = 2*3 + 0 + 1*5 = 11; sc=max(1,4)=4; (11/4)*10=27.5 -> 27.
        assert_eq!(s.monetization, 27);
        // total=3+1=4; recip=min(3,1)*10=10; 4*8+10=42.
        assert_eq!(s.network, 42);
        // total = 60*.2+60*.25+60*.2+40*.15+27*.1+42*.1 = 12+15+12+6+2.7+4.2=51.9 -> 51.
        assert_eq!(s.total, 51);
    }

    #[test]
    fn insights_und_actions_regeln() {
        // Niedrige Retention/Chat + Follower-Verlust, alle Samples >=3.
        let f = generate_insights(30.0, 5, 2.0, 5, -1.0, 0.0, 1, -10);
        assert_eq!(f.len(), 3);
        assert_eq!(f[0].kind, "neg"); // Retention
        assert_eq!(f[1].kind, "warn"); // Chat
        assert_eq!(f[2].kind, "neg"); // Follower-Verlust
        let a = generate_actions(30.0, 5, 2.0, 5, -1.0, 1);
        assert_eq!(a.len(), 3);
        assert_eq!(a[0].tag, "Retention");
        assert_eq!(a[1].tag, "Engagement");
        assert_eq!(a[2].tag, "Growth");
        assert_eq!(a[2].priority, "high");

        // Zu wenig Samples → info-Findings, keine Actions.
        let f2 = generate_insights(80.0, 1, 50.0, 1, 0.2, 0.2, 0, 0);
        assert_eq!(f2.len(), 2); // 2x info (Retention+Chat), follower_valid=0 → kein Follower-Finding
        assert!(f2.iter().all(|x| x.kind == "info"));
        assert!(generate_actions(80.0, 1, 50.0, 1, 0.2, 0).is_empty());
    }
}
