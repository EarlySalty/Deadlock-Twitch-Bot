//! KI-Schalter pro Discord-Server im Dashboard (Multi-Server-Plan, Paket K,
//! E14): Dashboard-KI (KI-Analyse, Folgechat, Assistent, Chat-Tiefenanalyse,
//! Titel) läuft nur für Streamer, deren Server die KI an hat.
//!
//! Durchgesetzt wird der Schalter im LLM-Eingang (`tb_llm::ai_guard`); hier
//! bekommt das Dashboard nur eine klare Antwort, bevor Daten gesammelt und
//! Sperren genommen werden.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// Fehlercode, an dem das Frontend die Sperre erkennt.
pub const AI_DISABLED_CODE: &str = "ai_disabled";

/// `Some(403)`, wenn der Server des Streamers die KI aus hat. Ist der
/// Schalter nicht lesbar, geht es weiter; der LLM-Eingang sperrt dann selbst.
pub async fn blocked(
    twitch_user_id: Option<&str>,
    channel_login: Option<&str>,
) -> Option<Response> {
    match tb_llm::streamer_ai_allowed(twitch_user_id, channel_login).await {
        Ok(false) => Some(
            (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "KI-Funktionen sind für den Server dieses Streamers ausgeschaltet.",
                    "code": AI_DISABLED_CODE,
                })),
            )
                .into_response(),
        ),
        _ => None,
    }
}
