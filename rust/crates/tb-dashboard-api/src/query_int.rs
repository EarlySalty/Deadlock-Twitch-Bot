//! Python-konformes Parsen beschränkter Query-Integer (`days`, `months`, `limit`).
//!
//! Port von `_parse_bounded_query_int` aus `bot/analytics/api_insights.py` /
//! `bot/analytics/api_performance.py`:
//!
//! ```python
//! raw = (request.query.get(name, str(default)) or str(default)).strip()
//! try:
//!     parsed = int(raw)
//! except (TypeError, ValueError):
//!     raise ValueError(f"{name} must be an integer")
//! return min(max(parsed, minimum), maximum)
//! ```
//!
//! Kontrakt (deshalb dieses Modul statt `Option<i32>` im Query-Struct):
//! Ein nicht-numerischer Wert ergibt in Python eine **400 mit JSON-Body**
//! `{"error": "<name> must be an integer"}`. axum würde bei `Option<i32>` und
//! `days=abc` die `serde_urlencoded`-Deserialisierung scheitern lassen und einen
//! generischen Plaintext-400 (`Failed to deserialize query string...`) liefern —
//! falsche Form. Darum tragen die betroffenen Handler den Rohwert als
//! `Option<String>` und parsen hier.

use axum::{http::StatusCode, Json};
use serde_json::{json, Value};

/// 400-Fehler in der Python-Form `{"error": "<name> must be an integer"}`.
/// Kleines Tupel statt `Response` als `Err`-Typ (vermeidet `result_large_err`,
/// folgt dem Crate-Idiom z. B. in `performance::require_auth`). `.into_response()`
/// am Call-Site liefert die fertige `axum::response::Response`.
pub type QueryIntError = (StatusCode, Json<Value>);

/// Parst einen beschränkten Query-Integer Python-konform.
///
/// `raw` ist der rohe (noch nicht getrimmte) Query-Wert; `None` bzw. nur
/// Whitespace ⇒ `default`. Geparster Wert wird auf `[minimum, maximum]` geklemmt.
/// Bei nicht-numerischem Wert ⇒ `Err` mit dem Python-identischen 400-JSON-Body.
pub fn parse_bounded_query_int(
    raw: Option<&str>,
    name: &str,
    default: i64,
    minimum: i64,
    maximum: i64,
) -> Result<i64, QueryIntError> {
    let trimmed = raw.map(str::trim).unwrap_or("");
    let parsed = if trimmed.is_empty() {
        default
    } else {
        trimmed.parse::<i64>().map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("{name} must be an integer") })),
            )
        })?
    };
    Ok(parsed.clamp(minimum, maximum))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fehlend_ergibt_default() {
        assert_eq!(parse_bounded_query_int(None, "days", 30, 7, 365).unwrap(), 30);
    }

    #[test]
    fn leer_und_whitespace_ergibt_default() {
        assert_eq!(parse_bounded_query_int(Some(""), "days", 30, 7, 365).unwrap(), 30);
        assert_eq!(parse_bounded_query_int(Some("   "), "days", 30, 7, 365).unwrap(), 30);
    }

    #[test]
    fn numerisch_wird_geparst_und_getrimmt() {
        assert_eq!(parse_bounded_query_int(Some(" 90 "), "days", 30, 7, 365).unwrap(), 90);
    }

    #[test]
    fn out_of_range_wird_geklemmt_nicht_400() {
        // Python: min(max(parsed, minimum), maximum) — KEIN Fehler.
        assert_eq!(parse_bounded_query_int(Some("1"), "days", 30, 7, 365).unwrap(), 7);
        assert_eq!(parse_bounded_query_int(Some("9999"), "days", 30, 7, 365).unwrap(), 365);
    }

    #[test]
    fn nicht_numerisch_ergibt_python_konformes_400_json() {
        let (status, Json(body)) =
            parse_bounded_query_int(Some("abc"), "days", 30, 7, 365).unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, json!({ "error": "days must be an integer" }));
    }

    #[test]
    fn fehlername_steckt_in_der_meldung() {
        let (_, Json(body)) =
            parse_bounded_query_int(Some("x"), "months", 12, 1, 24).unwrap_err();
        assert_eq!(body, json!({ "error": "months must be an integer" }));
    }
}
