//! Handler für `GET /twitch/api/v2/overview`.
//!
//! Auth: Partner (eigene Daten) + Admin/Localhost (beliebiger Streamer).

use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tb_analytics::overview::{
    overview_category_rank, overview_chat_per_100, overview_chatter_metrics, overview_metrics,
    overview_monetization_counts, overview_network_stats, overview_session_count,
    overview_sessions, OverviewMonetization, OverviewNetworkStats, OverviewSession,
};
use tb_http_core::ApiError;

use crate::auth::level::DashboardAuthLevel;

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

// Untagged-Serde-Enum: die große `Data`-Variante ist gewollt (1:1-JSON-Shape);
// Boxen würde die untagged-Serialisierung verkomplizieren ohne Laufzeitnutzen.
#[allow(clippy::large_enum_variant)]
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
    /// Lesefenster (`"full"`/`"last_stream"`, Python `data["window"]`).
    pub window: &'static str,
    /// `true` bei der kostenlosen Tagesform (Python `data["windowLimited"]`).
    #[serde(rename = "windowLimited")]
    pub window_limited: bool,
    pub summary: OverviewSummary,
    pub scores: HealthScores,
    pub findings: Vec<Finding>,
    pub actions: Vec<ActionItem>,
    pub sessions: Vec<OverviewSession>,
    pub correlations: Correlations,
    pub network: OverviewNetwork,
    #[serde(rename = "dataQuality")]
    pub data_quality: DataQuality,
    #[serde(rename = "categoryRank", skip_serializing_if = "Option::is_none")]
    pub category_rank: Option<i64>,
    #[serde(rename = "categoryTotal", skip_serializing_if = "Option::is_none")]
    pub category_total: Option<i64>,
}

/// Python `{"botFilterApplied": True}`.
#[derive(Serialize)]
pub struct DataQuality {
    #[serde(rename = "botFilterApplied")]
    pub bot_filter_applied: bool,
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

/// Metrik-Korrelationen (Python `_calculate_correlations`).
#[derive(Serialize)]
pub struct Correlations {
    #[serde(rename = "durationVsViewers")]
    pub duration_vs_viewers: f64,
    #[serde(rename = "chatVsRetention")]
    pub chat_vs_retention: f64,
}

/// Pearson-Korrelation, auf 2 Nachkommastellen gerundet (Python `corr`).
/// <2 Werte oder konstante Reihe (Nenner 0) → 0.
fn pearson(a: &[f64], b: &[f64]) -> f64 {
    if a.len() < 2 {
        return 0.0;
    }
    let n = a.len() as f64;
    let mean_a = a.iter().sum::<f64>() / n;
    let mean_b = b.iter().sum::<f64>() / n;
    let num: f64 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| (x - mean_a) * (y - mean_b))
        .sum();
    let den_a = a.iter().map(|x| (x - mean_a).powi(2)).sum::<f64>().sqrt();
    let den_b = b.iter().map(|y| (y - mean_b).powi(2)).sum::<f64>().sqrt();
    if den_a == 0.0 || den_b == 0.0 {
        return 0.0;
    }
    ((num / (den_a * den_b)) * 100.0).round() / 100.0
}

/// Korrelationen über die Sessions-Liste (Python `_calculate_correlations`).
/// <3 Sessions → beide 0.
fn calculate_correlations(sessions: &[OverviewSession]) -> Correlations {
    if sessions.len() < 3 {
        return Correlations {
            duration_vs_viewers: 0.0,
            chat_vs_retention: 0.0,
        };
    }
    let durations: Vec<f64> = sessions.iter().map(|s| s.duration as f64).collect();
    let viewers: Vec<f64> = sessions.iter().map(|s| s.avg_viewers).collect();
    let chatters: Vec<f64> = sessions.iter().map(|s| s.unique_chatters as f64).collect();
    let retention: Vec<f64> = sessions.iter().map(|s| s.retention_10m).collect();
    Correlations {
        duration_vs_viewers: pearson(&durations, &viewers),
        chat_vs_retention: pearson(&chatters, &retention),
    }
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
    // P1.27: Frontend-'Total Streams'-Tile liest `summary.streamCount` (Python-Soll
    // `api_overview.py:1134`). Alias zu `total_sessions`, damit der Tile nicht 0 zeigt.
    #[serde(rename = "streamCount")]
    pub stream_count: i64,
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

/// Lesefenster der Overview-Antwort (B16-FIX-OVERVIEW-WINDOW).
///
/// `Full` = klassisches Rolling-Window (`now-days` .. jetzt) mit Vorperiode für
/// Trends. `LastStream` = kostenlose „Tagesform": nur der letzte beendete Stream,
/// keine Vorperiode (Trends unterdrückt). Spiegelt Pythons String-Werte
/// `"full"`/`"last_stream"` (`api_v2.py:670-733`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowMode {
    Full,
    LastStream,
}

impl WindowMode {
    fn as_str(self) -> &'static str {
        match self {
            WindowMode::Full => "full",
            WindowMode::LastStream => "last_stream",
        }
    }
}

/// Entscheidet das Lesefenster anhand des Plans (Python `_resolve_read_window`,
/// api_v2.py:670-698).
///
/// - Kein Streamer-Kontext → `Full`.
/// - Privilegiert (Localhost/Admin) → `Full` (Bypass).
/// - Streamer mit dem konsolidierten `analytics`-Flag → `Full`.
/// - Sonst (kein Flag) → `LastStream` (kostenlose Tagesform, harte Server-
///   Erzwingung; der Client kann das Fenster NICHT überschreiben — Paywall).
async fn resolve_read_window(
    pool: &PgPool,
    privileged: bool,
    streamer: Option<&str>,
) -> WindowMode {
    let Some(login) = streamer.map(str::trim).filter(|s| !s.is_empty()) else {
        return WindowMode::Full;
    };
    if privileged {
        return WindowMode::Full;
    }
    match tb_analytics::plan::resolve_plan_snapshot(pool, login, "").await {
        Ok(snapshot) => {
            let ents = tb_analytics::plan::plan_entitlements(snapshot.plan_id);
            if ents.contains(&"analytics") {
                WindowMode::Full
            } else {
                WindowMode::LastStream
            }
        }
        // Plan unbekannt/DB-Fehler → fail-closed auf die kostenlose Tagesform.
        Err(_) => WindowMode::LastStream,
    }
}

/// Löst `window` in `(since, prev_since)` (RFC3339) auf (Python
/// `_window_since_dates`, api_v2.py:701-733).
///
/// - `LastStream`: `since = MAX(started_at)` der beendeten Sessions des Streamers
///   (Fallback `now-days` wenn keine), `prev_since = since` → leeres
///   Vorperioden-Fenster → Trends unterdrückt.
/// - `Full`: `since = now-days`, `prev_since = now-2*days`.
async fn window_since_dates(
    pool: &PgPool,
    streamer: Option<&str>,
    days: i64,
    window: WindowMode,
) -> (String, String) {
    let full = || {
        let since = (Utc::now() - Duration::days(days)).to_rfc3339();
        let prev = (Utc::now() - Duration::days(days * 2)).to_rfc3339();
        (since, prev)
    };
    match window {
        WindowMode::Full => full(),
        WindowMode::LastStream => {
            let login = streamer.unwrap_or("");
            // Geteilte „letzte beendete Session"-Definition — identisch zum
            // Paywall-Clamp in session_detail (eine Quelle, kein Drift).
            // `started_at` der letzten Session == `MAX(started_at)`.
            match crate::handlers::last_session::latest_ended_session(pool, login).await {
                Some(s) => {
                    let since = s.started_at.to_rfc3339();
                    (since.clone(), since) // prev == since → keine Trends
                }
                None => {
                    let since = (Utc::now() - Duration::days(days)).to_rfc3339();
                    (since.clone(), since)
                }
            }
        }
    }
}

/// `GET /twitch/api/v2/overview?streamer=<login>[&days=30]`
pub async fn overview_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<OverviewParams>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_authenticated() {
        return Err(ApiError::unauthorized());
    }

    // days: clip to [7, 365]
    let days = params.days.clamp(7, 365);
    // Partner darf nur eigene Daten sehen. Admin/Localhost kann beliebigen
    // Streamer über den Query-Param abfragen.
    let login = match &auth {
        DashboardAuthLevel::Partner { twitch_login, .. } => Some(twitch_login.to_lowercase()),
        _ => params
            .streamer
            .as_deref()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty()),
    };
    let login_ref = login.as_deref();

    // B16-FIX-OVERVIEW-WINDOW: Lesefenster serverseitig auflösen. Streamer ohne
    // das konsolidierte analytics-Flag bekommen die „Tagesform" (last_stream); der
    // Client kann das Fenster nicht überschreiben (Paywall-Durchsetzung).
    let window = resolve_read_window(&pool, auth.is_privileged(), login_ref).await;
    let (since, prev_since) = window_since_dates(&pool, login_ref, days, window).await;

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

    // Die folgenden Kennzahlen haengen nur am aufgeloesten Fenster und nicht
    // voneinander. Frueher lief jede einzeln nacheinander, also gut ein Dutzend
    // Runden zur Datenbank hintereinander; jetzt laufen sie in zwei Gruppen
    // gleichzeitig und die Antwortzeit richtet sich nach der langsamsten
    // Abfrage einer Gruppe statt nach der Summe aller Abfragen.
    //
    // Bewusst hoechstens vier Abfragen auf einmal: der
    // Verbindungs-Pool hat in der Voreinstellung zehn Plaetze
    // (`TWITCH_ANALYTICS_POOL_MAXSIZE`, tb-config). Bei acht gleichzeitigen
    // Abfragen belegt ein einziger Seitenaufruf fast den ganzen Pool, ein
    // zweiter Nutzer liefe in die Wartezeit beim Verbindungholen und bekaeme
    // einen Fehler statt Daten. Mit vier passen zwei Seitenaufrufe zeitgleich
    // in den Pool, und der Gewinn bleibt nahezu gleich.
    //
    // Erste Gruppe: die Abfragen, die intern selbst mehrere Runden brauchen.
    let (metrics_res, prev_res, net_res, mon_res) = tokio::join!(
        // Aktuelles Fenster.
        overview_metrics(&pool, &since, login_ref, None),
        // Vorperiode [prev_since, since) für Trend-Pfeile (Python _get_overview_data_sync).
        // Bei last_stream ist prev_since == since → leeres Fenster → keine Trends.
        overview_metrics(&pool, &prev_since, login_ref, Some(&since)),
        // Raid-Netzwerk-Kachel (zwei Abfragen).
        overview_network_stats(&pool, &since, login_ref),
        // Monetarisierungs-Events, drei Abfragen; fehlende Tabellen → 0.
        overview_monetization_counts(&pool, &since, login_ref),
    );

    // Zweite Gruppe: je eine Abfrage.
    let (chatter_res, category_res, chat_per_100_res, sessions_res) = tokio::join!(
        // Chatter-abgeleitete Metriken (Bot-gefiltert).
        overview_chatter_metrics(&pool, &since, login_ref),
        // Kategorie-Perzentil/Rang (speist Reach-Score + categoryRank/Total).
        overview_category_rank(&pool, &since, login_ref),
        // Chatter pro 100 Peak-Viewer (für Chat-Insights/Actions).
        overview_chat_per_100(&pool, &since, login_ref),
        // Sessions-Liste (jüngste 50).
        overview_sessions(&pool, &since, login_ref, 50),
    );

    let metrics = metrics_res
        .map_err(|_| ApiError::internal())?
        .ok_or_else(ApiError::internal)?;
    let prev = prev_res.map_err(|_| ApiError::internal())?;
    let net = net_res.map_err(|_| ApiError::internal())?;
    let mon = mon_res.map_err(|_| ApiError::internal())?;
    let chatter = chatter_res.map_err(|_| ApiError::internal())?;
    let category = category_res.map_err(|_| ApiError::internal())?;
    let chat_per_100 = chat_per_100_res.map_err(|_| ApiError::internal())?;
    let sessions = sessions_res.map_err(|_| ApiError::internal())?;
    let correlations = calculate_correlations(&sessions);

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
        category.map(|c| c.percentile), // Reach: Perzentil sonst avg_viewers/5-Fallback
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
        streamer: login.clone(),
        days,
        window: window.as_str(),
        window_limited: window == WindowMode::LastStream,
        scores,
        findings,
        actions,
        sessions,
        correlations,
        data_quality: DataQuality {
            bot_filter_applied: true,
        },
        category_rank: category.map(|c| c.rank),
        category_total: category.map(|c| c.total),
        summary: OverviewSummary {
            avg_viewers: metrics.avg_avg_viewers.unwrap_or(0.0),
            peak_viewers: metrics.max_peak_viewers.unwrap_or(0),
            total_hours_watched: metrics.total_hours_watched.unwrap_or(0.0),
            total_airtime: airtime,
            followers_delta: total_followers,
            total_sessions: metrics.session_count.unwrap_or(0),
            stream_count: metrics.session_count.unwrap_or(0),
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
        Router,
    };
    use sqlx::postgres::PgPoolOptions;
    use std::net::SocketAddr;
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
                peak_viewers     INTEGER,
                duration_seconds INTEGER,
                follower_delta   INTEGER,
                followers_start  INTEGER,
                followers_end    INTEGER,
                retention_5m     DOUBLE PRECISION,
                retention_10m    DOUBLE PRECISION,
                retention_20m    DOUBLE PRECISION,
                dropoff_pct      DOUBLE PRECISION,
                start_viewers    INTEGER,
                end_viewers      INTEGER,
                unique_chatters  INTEGER,
                first_time_chatters INTEGER,
                returning_chatters  INTEGER,
                stream_title     TEXT
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
                seen_via_chatters_api BOOLEAN DEFAULT FALSE,
                is_first_time_streamer BOOLEAN DEFAULT FALSE
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
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_stats_category (
                ts_utc       TIMESTAMPTZ,
                streamer     TEXT,
                viewer_count INTEGER
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL stats_category fehlgeschlagen");
        // Tabellen leeren damit Wiederholungsläufe nicht alte Daten sehen
        sqlx::query("TRUNCATE twitch_stream_sessions")
            .execute(&pool)
            .await
            .expect("TRUNCATE fehlgeschlagen");
        pool
    }

    fn make_router(pool: PgPool) -> Router {
        Router::new()
            .route("/twitch/api/v2/overview", get(overview_handler))
            .with_state(pool)
    }

    #[tokio::test]
    async fn returns_401_without_auth() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_overview_unauth").await;
        // Nicht-Loopback-IP + kein Cookie → DashboardAuthLevel::None → 401.
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        let req = Request::builder()
            .uri("/twitch/api/v2/overview?streamer=x")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .body(Body::empty())
            .unwrap();
        let res = make_router(pool).oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn returns_empty_for_unknown_streamer() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_overview_empty").await;
        let res = overview_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(OverviewParams { streamer: Some("nobody".into()), days: 30 }),
        )
            .await
            .unwrap()
            .into_response();
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

        let res = overview_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(OverviewParams { streamer: Some("streamer_x".into()), days: 30 }),
        )
            .await
            .unwrap()
            .into_response();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 16384).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert!((v["summary"]["avgViewers"].as_f64().unwrap() - 100.0).abs() < 0.001);
        assert_eq!(v["summary"]["totalSessions"], 1);
        // P1.27: streamCount-Alias muss identisch zu totalSessions emittiert werden.
        assert_eq!(v["summary"]["streamCount"], 1);
        assert_eq!(v["summary"]["streamCount"], v["summary"]["totalSessions"]);
        // B16-FIX-OVERVIEW-WINDOW: Admin-Token → volles Fenster, nicht limitiert.
        assert_eq!(v["window"], "full");
        assert_eq!(v["windowLimited"], false);
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
        // Sessions-Liste: 1 Session, alice als einziger Nicht-Bot-Chatter (returning).
        assert_eq!(v["sessions"].as_array().unwrap().len(), 1);
        assert_eq!(v["sessions"][0]["id"], 1);
        assert!((v["sessions"][0]["retention10m"].as_f64().unwrap() - 60.0).abs() < 0.01);
        assert_eq!(v["sessions"][0]["uniqueChatters"], 1);
        assert_eq!(v["sessions"][0]["peakViewers"], 200);
        // Correlations: nur 1 Session (<3) → 0. dataQuality-Konstante.
        assert_eq!(v["correlations"]["durationVsViewers"], 0.0);
        assert_eq!(v["correlations"]["chatVsRetention"], 0.0);
        assert_eq!(v["dataQuality"]["botFilterApplied"], true);
    }

    #[tokio::test]
    async fn partner_response_streamer_ist_effektiver_login() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_overview_partner_streamer").await;
        sqlx::query(
            r#"
            INSERT INTO twitch_stream_sessions
                (id, streamer_login, started_at, ended_at, avg_viewers, peak_viewers,
                 duration_seconds, follower_delta, followers_start, followers_end, retention_10m)
            VALUES
                (2, 'owner_login', NOW() - INTERVAL '1 day', NOW() - INTERVAL '23 hours',
                 10.0, 20, 3600, 1, 100, 101, 0.5)
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let res = overview_handler(
            DashboardAuthLevel::Partner {
                twitch_login: "Owner_Login".into(),
                twitch_user_id: "42".into(),
                display_name: "Owner".into(),
            },
            State(pool),
            Query(OverviewParams {
                streamer: Some("other_login".into()),
                days: 30,
            }),
        )
        .await
        .unwrap()
        .into_response();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 16384).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["streamer"], "owner_login");
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

    #[test]
    fn correlations_pearson() {
        // perfekt positiv / negativ / konstant.
        assert!((pearson(&[1.0, 2.0, 3.0], &[10.0, 20.0, 30.0]) - 1.0).abs() < 1e-9);
        assert!((pearson(&[1.0, 2.0, 3.0], &[30.0, 20.0, 10.0]) + 1.0).abs() < 1e-9);
        assert_eq!(pearson(&[1.0, 1.0, 1.0], &[1.0, 2.0, 3.0]), 0.0); // konstante Reihe → Nenner 0

        fn sess(duration: i64, avg: f64, chatters: i64, ret: f64) -> OverviewSession {
            OverviewSession {
                id: 0, date: String::new(), start_time: String::new(),
                duration, start_viewers: 0, peak_viewers: 0, end_viewers: 0,
                avg_viewers: avg, retention_5m: 0.0, retention_10m: ret, retention_20m: 0.0,
                dropoff_pct: 0.0, unique_chatters: chatters, total_chatter_sessions: chatters, first_time_chatters: 0,
                returning_chatters: 0, followers_start: 0, followers_end: 0, title: String::new(),
            }
        }
        // <3 Sessions → beide 0.
        let c0 = calculate_correlations(&[sess(1, 1.0, 1, 1.0), sess(2, 2.0, 2, 2.0)]);
        assert_eq!((c0.duration_vs_viewers, c0.chat_vs_retention), (0.0, 0.0));
        // duration↑viewers↑ = +1, chatters↑retention↓ = -1.
        let c = calculate_correlations(&[
            sess(1, 10.0, 1, 30.0),
            sess(2, 20.0, 2, 20.0),
            sess(3, 30.0, 3, 10.0),
        ]);
        assert!((c.duration_vs_viewers - 1.0).abs() < 1e-9);
        assert!((c.chat_vs_retention + 1.0).abs() < 1e-9);
    }

    // ── B16-FIX-OVERVIEW-WINDOW ───────────────────────────────────────────────

    #[test]
    fn window_mode_string() {
        assert_eq!(WindowMode::Full.as_str(), "full");
        assert_eq!(WindowMode::LastStream.as_str(), "last_stream");
    }

    /// Privilegiert ODER kein Streamer → Full; ohne DB-Plan (fail-closed) →
    /// LastStream. Streamer-Plan-Pfad ist DB-gated und separat abgedeckt.
    #[tokio::test]
    async fn resolve_window_privilegiert_und_kein_streamer() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_overview_window_resolve").await;
        // Localhost/Admin (privilegiert) → Full, egal welcher Streamer.
        assert_eq!(resolve_read_window(&pool, true, Some("nani")).await, WindowMode::Full);
        // Kein Streamer-Kontext → Full.
        assert_eq!(resolve_read_window(&pool, false, None).await, WindowMode::Full);
        // Partner ohne Plan (unbekannter Streamer) → LastStream (Paywall).
        assert_eq!(resolve_read_window(&pool, false, Some("ghost_free")).await, WindowMode::LastStream);
    }

    /// `window_since_dates(LastStream)` → since = MAX(started_at) der beendeten
    /// Session, prev == since (keine Trends). Full → since != prev.
    #[tokio::test]
    async fn window_since_last_stream() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_overview_window_since").await;
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at) \
             VALUES (1, 'nani', '2026-01-01T10:00:00Z', '2026-01-01T12:00:00Z'), \
                    (2, 'nani', '2026-02-01T10:00:00Z', '2026-02-01T12:00:00Z'), \
                    (3, 'nani', '2026-03-01T10:00:00Z', NULL)", // laufend → ignoriert
        )
        .execute(&pool)
        .await
        .unwrap();
        let (since, prev) = window_since_dates(&pool, Some("nani"), 30, WindowMode::LastStream).await;
        assert_eq!(since, prev, "last_stream: prev == since (keine Trends)");
        assert!(since.starts_with("2026-02-01"), "MAX(started_at) der beendeten Sessions, war {since}");
        // Full: prev liegt vor since.
        let (fs, fp) = window_since_dates(&pool, Some("nani"), 30, WindowMode::Full).await;
        assert!(fp < fs, "full: prev_since vor since");
    }
}
