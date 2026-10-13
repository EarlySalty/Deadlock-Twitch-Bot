//! Admin-Bulk-Re-Auth (`POST /raid/reauth-all`) — P3.7.
//!
//! Port der **SQL-Operation** aus `bot/raid/commands.py:459-572` (`cmd_reauth_all`)
//! — **ohne** die Discord-DM-Schleife (B10-Ausschluss, kein Discord-DM in Rust).
//! Der Handler ruft die Bulk-Primitive [`BulkReauthPort::snapshot_and_flag_reauth`]
//! (tb-raid P2.34) auf, die `needs_reauth=TRUE` für alle token-tragenden
//! Streamer in einem Schwung setzt. Antwort `{ok:true, flagged:<count>}`.
//!
//! Folgt dem `raid_oauth`-Muster: `tb-internal-api` hängt bewusst NICHT an
//! `tb-raid`, daher ist die Primitive hier über einen Port-Trait abstrahiert; die
//! Impl (`tb_raid::ReauthAdminStore`) wird in der Composition-Root (`tb-bot`)
//! injiziert. `None` (Port nicht verdrahtet) → 503.

use std::sync::Arc;

use axum::{response::IntoResponse, Extension, Json};
use serde::Serialize;
use tb_http_core::{ApiError, AuthLevel};

/// Abstraktion über die tb-raid-Bulk-Re-Auth-Primitive (P2.34).
/// Implementierung in `rust/bin/tb-bot` (Composition-Root) durch
/// `tb_raid::ReauthAdminStore`.
#[async_trait::async_trait]
pub trait BulkReauthPort: Send + Sync {
    /// Setzt `needs_reauth=TRUE` für alle token-tragenden Streamer; liefert die
    /// Anzahl betroffener Zeilen. Fehler → der Handler antwortet 500.
    async fn snapshot_and_flag_reauth(&self) -> Result<u64, String>;
}

/// Extension-Wrapper für den Router.
/// `None` = Bulk-Re-Auth-Stack nicht verdrahtet → der Handler antwortet 503.
#[derive(Clone)]
pub struct BulkReauthExt(pub Option<Arc<dyn BulkReauthPort>>);

#[derive(Serialize)]
pub struct ReauthAllResponse {
    pub ok: bool,
    pub flagged: u64,
}

/// `POST /internal/twitch/v1/raid/reauth-all`
///
/// Admin-only. Flaggt alle token-tragenden Streamer zur Neu-Autorisierung.
/// **Kein Discord-DM** (im Gegensatz zum Python-Befehl).
pub async fn reauth_all_handler(
    auth: AuthLevel,
    Extension(port): Extension<BulkReauthExt>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    let Some(port) = port.0 else {
        return Err(ApiError::unavailable());
    };

    let flagged = port.snapshot_and_flag_reauth().await.map_err(|e| {
        tracing::error!("snapshot_and_flag_reauth DB-Fehler: {e}");
        ApiError::internal()
    })?;

    tracing::info!(
        flagged,
        "reauth-all: {flagged} Streamer zur Neu-Autorisierung geflaggt"
    );
    Ok(Json(ReauthAllResponse { ok: true, flagged }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    struct StubPort {
        result: Result<u64, String>,
    }

    #[async_trait::async_trait]
    impl BulkReauthPort for StubPort {
        async fn snapshot_and_flag_reauth(&self) -> Result<u64, String> {
            self.result.clone()
        }
    }

    #[tokio::test]
    async fn admin_call_flags_and_returns_count() {
        let port: Arc<dyn BulkReauthPort> = Arc::new(StubPort { result: Ok(7) });
        let resp = match reauth_all_handler(AuthLevel::Admin, Extension(BulkReauthExt(Some(port))))
            .await
        {
            Ok(r) => r.into_response(),
            Err(_) => panic!("admin call should succeed"),
        };
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["ok"], true);
        assert_eq!(json["flagged"], 7);
    }

    #[tokio::test]
    async fn non_admin_is_unauthorized() {
        let port: Arc<dyn BulkReauthPort> = Arc::new(StubPort { result: Ok(7) });
        let status =
            match reauth_all_handler(AuthLevel::None, Extension(BulkReauthExt(Some(port)))).await {
                Ok(_) => panic!("non-admin should be denied"),
                Err(e) => e.into_response().status(),
            };
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn missing_port_is_unavailable() {
        let status =
            match reauth_all_handler(AuthLevel::Admin, Extension(BulkReauthExt(None))).await {
                Ok(_) => panic!("missing port should be 503"),
                Err(e) => e.into_response().status(),
            };
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }
}
