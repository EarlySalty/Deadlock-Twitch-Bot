//! Einheitliche JSON-Fehler-Antworten für die interne API.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

/// Einheitliches Fehler-Payload der internen API.
#[derive(Debug, Serialize)]
pub struct ApiErrorBody {
    pub error: &'static str,
    pub message: &'static str,
}

/// Axum-kompatibler Fehlertyp mit JSON-Serialisierung.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub body: ApiErrorBody,
}

impl ApiError {
    /// 403 Forbidden — nicht-loopback Zugriff.
    pub fn forbidden() -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            body: ApiErrorBody {
                error: "forbidden",
                message: "only loopback connections are allowed",
            },
        }
    }

    /// 401 Unauthorized — fehlendes oder falsches Token.
    pub fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            body: ApiErrorBody {
                error: "unauthorized",
                message: "missing or invalid internal token",
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}
