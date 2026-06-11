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

    /// 403 Forbidden — Discord-Scope-Guard hat eine ID außerhalb der
    /// Allowlist abgewiesen. Message-Parität zu Pythons
    /// `_safe_exception_error(error="forbidden", status=403,
    /// message="action outside configured scope")` (`raid.py`).
    pub fn forbidden_scope() -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            body: ApiErrorBody {
                error: "forbidden",
                message: "action outside configured scope",
            },
            dyn_body: None,
        }
    }

    /// 403 Forbidden — generische Ablehnung mit Python-Kurz-Message
    /// (`raid.py:53-57`: `error="forbidden", message="forbidden"`,
    /// z. B. auth-url für einen blockierten Login).
    pub fn forbidden_generic() -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            body: ApiErrorBody {
                error: "forbidden",
                message: "forbidden",
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

    /// 404 Not Found mit routenspezifischer Nachricht — Parität zu Pythons
    /// `_json_error("not_found", 404, <message>)`.
    pub fn not_found_with(message: &'static str) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            body: ApiErrorBody {
                error: "not_found",
                message,
            },
            dyn_body: None,
        }
    }

    /// 503 Service Unavailable — Upstream/Abhängigkeit nicht bereit.
    pub fn unavailable() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            body: ApiErrorBody {
                error: "upstream_unavailable",
                message: "upstream unavailable",
            },
            dyn_body: None,
        }
    }

    /// 400 Bad Request mit `error="bad_request"` und gegebener Nachricht.
    /// Rendert `{"error":"bad_request","message":<message>}` — Parität zu
    /// Pythons `_json_error("bad_request", 400, message)`.
    pub fn bad_request(message: &'static str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            body: ApiErrorBody {
                error: "bad_request",
                message,
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

    /// Effektiver JSON-Body als `Value` — was `IntoResponse` rendern würde.
    /// Für Layer, die den Fehler weiterreichen müssen (z. B. Idempotenz-Waiter).
    pub fn payload_json(&self) -> serde_json::Value {
        match &self.dyn_body {
            Some(v) => v.clone(),
            None => serde_json::json!({
                "error": self.body.error,
                "message": self.body.message,
            }),
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
