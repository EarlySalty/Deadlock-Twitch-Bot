//! Composition-Root-Implementierung des [`ScamEnforcePort`].
//!
//! Dünner Wrapper: die eigentliche Enforce-Logik liegt testbar als freie
//! Funktion `tb_chat::conversation_scam::enforce_verdict` in tb-chat.

use std::sync::Arc;

use sqlx::PgPool;
use tb_chat::conversation_scam::enforce_verdict;
use tb_chat::ChatApi;
use tb_internal_api::ScamEnforcePort;

struct ScamEnforceImpl {
    pool: PgPool,
    api: Arc<dyn ChatApi>,
}

#[async_trait::async_trait]
impl ScamEnforcePort for ScamEnforceImpl {
    async fn enforce(&self, verdict_id: i64) -> serde_json::Value {
        let outcome = enforce_verdict(&self.pool, self.api.as_ref(), verdict_id).await;
        serde_json::to_value(outcome).unwrap_or_else(|_| serde_json::json!({"status": "error"}))
    }
}

/// Baut den [`ScamEnforcePort`] aus der gebooteten [`ChatApi`] + Pool.
pub fn build_scam_enforce_port(
    chat_api: Option<Arc<dyn ChatApi>>,
    pool: PgPool,
) -> Option<Arc<dyn ScamEnforcePort>> {
    let api = chat_api?;
    Some(Arc::new(ScamEnforceImpl { pool, api }))
}
