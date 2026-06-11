//! Handler für `GET /internal/twitch/v1/market-share`.
//!
//! Markt-Dominanz: Viewer-Anteil der Partner-Streamer an der
//! Deadlock-Kategorie über die Zeit, plus Live-Snapshot des letzten Ticks.
//! Konsument ist das Python-Dashboard (8765), das die Admin-Route
//! `/twitch/api/v2/market-share` hierher proxied (Berechnung lebt in Rust,
//! s. `tb_analytics::market`).

use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tb_analytics::market::{market_current_tick, market_share_series, partner_roster};
use tb_http_core::{ApiError, AuthLevel};

#[derive(Deserialize)]
pub struct MarketShareParams {
    /// Zeitraum in Tagen. Default 7, min 1, max 365.
    #[serde(default = "default_days")]
    pub days: i64,
    /// `all` (ganze Kategorie) oder `german` (nur Streams mit Deutsch-Tag).
    #[serde(default)]
    pub scope: Option<String>,
}

fn default_days() -> i64 {
    7
}

#[derive(Serialize)]
pub struct MarketShareResponse {
    pub days: i64,
    pub scope: &'static str,
    #[serde(rename = "bucketSeconds")]
    pub bucket_seconds: i64,
    pub series: Vec<SeriesPoint>,
    pub peak: Option<PeakPoint>,
    pub current: Option<CurrentSnapshot>,
    pub roster: RosterInfo,
}

/// Partner-Bestand, unabhängig vom Live-Zustand.
#[derive(Serialize)]
pub struct RosterInfo {
    /// Aktive Partner gesamt (unter Vertrag, ohne Opt-out/Pause).
    #[serde(rename = "partnersTotal")]
    pub partners_total: i64,
    /// Davon im gewählten Zeitraum mit mindestens einem Deadlock-Stream.
    #[serde(rename = "partnersSeenInRange")]
    pub partners_seen_in_range: i64,
}

#[derive(Serialize)]
pub struct SeriesPoint {
    pub ts: DateTime<Utc>,
    #[serde(rename = "partnerViewers")]
    pub partner_viewers: f64,
    #[serde(rename = "totalViewers")]
    pub total_viewers: f64,
    #[serde(rename = "partnerStreams")]
    pub partner_streams: f64,
    #[serde(rename = "totalStreams")]
    pub total_streams: f64,
    #[serde(rename = "sharePct")]
    pub share_pct: f64,
}

#[derive(Serialize)]
pub struct PeakPoint {
    pub ts: DateTime<Utc>,
    #[serde(rename = "sharePct")]
    pub share_pct: f64,
    #[serde(rename = "partnerViewers")]
    pub partner_viewers: f64,
    #[serde(rename = "totalViewers")]
    pub total_viewers: f64,
}

#[derive(Serialize)]
pub struct CurrentSnapshot {
    pub ts: DateTime<Utc>,
    #[serde(rename = "totalViewers")]
    pub total_viewers: i64,
    #[serde(rename = "partnerViewers")]
    pub partner_viewers: i64,
    #[serde(rename = "totalStreams")]
    pub total_streams: i64,
    #[serde(rename = "partnerStreams")]
    pub partner_streams: i64,
    #[serde(rename = "sharePct")]
    pub share_pct: f64,
    #[serde(rename = "germanViewers")]
    pub german_viewers: i64,
    #[serde(rename = "germanStreams")]
    pub german_streams: i64,
    #[serde(rename = "germanPartnerViewers")]
    pub german_partner_viewers: i64,
    #[serde(rename = "germanPartnerStreams")]
    pub german_partner_streams: i64,
    #[serde(rename = "germanSharePct")]
    pub german_share_pct: f64,
    #[serde(rename = "topStreams")]
    pub top_streams: Vec<TopStream>,
}

#[derive(Serialize)]
pub struct TopStream {
    pub streamer: String,
    pub viewers: i64,
    #[serde(rename = "isPartner")]
    pub is_partner: bool,
    #[serde(rename = "isGerman")]
    pub is_german: bool,
    /// Helix-Stream-Sprache (ISO 639-1), `null` bei Alt-Ticks ohne Spalte.
    pub language: Option<String>,
}

/// Bucket-Breite passend zum Zeitraum, damit die Punktzahl handhabbar bleibt.
fn bucket_seconds_for(days: i64) -> i64 {
    if days <= 1 {
        900 // 15 min → ≤96 Punkte
    } else if days <= 7 {
        7200 // 2 h → ≤84 Punkte
    } else if days <= 31 {
        21600 // 6 h → ≤124 Punkte
    } else {
        86400 // 1 Tag → ≤365 Punkte
    }
}

fn share_pct(part: f64, total: f64) -> f64 {
    if total > 0.0 {
        part / total * 100.0
    } else {
        0.0
    }
}


/// `GET /internal/twitch/v1/market-share?days=7&scope=all|german`
pub async fn market_share_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<MarketShareParams>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let days = params.days.clamp(1, 365);
    let scope: &'static str = match params.scope.as_deref() {
        Some("german") => "german",
        _ => "all",
    };
    let german_only = scope == "german";
    let bucket_seconds = bucket_seconds_for(days);
    let since = Utc::now() - Duration::days(days);

    let rows = market_share_series(&pool, since, bucket_seconds, german_only)
        .await
        .map_err(|_| ApiError::internal())?;

    let series: Vec<SeriesPoint> = rows
        .into_iter()
        .map(|r| {
            let partner = r.partner_viewers.unwrap_or(0.0);
            let total = r.total_viewers.unwrap_or(0.0);
            SeriesPoint {
                ts: r.bucket,
                partner_viewers: partner,
                total_viewers: total,
                partner_streams: r.partner_streams.unwrap_or(0.0),
                total_streams: r.total_streams.unwrap_or(0.0),
                share_pct: share_pct(partner, total),
            }
        })
        .collect();

    // Bewusst ohne Mindestmarkt-Schwelle: auch ein 100%-Bucket mit kleinem
    // Markt ist echte Dominanz (alle Live-Kanäle gehören dem Netzwerk);
    // die Einordnung liefert der Hint (x von y Viewern) im Frontend.
    let peak = series
        .iter()
        .filter(|p| p.total_viewers > 0.0)
        .max_by(|a, b| a.share_pct.total_cmp(&b.share_pct))
        .map(|p| PeakPoint {
            ts: p.ts,
            share_pct: p.share_pct,
            partner_viewers: p.partner_viewers,
            total_viewers: p.total_viewers,
        });

    let tick = market_current_tick(&pool)
        .await
        .map_err(|_| ApiError::internal())?;

    let (partners_total, partners_seen_in_range) = partner_roster(&pool, since)
        .await
        .map_err(|_| ApiError::internal())?;

    let current = tick.first().map(|first| {
        let ts = first.ts_utc;
        let mut total_viewers = 0i64;
        let mut partner_viewers = 0i64;
        let mut partner_streams = 0i64;
        let mut german_viewers = 0i64;
        let mut german_streams = 0i64;
        let mut german_partner_viewers = 0i64;
        let mut german_partner_streams = 0i64;
        for row in &tick {
            let viewers = i64::from(row.viewer_count.unwrap_or(0));
            // DE-Markt = Deutsch-Tag ODER Partner (Partner zählen immer dazu).
            let german = row.is_german.unwrap_or(false) || row.is_partner;
            total_viewers += viewers;
            if row.is_partner {
                partner_viewers += viewers;
                partner_streams += 1;
            }
            if german {
                german_viewers += viewers;
                german_streams += 1;
                if row.is_partner {
                    german_partner_viewers += viewers;
                    german_partner_streams += 1;
                }
            }
        }
        // Top-Streams passend zum angefragten Scope: in der DE-Sicht nur
        // Streams des DE-Markts, nicht die globale Kategorie.
        let top_streams = tick
            .iter()
            .filter(|row| !german_only || row.is_german.unwrap_or(false) || row.is_partner)
            .take(15)
            .map(|row| TopStream {
                streamer: row.streamer.clone(),
                viewers: i64::from(row.viewer_count.unwrap_or(0)),
                is_partner: row.is_partner,
                is_german: row.is_german.unwrap_or(false) || row.is_partner,
                language: row.language.clone(),
            })
            .collect();
        CurrentSnapshot {
            ts,
            total_viewers,
            partner_viewers,
            total_streams: tick.len() as i64,
            partner_streams,
            share_pct: share_pct(partner_viewers as f64, total_viewers as f64),
            german_viewers,
            german_streams,
            german_partner_viewers,
            german_partner_streams,
            german_share_pct: share_pct(german_partner_viewers as f64, german_viewers as f64),
            top_streams,
        }
    });

    Ok(Json(MarketShareResponse {
        days,
        scope,
        bucket_seconds,
        series,
        peak,
        current,
        roster: RosterInfo {
            partners_total,
            partners_seen_in_range,
        },
    }))
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
            CREATE TABLE twitch_stats_category (
                ts_utc       TIMESTAMPTZ NOT NULL,
                streamer     TEXT NOT NULL,
                viewer_count INTEGER,
                is_partner   BOOLEAN DEFAULT FALSE,
                game_name    TEXT,
                stream_title TEXT,
                tags         TEXT,
                language     TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL fehlgeschlagen");
        // Minimal-Abbild der Prod-View für die Roster-Query.
        sqlx::query(
            "CREATE TABLE twitch_partners_all_state (twitch_login TEXT, is_partner_active INTEGER)",
        )
        .execute(&pool)
        .await
        .expect("Roster-DDL fehlgeschlagen");
        pool
    }

    fn make_router(pool: PgPool, token: &str) -> Router {
        Router::new()
            .route(
                "/internal/twitch/v1/market-share",
                get(market_share_handler),
            )
            .with_state(pool)
            .layer(Extension(ExpectedToken(token.to_string())))
    }

    fn req(token: Option<&str>, query: &str) -> Request<Body> {
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        let mut builder = Request::builder()
            .uri(format!("/internal/twitch/v1/market-share{query}"))
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com");
        if let Some(token) = token {
            builder = builder.header("x-internal-token", token);
        }
        builder.body(Body::empty()).unwrap()
    }

    #[tokio::test]
    async fn returns_401_without_auth() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_market_unauth").await;
        let res = make_router(pool, "tok").oneshot(req(None, "")).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn returns_share_and_current_snapshot() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_market_data").await;
        sqlx::query(
            r#"
            INSERT INTO twitch_stats_category
                (ts_utc, streamer, viewer_count, is_partner, tags, language)
            VALUES
                (NOW(), 'partner_a', 25, TRUE,  '["Deutsch"]', 'de'),
                (NOW(), 'big_intl',  75, FALSE, '["English"]', 'en')
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let res = make_router(pool, "tok")
            .oneshot(req(Some("tok"), "?days=1"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 64 * 1024).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["scope"], "all");
        assert_eq!(v["current"]["totalViewers"], 100);
        assert_eq!(v["current"]["partnerViewers"], 25);
        assert_eq!(v["current"]["partnerStreams"], 1);
        assert!((v["current"]["sharePct"].as_f64().unwrap() - 25.0).abs() < 1e-9);
        assert!((v["current"]["germanSharePct"].as_f64().unwrap() - 100.0).abs() < 1e-9);
        let series = v["series"].as_array().unwrap();
        assert_eq!(series.len(), 1);
        assert!((series[0]["sharePct"].as_f64().unwrap() - 25.0).abs() < 1e-9);
        assert!((v["peak"]["sharePct"].as_f64().unwrap() - 25.0).abs() < 1e-9);
        assert_eq!(v["roster"]["partnersTotal"], 0);
        assert_eq!(v["roster"]["partnersSeenInRange"], 1);
    }
}
