//! Raid-Ausführung — holt den User-Token, ruft die Twitch-Raid-API und schreibt
//! in jedem Fall die Raid-History. Port von `raid/executor.py` `start_raid`.
//!
//! Die Twitch-API ist als [`RaidApi`]-Port abstrahiert (echte Impl: HelixClient
//! in der Composition-Root), damit der Executor ohne Netz testbar bleibt.

use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::raid_history_store::{RaidHistoryStore, RecordRaidInput};
use crate::token_provider::TokenProvider;

/// Port zur Twitch-Raid-API. `Ok(())` = Raid gestartet; `Err(msg)` = API-/
/// Netzfehler mit Meldung (landet in der History).
#[async_trait::async_trait]
pub trait RaidApi: Send + Sync {
    async fn start_raid(
        &self,
        from_broadcaster_id: &str,
        to_broadcaster_id: &str,
        user_token: &str,
    ) -> Result<(), String>;
}

/// Eingabe für einen Raid-Versuch.
#[derive(Debug, Clone)]
pub struct RaidRequest {
    pub from_broadcaster_id: String,
    pub from_broadcaster_login: String,
    pub to_broadcaster_id: String,
    pub to_broadcaster_login: String,
    pub viewer_count: i32,
    pub stream_duration_sec: i32,
    pub target_stream_started_at: Option<DateTime<Utc>>,
    pub candidates_count: i32,
    pub reason: String,
}

/// Ergebnis eines Raid-Versuchs (Python: `(success, error_message)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RaidOutcome {
    Started,
    Failed(String),
}

pub struct RaidExecutor {
    api: Arc<dyn RaidApi>,
    token_provider: Arc<TokenProvider>,
    history: RaidHistoryStore,
}

impl RaidExecutor {
    pub fn new(
        api: Arc<dyn RaidApi>,
        token_provider: Arc<TokenProvider>,
        history: RaidHistoryStore,
    ) -> Self {
        Self {
            api,
            token_provider,
            history,
        }
    }

    /// Führt einen Raid aus. Schreibt in JEDEM Pfad (kein Token / API-Fehler /
    /// Erfolg) eine History-Zeile — wie Python.
    pub async fn execute(
        &self,
        req: &RaidRequest,
        now: DateTime<Utc>,
    ) -> Result<RaidOutcome, sqlx::Error> {
        let token = self
            .token_provider
            .get_valid_token(&req.from_broadcaster_id, now)
            .await?;
        let Some(token) = token else {
            let error = format!("No valid token for {}", req.from_broadcaster_login);
            self.record(req, false, Some(error.clone())).await?;
            return Ok(RaidOutcome::Failed(error));
        };

        match self
            .api
            .start_raid(&req.from_broadcaster_id, &req.to_broadcaster_id, &token)
            .await
        {
            Ok(()) => {
                self.record(req, true, None).await?;
                Ok(RaidOutcome::Started)
            }
            Err(error) => {
                self.record(req, false, Some(error.clone())).await?;
                Ok(RaidOutcome::Failed(error))
            }
        }
    }

    async fn record(
        &self,
        req: &RaidRequest,
        success: bool,
        error_message: Option<String>,
    ) -> Result<(), sqlx::Error> {
        self.history
            .record_raid(&RecordRaidInput {
                from_broadcaster_id: req.from_broadcaster_id.clone(),
                from_broadcaster_login: req.from_broadcaster_login.clone(),
                to_broadcaster_id: req.to_broadcaster_id.clone(),
                to_broadcaster_login: req.to_broadcaster_login.clone(),
                viewer_count: req.viewer_count,
                stream_duration_sec: req.stream_duration_sec,
                reason: Some(req.reason.clone()),
                success,
                error_message,
                target_stream_started_at: req.target_stream_started_at,
                candidates_count: req.candidates_count,
            })
            .await?;
        Ok(())
    }
}
