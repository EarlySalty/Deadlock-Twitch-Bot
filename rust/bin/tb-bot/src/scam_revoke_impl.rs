//! Composition-Root-Implementierung des [`ScamRevokePort`].
//!
//! Dünner Wrapper: die eigentliche Revoke-Logik (Urteil laden → ggf. Twitch-
//! Unban → als `overturned` markieren) liegt testbar als freie Funktion
//! `tb_chat::conversation_scam::revoke_verdict` in tb-chat. Hier wird sie nur
//! mit der live gebooteten [`ChatApi`] (Bot-User-Token) und dem Pool verdrahtet
//! und das Ergebnis als JSON für die interne API serialisiert.

use std::sync::Arc;

use sqlx::PgPool;
use tb_chat::conversation_scam::revoke_verdict;
use tb_chat::ChatApi;
use tb_internal_api::ScamRevokePort;

struct ScamRevokeImpl {
    pool: PgPool,
    api: Arc<dyn ChatApi>,
}

#[async_trait::async_trait]
impl ScamRevokePort for ScamRevokeImpl {
    async fn revoke(&self, verdict_id: i64) -> serde_json::Value {
        let outcome = revoke_verdict(&self.pool, self.api.as_ref(), verdict_id).await;
        serde_json::to_value(outcome)
            .unwrap_or_else(|_| serde_json::json!({"status": "error"}))
    }
}

/// Baut den [`ScamRevokePort`] aus der gebooteten [`ChatApi`] + Pool. `None`,
/// wenn der native Chat aus ist (kein Bot-Token gebootet) → der Handler
/// antwortet dann 503 statt stumm zu scheitern.
pub fn build_scam_revoke_port(
    chat_api: Option<Arc<dyn ChatApi>>,
    pool: PgPool,
) -> Option<Arc<dyn ScamRevokePort>> {
    let api = chat_api?;
    Some(Arc::new(ScamRevokeImpl { pool, api }))
}
