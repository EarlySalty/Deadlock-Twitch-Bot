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
//! - `GET /twitch/api/v2/market-share` — dünner Dashboard-Wrapper auf
//!   `tb_analytics::market` für die Markt-Dominanz-Berechnung (P2.106).
//!
//! **Auth:** Alle drei Routen sind privilegiert (Admin/Localhost). `market_data`
//! liefert bei fehlender Berechtigung 401; `market-share` nutzt das Python-
//! Admin-API-Gate (`auth_required`/`admin_required`); die HTML-Seite verlangt
//! ebenfalls Admin (Python `_require_token`).
//!
//! **clean-SQL:** Die Aggregation arbeitet auf TIMESTAMPTZ-Zeitspalten
//! (`message_ts`, `ts_utc`, `first_message_at`). Überwachte Kanäle = Einträge in
//! `twitch_streamers`, die NICHT in `twitch_partners` stehen (die früher genutzte
//! Spalte `is_monitored_only` wurde im Schema-Cleanup entfernt; die Definition
//! „kein Partner" bleibt identisch, vgl. admin_streamers-CTE).

use axum::{
    extract::{Extension, Query, State},
    http::{request::Parts, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use tb_analytics::market::{market_current_tick, market_share_series, partner_roster};
use tb_http_core::ExpectedToken;

use crate::auth::level::{is_local_request, DashboardAuthLevel};

/// Deadlock-Begriffe für das Meta-Snapshot-/Sentiment-Zählen (Python-Liste 1:1).
const DEADLOCK_TERMS: &[&str] = &[
    "abrams",
    "bebop",
    "dynamo",
    "grey talon",
    "haze",
    "infernus",
    "ivy",
    "kelvin",
    "lady geist",
    "mcginnis",
    "mo & krill",
    "paradox",
    "pocket",
    "seven",
    "vindicta",
    "viscous",
    "warden",
    "wraith",
    "yamato",
    "lash",
    "shiv",
    "urn",
    "midboss",
    "soul",
    "flex slot",
    "build",
    "op",
    "nerf",
    "buff",
    "patch",
];

/// Positiv-Wortliste für die simple Sentiment-Heuristik (Python `pos_words`).
const POS_WORDS: &[&str] = &["pog", "gg", "nice", "cool", "krass", "lol", "win", "stark"];
/// Negativ-Wortliste (Python `neg_words`).
const NEG_WORDS: &[&str] = &[
    "rip", "bad", "lose", "troll", "cringe", "throw", "sucks", "lag",
];

// ── HTML-Seite (P2.104 / P2.115) ──────────────────────────────────────────────

/// `GET /twitch/market` — interne Market-Research-Seite.
///
/// Admin/Localhost-gated (Python `_require_token`). Die Seite lädt ihre Daten per
/// JS aus `/twitch/api/market_data`. User-sichtbare Texte stehen final als
/// `MR_*`-Konstanten.
pub async fn market_research_handler(auth: DashboardAuthLevel) -> Response {
    if !auth.is_privileged() {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    Html(render_market_research_page()).into_response()
}

/// Render-Gerüst der Market-Research-Seite. Texte kommen aus den `MR_*`-Konstanten.
fn render_market_research_page() -> String {
    let labels = json!({
        "loading": MR_LOADING,
        "error": MR_ERROR,
        "empty": MR_EMPTY,
        "kpiMonitored": MR_KPI_MONITORED,
        "kpiViewers": MR_KPI_VIEWERS,
        "kpiHealth": MR_KPI_HEALTH,
        "kpiLurkers": MR_KPI_LURKERS,
        "kpiMessages": MR_KPI_MESSAGES,
        "channel": MR_TH_CHANNEL,
        "viewers": MR_TH_VIEWERS,
        "health": MR_TH_HEALTH,
        "lurkers": MR_TH_LURKERS,
        "messages": MR_TH_MESSAGES,
        "topic": MR_TH_TOPIC,
        "term": MR_TH_TERM,
        "count": MR_TH_COUNT,
        "streamer": MR_TH_STREAMER,
        "question": MR_TH_QUESTION,
        "source": MR_TH_SOURCE,
        "target": MR_TH_TARGET,
        "shared": MR_TH_SHARED,
        "positive": MR_SENTIMENT_POSITIVE,
        "negative": MR_SENTIMENT_NEGATIVE,
    })
    .to_string();
    format!(
        r#"<!DOCTYPE html>
<html lang="de">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
  :root {{ color-scheme: dark; }}
  body {{ font-family: system-ui, sans-serif; margin: 0; background: #101318; color: #f0f3f5; }}
  .wrap {{ max-width: 1180px; margin: 0 auto; padding: 24px; }}
  header {{ margin-bottom: 20px; }}
  h1 {{ font-size: 2rem; margin: 0 0 8px; letter-spacing: 0; }}
  h2 {{ font-size: 1.1rem; margin: 0 0 12px; letter-spacing: 0; }}
  p {{ margin: 0; color: #b8c0c8; }}
  .status {{ min-height: 24px; color: #b8c0c8; margin: 12px 0 0; }}
  .kpis {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(170px, 1fr)); gap: 12px; }}
  .kpi {{ background: #1a2028; border: 1px solid #2c3642; border-radius: 8px; padding: 14px; }}
  .kpi .label {{ color: #aeb8c2; font-size: .82rem; }}
  .kpi .value {{ font-size: 1.7rem; font-weight: 700; margin-top: 6px; }}
  section {{ margin-top: 28px; }}
  .panel {{ background: #171c23; border: 1px solid #29333f; border-radius: 8px; padding: 14px; overflow-x: auto; }}
  table {{ width: 100%; border-collapse: collapse; min-width: 520px; }}
  th, td {{ padding: 9px 8px; border-bottom: 1px solid #2a3440; text-align: left; white-space: nowrap; }}
  th {{ color: #aeb8c2; font-weight: 600; font-size: .82rem; }}
  .bars {{ display: grid; gap: 10px; }}
  .bar {{ display: grid; grid-template-columns: minmax(90px, 160px) 1fr 60px; gap: 10px; align-items: center; }}
  .track {{ height: 10px; background: #28323d; border-radius: 999px; overflow: hidden; }}
  .fill {{ height: 100%; background: #4fb286; }}
  .fill.neg {{ background: #d36b64; }}
  canvas {{ width: 100%; max-height: 260px; background: #171c23; border: 1px solid #29333f; border-radius: 8px; }}
  @media (max-width: 640px) {{
    .wrap {{ padding: 16px; }}
    .bar {{ grid-template-columns: 1fr; }}
  }}
</style>
</head>
<body>
<div class="wrap">
  <header>
    <h1>{title}</h1>
    <p>{subtitle}</p>
    <div id="status" class="status">{loading}</div>
  </header>
  <div class="kpis" id="kpis"></div>
  <section><h2>{h_history}</h2><canvas id="market-history" height="120"></canvas></section>
  <section><h2>{h_meta}</h2><div class="panel" id="meta-snapshot"></div></section>
  <section><h2>{h_sentiment}</h2><div class="panel" id="sentiment"></div></section>
  <section><h2>{h_overlap}</h2><div class="panel" id="overlap"></div></section>
  <section><h2>{h_questions}</h2><div class="panel" id="questions"></div></section>
  <section><h2>{h_channels}</h2><div class="panel"><table id="channels"><thead></thead><tbody></tbody></table></div></section>
</div>
<script>
  const L = {labels};
  const fmt = new Intl.NumberFormat('de-DE');
  const pct = (v) => `${{Number(v || 0).toFixed(1)}}%`;
  const byId = (id) => document.getElementById(id);
  const cell = (tag, value) => {{
    const node = document.createElement(tag);
    node.textContent = value == null ? '' : String(value);
    return node;
  }};
  const table = (headers, rows) => {{
    const t = document.createElement('table');
    const thead = document.createElement('thead');
    const hrow = document.createElement('tr');
    headers.forEach((h) => hrow.appendChild(cell('th', h)));
    thead.appendChild(hrow);
    const tbody = document.createElement('tbody');
    rows.forEach((row) => {{
      const tr = document.createElement('tr');
      row.forEach((value) => tr.appendChild(cell('td', value)));
      tbody.appendChild(tr);
    }});
    t.append(thead, tbody);
    return t;
  }};
  const replace = (id, node) => {{
    const target = byId(id);
    target.replaceChildren(node);
  }};
  const emptyNode = () => cell('div', L.empty);

  function renderKpis(data) {{
    const items = [
      [L.kpiMonitored, fmt.format(data.total_monitored || 0)],
      [L.kpiViewers, fmt.format(data.total_viewers || 0)],
      [L.kpiHealth, pct(data.avg_chat_health)],
      [L.kpiLurkers, pct(data.avg_lurker_ratio)],
      [L.kpiMessages, fmt.format(data.total_messages || 0)],
    ];
    byId('kpis').replaceChildren(...items.map(([label, value]) => {{
      const card = document.createElement('div');
      card.className = 'kpi';
      const labelNode = document.createElement('div');
      labelNode.className = 'label';
      labelNode.textContent = label;
      const valueNode = document.createElement('div');
      valueNode.className = 'value';
      valueNode.textContent = value;
      card.append(labelNode, valueNode);
      return card;
    }}));
  }}

  function drawHistory(points) {{
    const canvas = byId('market-history');
    const ctx = canvas.getContext('2d');
    const width = canvas.clientWidth || 900;
    const height = 240;
    canvas.width = width * window.devicePixelRatio;
    canvas.height = height * window.devicePixelRatio;
    ctx.scale(window.devicePixelRatio, window.devicePixelRatio);
    ctx.clearRect(0, 0, width, height);
    const values = (points || []).map((p) => Number(p.total_viewers || p.viewers || 0));
    if (!values.length) return;
    const max = Math.max(...values, 1);
    ctx.strokeStyle = '#4fb286';
    ctx.lineWidth = 2;
    ctx.beginPath();
    values.forEach((value, i) => {{
      const x = values.length === 1 ? 0 : (i / (values.length - 1)) * (width - 20) + 10;
      const y = height - 16 - (value / max) * (height - 32);
      if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
    }});
    ctx.stroke();
  }}

  function renderMeta(items) {{
    if (!items || !items.length) return replace('meta-snapshot', emptyNode());
    replace('meta-snapshot', table([L.term, L.count], items.map((item) => [
      item.term,
      fmt.format(item.count || 0),
    ])));
  }}

  function renderSentiment(sentiment) {{
    const pos = Number(sentiment?.positive || 0);
    const neg = Number(sentiment?.negative || 0);
    const max = Math.max(pos, neg, 1);
    const wrap = document.createElement('div');
    wrap.className = 'bars';
    [[L.positive, pos, ''], [L.negative, neg, 'neg']].forEach(([label, value, kind]) => {{
      const row = document.createElement('div');
      row.className = 'bar';
      row.appendChild(cell('div', label));
      const track = document.createElement('div');
      track.className = 'track';
      const fill = document.createElement('div');
      fill.className = `fill ${{kind}}`;
      fill.style.width = `${{(Number(value) / max) * 100}}%`;
      track.appendChild(fill);
      row.appendChild(track);
      row.appendChild(cell('div', fmt.format(value)));
      wrap.appendChild(row);
    }});
    replace('sentiment', wrap);
  }}

  function renderOverlap(items) {{
    if (!items || !items.length) return replace('overlap', emptyNode());
    replace('overlap', table([L.source, L.target, L.shared], items.map((item) => [
      item.source_login || item.source || item.a || '',
      item.target_login || item.target || item.b || '',
      fmt.format(item.shared_viewers || item.shared || item.count || 0),
    ])));
  }}

  function renderQuestions(items) {{
    if (!items || !items.length) return replace('questions', emptyNode());
    replace('questions', table([L.streamer, L.question], items.map((item) => [
      item.streamer_login || item.streamer || '',
      item.content || item.message || item.question || '',
    ])));
  }}

  function renderChannels(items) {{
    const tableNode = byId('channels');
    const rows = (items || []).map((item) => [
      item.login || '',
      fmt.format(item.viewers || 0),
      pct(item.chat_health),
      pct(item.lurker_ratio),
      Number(item.msg_per_min || 0).toFixed(2),
      item.top_topic || '',
    ]);
    tableNode.querySelector('thead').replaceChildren();
    tableNode.querySelector('tbody').replaceChildren();
    const rendered = table([L.channel, L.viewers, L.health, L.lurkers, L.messages, L.topic], rows);
    tableNode.querySelector('thead').replaceWith(rendered.querySelector('thead'));
    tableNode.querySelector('tbody').replaceWith(rendered.querySelector('tbody'));
  }}

  function renderMarket(data) {{
    byId('status').textContent = '';
    renderKpis(data);
    drawHistory(data.market_history);
    renderMeta(data.meta_snapshot);
    renderSentiment(data.sentiment || {{}});
    renderOverlap(data.overlap);
    renderQuestions(data.questions);
    renderChannels(data.channels);
  }}

  async function loadMarketData() {{
    try {{
      const res = await fetch('/twitch/api/market_data', {{ credentials: 'same-origin' }});
      if (!res.ok) throw new Error(String(res.status));
      const data = await res.json();
      window.__marketData = data;
      renderMarket(data);
      document.dispatchEvent(new CustomEvent('market-data-ready', {{ detail: data }}));
    }} catch (_error) {{
      byId('status').textContent = L.error;
    }}
  }}
  loadMarketData();
</script>
</body>
</html>"#,
        title = MR_TITLE,
        subtitle = MR_SUBTITLE,
        h_history = MR_H_HISTORY,
        h_meta = MR_H_META,
        h_sentiment = MR_H_SENTIMENT,
        h_overlap = MR_H_OVERLAP,
        h_questions = MR_H_QUESTIONS,
        h_channels = MR_H_CHANNELS,
        loading = MR_LOADING,
        labels = labels,
    )
}

// User-sichtbare deutsche Seitentexte der Market-Research-Seite (P2.115).
const MR_TITLE: &str = "Markt-Recherche";
const MR_SUBTITLE: &str =
    "Überblick über die beobachtete Deadlock-Streaming-Landschaft: Reichweite, Chat-Aktivität, Themen und Fragen.";
const MR_H_HISTORY: &str = "Reichweite im Zeitverlauf";
const MR_H_META: &str = "Meta-Snapshot";
const MR_H_SENTIMENT: &str = "Stimmung im Chat";
const MR_H_OVERLAP: &str = "Zuschauer-Überschneidung";
const MR_H_QUESTIONS: &str = "Häufige Fragen";
const MR_H_CHANNELS: &str = "Kanäle";
const MR_LOADING: &str = "Lädt …";
const MR_ERROR: &str = "Daten konnten nicht geladen werden.";
const MR_EMPTY: &str = "Noch keine Daten vorhanden.";
const MR_KPI_MONITORED: &str = "Beobachtete Kanäle";
const MR_KPI_VIEWERS: &str = "Zuschauer gesamt";
const MR_KPI_HEALTH: &str = "Chat-Health";
const MR_KPI_LURKERS: &str = "Lurker";
const MR_KPI_MESSAGES: &str = "Nachrichten";
const MR_TH_CHANNEL: &str = "Kanal";
const MR_TH_VIEWERS: &str = "Zuschauer";
const MR_TH_HEALTH: &str = "Health";
const MR_TH_LURKERS: &str = "Lurker";
const MR_TH_MESSAGES: &str = "Nachrichten";
const MR_TH_TOPIC: &str = "Thema";
const MR_TH_TERM: &str = "Begriff";
const MR_TH_COUNT: &str = "Anzahl";
const MR_TH_STREAMER: &str = "Streamer";
const MR_TH_QUESTION: &str = "Frage";
const MR_TH_SOURCE: &str = "Quelle";
const MR_TH_TARGET: &str = "Ziel";
const MR_TH_SHARED: &str = "Gemeinsam";
const MR_SENTIMENT_POSITIVE: &str = "Positiv";
const MR_SENTIMENT_NEGATIVE: &str = "Negativ";

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
    headers: HeaderMap,
    expected: Option<Extension<ExpectedToken>>,
    parts: Parts,
) -> Response {
    if !market_admin_allowed(&auth, &headers, expected.as_ref().map(|e| &e.0), &parts) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Unauthorized" })),
        )
            .into_response();
    }
    match build_market_data(&pool).await {
        Ok(payload) => Json(payload).into_response(),
        Err(error) => {
            let error_id = uuid::Uuid::new_v4().to_string();
            tracing::error!(%error, %error_id, "market data aggregation failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "market_data_failed", "error_id": error_id })),
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
                (
                    if connected > 0 {
                        connected
                    } else {
                        active_chatters
                    },
                    l,
                )
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
    sqlx::query_as!(
        MonitoredChannel,
        r#"
        SELECT
            s.twitch_login                       AS twitch_login,
            l.last_viewer_count                  AS "last_viewer_count?",
            l.active_session_id                  AS "active_session_id?"
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
    let row = sqlx::query!(
        r#"
        SELECT COUNT(*) AS "messages!", COUNT(DISTINCT chatter_login) AS "chatters!"
        FROM twitch_chat_messages
        WHERE streamer_login = $1
          AND message_ts >= now() - INTERVAL '1 hour'
        "#,
        login
    )
    .fetch_one(pool)
    .await?;
    Ok((row.messages, row.chatters))
}

/// (verbundene Chatter, Lurker) einer aktiven Session.
async fn lurker_stats(pool: &PgPool, session_id: i64) -> Result<(i64, i64), sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT COUNT(*) AS "connected!", SUM(CASE WHEN messages = 0 THEN 1 ELSE 0 END) AS "lurkers?"
        FROM twitch_session_chatters
        WHERE session_id = $1
        "#,
        session_id
    )
    .fetch_one(pool)
    .await?;
    Ok((row.connected, row.lurkers.unwrap_or(0)))
}

/// 24h-Markt-Verlauf aus `twitch_stats_category` (pro Tick aggregiert).
async fn market_history_24h(pool: &PgPool) -> Result<Vec<Value>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT ts_utc, SUM(viewer_count) AS "total_viewers?", COUNT(DISTINCT streamer) AS "streamer_count!"
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
        .map(|row| {
            json!({
                "ts": row.ts_utc.to_rfc3339(),
                "total_viewers": row.total_viewers.unwrap_or(0),
                "streamer_count": row.streamer_count,
            })
        })
        .collect())
}

/// Frage-Nachrichten der letzten 6 Stunden (enthalten `?`, Länge > 10).
async fn recent_questions(pool: &PgPool) -> Result<Vec<Value>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT content AS "content?", streamer_login, message_ts
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
        .map(|row| {
            json!({
                "content": row.content.unwrap_or_default(),
                "streamer": row.streamer_login,
                "ts": row.message_ts.to_rfc3339(),
            })
        })
        .collect())
}

/// Nachrichten-Inhalte der letzten Stunde (für Meta-Snapshot + Sentiment).
async fn recent_message_contents(pool: &PgPool) -> Result<Vec<String>, sqlx::Error> {
    let rows = sqlx::query_scalar!(
        r#"
        SELECT content AS "content?" FROM twitch_chat_messages
        WHERE message_ts >= now() - INTERVAL '1 hour'
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|c| c.unwrap_or_default()).collect())
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
    let rows = sqlx::query!(
        r#"
        SELECT c1.streamer_login AS "a!", c2.streamer_login AS "b!", COUNT(DISTINCT c1.chatter_login) AS "shared!"
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
        top_logins
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| json!({ "a": row.a, "b": row.b, "shared": row.shared }))
        .collect())
}

// ── Market-Share-Dashboard-Wrapper (P2.106) ──────────────────────────────────

/// Query-Parameter des Market-Share-Wrappers.
#[derive(Debug, Deserialize, Default)]
pub struct MarketShareQuery {
    #[serde(default)]
    pub days: Option<i64>,
    #[serde(default)]
    pub scope: Option<String>,
}

struct SharePoint {
    ts: DateTime<Utc>,
    partner_viewers: f64,
    total_viewers: f64,
    partner_streams: f64,
    total_streams: f64,
    share_pct: f64,
}

fn market_share_bucket_seconds_for(days: i64) -> i64 {
    if days <= 1 {
        900
    } else if days <= 7 {
        7200
    } else if days <= 31 {
        21600
    } else {
        86400
    }
}

fn market_share_pct(part: f64, total: f64) -> f64 {
    if total > 0.0 {
        part / total * 100.0
    } else {
        0.0
    }
}

async fn build_market_share(
    pool: &PgPool,
    days: Option<i64>,
    scope: Option<&str>,
) -> Result<Value, sqlx::Error> {
    let days = days.unwrap_or(7).clamp(1, 365);
    let scope: &'static str = match scope {
        Some("german") => "german",
        _ => "all",
    };
    let german_only = scope == "german";
    let bucket_seconds = market_share_bucket_seconds_for(days);
    let since = Utc::now() - ChronoDuration::days(days);

    let rows = market_share_series(pool, since, bucket_seconds, german_only).await?;
    let series: Vec<SharePoint> = rows
        .into_iter()
        .map(|r| {
            let partner = r.partner_viewers.unwrap_or(0.0);
            let total = r.total_viewers.unwrap_or(0.0);
            SharePoint {
                ts: r.bucket,
                partner_viewers: partner,
                total_viewers: total,
                partner_streams: r.partner_streams.unwrap_or(0.0),
                total_streams: r.total_streams.unwrap_or(0.0),
                share_pct: market_share_pct(partner, total),
            }
        })
        .collect();

    let peak = series
        .iter()
        .filter(|p| p.total_viewers > 0.0)
        .max_by(|a, b| a.share_pct.total_cmp(&b.share_pct))
        .map(|p| {
            json!({
                "ts": p.ts,
                "sharePct": p.share_pct,
                "partnerViewers": p.partner_viewers,
                "totalViewers": p.total_viewers,
            })
        });

    let series_json: Vec<Value> = series
        .iter()
        .map(|p| {
            json!({
                "ts": p.ts,
                "partnerViewers": p.partner_viewers,
                "totalViewers": p.total_viewers,
                "partnerStreams": p.partner_streams,
                "totalStreams": p.total_streams,
                "sharePct": p.share_pct,
            })
        })
        .collect();

    let tick = market_current_tick(pool).await?;
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
        let top_streams: Vec<Value> = tick
            .iter()
            .filter(|row| !german_only || row.is_german.unwrap_or(false) || row.is_partner)
            .take(15)
            .map(|row| {
                json!({
                    "streamer": row.streamer.clone(),
                    "viewers": i64::from(row.viewer_count.unwrap_or(0)),
                    "isPartner": row.is_partner,
                    "isGerman": row.is_german.unwrap_or(false) || row.is_partner,
                    "language": row.language.clone(),
                })
            })
            .collect();
        json!({
            "ts": ts,
            "totalViewers": total_viewers,
            "partnerViewers": partner_viewers,
            "totalStreams": tick.len() as i64,
            "partnerStreams": partner_streams,
            "sharePct": market_share_pct(partner_viewers as f64, total_viewers as f64),
            "germanViewers": german_viewers,
            "germanStreams": german_streams,
            "germanPartnerViewers": german_partner_viewers,
            "germanPartnerStreams": german_partner_streams,
            "germanSharePct": market_share_pct(german_partner_viewers as f64, german_viewers as f64),
            "topStreams": top_streams,
        })
    });

    let (partners_total, partners_seen_in_range) = partner_roster(pool, since).await?;

    Ok(json!({
        "days": days,
        "scope": scope,
        "bucketSeconds": bucket_seconds,
        "series": series_json,
        "peak": peak,
        "current": current,
        "roster": {
            "partnersTotal": partners_total,
            "partnersSeenInRange": partners_seen_in_range,
        },
    }))
}

fn constant_time_eq(provided: &[u8], expected: &[u8]) -> bool {
    if provided.len() != expected.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in provided.iter().zip(expected.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

fn admin_header_matches(headers: &HeaderMap, expected: Option<&ExpectedToken>) -> bool {
    let Some(ExpectedToken(expected)) = expected else {
        return false;
    };
    if expected.is_empty() {
        return false;
    }
    let provided = headers
        .get("X-Admin-Token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    constant_time_eq(provided.as_bytes(), expected.as_bytes())
}

fn market_admin_allowed(
    auth: &DashboardAuthLevel,
    headers: &HeaderMap,
    expected: Option<&ExpectedToken>,
    parts: &Parts,
) -> bool {
    auth.is_privileged() || is_local_request(parts) || admin_header_matches(headers, expected)
}

fn market_share_auth_error(
    auth: &DashboardAuthLevel,
    headers: &HeaderMap,
    expected: Option<&ExpectedToken>,
    parts: &Parts,
) -> Option<Response> {
    if market_admin_allowed(auth, headers, expected, parts) {
        return None;
    }
    match auth {
        DashboardAuthLevel::Admin { .. } => None,
        DashboardAuthLevel::None => Some(
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "auth_required", "required": "admin" })),
            )
                .into_response(),
        ),
        _ => Some(
            (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "admin_required",
                    "required": "admin",
                    "auth_level": auth.as_str(),
                })),
            )
                .into_response(),
        ),
    }
}

/// `GET /twitch/api/v2/market-share` — Dashboard-Wrapper auf native Analytics.
///
/// Port von `routes_market.py:api_market_share` ohne HTTP-Hop. Admin-Gate wie
/// Python `_require_v2_admin_api`; JSON-Shape identisch zum bisherigen Worker
/// `/internal/twitch/v1/market-share`.
pub async fn api_market_share_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    headers: HeaderMap,
    expected: Option<Extension<ExpectedToken>>,
    Query(params): Query<MarketShareQuery>,
    parts: Parts,
) -> Response {
    if let Some(resp) =
        market_share_auth_error(&auth, &headers, expected.as_ref().map(|e| &e.0), &parts)
    {
        return resp;
    }
    match build_market_share(&pool, params.days, params.scope.as_deref()).await {
        Ok(body) => Json(body).into_response(),
        Err(error) => {
            tracing::error!(%error, "market-share aggregation failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "internal_error",
                    "message": "internal server error",
                })),
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

    fn remote_parts() -> Parts {
        axum::http::Request::builder()
            .uri("/")
            .header(axum::http::header::HOST, "example.com")
            .body(())
            .unwrap()
            .into_parts()
            .0
    }

    // ── Reine Logik ───────────────────────────────────────────────────────────

    #[test]
    fn sentiment_classifies_pos_neg_neutral() {
        let msgs = vec![
            "pog so nice".to_string(),      // positiv
            "rip that lag".to_string(),     // negativ
            "hello there".to_string(),      // neutral
            "pog but also rip".to_string(), // pos + neg → neutral
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
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let html = String::from_utf8(body.to_vec()).unwrap();
        assert!(
            html.contains("/twitch/api/market_data"),
            "page must fetch market_data"
        );
        assert!(html.contains("function renderMarket"), "page must render data");
        assert!(html.contains(MR_TITLE), "page must carry the final German title");
        assert!(html.contains("<html"), "must be HTML");
    }

    // ── Market-Share-Wrapper ──────────────────────────────────────────────────

    #[tokio::test]
    async fn market_share_unauthenticated_401() {
        let Some(pool) = pool_or_skip("market_share_none").await else {
            return;
        };
        let resp = api_market_share_handler(
            DashboardAuthLevel::None,
            State(pool),
            HeaderMap::new(),
            None,
            Query(MarketShareQuery::default()),
            remote_parts(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["error"], "auth_required");
    }

    #[tokio::test]
    async fn market_share_partner_403() {
        let Some(pool) = pool_or_skip("market_share_partner").await else {
            return;
        };
        let resp = api_market_share_handler(
            partner(),
            State(pool),
            HeaderMap::new(),
            None,
            Query(MarketShareQuery::default()),
            remote_parts(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["error"], "admin_required");
        assert_eq!(v["auth_level"], "partner");
    }

    #[tokio::test]
    async fn market_share_admin_direct_payload() {
        let Some(pool) = pool_or_skip("market_share_direct").await else {
            return;
        };
        sqlx::query(
            r#"
            INSERT INTO twitch_stats_category
                (ts_utc, streamer, viewer_count, is_partner, tags, language)
            VALUES
                (NOW(), 'partner_a', 25, true,  '["Deutsch"]', 'de'),
                (NOW(), 'big_intl',  75, false, '["English"]', 'en')
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_partners_all_state (twitch_login, is_partner_active) VALUES ('partner_a', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let resp = api_market_share_handler(
            admin(),
            State(pool),
            HeaderMap::new(),
            None,
            Query(MarketShareQuery {
                days: Some(1),
                scope: None,
            }),
            remote_parts(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["days"], 1);
        assert_eq!(v["scope"], "all");
        assert_eq!(v["current"]["totalViewers"], 100);
        assert_eq!(v["current"]["partnerViewers"], 25);
        assert!((v["current"]["sharePct"].as_f64().unwrap() - 25.0).abs() < 1e-9);
        assert_eq!(v["roster"]["partnersTotal"], 1);
        assert_eq!(v["roster"]["partnersSeenInRange"], 1);
    }

    #[tokio::test]
    async fn market_share_db_error_500_internal_shape() {
        let Some(pool) = pool_or_skip("market_share_broken").await else {
            return;
        };
        sqlx::query("DROP TABLE twitch_stats_category")
            .execute(&pool)
            .await
            .unwrap();
        let resp = api_market_share_handler(
            admin(),
            State(pool),
            HeaderMap::new(),
            None,
            Query(MarketShareQuery::default()),
            remote_parts(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["error"], "internal_error");
    }

    // ── DB-Aggregation (clean-SQL, gegen echtes Postgres) ─────────────────────

    async fn pool_or_skip(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .unwrap();
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
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"CREATE TABLE twitch_live_state (
                   twitch_user_id TEXT NOT NULL, streamer_login TEXT NOT NULL,
                   last_viewer_count INTEGER DEFAULT 0, active_session_id BIGINT
               )"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"CREATE TABLE twitch_chat_messages (
                   id SERIAL PRIMARY KEY, session_id BIGINT NOT NULL DEFAULT 0,
                   streamer_login TEXT NOT NULL, chatter_login TEXT,
                   message_ts TIMESTAMPTZ NOT NULL, content TEXT
               )"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"CREATE TABLE twitch_session_chatters (
                   session_id BIGINT NOT NULL, streamer_login TEXT NOT NULL,
                   chatter_login TEXT NOT NULL, first_message_at TIMESTAMPTZ NOT NULL,
                   messages INTEGER DEFAULT 0
               )"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"CREATE TABLE twitch_stats_category (
                   ts_utc TIMESTAMPTZ, streamer TEXT, viewer_count INTEGER,
                   is_partner BOOLEAN DEFAULT FALSE, tags TEXT, language TEXT
               )"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"CREATE TABLE twitch_partners_all_state (
                   twitch_login TEXT, is_partner_active INTEGER DEFAULT 0
               )"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn market_data_full_payload_shape() {
        let Some(pool) = pool_or_skip("market_data_shape").await else {
            return;
        };
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
            "total_monitored",
            "total_viewers",
            "avg_chat_health",
            "avg_lurker_ratio",
            "total_messages",
            "market_history",
            "questions",
            "channels",
            "meta_snapshot",
            "sentiment",
            "overlap",
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
        let terms: Vec<&str> = payload["meta_snapshot"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|e| e["term"].as_str())
            .collect();
        assert!(terms.contains(&"abrams"));
        assert!(terms.contains(&"haze"));
    }

    #[tokio::test]
    async fn api_market_data_handler_gates_non_admin() {
        let Some(pool) = pool_or_skip("market_data_gate").await else {
            return;
        };
        let resp = api_market_data_handler(
            partner(),
            State(pool),
            HeaderMap::new(),
            None,
            remote_parts(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn api_market_data_handler_admin_200() {
        let Some(pool) = pool_or_skip("market_data_admin").await else {
            return;
        };
        let resp =
            api_market_data_handler(admin(), State(pool), HeaderMap::new(), None, remote_parts())
                .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
