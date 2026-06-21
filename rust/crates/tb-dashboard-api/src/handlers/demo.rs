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
        resp.headers_mut().insert(header::CONTENT_SECURITY_POLICY, value);
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
    with_demo_csp(
        (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            html,
        )
            .into_response(),
    )
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
              "avgViewers": 410.0, "peakViewers": 1087, "retention10m": 72.4,
              "uniqueChatters": 240, "title": "Ranked Grind bis Eternus" },
            { "id": 2, "date": "2026-06-10", "startTime": "19:30", "duration": 9000,
              "avgViewers": 355.0, "peakViewers": 905, "retention10m": 69.8,
              "uniqueChatters": 198, "title": "Patch-Day First Impressions" }
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
        "avgChatters": if deadlock { 45 } else { 45 },
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

/// `GET /twitch/demo/api/v2/monthly-stats` — Monatsaggregat (Fixture).
pub async fn demo_monthly_stats() -> Response {
    Json(json!({
        "streamer": DEMO_LOGIN,
        "months": [
            { "month": "2026-04", "sessions": 12, "avgViewers": 351, "peakViewers": 902,
              "totalHours": 41.0, "followersGained": 142 },
            { "month": "2026-05", "sessions": 14, "avgViewers": 372, "peakViewers": 998,
              "totalHours": 47.5, "followersGained": 168 },
            { "month": "2026-06", "sessions": 14, "avgViewers": 382, "peakViewers": 1087,
              "totalHours": 49.0, "followersGained": 183 }
        ]
    }))
    .into_response()
}

/// `GET /twitch/demo/api/v2/weekly-stats` — Wochentag-Verteilung (Fixture).
pub async fn demo_weekly_stats() -> Response {
    let labels = ["So", "Mo", "Di", "Mi", "Do", "Fr", "Sa"];
    let avg = [120, 280, 340, 410, 395, 460, 300];
    let weekdays: Vec<Value> = (0..7)
        .map(|i| {
            json!({
                "weekday": i,
                "weekdayLabel": labels[i as usize],
                "sessions": 2,
                "avgViewers": avg[i as usize],
            })
        })
        .collect();
    Json(json!({ "streamer": DEMO_LOGIN, "weekdays": weekdays })).into_response()
}

/// `GET /twitch/demo/api/v2/hourly-heatmap` — Wochentag×Stunde-Heatmap (Fixture).
pub async fn demo_hourly_heatmap() -> Response {
    // 6 repräsentative Slots statt voller 7×24-Matrix (lean).
    Json(json!({
        "streamer": DEMO_LOGIN,
        "cells": [
            { "weekday": 3, "hour": 19, "avgViewers": 410, "sessions": 4 },
            { "weekday": 3, "hour": 20, "avgViewers": 445, "sessions": 4 },
            { "weekday": 5, "hour": 20, "avgViewers": 480, "sessions": 5 },
            { "weekday": 5, "hour": 21, "avgViewers": 462, "sessions": 5 },
            { "weekday": 6, "hour": 16, "avgViewers": 300, "sessions": 3 },
            { "weekday": 0, "hour": 18, "avgViewers": 210, "sessions": 2 }
        ]
    }))
    .into_response()
}

/// `GET /twitch/demo/api/v2/calendar-heatmap` — Tages-Kalender-Heatmap (Fixture).
pub async fn demo_calendar_heatmap() -> Response {
    Json(json!({
        "streamer": DEMO_LOGIN,
        "days": [
            { "date": "2026-06-06", "sessions": 1, "totalHours": 3.5, "avgViewers": 355 },
            { "date": "2026-06-10", "sessions": 1, "totalHours": 2.5, "avgViewers": 372 },
            { "date": "2026-06-12", "sessions": 1, "totalHours": 3.2, "avgViewers": 410 }
        ]
    }))
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

/// `GET /twitch/demo/api/v2/monetization` — Monetarisierungs-Kachel (Fixture).
pub async fn demo_monetization() -> Response {
    Json(json!({
        "streamer": DEMO_LOGIN,
        "summary": { "subs": 84, "bits": 124000, "estimatedRevenueEur": 612.0 },
        "timeline": [
            { "date": "2026-06-06", "subs": 6, "bits": 8200 },
            { "date": "2026-06-10", "subs": 9, "bits": 11400 },
            { "date": "2026-06-12", "subs": 11, "bits": 13800 }
        ]
    }))
    .into_response()
}

/// `GET /twitch/demo/api/v2/category-leaderboard` — Kategorie-Ranking (Fixture).
pub async fn demo_category_leaderboard() -> Response {
    Json(json!({
        "streamer": DEMO_LOGIN,
        "categories": [
            { "category": "Deadlock", "rank": 12, "avgViewers": 410, "hours": 32.0 },
            { "category": "Just Chatting", "rank": 240, "avgViewers": 280, "hours": 9.0 }
        ]
    }))
    .into_response()
}

/// `GET /twitch/demo/api/v2/coaching` — Coaching-Hinweise (Fixture, Texte später).
pub async fn demo_coaching() -> Response {
    Json(json!({
        "streamer": DEMO_LOGIN,
        "generatedAt": "2026-06-13T11:00:00Z",
        "tips": [
            { "area": "retention", "title": "Zuschauer länger halten", "detail": "Die meisten Zuschauer springen in den ersten Minuten ab. Steige mit einer klaren Ansage zum heutigen Stream-Ziel ein und sprich neue Zuschauer aktiv an." },
            { "area": "growth", "title": "Reichweite ausbauen", "detail": "Setze Titel und Kategorie vor dem Live-Gehen, raide zum Schluss einen aktiven Partner und verwerte Highlights als kurze Clips." }
        ]
    }))
    .into_response()
}

/// `GET /twitch/demo/api/v2/follower-funnel` — Follower-Funnel (Fixture).
pub async fn demo_follower_funnel() -> Response {
    Json(json!({
        "streamer": DEMO_LOGIN,
        "stages": [
            { "stage": "viewers", "count": 1820 },
            { "stage": "chatters", "count": 634 },
            { "stage": "followers", "count": 183 },
            { "stage": "subs", "count": 84 }
        ]
    }))
    .into_response()
}

/// `GET /twitch/demo/api/v2/viewer-segments` — Viewer-Segmentierung (Fixture).
pub async fn demo_viewer_segments() -> Response {
    Json(json!({
        "streamer": DEMO_LOGIN,
        "segments": [
            { "segment": "loyal", "viewers": 210, "share": 0.34 },
            { "segment": "regular", "viewers": 280, "share": 0.44 },
            { "segment": "casual", "viewers": 144, "share": 0.22 }
        ],
        "exclusiveViewersPct": 100.0,
        "avgOtherChannels": 0.0
    }))
    .into_response()
}

/// `GET /twitch/demo/api/v2/viewer-directory` — Viewer-Verzeichnis (Fixture).
pub async fn demo_viewer_directory() -> Response {
    Json(json!({
        "streamer": DEMO_LOGIN,
        "viewers": [
            { "login": "viewer_alpha", "messages": 412, "watchMinutes": 1840 },
            { "login": "viewer_bravo", "messages": 388, "watchMinutes": 1620 }
        ],
        "count": 2
    }))
    .into_response()
}

/// `GET /twitch/demo/api/v2/lurker-analysis` — Lurker-Anteil (Fixture).
pub async fn demo_lurker_analysis() -> Response {
    Json(json!({
        "streamer": DEMO_LOGIN,
        "lurkerRatio": 0.67,
        "activeRatio": 0.33,
        "avgViewers": 382,
        "activeChatters": 421
    }))
    .into_response()
}

/// `GET /twitch/demo/api/v2/title-performance` — Titel-Performance (Fixture).
pub async fn demo_title_performance() -> Response {
    Json(json!({
        "streamer": DEMO_LOGIN,
        "titles": [
            { "title": "Ranked Grind bis Eternus", "avgViewers": 410, "retention10m": 72.4 },
            { "title": "Patch-Day First Impressions", "avgViewers": 355, "retention10m": 69.8 }
        ]
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
        .route("/twitch/demo/api/v2/hourly-heatmap", get(demo_hourly_heatmap))
        .route("/twitch/demo/api/v2/calendar-heatmap", get(demo_calendar_heatmap))
        .route("/twitch/demo/api/v2/chat-analytics", get(demo_chat_analytics))
        // P2.73: weitere abgeworfene Fixture-Kacheln.
        .route("/twitch/demo/api/v2/monetization", get(demo_monetization))
        .route(
            "/twitch/demo/api/v2/category-leaderboard",
            get(demo_category_leaderboard),
        )
        .route("/twitch/demo/api/v2/coaching", get(demo_coaching))
        .route("/twitch/demo/api/v2/follower-funnel", get(demo_follower_funnel))
        .route("/twitch/demo/api/v2/viewer-segments", get(demo_viewer_segments))
        .route(
            "/twitch/demo/api/v2/viewer-directory",
            get(demo_viewer_directory),
        )
        .route("/twitch/demo/api/v2/lurker-analysis", get(demo_lurker_analysis))
        .route(
            "/twitch/demo/api/v2/title-performance",
            get(demo_title_performance),
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
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    #[tokio::test]
    async fn auth_status_ist_demo_partner() {
        let app = build_demo_router();
        let resp = app
            .oneshot(Request::builder().uri("/twitch/demo/api/v2/auth-status").body(Body::empty()).unwrap())
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
            .oneshot(Request::builder().uri("/twitch/demo/api/v2/overview?days=3").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let (s, j) = json_body(resp).await;
        assert_eq!(s, StatusCode::OK);
        // days auf min 7 geklemmt.
        assert_eq!(j["days"], 7);
        assert!(j["scores"]["total"].is_number());
        assert!(j["summary"]["avgViewers"].is_number());
        assert!(j["sessions"].is_array());
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
        for key in ["id", "streamer", "days", "gameFilter", "model", "generatedAt", "points", "dataSnapshot"] {
            assert!(j.get(key).is_some(), "Key {key} fehlt");
        }
        // Alte Form darf NICHT mehr auftauchen.
        assert!(j.get("summary").is_none());
        assert!(j.get("sections").is_none());
        assert!(j["points"].is_array());
        assert_eq!(j["points"].as_array().unwrap().len(), 10);
        let p0 = &j["points"][0];
        for key in ["number", "priority", "title", "analysis", "action", "expectedImpact"] {
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
        for key in ["generatedAt", "points", "dataSnapshot", "kritischCount", "hochCount", "mittelCount"] {
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
            "/twitch/demo/api/v2/chat-analytics",
            "/twitch/demo/api/v2/monetization",
            "/twitch/demo/api/v2/category-leaderboard",
            "/twitch/demo/api/v2/coaching",
            "/twitch/demo/api/v2/follower-funnel",
            "/twitch/demo/api/v2/viewer-segments",
            "/twitch/demo/api/v2/viewer-directory",
            "/twitch/demo/api/v2/lurker-analysis",
            "/twitch/demo/api/v2/title-performance",
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
    async fn index_setzt_demo_csp() {
        let app = build_demo_router();
        let resp = app
            .oneshot(Request::builder().uri("/twitch/demo").body(Body::empty()).unwrap())
            .await
            .unwrap();
        // Ohne gebauten Dist → 404, aber CSP-Header ist trotzdem gesetzt.
        let csp = resp.headers().get(header::CONTENT_SECURITY_POLICY);
        assert!(csp.is_some(), "Demo-Index muss frame-ancestors-CSP setzen");
        assert!(csp.unwrap().to_str().unwrap().contains("frame-ancestors"));
    }
}
