//! Handler für `GET /twitch/raid/history`.
//!
//! Admin-only: gibt 401 zurück wenn [`DashboardAuthLevel`] nicht privileged
//! (Localhost/Admin) ist. Rendert die Python-kompatible Raid-History-HTML-Seite
//! aus `bot/dashboard/raids/pages.py`, aber mit dem nativen Daten-Layer
//! [`tb_analytics::raid_history::load_raid_history`]. Die Query-Parameter
//! `from`/`from_broadcaster` (Login-Filter) und `limit` (Default 50, im Loader
//! auf 1..=500 geklemmt) werden durchgereicht.

use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::Value;
use sqlx::PgPool;
use tb_http_core::ApiError;

use crate::auth::level::DashboardAuthLevel;

#[derive(Debug, Deserialize)]
pub struct RaidHistoryQuery {
    /// Login des Quell-Broadcasters (`from` ist die Kurzform aus dem Frontend).
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub from_broadcaster: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

/// `GET /twitch/raid/history`
pub async fn raid_history_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<RaidHistoryQuery>,
) -> Result<Response, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    // `from` und `from_broadcaster` sind Aliasse; der erste nicht-leere gewinnt.
    let from = params
        .from
        .as_deref()
        .or(params.from_broadcaster.as_deref());
    let rows = tb_analytics::raid_history::load_raid_history(&pool, from, params.limit)
        .await
        .map_err(|_| ApiError::internal())?;
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        render_raid_history_page(&rows),
    )
        .into_response())
}

fn render_raid_history_page(rows: &[Value]) -> String {
    format!(
        concat!(
            "<!doctype html><html lang=\"de\"><head><meta charset=\"utf-8\">",
            "<title>Raid History</title><style>",
            "body {{ font-family: sans-serif; margin: 32px; }}",
            "table {{ border-collapse: collapse; width: 100%; }}",
            "th, td {{ border: 1px solid #ddd; padding: 12px 10px; text-align: left; }}",
            "th {{ background-color: #9146FF; color: white; }}",
            "tr:nth-child(even) {{ background-color: #f2f2f2; }}",
            "</style></head><body>",
            "<h1>Raid History</h1>",
            "<p><a href=\"/twitch/admin\">Zurueck zum Dashboard</a></p>",
            "<table><thead><tr>",
            "<th>Status</th><th>Zeitpunkt</th><th>Von</th><th>Nach</th>",
            "<th>Viewer</th><th>Stream-Dauer</th><th>Kandidaten</th><th>Fehler</th>",
            "</tr></thead><tbody>{}</tbody></table></body></html>"
        ),
        render_raid_history_rows(rows)
    )
}

fn render_raid_history_rows(rows: &[Value]) -> String {
    if rows.is_empty() {
        return "<tr><td colspan=\"8\">Keine Raids gefunden</td></tr>".to_string();
    }

    rows.iter()
        .map(|entry| {
            let success_icon = if bool_field(entry, "success") {
                "OK"
            } else {
                "X"
            };
            let executed_at = string_field(entry, "executedAt")
                .chars()
                .take(19)
                .collect::<String>();
            let from = string_field(entry, "fromBroadcasterLogin");
            let to = string_field(entry, "toBroadcasterLogin");
            let viewer_count = int_field(entry, "viewerCount");
            let stream_duration_min = int_field(entry, "streamDurationSec") / 60;
            let candidates_count = int_field(entry, "candidatesCount");
            let error = string_field(entry, "errorMessage");

            format!(
                concat!(
                    "<tr>",
                    "<td>{}</td>",
                    "<td>{}</td>",
                    "<td><strong>{}</strong></td>",
                    "<td><strong>{}</strong></td>",
                    "<td>{}</td>",
                    "<td>{} min</td>",
                    "<td>{}</td>",
                    "<td style=\"color: red; font-size: 0.85em;\">{}</td>",
                    "</tr>"
                ),
                html_escape(success_icon),
                html_escape(&executed_at),
                html_escape(&from),
                html_escape(&to),
                viewer_count,
                stream_duration_min,
                candidates_count,
                html_escape(&error)
            )
        })
        .collect::<Vec<_>>()
        .join("")
}

fn string_field(entry: &Value, key: &str) -> String {
    entry
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn int_field(entry: &Value, key: &str) -> i64 {
    entry.get(key).and_then(Value::as_i64).unwrap_or(0)
}

fn bool_field(entry: &Value, key: &str) -> bool {
    entry.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn html_escape(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}
