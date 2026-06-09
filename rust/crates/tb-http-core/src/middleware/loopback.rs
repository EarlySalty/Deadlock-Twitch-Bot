//! Loopback-Middleware: blockiert alle Requests, die nicht von 127.x.x.x kommen.

use crate::error::ApiError;
use axum::{
    extract::ConnectInfo,
    http::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::net::SocketAddr;

/// Axum-Middleware: lässt nur Loopback-Verbindungen (127.x.x.x) durch.
///
/// Muss in der Middleware-Kette vor der Auth-Prüfung stehen.
pub async fn loopback_only(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    if !addr.ip().is_loopback() {
        return ApiError::forbidden().into_response();
    }
    next.run(req).await
}
