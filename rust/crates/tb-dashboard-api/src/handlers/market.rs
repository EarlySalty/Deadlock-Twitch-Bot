//! Markt-Research-Dashboard (interne Admin-/Research-Tools).
//!
//! Nativer Port von `bot/dashboard/routes_market.py` + der HTML-Seite aus
//! `bot/dashboard/pages.py:build_market_research_page`. Deckt drei Routen ab:
//!
//! - `GET /twitch/market` — gerenderte Market-Research-HTML-Seite (P2.104/P2.115).
//!   Liest ihre Daten clientseitig aus `/twitch/api/market_data`.
//! - `GET /twitch/api/market_data` — aggregierte Markt-Daten als JSON
//!   (P2.105/P2.116): überwachte Kanäle, Chat-Health, Lurker-Ratio, 24h-Verlauf,
//!   Fragen-Radar, Deadlock-Term-Sentiment, Viewer-Overlap.
//! - `GET /twitch/api/v2/market-share` — Admin-Proxy auf den internen Rust-Worker
//!   (`tb-internal-api`, :8776) für die Markt-Dominanz-Berechnung (P2.106).
//!
//! **Auth:** Alle drei Routen sind privilegiert (Admin/Localhost). `market_data`
//! und `market-share` liefern bei fehlender Berechtigung 401 bzw. werden vom Proxy
//! gegateted; die HTML-Seite verlangt ebenfalls Admin (Python `_require_token`).
//!
//! **clean-SQL:** Die Aggregation arbeitet auf TIMESTAMPTZ-Zeitspalten
//! (`message_ts`, `ts_utc`, `first_message_at`). Überwachte Kanäle = Einträge in
//! `twitch_streamers`, die NICHT in `twitch_partners` stehen (die früher genutzte
//! Spalte `is_monitored_only` wurde im Schema-Cleanup entfernt; die Definition
//! „kein Partner" bleibt identisch, vgl. admin_streamers-CTE).

use std::time::Duration;

use axum::{
    extract::{Extension, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::auth::level::DashboardAuthLevel;

/// Deadlock-Begriffe für das Meta-Snapshot-/Sentiment-Zählen (Python-Liste 1:1).
const DEADLOCK_TERMS: &[&str] = &[
    "abrams", "bebop", "dynamo", "grey talon", "haze", "infernus", "ivy", "kelvin", "lady geist",
    "mcginnis", "mo & krill", "paradox", "pocket", "seven", "vindicta", "viscous", "warden",
    "wraith", "yamato", "lash", "shiv", "urn", "midboss", "soul", "flex slot", "build", "op",
    "nerf", "buff", "patch",
];

/// Positiv-Wortliste für die simple Sentiment-Heuristik (Python `pos_words`).
const POS_WORDS: &[&str] = &["pog", "gg", "nice", "cool", "krass", "lol", "win", "stark"];
/// Negativ-Wortliste (Python `neg_words`).
const NEG_WORDS: &[&str] = &["rip", "bad", "lose", "troll", "cringe", "throw", "sucks", "lag"];

// ── HTML-Seite (P2.104 / P2.115) ──────────────────────────────────────────────

/// `GET /twitch/market` — interne Market-Research-Seite.
///
/// Admin/Localhost-gated (Python `_require_token`). Die Seite lädt ihre Daten per
/// JS aus `/twitch/api/market_data`. User-sichtbare Texte sind PLATZHALTER (Claude
/// setzt den finalen Wortlaut).
pub async fn market_research_handler(auth: DashboardAuthLevel) -> Response {
    if !auth.is_privileged() {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    Html(render_market_research_page()).into_response()
}

/// Render-Gerüst der Market-Research-Seite. Texte sind PLATZHALTER.
fn render_market_research_page() -> String {
    // PLATZHALTER: alle user-sichtbaren deutschen Texte (Titel, Headlines, KPI-
    // Labels, Sektionsüberschriften) setzt Claude. Das Gerüst hält die Struktur
    // (KPI-Kacheln, 24h-Chart, Meta-Snapshot, Sentiment, Overlap, Fragen-Radar,
    // Live-Kanäle) und den Fetch auf /twitch/api/market_data.
    format!(
        r#"<!DOCTYPE html>
<html lang="de">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
  body {{ font-family: system-ui, sans-serif; margin: 0; background: #0f1117; color: #e6e6e6; }}
  .wrap {{ max-width: 1200px; margin: 0 auto; padding: 24px; }}
  .kpis {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 16px; }}
  .kpi {{ background: #1a1d27; border-radius: 12px; padding: 16px; }}
  .kpi .value {{ font-size: 1.8rem; font-weight: 700; }}
  section {{ margin-top: 32px; }}
  table {{ width: 100%; border-collapse: collapse; }}
  th, td {{ padding: 8px; border-bottom: 1px solid #2a2e3a; text-align: left; }}
</style>
</head>
<body>
<div class="wrap">
  <h1>{title}</h1>
  <p>{subtitle}</p>
  <div class="kpis" id="kpis"></div>
  <section><h2>{h_history}</h2><canvas id="market-history" height="120"></canvas></section>
  <section><h2>{h_meta}</h2><div id="meta-snapshot"></div></section>
  <section><h2>{h_sentiment}</h2><div id="sentiment"></div></section>
  <section><h2>{h_overlap}</h2><div id="overlap"></div></section>
  <section><h2>{h_questions}</h2><div id="questions"></div></section>
  <section><h2>{h_channels}</h2><table id="channels"><thead></thead><tbody></tbody></table></section>
</div>
<script>
  async function loadMarketData() {{
    const res = await fetch('/twitch/api/market_data', {{ credentials: 'same-origin' }});
    if (!res.ok) return;
    const data = await res.json();
    window.__marketData = data;
    document.dispatchEvent(new CustomEvent('market-data-ready', {{ detail: data }}));
  }}
  loadMarketData();
</script>
</body>
</html>"#,
        // PLATZHALTER-Texte:
        title = MR_TITLE,
        subtitle = MR_SUBTITLE,
        h_history = MR_H_HISTORY,
        h_meta = MR_H_META,
        h_sentiment = MR_H_SENTIMENT,
        h_overlap = MR_H_OVERLAP,
        h_questions = MR_H_QUESTIONS,
        h_channels = MR_H_CHANNELS,
    )
}

// User-sichtbare deutsche Seitentexte der Market-Research-Seite (P2.115).
const MR_TITLE: &str = "Markt-Recherche";
const MR_SUBTITLE: &str = "Überblick über die Deadlock-Streaming-Landschaft im DACH-Raum";
const MR_H_HISTORY: &str = "Zuschauer-Verlauf (24 Stunden)";
const MR_H_META: &str = "Markt-Schnappschuss";
const MR_H_SENTIMENT: &str = "Stimmung im Chat";
const MR_H_OVERLAP: &str = "Zuschauer-Überschneidung";
const MR_H_QUESTIONS: &str = "Offene Fragen";
const MR_H_CHANNELS: &str = "Beobachtete Kanäle";

// ── Markt-Daten-API (P2.105 / P2.116) ─────────────────────────────────────────

/// `GET /twitch/api/market_data` — aggregierte Markt-Daten als JSON.
///
/// Admin/Localhost-gated (401 sonst). Port von
/// `routes_market.py:api_market_data`. Liefert die volle Payload-Shape:
/// `total_monitored`, `total_viewers`, `avg_chat_health`, `avg_lurker_ratio`,
/// `total_messages`, `market_history`, `questions`, `channels`, `meta_snapshot`,
/// `sentiment`, `overlap`.
pub async fn api_market_data_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Response {
    if !auth.is_privileged() {
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "Unauthorized" }))).into_response();
    }
    match build_market_data(&pool).await {
        Ok(payload) => Json(payload).into_response(),
        Err(error) => {
            tracing::error!(%error, "market data aggregation failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "market_data_failed" })),
            )
                .into_response()
        }
    }
}

/// Ein überwachter Kanal mit aktuellem Viewer-Stand.
#[derive(Debug, sqlx::FromRow)]
struct MonitoredChannel {
    twitch_login: String,
    last_viewer_count: Option<i32>,
    active_session_id: Option<i64>,
}

/// Aggregierte Kanal-Metriken (nach der Pro-Kanal-Berechnung).
#[derive(Debug, Clone)]
struct ChannelMetrics {
    login: String,
    viewers: i64,
    chat_health: f64,
    lurker_ratio: f64,
    msg_per_min: f64,
}

/// Baut die vollständige Markt-Daten-Payload (clean-SQL über TIMESTAMPTZ-Spalten).
async fn build_market_data(pool: &PgPool) -> Result<Value, sqlx::Error> {
    let monitored = load_monitored_channels(pool).await?;

    let mut channels: Vec<ChannelMetrics> = Vec::with_capacity(monitored.len());
    let mut total_viewers: i64 = 0;
    for ch in &monitored {
        let viewers = ch.last_viewer_count.unwrap_or(0).max(0) as i64;
        total_viewers += viewers;

        let (msgs, active_chatters) = chat_stats_last_hour(pool, &ch.twitch_login).await?;
        let (total_connected, lurkers) = match ch.active_session_id {
            Some(session_id) => {
                let (connected, l) = lurker_stats(pool, session_id).await?;
                // Python: total_connected = connected ODER active_chatters (Fallback).
                (if connected > 0 { connected } else { active_chatters }, l)
            }
            None => (active_chatters, 0),
        };

        let chat_health = if viewers > 0 {
            ((active_chatters as f64 / viewers.max(1) as f64) * 100.0).min(100.0)
        } else {
            0.0
        };
        let lurker_ratio = (lurkers as f64 / total_connected.max(1) as f64) * 100.0;
        let msg_per_min = msgs as f64 / 60.0;

        channels.push(ChannelMetrics {
            login: ch.twitch_login.clone(),
            viewers,
            chat_health,
            lurker_ratio,
            msg_per_min,
        });
    }

    channels.sort_by_key(|c| std::cmp::Reverse(c.viewers));

    let channel_count = channels.len().max(1) as f64;
    let avg_health = channels.iter().map(|c| c.chat_health).sum::<f64>() / channel_count;
    let avg_lurker = channels.iter().map(|c| c.lurker_ratio).sum::<f64>() / channel_count;

    let market_history = market_history_24h(pool).await?;
    let questions = recent_questions(pool).await?;

    let recent_msgs = recent_message_contents(pool).await?;
    let (meta_snapshot, sentiment) = compute_meta_and_sentiment(&recent_msgs);

    let top_logins: Vec<String> = channels.iter().take(5).map(|c| c.login.clone()).collect();
    let overlap = viewer_overlap(pool, &top_logins).await?;

    let channels_json: Vec<Value> = channels
        .iter()
        .map(|c| {
            json!({
                "login": c.login,
                "viewers": c.viewers,
                "is_live": c.viewers > 0,
                "chat_health": c.chat_health,
                "lurker_ratio": c.lurker_ratio,
                "msg_per_min": c.msg_per_min,
                "top_topic": "n/a",
            })
        })
        .collect();

    Ok(json!({
        "total_monitored": channels.len(),
        "total_viewers": total_viewers,
        "avg_chat_health": avg_health,
        "avg_lurker_ratio": avg_lurker,
        "total_messages": recent_msgs.len(),
        "market_history": market_history,
        "questions": questions,
        "channels": channels_json,
        "meta_snapshot": meta_snapshot,
        "sentiment": sentiment,
        "overlap": overlap,
    }))
}

/// Überwachte Kanäle = `twitch_streamers` ohne Partner-Eintrag (clean-SQL-Ersatz
/// für das entfernte `is_monitored_only`-Flag).
async fn load_monitored_channels(pool: &PgPool) -> Result<Vec<MonitoredChannel>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT
            s.twitch_login                       AS twitch_login,
            l.last_viewer_count                  AS last_viewer_count,
            l.active_session_id                  AS active_session_id
        FROM twitch_streamers s
        LEFT JOIN twitch_live_state l ON s.twitch_user_id = l.twitch_user_id
        WHERE NOT EXISTS (
            SELECT 1 FROM twitch_partners p
            WHERE p.twitch_user_id = s.twitch_user_id
               OR LOWER(p.twitch_login) = LOWER(s.twitch_login)
        )
        "#,
    )
    .fetch_all(pool)
    .await
}

/// Nachrichten + distinkte Chatter der letzten Stunde für einen Kanal.
async fn chat_stats_last_hour(pool: &PgPool, login: &str) -> Result<(i64, i64), sqlx::Error> {
    let row: (i64, i64) = sqlx::query_as(
        r#"
        SELECT COUNT(*), COUNT(DISTINCT chatter_login)
        FROM twitch_chat_messages
        WHERE streamer_login = $1
          AND message_ts >= now() - INTERVAL '1 hour'
        "#,
    )
    .bind(login)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// (verbundene Chatter, Lurker) einer aktiven Session.
async fn lurker_stats(pool: &PgPool, session_id: i64) -> Result<(i64, i64), sqlx::Error> {
    let row: (Option<i64>, Option<i64>) = sqlx::query_as(
        r#"
        SELECT COUNT(*), SUM(CASE WHEN messages = 0 THEN 1 ELSE 0 END)
        FROM twitch_session_chatters
        WHERE session_id = $1
        "#,
    )
    .bind(session_id)
    .fetch_one(pool)
    .await?;
    Ok((row.0.unwrap_or(0), row.1.unwrap_or(0)))
}

/// 24h-Markt-Verlauf aus `twitch_stats_category` (pro Tick aggregiert).
async fn market_history_24h(pool: &PgPool) -> Result<Vec<Value>, sqlx::Error> {
    let rows: Vec<(chrono::DateTime<chrono::Utc>, Option<i64>, i64)> = sqlx::query_as(
        r#"
        SELECT ts_utc, SUM(viewer_count) AS total_viewers, COUNT(DISTINCT streamer) AS streamer_count
        FROM twitch_stats_category
        WHERE ts_utc >= now() - INTERVAL '24 hours'
        GROUP BY ts_utc
        ORDER BY ts_utc ASC
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(ts, total, streamers)| {
            json!({
                "ts": ts.to_rfc3339(),
                "total_viewers": total.unwrap_or(0),
                "streamer_count": streamers,
            })
        })
        .collect())
}

/// Frage-Nachrichten der letzten 6 Stunden (enthalten `?`, Länge > 10).
async fn recent_questions(pool: &PgPool) -> Result<Vec<Value>, sqlx::Error> {
    let rows: Vec<(Option<String>, String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        r#"
        SELECT content, streamer_login, message_ts
        FROM twitch_chat_messages
        WHERE message_ts >= now() - INTERVAL '6 hours'
          AND content LIKE '%?%'
          AND length(content) > 10
        ORDER BY message_ts DESC
        LIMIT 20
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(content, streamer, ts)| {
            json!({
                "content": content.unwrap_or_default(),
                "streamer": streamer,
                "ts": ts.to_rfc3339(),
            })
        })
        .collect())
}

/// Nachrichten-Inhalte der letzten Stunde (für Meta-Snapshot + Sentiment).
async fn recent_message_contents(pool: &PgPool) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<(Option<String>,)> = sqlx::query_as(
        r#"
        SELECT content FROM twitch_chat_messages
        WHERE message_ts >= now() - INTERVAL '1 hour'
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(c,)| c.unwrap_or_default()).collect())
}

/// Zählt Deadlock-Terme + Sentiment über die letzten Nachrichten (Python-Logik).
fn compute_meta_and_sentiment(messages: &[String]) -> (Value, Value) {
    let mut term_counts: Vec<(&str, i64)> = DEADLOCK_TERMS.iter().map(|t| (*t, 0)).collect();
    let (mut positive, mut negative, mut neutral) = (0i64, 0i64, 0i64);

    for raw in messages {
        let content = raw.to_lowercase();
        for entry in term_counts.iter_mut() {
            if content.contains(entry.0) {
                entry.1 += 1;
            }
        }
        let is_pos = POS_WORDS.iter().any(|w| content.contains(w));
        let is_neg = NEG_WORDS.iter().any(|w| content.contains(w));
        if is_pos && !is_neg {
            positive += 1;
        } else if is_neg && !is_pos {
            negative += 1;
        } else {
            neutral += 1;
        }
    }

    let mut meta: Vec<(&str, i64)> = term_counts.into_iter().filter(|(_, c)| *c > 0).collect();
    meta.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    meta.truncate(10);
    let meta_snapshot: Vec<Value> = meta
        .into_iter()
        .map(|(term, count)| json!({ "term": term, "count": count }))
        .collect();

    let total = (positive + negative + neutral).max(1) as f64;
    let round1 = |n: i64| (n as f64 / total * 100.0 * 10.0).round() / 10.0;
    let sentiment = json!({
        "positive": positive,
        "negative": negative,
        "neutral": neutral,
        "pos_pct": round1(positive),
        "neg_pct": round1(negative),
        "neu_pct": round1(neutral),
    });
    (Value::Array(meta_snapshot), sentiment)
}

/// Viewer-Overlap der Top-Kanäle (Self-Join über gemeinsame Chatter, 6h).
async fn viewer_overlap(pool: &PgPool, top_logins: &[String]) -> Result<Vec<Value>, sqlx::Error> {
    if top_logins.len() < 2 {
        return Ok(Vec::new());
    }
    let rows: Vec<(String, String, i64)> = sqlx::query_as(
        r#"
        SELECT c1.streamer_login, c2.streamer_login, COUNT(DISTINCT c1.chatter_login)
        FROM twitch_chat_messages c1
        JOIN twitch_chat_messages c2
          ON c1.chatter_login = c2.chatter_login
         AND c1.streamer_login < c2.streamer_login
        WHERE c1.message_ts >= now() - INTERVAL '6 hours'
          AND c2.message_ts >= now() - INTERVAL '6 hours'
          AND c1.streamer_login = ANY($1)
          AND c2.streamer_login = ANY($1)
        GROUP BY 1, 2
        ORDER BY 3 DESC
        LIMIT 5
        "#,
    )
    .bind(top_logins)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(a, b, shared)| json!({ "a": a, "b": b, "shared": shared }))
        .collect())
}

// ── Market-Share-Proxy (P2.106) ───────────────────────────────────────────────

/// Query-Parameter des Market-Share-Proxys (durchgereicht an den Worker).
#[derive(Debug, Deserialize, Default)]
pub struct MarketShareQuery {
    #[serde(default)]
    pub days: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
}

/// Konfiguration des internen Worker-Proxys (als Extension injizierbar; sonst
/// aus der Umgebung). Hält Host/Port/Token NICHT im Log.
#[derive(Clone)]
pub struct MarketShareProxyConfig {
    pub base_url: String,
    pub token: String,
}

impl MarketShareProxyConfig {
    /// Liest die Proxy-Config aus der Umgebung (Infisical/Env). `None`, wenn das
    /// interne Token fehlt → Handler antwortet mit 503 `internal_token_missing`.
    pub fn from_env() -> Option<Self> {
        let token = std::env::var("TWITCH_INTERNAL_API_TOKEN")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())?;
        let host = std::env::var("TWITCH_INTERNAL_API_HOST")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let port = std::env::var("TWITCH_INTERNAL_API_PORT")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "8776".to_string());
        Some(Self {
            base_url: format!("http://{host}:{port}"),
            token,
        })
    }
}

/// `GET /twitch/api/v2/market-share` — Admin-Proxy auf den internen Worker.
///
/// Port von `routes_market.py:api_market_share`. Admin-Gate (sonst 401); fehlt das
/// interne Token (keine Proxy-Config) → 503 `internal_token_missing`; Upstream-
/// Fehler → 502 `market_share_unavailable`; sonst Status + Body durchgereicht.
pub async fn api_market_share_handler(
    auth: DashboardAuthLevel,
    proxy: Option<Extension<MarketShareProxyConfig>>,
    Query(params): Query<MarketShareQuery>,
) -> Response {
    if !auth.is_privileged() {
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "auth_required" }))).into_response();
    }
    let Some(Extension(proxy)) = proxy else {
        tracing::warn!("market-share: TWITCH_INTERNAL_API_TOKEN fehlt");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "internal_token_missing" })),
        )
            .into_response();
    };

    let days = params.days.unwrap_or_else(|| "7".to_string());
    let scope = params.scope.unwrap_or_else(|| "all".to_string());
    let url = format!("{}/internal/twitch/v1/market-share", proxy.base_url);

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(error) => {
            tracing::error!(%error, "market-share: HTTP-Client-Build fehlgeschlagen");
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": "market_share_unavailable" })),
            )
                .into_response();
        }
    };

    let response = client
        .get(&url)
        .query(&[("days", days.as_str()), ("scope", scope.as_str())])
        .header("X-Internal-Token", &proxy.token)
        .send()
        .await;

    match response {
        Ok(resp) => {
            let status = StatusCode::from_u16(resp.status().as_u16())
                .unwrap_or(StatusCode::BAD_GATEWAY);
            match resp.json::<Value>().await {
                Ok(body) => (status, Json(body)).into_response(),
                Err(error) => {
                    tracing::warn!(%error, "market-share: Upstream-Body kein JSON");
                    (
                        StatusCode::BAD_GATEWAY,
                        Json(json!({ "error": "market_share_unavailable" })),
                    )
                        .into_response()
                }
            }
        }
        Err(error) => {
            tracing::warn!(%error, "market-share: Worker-Proxy fehlgeschlagen");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": "market_share_unavailable" })),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    fn admin() -> DashboardAuthLevel {
        DashboardAuthLevel::admin()
    }

    fn partner() -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: "p".into(),
            twitch_user_id: "1".into(),
            display_name: String::new(),
        }
    }

    // ── Reine Logik ───────────────────────────────────────────────────────────

    #[test]
    fn sentiment_classifies_pos_neg_neutral() {
        let msgs = vec![
            "pog so nice".to_string(),       // positiv
            "rip that lag".to_string(),      // negativ
            "hello there".to_string(),       // neutral
            "pog but also rip".to_string(),  // pos + neg → neutral
        ];
        let (_meta, sentiment) = compute_meta_and_sentiment(&msgs);
        assert_eq!(sentiment["positive"], 1);
        assert_eq!(sentiment["negative"], 1);
        assert_eq!(sentiment["neutral"], 2);
    }

    #[test]
    fn meta_snapshot_counts_terms_and_truncates() {
        let msgs = vec![
            "abrams op build".to_string(),
            "abrams nerf".to_string(),
            "haze buff".to_string(),
        ];
        let (meta, _s) = compute_meta_and_sentiment(&msgs);
        let arr = meta.as_array().unwrap();
        // abrams kommt 2x, haze/op/build/nerf/buff je 1x.
        let abrams = arr.iter().find(|e| e["term"] == "abrams").unwrap();
        assert_eq!(abrams["count"], 2);
        assert!(arr.len() <= 10);
    }

    // ── HTML-Seite ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn market_research_requires_privilege() {
        let resp = market_research_handler(partner()).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn market_research_renders_for_admin() {
        let resp = market_research_handler(admin()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let html = String::from_utf8(body.to_vec()).unwrap();
        assert!(html.contains("/twitch/api/market_data"), "page must fetch market_data");
        assert!(html.contains("<html"), "must be HTML");
    }

    // ── Market-Share-Proxy ────────────────────────────────────────────────────

    #[tokio::test]
    async fn market_share_unauthenticated_401() {
        let resp = api_market_share_handler(partner(), None, Query(MarketShareQuery::default())).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn market_share_without_token_503() {
        let resp =
            api_market_share_handler(admin(), None, Query(MarketShareQuery::default())).await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["error"], "internal_token_missing");
    }

    #[tokio::test]
    async fn market_share_proxies_status_and_body() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/internal/twitch/v1/market-share"))
            .and(header("X-Internal-Token", "secret-test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "share_pct": 42.0 })))
            .mount(&server)
            .await;

        let proxy = MarketShareProxyConfig {
            base_url: server.uri(),
            token: "secret-test-token".to_string(),
        };
        let resp = api_market_share_handler(
            admin(),
            Some(Extension(proxy)),
            Query(MarketShareQuery::default()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["share_pct"], 42.0);
    }

    #[tokio::test]
    async fn market_share_upstream_error_502() {
        let proxy = MarketShareProxyConfig {
            // Unerreichbarer Port → Connect-Fehler.
            base_url: "http://127.0.0.1:1".to_string(),
            token: "t".to_string(),
        };
        let resp = api_market_share_handler(
            admin(),
            Some(Extension(proxy)),
            Query(MarketShareQuery::default()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    }

    // ── DB-Aggregation (clean-SQL, gegen echtes Postgres) ─────────────────────

    async fn pool_or_skip(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let pool = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&pool).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&pool).await.unwrap();
        sqlx::query(&format!("SET search_path TO {schema}")).execute(&pool).await.unwrap();
        // clean-SQL: TIMESTAMPTZ-Zeitspalten.
        sqlx::query(
            r#"CREATE TABLE twitch_streamers (
                   twitch_login TEXT NOT NULL, twitch_user_id TEXT, created_at TIMESTAMPTZ DEFAULT now()
               )"#,
        ).execute(&pool).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE twitch_partners (
                   id BIGINT, twitch_user_id TEXT NOT NULL, twitch_login TEXT NOT NULL,
                   status TEXT DEFAULT 'active'
               )"#,
        ).execute(&pool).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE twitch_live_state (
                   twitch_user_id TEXT NOT NULL, streamer_login TEXT NOT NULL,
                   last_viewer_count INTEGER DEFAULT 0, active_session_id BIGINT
               )"#,
        ).execute(&pool).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE twitch_chat_messages (
                   id SERIAL PRIMARY KEY, session_id BIGINT NOT NULL DEFAULT 0,
                   streamer_login TEXT NOT NULL, chatter_login TEXT,
                   message_ts TIMESTAMPTZ NOT NULL, content TEXT
               )"#,
        ).execute(&pool).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE twitch_session_chatters (
                   session_id BIGINT NOT NULL, streamer_login TEXT NOT NULL,
                   chatter_login TEXT NOT NULL, first_message_at TIMESTAMPTZ NOT NULL,
                   messages INTEGER DEFAULT 0
               )"#,
        ).execute(&pool).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE twitch_stats_category (
                   ts_utc TIMESTAMPTZ, streamer TEXT, viewer_count INTEGER,
                   is_partner BOOLEAN DEFAULT FALSE, tags TEXT
               )"#,
        ).execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn market_data_full_payload_shape() {
        let Some(pool) = pool_or_skip("market_data_shape").await else { return };
        // Ein überwachter Kanal (kein Partner) + ein Partner (ausgeschlossen).
        sqlx::query("INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('mon', '100'), ('partnerx', '200')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_partners (id, twitch_user_id, twitch_login) VALUES (1, '200', 'partnerx')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, streamer_login, last_viewer_count, active_session_id) VALUES ('100', 'mon', 50, 7)")
            .execute(&pool).await.unwrap();
        // Chat-Messages der letzten Stunde.
        sqlx::query(
            "INSERT INTO twitch_chat_messages (streamer_login, chatter_login, message_ts, content) \
             VALUES ('mon', 'a', now() - INTERVAL '5 minutes', 'abrams is op pog'), \
                    ('mon', 'b', now() - INTERVAL '10 minutes', 'why is haze nerfed?')",
        ).execute(&pool).await.unwrap();
        // Session-Chatter (ein Lurker).
        sqlx::query(
            "INSERT INTO twitch_session_chatters (session_id, streamer_login, chatter_login, first_message_at, messages) \
             VALUES (7, 'mon', 'a', now(), 3), (7, 'mon', 'lurk', now(), 0)",
        ).execute(&pool).await.unwrap();
        // Stats-History.
        sqlx::query("INSERT INTO twitch_stats_category (ts_utc, streamer, viewer_count) VALUES (now() - INTERVAL '1 hour', 'mon', 50)")
            .execute(&pool).await.unwrap();

        let payload = build_market_data(&pool).await.unwrap();
        // Shape-Vollständigkeit.
        for key in [
            "total_monitored", "total_viewers", "avg_chat_health", "avg_lurker_ratio",
            "total_messages", "market_history", "questions", "channels", "meta_snapshot",
            "sentiment", "overlap",
        ] {
            assert!(payload.get(key).is_some(), "missing key {key}");
        }
        // Nur der nicht-Partner-Kanal ist überwacht.
        assert_eq!(payload["total_monitored"], 1);
        assert_eq!(payload["total_viewers"], 50);
        let channels = payload["channels"].as_array().unwrap();
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0]["login"], "mon");
        // Lurker-Ratio: 1 von 2 = 50%.
        assert_eq!(channels[0]["lurker_ratio"], 50.0);
        // Frage erkannt.
        assert_eq!(payload["questions"].as_array().unwrap().len(), 1);
        // Meta-Snapshot enthält abrams + haze.
        let terms: Vec<&str> = payload["meta_snapshot"].as_array().unwrap()
            .iter().filter_map(|e| e["term"].as_str()).collect();
        assert!(terms.contains(&"abrams"));
        assert!(terms.contains(&"haze"));
    }

    #[tokio::test]
    async fn api_market_data_handler_gates_non_admin() {
        let Some(pool) = pool_or_skip("market_data_gate").await else { return };
        let resp = api_market_data_handler(partner(), State(pool)).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn api_market_data_handler_admin_200() {
        let Some(pool) = pool_or_skip("market_data_admin").await else { return };
        let resp = api_market_data_handler(admin(), State(pool)).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
