//! Öffentliches Demo-Dashboard (`/twitch/demo`) — nativ + lean.
//!
//! B6-DEMO-LEAN. Marketing-Demo der Analytics-Oberfläche **ohne Login**. Liefert
//! dieselbe React-SPA wie `/analyse` (geteilter Dist), aber mit `demoMode:true`
//! und `apiBase=/twitch/demo/api/v2`, sodass der Client gegen die statischen
//! Demo-JSON-Endpoints dieses Moduls läuft — kein DB-Zugriff, keine echten Daten.
//!
//! **Lean** (Grillme Block 6): NICHT die 3589-LOC-`demo_data.py` verbatim
//! portieren. Wir bedienen das Kern-Endpoint-Set, das die Hauptansicht braucht
//! (`auth-status`, `streamers`, `overview`, `ai/analysis`, `ai/history`), mit
//! kompakten, fest codierten Demo-Payloads. Weitere Kacheln können bei Bedarf
//! additiv ergänzt werden.
//!
//! Sicherheit: komplett auth-frei; CSP `frame-ancestors` erlaubt das Einbetten
//! durch die Community-Site (Env `TWITCH_DEMO_EMBED_ORIGINS`).

use axum::{
    extract::Query,
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::handlers::spa;

/// Default-Origin, die das Demo einbetten darf (Python-Default).
const DEFAULT_DEMO_EMBED_ORIGIN: &str = "https://deutsche-deadlock-community.de";

/// Demo-Streamer-Login, der durchgängig in den Payloads erscheint.
const DEMO_LOGIN: &str = "midcore_live";

/// Runtime-Script: zeigt die SPA auf die Demo-API + aktiviert den Demo-Modus.
const DEMO_RUNTIME_SCRIPT: &str = concat!(
    "<script>window.__TWITCH_DASHBOARD_RUNTIME__=Object.freeze(",
    r#"{"apiBase":"/twitch/demo/api/v2","demoMode":true,"allowedDemoProfiles":["midcore_live"]}"#,
    ");</script>",
);

/// CSP-Header-Wert für die Demo-Seiten (erlaubt Embedding durch die Community).
fn demo_csp_value() -> String {
    let origins = std::env::var("TWITCH_DEMO_EMBED_ORIGINS")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_DEMO_EMBED_ORIGIN.to_string());
    format!("frame-ancestors 'self' {}", origins.trim())
}

/// Fügt den Demo-CSP-Header an eine Response an (Embedding statt X-Frame DENY).
fn with_demo_csp(mut resp: Response) -> Response {
    if let Ok(value) = HeaderValue::from_str(&demo_csp_value()) {
        resp.headers_mut()
            .insert(header::CONTENT_SECURITY_POLICY, value);
    }
    resp
}

// ── HTML-/Asset-Serving (geteilter Dist mit /analyse) ────────────────────────

/// `GET /twitch/demo` (+ `/twitch/demo/`) — SPA-Shell im Demo-Modus.
pub async fn demo_index_handler() -> Response {
    let index = spa::dist_root().join("index.html");
    let html = match tokio::fs::read_to_string(&index).await {
        Ok(s) => s,
        Err(_) => {
            return with_demo_csp(
                (
                    StatusCode::NOT_FOUND,
                    "Demo dashboard not built. Run npm run build in dashboard_v2/",
                )
                    .into_response(),
            )
        }
    };
    // Asset-Prefix auf den Demo-Pfad umschreiben + Demo-Runtime injizieren.
    let html = html
        .replace("/twitch/dashboard-v2/", "/twitch/demo/dashboard-v2/")
        .replacen("</head>", &format!("{DEMO_RUNTIME_SCRIPT}\n  </head>"), 1);
    with_demo_csp(([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response())
}

/// `GET /twitch/demo/dashboard-v2/{path}` — statische Assets (geteilter Dist).
pub async fn demo_assets_handler(
    axum::extract::Path(asset_path): axum::extract::Path<String>,
) -> Response {
    with_demo_csp(spa::serve_asset(asset_path.trim_start_matches('/')).await)
}

// ── JSON-Demo-API ────────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct DemoOverviewQuery {
    #[serde(default)]
    pub days: Option<i64>,
}

#[derive(Deserialize, Default)]
pub struct DemoAnalyticsQuery {
    #[serde(default)]
    pub days: Option<i64>,
    #[serde(default)]
    pub months: Option<i64>,
    #[serde(default)]
    pub page: Option<i64>,
    #[serde(default)]
    pub per_page: Option<i64>,
}

/// `GET /twitch/demo/api/v2/auth-status` — Fake-Auth (Demo-Partner).
pub async fn demo_auth_status() -> Response {
    Json(json!({
        "authenticated": true,
        "level": "partner",
        "authLevel": "partner",
        "demoMode": true,
        "isAdmin": false,
        "adminEligible": false,
        "adminMode": false,
        "isLocalhost": false,
        "canViewAllStreamers": false,
        "twitchLogin": DEMO_LOGIN,
        "displayName": "MidCore Live",
        "plan": null,
        "permissions": {
            "viewAllStreamers": false,
            "viewComparison": true,
            "viewChatAnalytics": true,
            "viewOverlap": true
        }
    }))
    .into_response()
}

/// `GET /twitch/demo/api/v2/streamers` — Demo-Profil-Liste.
///
/// P3.16: konsistent mit `allowedDemoProfiles` (nur `midcore_live`) — kein Leak
/// eines nicht unterstützten Profils (`smallquest_tv`).
pub async fn demo_streamers() -> Response {
    Json(json!([
        { "login": DEMO_LOGIN, "isPartner": true }
    ]))
    .into_response()
}

/// `GET /twitch/demo/api/v2/overview` — kompaktes Demo-Overview.
pub async fn demo_overview(Query(q): Query<DemoOverviewQuery>) -> Response {
    let days = q.days.unwrap_or(30).clamp(7, 365);
    Json(demo_overview_payload(days)).into_response()
}

fn demo_overview_payload(days: i64) -> Value {
    json!({
        "streamer": DEMO_LOGIN,
        "days": days,
        "window": "full",
        "windowLimited": false,
        "scores": {
            "total": 72, "reach": 68, "retention": 78,
            "engagement": 74, "growth": 65, "monetization": 55, "network": 71
        },
        "summary": {
            "avgViewers": 382, "peakViewers": 1087,
            "totalHoursWatched": 5342, "totalAirtime": 14.0,
            "followersDelta": 167, "followersGained": 183,
            "followersPerHour": 11.9, "followersGainedPerHour": 13.1,
            "retention10m": 71.2, "retentionReliable": true,
            "uniqueChatters": 634, "activeChatters": 421, "uniqueViewers": 1820,
            "engagementRate": 23.1, "totalSessions": 14,
            "avgViewersTrend": 6.8, "followersTrend": 12.4, "retentionTrend": 1.9
        },
        "sessions": [
            { "id": 1, "date": "2026-06-12", "startTime": "20:00", "duration": 11400,
              "startViewers": 286, "peakViewers": 1087, "endViewers": 436,
              "avgViewers": 410.0, "retention5m": 79.8, "retention10m": 72.4,
              "retention20m": 64.9, "dropoffPct": 17.6, "totalChatterSessions": 246,
              "uniqueChatters": 240, "firstTimeChatters": 39, "returningChatters": 201,
              "followersStart": 12480, "followersEnd": 12528, "title": "Ranked Grind bis Eternus" },
            { "id": 2, "date": "2026-06-10", "startTime": "19:30", "duration": 9000,
              "startViewers": 241, "peakViewers": 905, "endViewers": 362,
              "avgViewers": 355.0, "retention5m": 76.5, "retention10m": 69.8,
              "retention20m": 61.7, "dropoffPct": 19.1, "totalChatterSessions": 205,
              "uniqueChatters": 198, "firstTimeChatters": 31, "returningChatters": 167,
              "followersStart": 12443, "followersEnd": 12480, "title": "Patch-Day First Impressions" },
            { "id": 3, "date": "2026-06-06", "startTime": "20:15", "duration": 9900,
              "startViewers": 268, "peakViewers": 842, "endViewers": 351,
              "avgViewers": 376.0, "retention5m": 78.1, "retention10m": 71.0,
              "retention20m": 63.4, "dropoffPct": 18.5, "totalChatterSessions": 218,
              "uniqueChatters": 211, "firstTimeChatters": 34, "returningChatters": 177,
              "followersStart": 12412, "followersEnd": 12443, "title": "Hero Pool Review" },
            { "id": 4, "date": "2026-06-03", "startTime": "19:45", "duration": 10800,
              "startViewers": 254, "peakViewers": 795, "endViewers": 337,
              "avgViewers": 348.0, "retention5m": 77.4, "retention10m": 70.5,
              "retention20m": 62.2, "dropoffPct": 18.9, "totalChatterSessions": 196,
              "uniqueChatters": 189, "firstTimeChatters": 27, "returningChatters": 162,
              "followersStart": 12387, "followersEnd": 12412, "title": "Community Scrims am Abend" }
        ],
        "findings": [
            { "type": "pos", "title": "Starke Bindung", "text": "10-Minuten-Retention über dem Kategorie-Schnitt." },
            { "type": "info", "title": "Wachstum stabil", "text": "Follower pro Stunde im oberen Drittel." }
        ],
        "actions": [
            { "tag": "Titel", "text": "Patch-Keywords früh in den Titel ziehen.", "priority": "high" }
        ],
        "correlations": { "durationVsViewers": 0.42, "chatVsRetention": 0.61 },
        "network": { "sent": 8, "received": 5, "sentViewers": 2840 },
        "dataQuality": { "botFilterApplied": true },
        "categoryRank": 12, "categoryTotal": 58
    })
}

#[derive(Deserialize, Default)]
pub struct DemoAiQuery {
    #[serde(default)]
    pub days: Option<i64>,
    #[serde(default)]
    pub game_filter: Option<String>,
}

/// Liefert `"deadlock"` nur für genau diesen Filter, sonst `"all"` (Python-Parität).
fn normalize_game_filter(raw: Option<&str>) -> &'static str {
    match raw.map(str::trim) {
        Some("deadlock") => "deadlock",
        _ => "all",
    }
}

/// Demo-`dataSnapshot` — Kennzahlen-Block, der das Prod-`ai_analysis`-Schema
/// spiegelt. `deadlock`-Filter skaliert einige Werte (Python-Parität).
fn demo_ai_snapshot(game_filter: &str) -> Value {
    let deadlock = game_filter == "deadlock";
    json!({
        "streamCount": if deadlock { 10 } else { 14 },
        "totalHours": if deadlock { 10.1 } else { 14.0 },
        "avgViewers": if deadlock { 405 } else { 382 },
        "peakViewers": if deadlock { 1174 } else { 1087 },
        "followersGained": if deadlock { 139 } else { 183 },
        "avgRetention10m": 71.2,
        "avgDropoffPct": if deadlock { 16.6 } else { 18.0 },
        "avgChatters": 45,
    })
}

/// Baut das 10-Punkte-Demo-Plan-Array im Prod-`points`-Schema
/// (`number/priority/title/analysis/action/expectedImpact`).
fn demo_ai_points() -> Value {
    // 3x kritisch, 4x hoch, 3x mittel — entspricht Pythons Demo-Verteilung.
    let priorities = [
        "kritisch", "kritisch", "kritisch", "hoch", "hoch", "hoch", "hoch", "mittel", "mittel",
        "mittel",
    ];
    const DEMO_POINTS: [(&str, &str, &str, &str); 10] = [
        ("Streamtitel ohne Suchbegriffe", "Deine letzten Titel enthalten weder \"Deadlock\" noch deinen Rang – in der Kategorie-Suche bist du dadurch kaum auffindbar.", "Setze \"Deadlock\" und deinen aktuellen Rang an den Titelanfang.", "Mehr Sichtbarkeit in der Kategorie, geschätzt +10–15 % Erstzuschauer."),
        ("Unregelmäßige Startzeiten", "Deine Startzeiten schwanken um mehrere Stunden – Stammzuschauer können dich schlecht einplanen.", "Lege zwei feste Wochentage mit fixer Uhrzeit fest und kommuniziere sie im Kanal.", "Stabilere Anfangszuschauer und höhere Wiederkehrrate."),
        ("Kein klarer Stream-Fokus", "Du wechselst innerhalb eines Streams häufig die Inhalte – das erschwert neuen Zuschauern das Hängenbleiben.", "Definiere pro Stream ein Leitthema, z. B. Ranked-Grind oder Hero-Guide.", "Längere Verweildauer in den ersten zehn Minuten."),
        ("Wenig Chat-Interaktion in Flauten", "In zuschauerschwachen Phasen sinkt deine Ansprache an den Chat deutlich ab.", "Stelle in ruhigen Momenten gezielt Fragen an den Chat.", "Höhere Zahl an Nachrichten pro Zuschauer."),
        ("Raids werden selten genutzt", "Du beendest Streams oft ohne Raid und verschenkst damit Reichweite im Partnernetz.", "Raide am Ende jedes Streams einen aktiven Partner.", "Mehr gegenseitige Zuschauer-Übergaben."),
        ("Kategorie zu spät gesetzt", "Die Kategorie wird teils erst nach Streambeginn korrekt gesetzt.", "Setze Kategorie und Titel bereits vor dem Live-Gehen.", "Bessere Auffindbarkeit ab der ersten Minute."),
        ("Highlights ungenutzt", "Aus deinen Streams werden kaum Clips oder Highlights weiterverwertet.", "Erstelle pro Stream ein bis zwei Clips für Kurzvideo-Plattformen.", "Zusätzliche Reichweite außerhalb von Twitch."),
        ("Panels unvollständig", "Deine Kanal-Panels enthalten wenig Infos zu Zeitplan und Social Media.", "Ergänze Panels für Zeitplan, Regeln und Social Media.", "Mehr Follows durch klare Kanal-Infos."),
        ("Selten Umfragen oder Votings", "Interaktive Elemente wie Umfragen kommen kaum vor.", "Starte pro Stream eine kurze Chat-Umfrage.", "Stärkere Bindung der Stammzuschauer."),
        ("Wenig Off-Stream-Kommunikation", "Zwischen den Streams gibt es kaum Ankündigungen.", "Kündige den nächsten Stream über Discord und Social Media an.", "Höhere Anfangszuschauerzahl."),
    ];
    let points: Vec<Value> = priorities
        .iter()
        .zip(DEMO_POINTS.iter())
        .enumerate()
        .map(|(i, (prio, (title, analysis, action, expected)))| {
            json!({
                "number": i + 1,
                "priority": prio,
                "title": title,
                "analysis": analysis,
                "action": action,
                "expectedImpact": expected,
            })
        })
        .collect();
    Value::Array(points)
}

/// Baut eine vollständige Demo-AI-Analyse im Prod-Schema (P1.28).
fn demo_ai_analysis_payload(days: i64, game_filter: &str, generated_at: &str, id: i64) -> Value {
    json!({
        "id": id,
        "streamer": DEMO_LOGIN,
        "days": days,
        "gameFilter": game_filter,
        "model": "opus",
        "generatedAt": generated_at,
        "points": demo_ai_points(),
        "dataSnapshot": demo_ai_snapshot(game_filter),
    })
}

/// Zählt Punkte mit der gegebenen Priorität.
fn count_priority(points: &Value, prio: &str) -> usize {
    points
        .as_array()
        .map(|arr| arr.iter().filter(|p| p["priority"] == prio).count())
        .unwrap_or(0)
}

/// `GET|POST /twitch/demo/api/v2/ai/analysis` — Demo-KI-Coach im Prod-Schema.
///
/// P1.28: liefert `id/streamer/days/gameFilter/model/generatedAt/points[]/dataSnapshot`
/// statt der alten `summary/sections`-Form, damit die AI-Coach-Komponente die
/// gleichen Felder wie unter `/twitch/api/v2/ai/analysis` rendert.
pub async fn demo_ai_analysis(Query(q): Query<DemoAiQuery>) -> Response {
    let days = q.days.unwrap_or(30).clamp(7, 365);
    let game_filter = normalize_game_filter(q.game_filter.as_deref());
    Json(demo_ai_analysis_payload(
        days,
        game_filter,
        "2026-06-13T11:00:00Z",
        9030,
    ))
    .into_response()
}

#[derive(Deserialize, Default)]
pub struct DemoAiHistoryQuery {
    #[serde(default)]
    pub limit: Option<i64>,
}

/// `GET /twitch/demo/api/v2/ai/history` — Demo-Verlauf im Prod-Schema (P1.28).
///
/// Jeder Eintrag trägt `generatedAt/points/dataSnapshot` plus die drei
/// Prioritäts-Zähler (`kritischCount/hochCount/mittelCount`).
pub async fn demo_ai_history(Query(q): Query<DemoAiHistoryQuery>) -> Response {
    let limit = q.limit.unwrap_or(20).clamp(1, 50) as usize;
    // (days, game_filter, generatedAt, id) — neueste zuerst.
    let presets = [
        (90i64, "all", "2026-06-02T14:00:00Z", 7003i64),
        (30, "all", "2026-06-10T11:00:00Z", 7002),
        (14, "deadlock", "2026-06-12T09:00:00Z", 7001),
    ];
    let entries: Vec<Value> = presets
        .iter()
        .map(|(days, gf, gen_at, id)| {
            let mut entry = demo_ai_analysis_payload(*days, gf, gen_at, *id);
            let points = entry["points"].clone();
            entry["kritischCount"] = json!(count_priority(&points, "kritisch"));
            entry["hochCount"] = json!(count_priority(&points, "hoch"));
            entry["mittelCount"] = json!(count_priority(&points, "mittel"));
            entry
        })
        .take(limit)
        .collect();
    Json(Value::Array(entries)).into_response()
}

// ── Lean-Fixture-Kacheln (P2.72 / P2.73) ─────────────────────────────────────
//
// Die öffentliche Marketing-Demo bewirbt Kacheln, die im Rust-Cutover sonst 404
// liefern. Wir bedienen sie additiv mit kompakten, fest codierten Fixtures
// (keine echten Daten, kein DB-Zugriff) — strukturell plausibel, aber bewusst
// schlank ("lean", Grillme Block 6).

/// `GET /twitch/demo/api/v2/monthly-stats` — Monatsaggregat im Frontend-Vertrag.
pub async fn demo_monthly_stats(Query(q): Query<DemoAnalyticsQuery>) -> Response {
    let months = q.months.unwrap_or(12).clamp(1, 24) as usize;
    let items = vec![
        json!({"year": 2026, "month": 6, "monthLabel": "Jun", "totalHoursWatched": 5342.0, "totalAirtime": 14.0, "avgViewers": 382.0, "peakViewers": 1087, "followerDelta": 167, "totalChatterSessions": 634, "streamCount": 14}),
        json!({"year": 2026, "month": 5, "monthLabel": "Mai", "totalHoursWatched": 16892.0, "totalAirtime": 45.4, "avgViewers": 372.0, "peakViewers": 998, "followerDelta": 168, "totalChatterSessions": 780, "streamCount": 14}),
        json!({"year": 2026, "month": 4, "monthLabel": "Apr", "totalHoursWatched": 14742.0, "totalAirtime": 42.0, "avgViewers": 351.0, "peakViewers": 902, "followerDelta": 142, "totalChatterSessions": 712, "streamCount": 12}),
        json!({"year": 2026, "month": 3, "monthLabel": "Mar", "totalHoursWatched": 13688.0, "totalAirtime": 41.6, "avgViewers": 329.0, "peakViewers": 840, "followerDelta": 121, "totalChatterSessions": 690, "streamCount": 12}),
        json!({"year": 2026, "month": 2, "monthLabel": "Feb", "totalHoursWatched": 11840.0, "totalAirtime": 39.6, "avgViewers": 299.0, "peakViewers": 760, "followerDelta": 104, "totalChatterSessions": 621, "streamCount": 11}),
        json!({"year": 2026, "month": 1, "monthLabel": "Jan", "totalHoursWatched": 10592.0, "totalAirtime": 38.1, "avgViewers": 278.0, "peakViewers": 705, "followerDelta": 92, "totalChatterSessions": 580, "streamCount": 11}),
        json!({"year": 2025, "month": 12, "monthLabel": "Dez", "totalHoursWatched": 9360.0, "totalAirtime": 36.0, "avgViewers": 260.0, "peakViewers": 665, "followerDelta": 83, "totalChatterSessions": 542, "streamCount": 10}),
        json!({"year": 2025, "month": 11, "monthLabel": "Nov", "totalHoursWatched": 8460.0, "totalAirtime": 34.0, "avgViewers": 249.0, "peakViewers": 620, "followerDelta": 74, "totalChatterSessions": 505, "streamCount": 10}),
        json!({"year": 2025, "month": 10, "monthLabel": "Okt", "totalHoursWatched": 7462.0, "totalAirtime": 32.3, "avgViewers": 231.0, "peakViewers": 590, "followerDelta": 68, "totalChatterSessions": 470, "streamCount": 9}),
        json!({"year": 2025, "month": 9, "monthLabel": "Sep", "totalHoursWatched": 6534.0, "totalAirtime": 30.4, "avgViewers": 215.0, "peakViewers": 540, "followerDelta": 61, "totalChatterSessions": 441, "streamCount": 9}),
        json!({"year": 2025, "month": 8, "monthLabel": "Aug", "totalHoursWatched": 5720.0, "totalAirtime": 28.6, "avgViewers": 200.0, "peakViewers": 510, "followerDelta": 52, "totalChatterSessions": 390, "streamCount": 8}),
        json!({"year": 2025, "month": 7, "monthLabel": "Jul", "totalHoursWatched": 4947.0, "totalAirtime": 26.6, "avgViewers": 186.0, "peakViewers": 470, "followerDelta": 44, "totalChatterSessions": 351, "streamCount": 8}),
    ];
    Json(Value::Array(items.into_iter().take(months).collect())).into_response()
}

/// `GET /twitch/demo/api/v2/weekly-stats` — Wochentag-Verteilung im Frontend-Vertrag.
pub async fn demo_weekly_stats() -> Response {
    Json(json!([
        { "weekday": 0, "weekdayLabel": "So", "streamCount": 1, "avgHours": 2.8, "avgViewers": 214.0, "avgPeak": 520.0, "totalFollowers": 12 },
        { "weekday": 1, "weekdayLabel": "Mo", "streamCount": 1, "avgHours": 2.9, "avgViewers": 286.0, "avgPeak": 690.0, "totalFollowers": 24 },
        { "weekday": 2, "weekdayLabel": "Di", "streamCount": 2, "avgHours": 3.1, "avgViewers": 338.0, "avgPeak": 780.0, "totalFollowers": 38 },
        { "weekday": 3, "weekdayLabel": "Mi", "streamCount": 3, "avgHours": 3.2, "avgViewers": 410.0, "avgPeak": 930.0, "totalFollowers": 46 },
        { "weekday": 4, "weekdayLabel": "Do", "streamCount": 2, "avgHours": 3.0, "avgViewers": 395.0, "avgPeak": 890.0, "totalFollowers": 35 },
        { "weekday": 5, "weekdayLabel": "Fr", "streamCount": 3, "avgHours": 3.4, "avgViewers": 462.0, "avgPeak": 1087.0, "totalFollowers": 58 },
        { "weekday": 6, "weekdayLabel": "Sa", "streamCount": 2, "avgHours": 3.0, "avgViewers": 304.0, "avgPeak": 720.0, "totalFollowers": 20 }
    ]))
    .into_response()
}

/// `GET /twitch/demo/api/v2/hourly-heatmap` — Wochentag x Stunde im Frontend-Vertrag.
pub async fn demo_hourly_heatmap() -> Response {
    let items: Vec<Value> = (0..7)
        .flat_map(|weekday| {
            (0..24).map(move |hour| {
                let strong = matches!(
                    (weekday, hour),
                    (3, 19) | (3, 20) | (4, 20) | (5, 20) | (5, 21)
                );
                let warm = matches!(
                    (weekday, hour),
                    (1, 19) | (2, 19) | (2, 20) | (3, 18) | (4, 19) | (5, 19) | (6, 16)
                );
                let stream_count = if strong {
                    3
                } else if warm {
                    2
                } else {
                    0
                };
                let hour_base = match hour {
                    19..=21 => 285,
                    17..=18 => 225,
                    15..=16 => 180,
                    _ => 95,
                };
                let weekday_boost = match weekday {
                    3 => 80,
                    4 => 60,
                    5 => 115,
                    6 => 45,
                    2 => 35,
                    _ => 0,
                };
                let avg_viewers = if stream_count > 0 {
                    (hour_base + weekday_boost) as f64
                } else {
                    0.0
                };
                json!({
                    "weekday": weekday,
                    "hour": hour,
                    "streamCount": stream_count,
                    "avgViewers": avg_viewers,
                    "avgPeak": if stream_count > 0 { (avg_viewers * 2.15).round() } else { 0.0 }
                })
            })
        })
        .collect();
    Json(Value::Array(items)).into_response()
}

/// `GET /twitch/demo/api/v2/calendar-heatmap` — Tages-Kalender-Heatmap im Frontend-Vertrag.
pub async fn demo_calendar_heatmap() -> Response {
    Json(json!([
        { "date": "2026-05-07", "value": 1224.0, "streamCount": 1, "hoursWatched": 1224.0 },
        { "date": "2026-05-10", "value": 980.0, "streamCount": 1, "hoursWatched": 980.0 },
        { "date": "2026-05-14", "value": 1408.0, "streamCount": 1, "hoursWatched": 1408.0 },
        { "date": "2026-05-17", "value": 1160.0, "streamCount": 1, "hoursWatched": 1160.0 },
        { "date": "2026-05-21", "value": 1512.0, "streamCount": 1, "hoursWatched": 1512.0 },
        { "date": "2026-05-24", "value": 1295.0, "streamCount": 1, "hoursWatched": 1295.0 },
        { "date": "2026-05-28", "value": 1720.0, "streamCount": 1, "hoursWatched": 1720.0 },
        { "date": "2026-06-03", "value": 1312.0, "streamCount": 1, "hoursWatched": 1312.0 },
        { "date": "2026-06-06", "value": 1242.5, "streamCount": 1, "hoursWatched": 1242.5 },
        { "date": "2026-06-10", "value": 930.0, "streamCount": 1, "hoursWatched": 930.0 },
        { "date": "2026-06-12", "value": 1312.0, "streamCount": 1, "hoursWatched": 1312.0 },
        { "date": "2026-06-17", "value": 1376.0, "streamCount": 1, "hoursWatched": 1376.0 },
        { "date": "2026-06-21", "value": 1098.0, "streamCount": 1, "hoursWatched": 1098.0 },
        { "date": "2026-06-26", "value": 1560.0, "streamCount": 1, "hoursWatched": 1560.0 }
    ]))
    .into_response()
}

/// `GET /twitch/demo/api/v2/chat-analytics` — Chat-Kennzahlen (Fixture).
pub async fn demo_chat_analytics() -> Response {
    Json(json!({
        "streamer": DEMO_LOGIN,
        "totals": {
            "messages": 18420, "uniqueChatters": 634, "activeChatters": 421,
            "messagesPerMinute": 7.3, "emoteShare": 0.31
        },
        "topChatters": [
            { "login": "viewer_alpha", "messages": 412 },
            { "login": "viewer_bravo", "messages": 388 },
            { "login": "viewer_charlie", "messages": 301 }
        ],
        "topEmotes": [
            { "emote": "deadlockHype", "count": 1240 },
            { "emote": "PogChamp", "count": 980 }
        ]
    }))
    .into_response()
}

/// `GET /twitch/demo/api/v2/monetization` — Monetization-Stats im Frontend-Vertrag.
pub async fn demo_monetization(Query(q): Query<DemoAnalyticsQuery>) -> Response {
    let days = q.days.unwrap_or(30).clamp(7, 365);
    Json(json!({
        "ads": {
            "total": 18,
            "auto": 12,
            "manual": 6,
            "sessions_with_ads": 7,
            "avg_duration_s": 63.3,
            "avg_viewer_drop_pct": 4.8,
            "worst_ads": [
                { "started_at": "2026-06-12T20:48:00Z", "duration_s": 90, "drop_pct": 11.4, "pre_avg_viewers": 114.0, "post_avg_viewers": 101.0, "low_sample": false, "is_automatic": true, "min_into_stream": 48, "recovery_min": 8 },
                { "started_at": "2026-06-10T21:05:00Z", "duration_s": 60, "drop_pct": 8.2, "pre_avg_viewers": 98.0, "post_avg_viewers": 90.0, "low_sample": false, "is_automatic": false, "min_into_stream": 95, "recovery_min": 6 }
            ],
            "duration_impact": {
                "30s": { "avg_drop": 2.4, "count": 5 },
                "60s": { "avg_drop": 4.9, "count": 9 },
                "90s": { "avg_drop": 8.1, "count": 4 },
                "120s_plus": { "avg_drop": null, "count": 0 }
            },
            "position_impact": {
                "early_0_30m": { "avg_drop": 3.1, "count": 3 },
                "mid_30_60m": { "avg_drop": 5.7, "count": 6 },
                "late_60_90m": { "avg_drop": 4.2, "count": 5 },
                "endgame_90m": { "avg_drop": 6.8, "count": 4 }
            },
            "auto_vs_manual": {
                "auto_avg_drop": 5.2,
                "manual_avg_drop": 3.9,
                "auto_count": 12,
                "manual_count": 6
            },
            "best_ad_time": "20:45",
            "avg_recovery_min": 6.4,
            "recovery_by_duration": {
                "30s": { "avg_recovery_min": 3.8, "count": 5 },
                "60s": { "avg_recovery_min": 6.1, "count": 9 },
                "90s": { "avg_recovery_min": 9.5, "count": 4 },
                "120s_plus": { "avg_recovery_min": null, "count": 0 }
            }
        },
        "hype_train": { "total": 4, "avg_level": 3.2, "max_level": 5, "avg_duration_s": 312.0 },
        "bits": { "total": 124000, "cheer_events": 86 },
        "subs": { "total_events": 84, "gifted": 27 },
        "window_days": days
    }))
    .into_response()
}

/// `GET /twitch/demo/api/v2/category-leaderboard` — Kategorie-Ranking (Fixture).
pub async fn demo_category_leaderboard() -> Response {
    Json(json!({
        "leaderboard": [
            { "rank": 1, "streamer": "laneprime_tv", "avgViewers": 1260, "peakViewers": 3410, "isPartner": true, "isYou": false },
            { "rank": 2, "streamer": "deny_dynamo", "avgViewers": 1018, "peakViewers": 2860, "isPartner": true, "isYou": false },
            { "rank": 3, "streamer": "geistlane", "avgViewers": 894, "peakViewers": 2425, "isPartner": true, "isYou": false },
            { "rank": 4, "streamer": "orbital_vault", "avgViewers": 748, "peakViewers": 1904, "isPartner": true, "isYou": false },
            { "rank": 5, "streamer": "patronpush", "avgViewers": 682, "peakViewers": 1765, "isPartner": false, "isYou": false },
            { "rank": 6, "streamer": "zipline_zero", "avgViewers": 611, "peakViewers": 1608, "isPartner": true, "isYou": false },
            { "rank": 7, "streamer": "soulshop_live", "avgViewers": 566, "peakViewers": 1490, "isPartner": false, "isYou": false },
            { "rank": 8, "streamer": "metro_guardian", "avgViewers": 512, "peakViewers": 1324, "isPartner": true, "isYou": false },
            { "rank": 9, "streamer": "lantern_macro", "avgViewers": 474, "peakViewers": 1242, "isPartner": false, "isYou": false },
            { "rank": 10, "streamer": "bridgeboss", "avgViewers": 445, "peakViewers": 1198, "isPartner": true, "isYou": false },
            { "rank": 11, "streamer": "rookroute", "avgViewers": 414, "peakViewers": 1126, "isPartner": false, "isYou": false },
            { "rank": 12, "streamer": DEMO_LOGIN, "avgViewers": 382, "peakViewers": 1087, "isPartner": true, "isYou": true },
            { "rank": 13, "streamer": "aim_dojo_live", "avgViewers": 361, "peakViewers": 980, "isPartner": false, "isYou": false },
            { "rank": 14, "streamer": "urnwalker", "avgViewers": 338, "peakViewers": 914, "isPartner": false, "isYou": false },
            { "rank": 15, "streamer": "railgun_room", "avgViewers": 319, "peakViewers": 875, "isPartner": true, "isYou": false }
        ],
        "totalStreamers": 58,
        "yourRank": 12,
        "yourTier": "Oberes Mittelfeld"
    }))
    .into_response()
}

/// `GET /twitch/demo/api/v2/coaching` — Coaching-Hinweise (Fixture, Texte später).
pub async fn demo_coaching() -> Response {
    Json(json!({
        "streamer": DEMO_LOGIN,
        "days": 30,
        "empty": false,
        "efficiency": {
            "viewerHoursPerStreamHour": 382.0,
            "categoryAvg": 318.0,
            "topPerformers": [
                { "streamer": "laneprime_tv", "ratio": 1260.0 },
                { "streamer": "deny_dynamo", "ratio": 1018.0 },
                { "streamer": "geistlane", "ratio": 894.0 },
                { "streamer": DEMO_LOGIN, "ratio": 382.0 }
            ],
            "percentile": 79,
            "totalStreamHours": 44.0,
            "totalViewerHours": 16808.0,
            "growthPer10Hours": 41.6,
            "growthCategoryAvg": 24.2,
            "growthTopPerformers": [
                { "streamer": "laneprime_tv", "value": 58.1 },
                { "streamer": "deny_dynamo", "value": 51.4 },
                { "streamer": DEMO_LOGIN, "value": 41.6 },
                { "streamer": "aim_dojo_live", "value": 29.8 }
            ],
            "growthPercentile": 82
        },
        "titleAnalysis": {
            "yourTitles": [
                { "title": "Ranked Grind bis Eternus", "avgViewers": 438, "peakViewers": 1087, "chatters": 246, "usageCount": 5 },
                { "title": "Patch-Day First Impressions", "avgViewers": 386, "peakViewers": 905, "chatters": 205, "usageCount": 3 },
                { "title": "Hero Pool Review", "avgViewers": 344, "peakViewers": 760, "chatters": 174, "usageCount": 2 },
                { "title": "Community Scrims am Abend", "avgViewers": 332, "peakViewers": 795, "chatters": 168, "usageCount": 2 },
                { "title": "Duo Queue und VOD Review", "avgViewers": 309, "peakViewers": 701, "chatters": 143, "usageCount": 2 }
            ],
            "categoryTopTitles": [
                { "title": "Deadlock Ranked Race", "streamer": "laneprime_tv", "avgViewers": 1260 },
                { "title": "Patch Meta Breakdown", "streamer": "deny_dynamo", "avgViewers": 1018 },
                { "title": "High MMR Hero Lab", "streamer": "geistlane", "avgViewers": 894 },
                { "title": "Viewer Games Queue", "streamer": "orbital_vault", "avgViewers": 748 },
                { "title": "Road to Ascendant", "streamer": "patronpush", "avgViewers": 682 }
            ],
            "yourMissingPatterns": ["Rang oder Ziel im Titel (z. B. \"Push auf Eternus\")", "Patch- oder Hero-Bezug als Aufhänger", "Konkreter Hook statt nur \"Ranked\""],
            "topPerformerPatterns": ["\"Deadlock\" in den ersten beiden Wörtern", "Klares Versprechen wie \"High MMR Hero Lab\"", "Hero- oder Meta-Name direkt im Titel"],
            "varietyPct": 35.7,
            "uniqueTitleCount": 5,
            "totalSessionCount": 14,
            "avgPeerVarietyPct": 54.2,
            "peerVariety": [
                { "streamer": "rookroute", "uniqueTitles": 7, "totalSessions": 14, "varietyPct": 50.0 },
                { "streamer": "aim_dojo_live", "uniqueTitles": 8, "totalSessions": 13, "varietyPct": 61.5 },
                { "streamer": "urnwalker", "uniqueTitles": 6, "totalSessions": 12, "varietyPct": 50.0 }
            ]
        },
        "scheduleOptimizer": {
            "sweetSpots": [
                { "weekday": 2, "hour": 18, "categoryViewers": 2140, "competitors": 7, "opportunityScore": 82.4 },
                { "weekday": 3, "hour": 19, "categoryViewers": 2860, "competitors": 9, "opportunityScore": 79.8 },
                { "weekday": 4, "hour": 20, "categoryViewers": 3120, "competitors": 11, "opportunityScore": 76.5 },
                { "weekday": 0, "hour": 17, "categoryViewers": 1760, "competitors": 5, "opportunityScore": 74.0 },
                { "weekday": 5, "hour": 21, "categoryViewers": 3580, "competitors": 15, "opportunityScore": 70.6 }
            ],
            "yourCurrentSlots": [
                { "weekday": 3, "hour": 20, "count": 4 },
                { "weekday": 5, "hour": 20, "count": 3 },
                { "weekday": 2, "hour": 19, "count": 3 },
                { "weekday": 4, "hour": 20, "count": 2 }
            ],
            "competitionHeatmap": [
                { "weekday": 2, "hour": 18, "competitors": 7, "categoryViewers": 2140 },
                { "weekday": 2, "hour": 19, "competitors": 10, "categoryViewers": 2620 },
                { "weekday": 3, "hour": 19, "competitors": 9, "categoryViewers": 2860 },
                { "weekday": 3, "hour": 20, "competitors": 13, "categoryViewers": 3240 },
                { "weekday": 4, "hour": 20, "competitors": 11, "categoryViewers": 3120 },
                { "weekday": 5, "hour": 21, "competitors": 15, "categoryViewers": 3580 }
            ]
        },
        "durationAnalysis": {
            "buckets": [
                { "label": "1-2 h", "streamCount": 2, "avgViewers": 304.0, "avgChatters": 132.0, "avgRetention5m": 73.8, "efficiencyRatio": 304.0 },
                { "label": "2-3 h", "streamCount": 5, "avgViewers": 361.0, "avgChatters": 187.0, "avgRetention5m": 77.1, "efficiencyRatio": 361.0 },
                { "label": "3-4 h", "streamCount": 6, "avgViewers": 412.0, "avgChatters": 224.0, "avgRetention5m": 79.4, "efficiencyRatio": 412.0 },
                { "label": "4 h+", "streamCount": 1, "avgViewers": 344.0, "avgChatters": 169.0, "avgRetention5m": 75.2, "efficiencyRatio": 344.0 }
            ],
            "optimalLabel": "3-4 h",
            "currentAvgHours": 3.1,
            "correlation": 0.18
        },
        "crossCommunity": {
            "totalUniqueChatters": 634,
            "chatterSources": [
                { "sourceStreamer": "partner_one", "sharedChatters": 214, "percentage": 33.8 },
                { "sourceStreamer": "partner_two", "sharedChatters": 168, "percentage": 26.5 },
                { "sourceStreamer": "aim_dojo_live", "sharedChatters": 91, "percentage": 14.4 },
                { "sourceStreamer": "rookroute", "sharedChatters": 76, "percentage": 12.0 }
            ],
            "isolatedChatters": 286,
            "isolatedPercentage": 45.1,
            "ecosystemSummary": "Rund 55 % deiner Chatter teilst du mit anderen Deadlock-Kanälen — vor allem mit zwei Partnern, die zusammen über die Hälfte eurer Überschneidung ausmachen. Die übrigen 45 % schauen ausschließlich bei dir: ein loyaler Kern, auf dem sich aufbauen lässt."
        },
        "tagOptimization": {
            "yourTags": [
                { "tags": "Deadlock, Ranked, Deutsch", "avgViewers": 438, "usageCount": 6 },
                { "tags": "Deadlock, Patch, Meta", "avgViewers": 386, "usageCount": 3 },
                { "tags": "Deadlock, Coaching", "avgViewers": 344, "usageCount": 3 },
                { "tags": "Deadlock, Community", "avgViewers": 332, "usageCount": 2 }
            ],
            "categoryBestTags": [
                { "tags": "Deadlock, Ranked, High MMR", "avgViewers": 812, "streamerCount": 9 },
                { "tags": "Deadlock, Patch, Analysis", "avgViewers": 744, "streamerCount": 7 },
                { "tags": "Deadlock, Viewer Games", "avgViewers": 621, "streamerCount": 6 },
                { "tags": "Deadlock, Hero Guide", "avgViewers": 588, "streamerCount": 5 }
            ],
            "missingHighPerformers": ["High MMR", "Analysis", "Viewer Games"],
            "underperformingTags": ["Coaching"]
        },
        "retentionCoaching": {
            "your5mRetention": 78.0,
            "category5mRetention": 68.4,
            "yourViewerCurve": [
                { "minute": 0, "avgViewerPct": 100.0 },
                { "minute": 5, "avgViewerPct": 78.0 },
                { "minute": 10, "avgViewerPct": 71.2 },
                { "minute": 15, "avgViewerPct": 66.8 },
                { "minute": 20, "avgViewerPct": 62.9 },
                { "minute": 25, "avgViewerPct": 59.7 },
                { "minute": 30, "avgViewerPct": 56.4 }
            ],
            "topPerformerCurve": [
                { "minute": 0, "avgViewerPct": 100.0 },
                { "minute": 5, "avgViewerPct": 84.5 },
                { "minute": 10, "avgViewerPct": 79.8 },
                { "minute": 15, "avgViewerPct": 75.1 },
                { "minute": 20, "avgViewerPct": 71.6 },
                { "minute": 25, "avgViewerPct": 68.8 },
                { "minute": 30, "avgViewerPct": 65.0 }
            ],
            "criticalDropoffMinute": 5
        },
        "doubleStreamDetection": {
            "detected": true,
            "count": 2,
            "occurrences": [
                { "date": "2026-06-01", "sessionCount": 2, "avgViewers": 286.0 },
                { "date": "2026-06-08", "sessionCount": 2, "avgViewers": 301.0 }
            ],
            "singleDayAvg": 392.0,
            "doubleDayAvg": 294.0
        },
        "chatConcentration": {
            "totalChatters": 634,
            "totalMessages": 18420,
            "msgsPerChatter": 29.1,
            "loyaltyBuckets": {
                "1x": { "count": 248, "pct": 39.1, "messages": 2104 },
                "2-5x": { "count": 259, "pct": 40.9, "messages": 6720 },
                "6+": { "count": 127, "pct": 20.0, "messages": 9596 }
            },
            "topChatters": [
                { "login": "viewer_alpha", "messages": 412, "sessions": 11, "sharePct": 2.2, "cumulativePct": 2.2 },
                { "login": "viewer_bravo", "messages": 388, "sessions": 9, "sharePct": 2.1, "cumulativePct": 4.3 },
                { "login": "viewer_charlie", "messages": 301, "sessions": 8, "sharePct": 1.6, "cumulativePct": 5.9 },
                { "login": "patch_note_reader", "messages": 244, "sessions": 7, "sharePct": 1.3, "cumulativePct": 7.2 },
                { "login": "silent_anchor", "messages": 220, "sessions": 9, "sharePct": 1.2, "cumulativePct": 8.4 }
            ],
            "concentrationIndex": 0.24,
            "top1Pct": 2.2,
            "top3Pct": 5.9,
            "ownOneTimerPct": 39.1,
            "avgPeerOneTimerPct": 46.8
        },
        "raidNetwork": {
            "totalSent": 8,
            "totalReceived": 6,
            "totalSentViewers": 2840,
            "totalReceivedViewers": 674,
            "avgSentViewers": 355.0,
            "avgReceivedViewers": 112.3,
            "reciprocityRatio": 0.75,
            "mutualPartners": 3,
            "totalPartners": 5,
            "partners": [
                { "login": "partner_one", "sentCount": 2, "sentAvgViewers": 420.0, "receivedCount": 3, "receivedAvgViewers": 148.0, "reciprocity": "mutual", "balance": 1 },
                { "login": "partner_two", "sentCount": 2, "sentAvgViewers": 365.0, "receivedCount": 2, "receivedAvgViewers": 96.0, "reciprocity": "mutual", "balance": 0 },
                { "login": "aim_dojo_live", "sentCount": 1, "sentAvgViewers": 298.0, "receivedCount": 1, "receivedAvgViewers": 78.0, "reciprocity": "mutual", "balance": 0 },
                { "login": "rookroute", "sentCount": 2, "sentAvgViewers": 330.0, "receivedCount": 0, "receivedAvgViewers": 0.0, "reciprocity": "sentOnly", "balance": 2 },
                { "login": "bridgeboss", "sentCount": 1, "sentAvgViewers": 312.0, "receivedCount": 0, "receivedAvgViewers": 0.0, "reciprocity": "sentOnly", "balance": 1 }
            ]
        },
        "peerComparison": {
            "ownData": {
                "login": DEMO_LOGIN,
                "sessions": 14,
                "avgViewers": 382,
                "maxPeak": 1087,
                "avgHours": 3.1,
                "avgChatters": 421,
                "retention5m": 78.0,
                "totalHours": 44.0,
                "followsGained": 183,
                "uniqueTitles": 5,
                "titleVariety": 35.7
            },
            "ownRank": 12,
            "totalStreamers": 58,
            "similarPeers": [
                { "login": "rookroute", "sessions": 14, "avgViewers": 414, "maxPeak": 1126, "avgHours": 3.0, "avgChatters": 438, "retention5m": 77.4, "totalHours": 42.0, "followsGained": 176, "uniqueTitles": 7, "titleVariety": 50.0 },
                { "login": "aim_dojo_live", "sessions": 13, "avgViewers": 361, "maxPeak": 980, "avgHours": 2.9, "avgChatters": 386, "retention5m": 75.9, "totalHours": 37.7, "followsGained": 142, "uniqueTitles": 8, "titleVariety": 61.5 },
                { "login": "urnwalker", "sessions": 12, "avgViewers": 338, "maxPeak": 914, "avgHours": 3.2, "avgChatters": 352, "retention5m": 73.2, "totalHours": 38.4, "followsGained": 119, "uniqueTitles": 6, "titleVariety": 50.0 }
            ],
            "aspirationalPeers": [
                { "login": "bridgeboss", "sessions": 16, "avgViewers": 445, "maxPeak": 1198, "avgHours": 3.1, "avgChatters": 486, "retention5m": 80.4, "totalHours": 49.6, "followsGained": 226, "uniqueTitles": 9, "titleVariety": 56.3 },
                { "login": "lantern_macro", "sessions": 15, "avgViewers": 474, "maxPeak": 1242, "avgHours": 3.0, "avgChatters": 502, "retention5m": 81.1, "totalHours": 45.0, "followsGained": 244, "uniqueTitles": 10, "titleVariety": 66.7 },
                { "login": "metro_guardian", "sessions": 15, "avgViewers": 512, "maxPeak": 1324, "avgHours": 3.3, "avgChatters": 548, "retention5m": 82.0, "totalHours": 49.5, "followsGained": 271, "uniqueTitles": 9, "titleVariety": 60.0 }
            ],
            "metricsRanked": {
                "avgViewers": { "rank": 12, "total": 58, "value": 382 },
                "maxPeak": { "rank": 12, "total": 58, "value": 1087 },
                "avgChatters": { "rank": 10, "total": 58, "value": 421 },
                "retention5m": { "rank": 9, "total": 58, "value": 78.0 },
                "sessions": { "rank": 8, "total": 58, "value": 14 },
                "titleVariety": { "rank": 31, "total": 58, "value": 35.7 }
            },
            "gapToNext": { "login": "rookroute", "avgViewersDiff": 32, "chatDiff": 17, "retentionDiff": 0.6 }
        },
        "competitionDensity": {
            "hourly": [
                { "hour": 17, "activeStreamers": 5, "avgViewers": 352, "avgPeak": 812, "opportunityScore": 76.0, "yourData": null },
                { "hour": 18, "activeStreamers": 7, "avgViewers": 306, "avgPeak": 744, "opportunityScore": 82.4, "yourData": { "count": 1, "avgViewers": 338, "avgPeak": 780, "avgChatters": 186 } },
                { "hour": 19, "activeStreamers": 10, "avgViewers": 262, "avgPeak": 690, "opportunityScore": 73.2, "yourData": { "count": 3, "avgViewers": 368, "avgPeak": 905, "avgChatters": 214 } },
                { "hour": 20, "activeStreamers": 13, "avgViewers": 249, "avgPeak": 664, "opportunityScore": 65.8, "yourData": { "count": 6, "avgViewers": 410, "avgPeak": 1087, "avgChatters": 246 } },
                { "hour": 21, "activeStreamers": 15, "avgViewers": 239, "avgPeak": 620, "opportunityScore": 60.1, "yourData": { "count": 2, "avgViewers": 386, "avgPeak": 842, "avgChatters": 205 } },
                { "hour": 22, "activeStreamers": 9, "avgViewers": 228, "avgPeak": 588, "opportunityScore": 68.4, "yourData": null }
            ],
            "weekly": [
                { "weekday": 0, "weekdayLabel": "So", "activeStreamers": 18, "avgViewers": 214, "yourData": { "count": 1, "avgViewers": 304, "avgPeak": 720 } },
                { "weekday": 1, "weekdayLabel": "Mo", "activeStreamers": 22, "avgViewers": 238, "yourData": { "count": 1, "avgViewers": 286, "avgPeak": 690 } },
                { "weekday": 2, "weekdayLabel": "Di", "activeStreamers": 31, "avgViewers": 306, "yourData": { "count": 2, "avgViewers": 338, "avgPeak": 780 } },
                { "weekday": 3, "weekdayLabel": "Mi", "activeStreamers": 29, "avgViewers": 342, "yourData": { "count": 3, "avgViewers": 410, "avgPeak": 930 } },
                { "weekday": 4, "weekdayLabel": "Do", "activeStreamers": 34, "avgViewers": 318, "yourData": { "count": 2, "avgViewers": 395, "avgPeak": 890 } },
                { "weekday": 5, "weekdayLabel": "Fr", "activeStreamers": 42, "avgViewers": 356, "yourData": { "count": 3, "avgViewers": 462, "avgPeak": 1087 } },
                { "weekday": 6, "weekdayLabel": "Sa", "activeStreamers": 38, "avgViewers": 284, "yourData": { "count": 2, "avgViewers": 304, "avgPeak": 720 } }
            ],
            "sweetSpots": [
                { "hour": 18, "activeStreamers": 7, "avgViewers": 306, "avgPeak": 744, "opportunityScore": 82.4, "yourData": { "count": 1, "avgViewers": 338, "avgPeak": 780, "avgChatters": 186 } },
                { "hour": 17, "activeStreamers": 5, "avgViewers": 352, "avgPeak": 812, "opportunityScore": 76.0, "yourData": null },
                { "hour": 19, "activeStreamers": 10, "avgViewers": 262, "avgPeak": 690, "opportunityScore": 73.2, "yourData": { "count": 3, "avgViewers": 368, "avgPeak": 905, "avgChatters": 214 } }
            ]
        },
        "recommendations": [
            { "priority": "critical", "category": "Retention", "title": "Die ersten fünf Minuten festziehen", "description": "Dein größter Zuschauerverlust passiert direkt nach dem Einstieg: Nach fünf Minuten ist rund ein Fünftel schon wieder weg. Ein klarer Einstieg mit Tagesziel und direkter Ansprache neuer Zuschauer hält mehr Leute im Stream.", "estimatedImpact": "Bis zu +6 Prozentpunkte 5-Minuten-Retention.", "evidence": "Nach 5 Minuten hältst du 78 % — die Kategorie-Spitze hält in derselben Phase rund 85 %.", "icon": "TrendingDown" },
            { "priority": "high", "category": "Schedule", "title": "Aus der vollsten Sendezeit ausweichen", "description": "Deine Streams liegen vor allem Mittwoch und Freitag um 20 Uhr — genau dann konkurrieren 13 bis 15 andere Deadlock-Kanäle um dieselben Zuschauer. Dienstag 18 Uhr ist deutlich freier bei fast gleicher Nachfrage.", "estimatedImpact": "Mehr Sichtbarkeit pro Stream, geschätzt +8 % Anfangszuschauer.", "evidence": "Dein bester freier Slot (Di 18 Uhr) erreicht Chancen-Score 82 bei nur 7 Konkurrenten — deine 20-Uhr-Slots liegen bei 13 bis 15.", "icon": "Calendar" },
            { "priority": "high", "category": "Titel", "title": "Suchbegriffe in den Titelanfang", "description": "In deinen Titeln fehlen die Begriffe, über die neue Zuschauer dich finden — Rang, Patch oder Hero. Außerdem wiederholst du dich öfter als vergleichbare Kanäle, das macht jeden Stream schwerer unterscheidbar.", "estimatedImpact": "Geschätzt +10–15 % Erstzuschauer aus der Kategorie-Suche.", "evidence": "Deine Titel-Vielfalt liegt bei 36 % — vergleichbare Kanäle kommen auf 50 bis 65 %.", "icon": "Type" },
            { "priority": "medium", "category": "Tags", "title": "Die starken Kategorie-Tags übernehmen", "description": "Drei Tags, die in der Kategorie überdurchschnittlich ziehen, fehlen bei dir komplett: High MMR, Analysis und Viewer Games. Wer sie setzt, taucht in mehr Filter-Suchen auf.", "estimatedImpact": "Breitere Auffindbarkeit über die Tag-Suche.", "evidence": "Die Top-Tags der Kategorie bringen im Schnitt 812 Zuschauer — deine aktuellen Tags liegen bei 438.", "icon": "Tag" },
            { "priority": "medium", "category": "Community", "title": "Raids gezielter platzieren", "description": "Knapp die Hälfte deiner Chatter kommt nur zu dir — dein Netzwerk ist noch dünn. An rookroute und bridgeboss raidest du regelmäßig, zurück kommt aber nichts. Setz deine Raids stärker auf Partner, die zurückraiden.", "estimatedImpact": "Mehr eingehende Zuschauer über gegenseitige Raids.", "evidence": "45 % deiner Chatter tauchen nur bei dir auf; bei zwei deiner fünf Raid-Partner ist die Bilanz einseitig.", "icon": "Users" }
        ],
        "aiSummary": "Dein Kanal steht solide: 382 Zuschauer im Schnitt, starke Bindung und gesundes Follower-Wachstum bringen dich auf Rang 12 von 58 in der Kategorie. Die größten Hebel liegen woanders als bei der reinen Reichweite — die ersten fünf Minuten kosten dich noch zu viele Zuschauer, und deine besten Sendezeiten fallen in die vollste Konkurrenzphase. Wer hier nachschärft und Titel wie Tags auf Auffindbarkeit trimmt, rückt aus dem oberen Mittelfeld Richtung Kategorie-Spitze."
    }))
    .into_response()
}

/// `GET /twitch/demo/api/v2/follower-funnel` — Follower-Funnel im Frontend-Vertrag.
pub async fn demo_follower_funnel() -> Response {
    Json(json!({
        "uniqueViewers": 1820,
        "returningViewers": 914,
        "newFollowers": 183,
        "netFollowerDelta": 167,
        "conversionRate": 10.05,
        "avgTimeToFollow": 24.0,
        "followersBySource": { "organic": 126, "raids": 42, "hosts": 4, "other": 11 }
    }))
    .into_response()
}

/// `GET /twitch/demo/api/v2/watch-time-distribution` — Watch-Time im Frontend-Vertrag.
pub async fn demo_watch_time_distribution() -> Response {
    Json(json!({
        "under5min": 11.8,
        "min5to15": 18.4,
        "min15to30": 22.7,
        "min30to60": 28.9,
        "over60min": 18.2,
        "avgWatchTime": 34.6,
        "medianWatchTime": 29.4,
        "dataQuality": {
            "method": "real_samples",
            "coverage": 0.42,
            "sample_count": 764,
            "viewer_base_count": 1820,
            "required_min_samples": 25,
            "required_min_coverage": 0.15,
            "confidence": "high",
            "sessions": 14
        },
        "sessionCount": 14,
        "previous": {
            "under5min": 14.5,
            "min5to15": 20.2,
            "min15to30": 24.6,
            "min30to60": 25.8,
            "over60min": 14.9,
            "avgWatchTime": 30.9,
            "medianWatchTime": 26.1,
            "sessionCount": 13
        },
        "deltas": {
            "under5min": -2.7,
            "min5to15": -1.8,
            "min15to30": -1.9,
            "min30to60": 3.1,
            "over60min": 3.3,
            "avgWatchTime": 12.0
        }
    }))
    .into_response()
}

/// `GET /twitch/demo/api/v2/audience-demographics` — Audience-Mix im Frontend-Vertrag.
pub async fn demo_audience_demographics() -> Response {
    Json(json!({
        "viewerTypes": [
            { "label": "Dedicated", "percentage": 23.4 },
            { "label": "Regular", "percentage": 31.8 },
            { "label": "Casual", "percentage": 20.6 },
            { "label": "Lurker", "percentage": 18.7 },
            { "label": "New", "percentage": 5.5 }
        ],
        "activityPattern": "balanced",
        "primaryLanguage": "de",
        "languageConfidence": 93.0,
        "peakActivityHours": [19, 20, 21],
        "peakHoursMethod": "real_samples",
        "chatPenetrationPct": 34.8,
        "chatPenetrationReliable": true,
        "messagesPer100ViewerMinutes": 3.4,
        "viewerMinutes": 320520,
        "legacyInteractionActivePerAvgViewer": 110.2,
        "interactiveRate": 34.8,
        "interactionRateActivePerViewer": 34.8,
        "interactionRateActivePerAvgViewer": 110.2,
        "interactionRateReliable": true,
        "loyaltyScore": 55.2,
        "timezone": "Europe/Berlin",
        "dataQuality": {
            "confidence": "high",
            "sessions": 14,
            "method": "real_samples",
            "peakMethod": "real_samples",
            "coverage": 0.42,
            "sampleCount": 764,
            "peakSessionCount": 14,
            "peakSessionsWithActivity": 12,
            "interactiveSampleCount": 634,
            "interactionCoverage": 0.35,
            "chattersCoverage": 0.42,
            "chattersApiCoverage": 0.42,
            "passiveViewerSamples": 340,
            "sessionsWithChat": 14,
            "chatSessionCoverage": 1.0,
            "viewerSampleCount": 1820,
            "viewerMinutesSource": "real_samples"
        }
    }))
    .into_response()
}

/// `GET /twitch/demo/api/v2/lurker-analysis` — Lurker-Analyse im Frontend-Vertrag.
pub async fn demo_lurker_analysis() -> Response {
    Json(json!({
        "dataAvailable": true,
        "regularLurkers": [
            { "login": "silent_anchor", "lurkSessions": 9, "firstSeen": "2026-05-18T19:05:00Z", "lastSeen": "2026-06-26T21:40:00Z" },
            { "login": "watch_only_max", "lurkSessions": 8, "firstSeen": "2026-05-24T18:55:00Z", "lastSeen": "2026-06-21T20:12:00Z" },
            { "login": "quiet_orbital", "lurkSessions": 7, "firstSeen": "2026-05-29T19:20:00Z", "lastSeen": "2026-06-17T22:04:00Z" }
        ],
        "lurkerStats": { "ratio": 0.41, "avgSessions": 3.8, "totalLurkers": 746, "totalViewers": 1820 },
        "conversionStats": { "rate": 0.16, "eligible": 214, "converted": 34 }
    }))
    .into_response()
}

/// `GET /twitch/demo/api/v2/viewer-profiles` — Viewer-Profile im Frontend-Vertrag.
pub async fn demo_viewer_profiles() -> Response {
    Json(json!({
        "dataAvailable": true,
        "profiles": {
            "exclusive": 914,
            "loyalMulti": 342,
            "casual": 286,
            "explorer": 178,
            "passive": 100,
            "total": 1820
        },
        "exclusivityDistribution": [
            { "streamerCount": 1, "viewerCount": 914 },
            { "streamerCount": 2, "viewerCount": 406 },
            { "streamerCount": 3, "viewerCount": 238 },
            { "streamerCount": 4, "viewerCount": 146 },
            { "streamerCount": 5, "viewerCount": 116 }
        ]
    }))
    .into_response()
}

/// `GET /twitch/demo/api/v2/viewer-segments` — Viewer-Segmentierung im Frontend-Vertrag.
pub async fn demo_viewer_segments(Query(q): Query<DemoAnalyticsQuery>) -> Response {
    let days = q.days.unwrap_or(30).clamp(7, 365);
    Json(json!({
        "days": days,
        "segments": {
            "dedicated": { "count": 426, "pct": 23.4, "avgMessages": 18.6, "avgSessions": 6.4 },
            "regular": { "count": 579, "pct": 31.8, "avgMessages": 7.8, "avgSessions": 3.1 },
            "casual": { "count": 375, "pct": 20.6, "avgMessages": 2.4, "avgSessions": 1.5 },
            "lurker": { "count": 340, "pct": 18.7, "avgMessages": 0.0, "avgSessions": 2.6 },
            "new": { "count": 100, "pct": 5.5, "avgMessages": 1.9, "avgSessions": 1.0 }
        },
        "churnRisk": {
            "atRisk": 3,
            "recentlyChurned": 1,
            "atRiskViewers": [
                { "login": "old_regular", "sessions": 6, "messages": 42, "daysSinceLastSeen": 18, "category": "regular", "recentlySeenAt": ["partner_one", "partner_two"] },
                { "login": "aim_lab_fan", "sessions": 5, "messages": 31, "daysSinceLastSeen": 22, "category": "regular", "recentlySeenAt": ["partner_one"] },
                { "login": "viscous_main", "sessions": 4, "messages": 19, "daysSinceLastSeen": 29, "category": "casual", "recentlySeenAt": [] }
            ]
        },
        "crossChannelStats": {
            "exclusiveViewersPct": 50.2,
            "avgOtherChannels": 0.8,
            "topSharedChannels": [
                { "streamer": "partner_one", "sharedCount": 214, "direction": "bidirectional" },
                { "streamer": "partner_two", "sharedCount": 168, "direction": "bidirectional" },
                { "streamer": "aimdojo_live", "sharedCount": 91, "direction": "incoming" }
            ]
        }
    }))
    .into_response()
}

/// `GET /twitch/demo/api/v2/viewer-directory` — Viewer-Verzeichnis im Frontend-Vertrag.
pub async fn demo_viewer_directory(Query(q): Query<DemoAnalyticsQuery>) -> Response {
    let days = q.days.unwrap_or(30).clamp(7, 365);
    let page = q.page.unwrap_or(1).max(1);
    let per_page = q.per_page.unwrap_or(50).clamp(10, 100);
    Json(json!({
        "viewers": [
            { "login": "viewer_alpha", "totalSessions": 11, "totalMessages": 412, "firstSeen": "2026-05-18T19:02:00Z", "lastSeen": "2026-06-26T21:52:00Z", "daysSinceLastSeen": 1, "otherChannels": 0, "topOtherChannels": [], "category": "dedicated", "avgMessagesPerSession": 37.5, "isLurker": false },
            { "login": "viewer_bravo", "totalSessions": 9, "totalMessages": 388, "firstSeen": "2026-05-21T18:48:00Z", "lastSeen": "2026-06-26T21:46:00Z", "daysSinceLastSeen": 1, "otherChannels": 2, "topOtherChannels": ["partner_one", "partner_two"], "category": "dedicated", "avgMessagesPerSession": 43.1, "isLurker": false },
            { "login": "silent_anchor", "totalSessions": 9, "totalMessages": 0, "firstSeen": "2026-05-18T19:05:00Z", "lastSeen": "2026-06-26T21:40:00Z", "daysSinceLastSeen": 1, "otherChannels": 1, "topOtherChannels": ["partner_one"], "category": "lurker", "avgMessagesPerSession": 0.0, "isLurker": true },
            { "login": "new_wave", "totalSessions": 1, "totalMessages": 14, "firstSeen": "2026-06-26T20:10:00Z", "lastSeen": "2026-06-26T21:35:00Z", "daysSinceLastSeen": 1, "otherChannels": 0, "topOtherChannels": [], "category": "new", "avgMessagesPerSession": 14.0, "isLurker": false },
            { "login": "patch_note_reader", "totalSessions": 3, "totalMessages": 24, "firstSeen": "2026-06-03T19:22:00Z", "lastSeen": "2026-06-21T20:19:00Z", "daysSinceLastSeen": 6, "otherChannels": 3, "topOtherChannels": ["partner_one", "aimdojo_live", "partner_two"], "category": "regular", "avgMessagesPerSession": 8.0, "isLurker": false }
        ],
        "total": 5,
        "page": page,
        "perPage": per_page,
        "days": days,
        "summary": {
            "totalViewers": 1820,
            "activeViewers": 1074,
            "lurkers": 746,
            "exclusiveViewers": 914,
            "sharedViewers": 906,
            "avgSessionsPerViewer": 2.7,
            "avgOtherChannels": 0.8
        }
    }))
    .into_response()
}

/// `GET /twitch/demo/api/v2/viewer-timeline` — Viewer-Timeline im Frontend-Vertrag.
pub async fn demo_viewer_timeline() -> Response {
    Json(json!([
        { "timestamp": "2026-06-26 19:30", "avgViewers": 214.0, "peakViewers": 280, "minViewers": 180, "samples": 6 },
        { "timestamp": "2026-06-26 20:00", "avgViewers": 382.0, "peakViewers": 510, "minViewers": 296, "samples": 6 },
        { "timestamp": "2026-06-26 20:30", "avgViewers": 458.0, "peakViewers": 780, "minViewers": 390, "samples": 6 },
        { "timestamp": "2026-06-26 21:00", "avgViewers": 431.0, "peakViewers": 1087, "minViewers": 360, "samples": 6 },
        { "timestamp": "2026-06-26 21:30", "avgViewers": 405.0, "peakViewers": 620, "minViewers": 344, "samples": 6 }
    ]))
    .into_response()
}

/// `GET /twitch/demo/api/v2/ads-schedule` — Ads-Schedule im Frontend-Vertrag.
pub async fn demo_ads_schedule() -> Response {
    Json(json!({
        "current": {
            "next_ad_at": "2026-06-30T20:45:00Z",
            "last_ad_at": "2026-06-30T19:42:00Z",
            "duration": 60,
            "preroll_free_time": 1320,
            "snooze_count": 2,
            "snooze_refresh_at": "2026-06-30T20:15:00Z",
            "snapshot_at": "2026-06-30T20:00:00Z"
        },
        "history": [
            { "snapshot_at": "2026-06-30T20:00:00Z", "next_ad_at": "2026-06-30T20:45:00Z", "duration": 60, "preroll_free_time": 1320 },
            { "snapshot_at": "2026-06-30T19:30:00Z", "next_ad_at": "2026-06-30T19:42:00Z", "duration": 60, "preroll_free_time": 900 },
            { "snapshot_at": "2026-06-30T19:00:00Z", "next_ad_at": "2026-06-30T19:42:00Z", "duration": 60, "preroll_free_time": 2700 }
        ]
    }))
    .into_response()
}

/// `GET /twitch/demo/api/v2/tag-analysis-extended` — Tag-Performance im Frontend-Vertrag.
pub async fn demo_tag_analysis_extended() -> Response {
    Json(json!({
        "tags": [
            { "tagName": "Deadlock", "usageCount": 14, "avgViewers": 405.0, "avgRetention10m": 72.4, "avgFollowerGain": 13.1, "trend": "up", "trendValue": 18.0, "bestTimeSlot": "20:00", "avgStreamDuration": 10800.0, "categoryRank": 12 },
            { "tagName": "Ranked", "usageCount": 9, "avgViewers": 438.0, "avgRetention10m": 74.8, "avgFollowerGain": 15.6, "trend": "up", "trendValue": 22.0, "bestTimeSlot": "21:00", "avgStreamDuration": 11400.0, "categoryRank": 8 },
            { "tagName": "Patch", "usageCount": 4, "avgViewers": 386.0, "avgRetention10m": 69.3, "avgFollowerGain": 11.2, "trend": "stable", "trendValue": 2.0, "bestTimeSlot": "19:00", "avgStreamDuration": 9600.0, "categoryRank": 16 },
            { "tagName": "Coaching", "usageCount": 3, "avgViewers": 344.0, "avgRetention10m": 68.1, "avgFollowerGain": 8.4, "trend": "down", "trendValue": -6.0, "bestTimeSlot": "18:00", "avgStreamDuration": 9000.0, "categoryRank": 23 }
        ],
        "peerBenchmark": { "avgViewers": 318.0, "retention10m": 63.5 }
    }))
    .into_response()
}

/// `GET /twitch/demo/api/v2/title-performance` — Titel-Performance im Frontend-Vertrag.
pub async fn demo_title_performance() -> Response {
    Json(json!({
        "titles": [
            { "title": "Ranked Grind bis Eternus", "usageCount": 5, "avgViewers": 438.0, "avgRetention10m": 74.8, "avgFollowerGain": 15.6, "peakViewers": 1087, "keywords": ["Ranked", "Eternus"] },
            { "title": "Patch-Day First Impressions", "usageCount": 3, "avgViewers": 386.0, "avgRetention10m": 69.8, "avgFollowerGain": 12.0, "peakViewers": 905, "keywords": ["Patch", "First"] },
            { "title": "Hero Pool Review", "usageCount": 2, "avgViewers": 344.0, "avgRetention10m": 68.1, "avgFollowerGain": 8.5, "peakViewers": 760, "keywords": ["Hero", "Review"] }
        ],
        "peerBenchmark": { "avgViewers": 318.0, "retention10m": 63.5 }
    }))
    .into_response()
}

/// `GET /twitch/demo/api/v2/raid-retention` — Raid-Retention im Frontend-Vertrag.
pub async fn demo_raid_retention() -> Response {
    Json(json!({
        "dataAvailable": true,
        "summary": {
            "avgRetentionPct": 42.6,
            "avgConversionPct": 18.4,
            "totalNewChatters": 87,
            "raidCount": 6
        },
        "raids": [
            { "raidId": 9101, "toBroadcaster": "partner_one", "viewersSent": 420, "executedAt": "2026-06-26T22:18:00Z", "chattersAt5m": 148, "chattersAt15m": 106, "chattersAt30m": 82, "retention30mPct": 19.5, "newChatters": 24, "chatterConversionPct": 5.7, "knownFromRaider": 31 },
            { "raidId": 9100, "toBroadcaster": "partner_two", "viewersSent": 365, "executedAt": "2026-06-21T21:55:00Z", "chattersAt5m": 172, "chattersAt15m": 131, "chattersAt30m": 98, "retention30mPct": 26.8, "newChatters": 29, "chatterConversionPct": 7.9, "knownFromRaider": 22 },
            { "raidId": 9098, "toBroadcaster": "aimdojo_live", "viewersSent": 298, "executedAt": "2026-06-17T22:02:00Z", "chattersAt5m": 129, "chattersAt15m": 94, "chattersAt30m": 73, "retention30mPct": 24.5, "newChatters": 18, "chatterConversionPct": 6.0, "knownFromRaider": 15 }
        ]
    }))
    .into_response()
}

/// `GET /twitch/demo/api/v2/raid-analytics` — Incoming-Raid-Analytics im Frontend-Vertrag.
pub async fn demo_raid_analytics(Query(q): Query<DemoAnalyticsQuery>) -> Response {
    let days = q.days.unwrap_or(30).clamp(7, 365);
    Json(json!({
        "per_source": [
            { "from_channel": "partner_one", "raids_received": 3, "avg_viewers_sent": 148.0, "avg_new_chatters": 21.0, "avg_retention_30m": 38.4, "follows_attributed": 31, "conversion_rate": 7.0, "known_audience_overlap": 0.22 },
            { "from_channel": "partner_two", "raids_received": 2, "avg_viewers_sent": 96.0, "avg_new_chatters": 14.0, "avg_retention_30m": 34.2, "follows_attributed": 16, "conversion_rate": 8.3, "known_audience_overlap": 0.18 }
        ],
        "follow_attribution": {
            "total_follows": 183,
            "raid_follows": 42,
            "organic_follows": 141,
            "raid_conversion_rate": 6.8
        },
        "retention_curves": [
            { "raid_id": 7001, "from": "partner_one", "viewers_sent": 152, "new_chatters": 24, "retention_curve": { "plus5m": 64.0, "plus15m": 46.0, "plus30m": 36.0 } },
            { "raid_id": 7002, "from": "partner_two", "viewers_sent": 104, "new_chatters": 18, "retention_curve": { "plus5m": 58.0, "plus15m": 41.0, "plus30m": 31.0 } }
        ],
        "incoming_raids": [
            { "from_channel": "partner_one", "detected_at": "2026-06-26T20:12:00Z", "viewers_sent": 152, "classification": "organic", "unraid_seen": false, "impact": { "viewers_before": 318, "viewers_peak_after": 612, "boost_pct": 92.5, "retention_5m_pct": 64.0, "retention_15m_pct": 46.0, "retention_30m_pct": 36.0, "follows_after_raid": 18 } },
            { "from_channel": "partner_two", "detected_at": "2026-06-21T19:48:00Z", "viewers_sent": 104, "classification": "organic", "unraid_seen": false, "impact": { "viewers_before": 286, "viewers_peak_after": 438, "boost_pct": 53.1, "retention_5m_pct": 58.0, "retention_15m_pct": 41.0, "retention_30m_pct": 31.0, "follows_after_raid": 11 } },
            { "from_channel": "aimdojo_live", "detected_at": "2026-06-17T20:05:00Z", "viewers_sent": 78, "classification": "organic", "unraid_seen": true, "impact": { "viewers_before": 340, "viewers_peak_after": 451, "boost_pct": 32.6, "retention_5m_pct": 51.0, "retention_15m_pct": 35.0, "retention_30m_pct": 27.0, "follows_after_raid": 7 } }
        ],
        "incoming_summary": {
            "total_raids_received": 6,
            "avg_viewers_received": 112.3,
            "avg_boost_pct": 59.4,
            "avg_retention_15m": 40.7,
            "best_raider": "partner_one",
            "raid_balance": { "sent": 8, "received": 6 }
        },
        "window_days": days,
        "dataQuality": {
            "botFilterApplied": true,
            "retentionCurveSampleSize": 6,
            "perSourceUsesFullWindow": true,
            "raidMetricBatchSize": 50
        }
    }))
    .into_response()
}

/// Baut den öffentlichen Demo-Router (kein Auth, kein Pool).
pub fn build_demo_router() -> Router {
    Router::new()
        .route("/twitch/demo", get(demo_index_handler))
        .route("/twitch/demo/", get(demo_index_handler))
        .route("/twitch/demo/dashboard-v2/*path", get(demo_assets_handler))
        .route("/twitch/demo/api/v2/auth-status", get(demo_auth_status))
        .route("/twitch/demo/api/v2/streamers", get(demo_streamers))
        .route("/twitch/demo/api/v2/overview", get(demo_overview))
        .route(
            "/twitch/demo/api/v2/ai/analysis",
            get(demo_ai_analysis).post(demo_ai_analysis),
        )
        .route("/twitch/demo/api/v2/ai/history", get(demo_ai_history))
        // P2.72: fünf zuvor unportierte Kern-Kacheln.
        .route("/twitch/demo/api/v2/monthly-stats", get(demo_monthly_stats))
        .route("/twitch/demo/api/v2/weekly-stats", get(demo_weekly_stats))
        .route(
            "/twitch/demo/api/v2/hourly-heatmap",
            get(demo_hourly_heatmap),
        )
        .route(
            "/twitch/demo/api/v2/calendar-heatmap",
            get(demo_calendar_heatmap),
        )
        .route(
            "/twitch/demo/api/v2/viewer-timeline",
            get(demo_viewer_timeline),
        )
        .route(
            "/twitch/demo/api/v2/chat-analytics",
            get(demo_chat_analytics),
        )
        // P2.73: weitere abgeworfene Fixture-Kacheln.
        .route("/twitch/demo/api/v2/monetization", get(demo_monetization))
        .route("/twitch/demo/api/v2/ads-schedule", get(demo_ads_schedule))
        .route(
            "/twitch/demo/api/v2/category-leaderboard",
            get(demo_category_leaderboard),
        )
        .route("/twitch/demo/api/v2/coaching", get(demo_coaching))
        .route(
            "/twitch/demo/api/v2/follower-funnel",
            get(demo_follower_funnel),
        )
        .route(
            "/twitch/demo/api/v2/watch-time-distribution",
            get(demo_watch_time_distribution),
        )
        .route(
            "/twitch/demo/api/v2/audience-demographics",
            get(demo_audience_demographics),
        )
        .route(
            "/twitch/demo/api/v2/viewer-profiles",
            get(demo_viewer_profiles),
        )
        .route(
            "/twitch/demo/api/v2/viewer-segments",
            get(demo_viewer_segments),
        )
        .route(
            "/twitch/demo/api/v2/viewer-directory",
            get(demo_viewer_directory),
        )
        .route(
            "/twitch/demo/api/v2/lurker-analysis",
            get(demo_lurker_analysis),
        )
        .route(
            "/twitch/demo/api/v2/tag-analysis-extended",
            get(demo_tag_analysis_extended),
        )
        .route(
            "/twitch/demo/api/v2/title-performance",
            get(demo_title_performance),
        )
        .route(
            "/twitch/demo/api/v2/raid-retention",
            get(demo_raid_retention),
        )
        .route(
            "/twitch/demo/api/v2/raid-analytics",
            get(demo_raid_analytics),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    async fn json_body(resp: Response) -> (StatusCode, Value) {
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn demo_json(path: &str) -> Value {
        let resp = build_demo_router()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let (status, json) = json_body(resp).await;
        assert_eq!(status, StatusCode::OK, "{path} muss 200 liefern");
        json
    }

    #[tokio::test]
    async fn auth_status_ist_demo_partner() {
        let app = build_demo_router();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/twitch/demo/api/v2/auth-status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let (s, j) = json_body(resp).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["demoMode"], true);
        assert_eq!(j["twitchLogin"], DEMO_LOGIN);
    }

    #[tokio::test]
    async fn overview_hat_pflicht_keys_und_days_clamp() {
        let app = build_demo_router();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/twitch/demo/api/v2/overview?days=3")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let (s, j) = json_body(resp).await;
        assert_eq!(s, StatusCode::OK);
        // days auf min 7 geklemmt.
        assert_eq!(j["days"], 7);
        assert!(j["scores"]["total"].is_number());
        assert!(j["summary"]["avgViewers"].is_number());
        assert!(j["sessions"].is_array());
        let session = &j["sessions"][0];
        for key in [
            "startViewers",
            "dropoffPct",
            "totalChatterSessions",
            "firstTimeChatters",
            "returningChatters",
            "followersStart",
            "followersEnd",
            "retention5m",
            "retention20m",
            "endViewers",
        ] {
            assert!(session.get(key).is_some(), "session key {key} fehlt");
        }
    }

    #[tokio::test]
    async fn category_leaderboard_matcht_contract() {
        let j = demo_json("/twitch/demo/api/v2/category-leaderboard").await;
        let entries = j["leaderboard"].as_array().unwrap();
        assert!(!entries.is_empty());
        for entry in entries {
            for key in ["rank", "streamer", "avgViewers", "peakViewers", "isPartner"] {
                assert!(entry.get(key).is_some(), "leaderboard key {key} fehlt");
            }
        }
        assert!(j["totalStreamers"].is_number());
        assert!(j["yourRank"].is_number());
        assert!(j.get("yourTier").is_some());
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry["isYou"] == true)
                .count(),
            1
        );
        assert!(j.get("categories").is_none());
    }

    #[tokio::test]
    async fn coaching_matcht_coachingdata_contract() {
        let j = demo_json("/twitch/demo/api/v2/coaching").await;
        assert_eq!(j["empty"], false);
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
            "recommendations",
        ] {
            assert!(j.get(key).is_some(), "coaching key {key} fehlt");
        }
        assert!(!j["recommendations"].as_array().unwrap().is_empty());
        assert!(j.get("aiSummary").is_some());
        assert!(j.get("tips").is_none());
    }

    #[tokio::test]
    async fn ai_analysis_per_get_und_post() {
        let app = build_demo_router();
        for method in ["GET", "POST"] {
            let resp = build_demo_router()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri("/twitch/demo/api/v2/ai/analysis")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK, "{method} muss 200 liefern");
        }
        let _ = app;
    }

    #[tokio::test]
    async fn ai_analysis_matcht_prod_schema() {
        // P1.28: Demo-AI-Analyse trägt das Prod-Schema (points/dataSnapshot/...)
        // statt summary/sections/createdAt.
        let resp = build_demo_router()
            .oneshot(
                Request::builder()
                    .uri("/twitch/demo/api/v2/ai/analysis")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let (s, j) = json_body(resp).await;
        assert_eq!(s, StatusCode::OK);
        for key in [
            "id",
            "streamer",
            "days",
            "gameFilter",
            "model",
            "generatedAt",
            "points",
            "dataSnapshot",
        ] {
            assert!(j.get(key).is_some(), "Key {key} fehlt");
        }
        // Alte Form darf NICHT mehr auftauchen.
        assert!(j.get("summary").is_none());
        assert!(j.get("sections").is_none());
        assert!(j["points"].is_array());
        assert_eq!(j["points"].as_array().unwrap().len(), 10);
        let p0 = &j["points"][0];
        for key in [
            "number",
            "priority",
            "title",
            "analysis",
            "action",
            "expectedImpact",
        ] {
            assert!(p0.get(key).is_some(), "point key {key} fehlt");
        }
        assert!(j["dataSnapshot"]["streamCount"].is_number());
    }

    #[tokio::test]
    async fn ai_analysis_gamefilter_deadlock() {
        // P1.28: gameFilter=deadlock-Zweig.
        let resp = build_demo_router()
            .oneshot(
                Request::builder()
                    .uri("/twitch/demo/api/v2/ai/analysis?game_filter=deadlock")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let (s, j) = json_body(resp).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["gameFilter"], "deadlock");
    }

    #[tokio::test]
    async fn ai_history_matcht_prod_schema() {
        // P1.28: history-Einträge mit generatedAt/points + Prioritäts-Zählern.
        let resp = build_demo_router()
            .oneshot(
                Request::builder()
                    .uri("/twitch/demo/api/v2/ai/history")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let (s, j) = json_body(resp).await;
        assert_eq!(s, StatusCode::OK);
        let entry = &j[0];
        for key in [
            "generatedAt",
            "points",
            "dataSnapshot",
            "kritischCount",
            "hochCount",
            "mittelCount",
        ] {
            assert!(entry.get(key).is_some(), "history key {key} fehlt");
        }
        // Kein altes createdAt.
        assert!(entry.get("createdAt").is_none());
        // Zähler stimmen mit der Punkte-Verteilung überein (3/4/3).
        assert_eq!(entry["kritischCount"], 3);
        assert_eq!(entry["hochCount"], 4);
        assert_eq!(entry["mittelCount"], 3);
    }

    #[tokio::test]
    async fn streamers_konsistent_mit_allowed_profiles() {
        // P3.16: streamers-Liste enthält nur midcore_live (kein smallquest_tv-Leak).
        let resp = build_demo_router()
            .oneshot(
                Request::builder()
                    .uri("/twitch/demo/api/v2/streamers")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let (s, j) = json_body(resp).await;
        assert_eq!(s, StatusCode::OK);
        let logins: Vec<&str> = j
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["login"].as_str().unwrap())
            .collect();
        assert_eq!(logins, vec![DEMO_LOGIN]);
    }

    #[tokio::test]
    async fn lean_fixture_kacheln_liefern_200() {
        // P2.72 + P2.73: zuvor 404er Kacheln liefern jetzt 200-Fixtures.
        let paths = [
            "/twitch/demo/api/v2/monthly-stats",
            "/twitch/demo/api/v2/weekly-stats",
            "/twitch/demo/api/v2/hourly-heatmap",
            "/twitch/demo/api/v2/calendar-heatmap",
            "/twitch/demo/api/v2/viewer-timeline",
            "/twitch/demo/api/v2/chat-analytics",
            "/twitch/demo/api/v2/monetization",
            "/twitch/demo/api/v2/ads-schedule",
            "/twitch/demo/api/v2/category-leaderboard",
            "/twitch/demo/api/v2/coaching",
            "/twitch/demo/api/v2/follower-funnel",
            "/twitch/demo/api/v2/watch-time-distribution",
            "/twitch/demo/api/v2/audience-demographics",
            "/twitch/demo/api/v2/viewer-profiles",
            "/twitch/demo/api/v2/viewer-segments",
            "/twitch/demo/api/v2/viewer-directory",
            "/twitch/demo/api/v2/lurker-analysis",
            "/twitch/demo/api/v2/tag-analysis-extended",
            "/twitch/demo/api/v2/title-performance",
            "/twitch/demo/api/v2/raid-retention",
            "/twitch/demo/api/v2/raid-analytics",
        ];
        for path in paths {
            let resp = build_demo_router()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK, "{path} muss 200 liefern");
        }
    }

    #[tokio::test]
    async fn demo_performance_endpoints_match_frontend_contracts() {
        let monthly = demo_json("/twitch/demo/api/v2/monthly-stats").await;
        assert!(monthly.is_array());
        let month = &monthly[0];
        for key in [
            "year",
            "month",
            "totalHoursWatched",
            "totalAirtime",
            "avgViewers",
            "peakViewers",
            "followerDelta",
            "totalChatterSessions",
            "streamCount",
        ] {
            assert!(month[key].is_number(), "monthly-stats {key} muss Zahl sein");
        }
        assert!(month["monthLabel"].is_string());
        assert!(month.get("months").is_none());

        let weekly = demo_json("/twitch/demo/api/v2/weekly-stats").await;
        assert!(weekly.is_array());
        let weekday = &weekly[0];
        for key in [
            "weekday",
            "streamCount",
            "avgHours",
            "avgViewers",
            "avgPeak",
            "totalFollowers",
        ] {
            assert!(
                weekday[key].is_number(),
                "weekly-stats {key} muss Zahl sein"
            );
        }
        assert!(weekday["weekdayLabel"].is_string());

        let hourly = demo_json("/twitch/demo/api/v2/hourly-heatmap").await;
        assert!(hourly.is_array());
        let cell = &hourly[0];
        for key in ["weekday", "hour", "streamCount", "avgViewers", "avgPeak"] {
            assert!(cell[key].is_number(), "hourly-heatmap {key} muss Zahl sein");
        }
        assert!(hourly.get("cells").is_none());

        let calendar = demo_json("/twitch/demo/api/v2/calendar-heatmap").await;
        assert!(calendar.is_array());
        let day = &calendar[0];
        assert!(day["date"].is_string());
        for key in ["value", "streamCount", "hoursWatched"] {
            assert!(
                day[key].is_number(),
                "calendar-heatmap {key} muss Zahl sein"
            );
        }

        let timeline = demo_json("/twitch/demo/api/v2/viewer-timeline").await;
        assert!(timeline.is_array());
        let point = &timeline[0];
        assert!(point["timestamp"].is_string());
        for key in ["avgViewers", "peakViewers", "minViewers", "samples"] {
            assert!(
                point[key].is_number(),
                "viewer-timeline {key} muss Zahl sein"
            );
        }
    }

    #[tokio::test]
    async fn demo_audience_endpoints_match_frontend_contracts() {
        let watch = demo_json("/twitch/demo/api/v2/watch-time-distribution").await;
        for key in [
            "under5min",
            "min5to15",
            "min15to30",
            "min30to60",
            "over60min",
            "avgWatchTime",
            "medianWatchTime",
            "sessionCount",
        ] {
            assert!(
                watch[key].is_number(),
                "watch-time-distribution {key} muss Zahl sein"
            );
        }
        assert_eq!(watch["dataQuality"]["method"], "real_samples");
        assert!(watch["previous"]["avgWatchTime"].is_number());
        assert!(watch["deltas"]["avgWatchTime"].is_number());

        let funnel = demo_json("/twitch/demo/api/v2/follower-funnel").await;
        for key in [
            "uniqueViewers",
            "returningViewers",
            "newFollowers",
            "netFollowerDelta",
            "conversionRate",
            "avgTimeToFollow",
        ] {
            assert!(
                funnel[key].is_number(),
                "follower-funnel {key} muss Zahl sein"
            );
        }
        for key in ["organic", "raids", "hosts", "other"] {
            assert!(funnel["followersBySource"][key].is_number());
        }

        let demographics = demo_json("/twitch/demo/api/v2/audience-demographics").await;
        assert!(demographics["viewerTypes"].is_array());
        assert!(demographics["activityPattern"].is_string());
        assert!(demographics["primaryLanguage"].is_string());
        assert!(demographics["peakActivityHours"].is_array());
        for key in ["languageConfidence", "interactiveRate", "loyaltyScore"] {
            assert!(
                demographics[key].is_number(),
                "audience-demographics {key} muss Zahl sein"
            );
        }
        assert!(demographics["dataQuality"]["confidence"].is_string());

        let lurker = demo_json("/twitch/demo/api/v2/lurker-analysis").await;
        assert_eq!(lurker["dataAvailable"], true);
        assert!(lurker["regularLurkers"].is_array());
        assert!(lurker["lurkerStats"]["ratio"].is_number());
        assert!(lurker["lurkerStats"]["avgSessions"].is_number());
        assert!(lurker["conversionStats"]["rate"].is_number());

        let profiles = demo_json("/twitch/demo/api/v2/viewer-profiles").await;
        assert_eq!(profiles["dataAvailable"], true);
        for key in [
            "exclusive",
            "loyalMulti",
            "casual",
            "explorer",
            "passive",
            "total",
        ] {
            assert!(
                profiles["profiles"][key].is_number(),
                "viewer-profiles {key} muss Zahl sein"
            );
        }
        assert!(profiles["exclusivityDistribution"].is_array());
        assert!(profiles["exclusivityDistribution"][0]["streamerCount"].is_number());
        assert!(profiles["exclusivityDistribution"][0]["viewerCount"].is_number());
    }

    #[tokio::test]
    async fn demo_viewer_endpoints_match_frontend_contracts() {
        let segments = demo_json("/twitch/demo/api/v2/viewer-segments").await;
        assert!(segments["days"].is_number());
        assert!(segments["segments"].is_object());
        for key in ["count", "pct", "avgMessages", "avgSessions"] {
            assert!(
                segments["segments"]["dedicated"][key].is_number(),
                "segment {key} muss Zahl sein"
            );
        }
        assert!(segments["churnRisk"]["atRisk"].is_number());
        assert!(segments["churnRisk"]["recentlyChurned"].is_number());
        assert!(segments["churnRisk"]["atRiskViewers"].is_array());
        assert!(segments["crossChannelStats"]["exclusiveViewersPct"].is_number());
        assert!(segments["crossChannelStats"]["avgOtherChannels"].is_number());
        assert!(segments["crossChannelStats"]["topSharedChannels"].is_array());

        let directory = demo_json("/twitch/demo/api/v2/viewer-directory").await;
        assert!(directory["viewers"].is_array());
        for key in ["total", "page", "perPage", "days"] {
            assert!(
                directory[key].is_number(),
                "viewer-directory {key} muss Zahl sein"
            );
        }
        for key in [
            "totalViewers",
            "activeViewers",
            "lurkers",
            "exclusiveViewers",
            "sharedViewers",
            "avgSessionsPerViewer",
            "avgOtherChannels",
        ] {
            assert!(
                directory["summary"][key].is_number(),
                "viewer-directory summary {key} muss Zahl sein"
            );
        }
        let viewer = &directory["viewers"][0];
        assert!(viewer["login"].is_string());
        for key in [
            "totalSessions",
            "totalMessages",
            "daysSinceLastSeen",
            "otherChannels",
            "avgMessagesPerSession",
        ] {
            assert!(viewer[key].is_number(), "viewer entry {key} muss Zahl sein");
        }
        assert!(viewer["topOtherChannels"].is_array());
        assert!(viewer["category"].is_string());
        assert!(viewer["isLurker"].is_boolean());
    }

    #[tokio::test]
    async fn demo_growth_and_monetization_endpoints_match_frontend_contracts() {
        let monetization = demo_json("/twitch/demo/api/v2/monetization").await;
        for key in [
            "total",
            "auto",
            "manual",
            "sessions_with_ads",
            "avg_duration_s",
        ] {
            assert!(
                monetization["ads"][key].is_number(),
                "monetization ads.{key} muss Zahl sein"
            );
        }
        assert!(monetization["ads"]["avg_viewer_drop_pct"].is_number());
        assert!(monetization["ads"]["worst_ads"].is_array());
        assert!(monetization["hype_train"]["total"].is_number());
        assert!(monetization["bits"]["total"].is_number());
        assert!(monetization["bits"]["cheer_events"].is_number());
        assert!(monetization["subs"]["total_events"].is_number());
        assert!(monetization["subs"]["gifted"].is_number());
        assert!(monetization["window_days"].is_number());

        let schedule = demo_json("/twitch/demo/api/v2/ads-schedule").await;
        assert!(schedule["current"].is_object());
        assert!(schedule["current"]["snapshot_at"].is_string());
        assert!(schedule["current"]["duration"].is_number());
        assert!(schedule["history"].is_array());
        assert!(schedule["history"][0]["snapshot_at"].is_string());

        let tags = demo_json("/twitch/demo/api/v2/tag-analysis-extended").await;
        assert!(tags["tags"].is_array());
        let tag = &tags["tags"][0];
        assert!(tag["tagName"].is_string());
        for key in [
            "usageCount",
            "avgViewers",
            "avgRetention10m",
            "avgFollowerGain",
            "trendValue",
            "avgStreamDuration",
            "categoryRank",
        ] {
            assert!(
                tag[key].is_number(),
                "tag-analysis-extended {key} muss Zahl sein"
            );
        }
        assert!(tag["trend"].is_string());
        assert!(tag["bestTimeSlot"].is_string());
        assert!(tags["peerBenchmark"]["avgViewers"].is_number());

        let titles = demo_json("/twitch/demo/api/v2/title-performance").await;
        assert!(titles["titles"].is_array());
        let title = &titles["titles"][0];
        assert!(title["title"].is_string());
        for key in [
            "usageCount",
            "avgViewers",
            "avgRetention10m",
            "avgFollowerGain",
            "peakViewers",
        ] {
            assert!(
                title[key].is_number(),
                "title-performance {key} muss Zahl sein"
            );
        }
        assert!(title["keywords"].is_array());
        assert!(titles["peerBenchmark"]["retention10m"].is_number());

        let raid_retention = demo_json("/twitch/demo/api/v2/raid-retention").await;
        assert_eq!(raid_retention["dataAvailable"], true);
        for key in [
            "avgRetentionPct",
            "avgConversionPct",
            "totalNewChatters",
            "raidCount",
        ] {
            assert!(
                raid_retention["summary"][key].is_number(),
                "raid-retention summary {key} muss Zahl sein"
            );
        }
        assert!(raid_retention["raids"].is_array());
        assert!(raid_retention["raids"][0]["retention30mPct"].is_number());

        let raid_analytics = demo_json("/twitch/demo/api/v2/raid-analytics").await;
        assert!(raid_analytics["per_source"].is_array());
        assert!(raid_analytics["follow_attribution"]["total_follows"].is_number());
        assert!(raid_analytics["retention_curves"].is_array());
        assert!(raid_analytics["incoming_raids"].is_array());
        assert!(raid_analytics["incoming_raids"][0]["impact"]["boost_pct"].is_number());
        assert!(raid_analytics["incoming_summary"]["raid_balance"]["sent"].is_number());
        assert!(raid_analytics["window_days"].is_number());
        assert!(raid_analytics["dataQuality"]["botFilterApplied"].is_boolean());
    }

    #[tokio::test]
    async fn index_setzt_demo_csp() {
        let app = build_demo_router();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/twitch/demo")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // Ohne gebauten Dist → 404, aber CSP-Header ist trotzdem gesetzt.
        let csp = resp.headers().get(header::CONTENT_SECURITY_POLICY);
        assert!(csp.is_some(), "Demo-Index muss frame-ancestors-CSP setzen");
        assert!(csp.unwrap().to_str().unwrap().contains("frame-ancestors"));
    }
}
