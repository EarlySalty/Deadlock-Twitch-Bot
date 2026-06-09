//! Extrahiert den `Idempotency-Key`-Header als typisierter axum-Extraktor.

use crate::constants::IDEMPOTENCY_KEY_HEADER;
use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};

/// Optionaler Idempotency-Key aus dem `Idempotency-Key`-Header.
///
/// Ist der Header nicht vorhanden oder nicht valides UTF-8, gibt `None` zurück.
#[derive(Debug, Clone)]
pub struct IdempotencyKey(pub Option<String>);

#[async_trait]
impl<S: Send + Sync> FromRequestParts<S> for IdempotencyKey {
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let key = parts
            .headers
            .get(IDEMPOTENCY_KEY_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        Ok(IdempotencyKey(key))
    }
}
