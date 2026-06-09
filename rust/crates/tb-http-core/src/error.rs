//! Einheitliche JSON-Fehler-Antworten für die interne API.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use serde_json::Value;

/// Einheitliches Fehler-Payload der internen API (statische Strings).
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
    /// Optionaler dynamischer JSON-Override (überschreibt `body` bei IntoResponse).
    dyn_body: Option<Value>,
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
            dyn_body: None,
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
            dyn_body: None,
        }
    }

    /// 403 Forbidden — kein analytics.extended-Entitlement.
    pub fn plan_required() -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            body: ApiErrorBody {
                error: "plan_required",
                message: "analytics.extended entitlement required",
            },
            dyn_body: None,
        }
    }

    /// 500 Internal Server Error.
    pub fn internal() -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            body: ApiErrorBody {
                error: "internal_error",
                message: "internal server error",
            },
            dyn_body: None,
        }
    }

    /// 404 Not Found.
    pub fn not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            body: ApiErrorBody {
                error: "not_found",
                message: "resource not found",
            },
            dyn_body: None,
        }
    }

    /// 400 Bad Request mit dynamischem JSON-Body.
    pub fn bad_request_with_body(body: serde_json::Value) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            body: ApiErrorBody {
                error: "bad_request",
                message: "bad request",
            },
            dyn_body: Some(body),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if let Some(dyn_body) = self.dyn_body {
            (self.status, Json(dyn_body)).into_response()
        } else {
            (self.status, Json(self.body)).into_response()
        }
    }
}
