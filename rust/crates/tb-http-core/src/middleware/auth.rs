//! Interne Auth-Middleware: prüft den X-Internal-Token-Header (constant-time).

use crate::constants::INTERNAL_TOKEN_HEADER;
use crate::error::ApiError;
use axum::{
    extract::State,
    http::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};

/// Axum-Middleware: prüft `X-Internal-Token` gegen den konfigurierten Token.
///
/// Leeres konfiguriertes Token → fail-closed (immer 401).
/// Vergleich via constant-time-Funktion (`subtle`-frei: direkte Byte-Iteration).
pub async fn internal_auth(
    State(expected_token): State<String>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    // Leeres konfiguriertes Token → immer 401 (fail-closed)
    if expected_token.is_empty() {
        return ApiError::unauthorized().into_response();
    }

    let provided = req
        .headers()
        .get(INTERNAL_TOKEN_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !constant_time_eq(provided.as_bytes(), expected_token.as_bytes()) {
        return ApiError::unauthorized().into_response();
    }

    next.run(req).await
}

/// Constant-time Byte-Vergleich (verhindert Timing-Angriffe).
///
/// Gibt `true` zurück, wenn beide Slices identisch sind.
/// Laufzeit ist proportional zur Länge von `expected`, unabhängig vom Mismatch.
fn constant_time_eq(provided: &[u8], expected: &[u8]) -> bool {
    if provided.len() != expected.len() {
        // Längenunterschied ist selbst keine Information, die Timing enthüllt —
        // early-return hier ist akzeptabel, da der Angreifer die Tokenlänge kennt.
        return false;
    }
    let mut diff: u8 = 0;
    for (a, b) in provided.iter().zip(expected.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gleiche_werte_sind_gleich() {
        assert!(constant_time_eq(b"abc", b"abc"));
    }

    #[test]
    fn unterschiedliche_werte_sind_ungleich() {
        assert!(!constant_time_eq(b"abc", b"abd"));
    }

    #[test]
    fn unterschiedliche_laengen_sind_ungleich() {
        assert!(!constant_time_eq(b"ab", b"abc"));
    }

    #[test]
    fn leere_strings_sind_gleich() {
        assert!(constant_time_eq(b"", b""));
    }
}
