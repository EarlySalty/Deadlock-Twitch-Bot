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
pub async fn demo_streamers() -> Response {
    Json(json!([
        { "login": "smallquest_tv", "isPartner": true },
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

/// `GET|POST /twitch/demo/api/v2/ai/analysis` — Demo-KI-Coach-Text.
pub async fn demo_ai_analysis() -> Response {
    Json(json!({
        "streamer": DEMO_LOGIN,
        "model": "demo",
        "generatedAt": "2026-06-13T10:00:00Z",
        "summary": "Deine letzten Streams zeigen konstante Reichweite und eine \
                    überdurchschnittliche Frühretention. Größter Hebel: Titel- und \
                    Kategorie-Timing rund um Patch-Tage.",
        "sections": [
            { "title": "Was gut läuft", "items": [
                "Stabile Average-Viewers über 14 Streams",
                "Chat-Engagement deutlich über Median"
            ]},
            { "title": "Nächste Schritte", "items": [
                "Patch-Keywords in den ersten 30 Minuten testen",
                "Raids gezielt an aktive Partner ausspielen"
            ]}
        ]
    }))
    .into_response()
}

/// `GET /twitch/demo/api/v2/ai/history` — Demo-Verlauf vergangener Analysen.
pub async fn demo_ai_history() -> Response {
    Json(json!([
        { "id": 2, "createdAt": "2026-06-13T10:00:00Z", "model": "demo",
          "summary": "Frühretention stark, Titel-Timing als Hebel." },
        { "id": 1, "createdAt": "2026-06-06T10:00:00Z", "model": "demo",
          "summary": "Wachstum stabil, Raids weiter ausbauen." }
    ]))
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
