//! Raid-Ausführung — holt den User-Token, ruft die Twitch-Raid-API und schreibt
//! in jedem Fall die Raid-History. Port von `raid/executor.py` `start_raid`.
//!
//! Die Twitch-API ist als [`RaidApi`]-Port abstrahiert (echte Impl: HelixClient
//! in der Composition-Root), damit der Executor ohne Netz testbar bleibt.

use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::raid_blacklist::RaidBlacklistStore;
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

    async fn start_raid_classified(
        &self, from_id: &str, to_id: &str, token: &str,
    ) -> Result<(), RaidStartError> {
        self.start_raid(from_id, to_id, token).await.map_err(|message| RaidStartError {
            message, definitive_rejection: false,
        })
    }
}

pub struct RaidStartError {
    pub message: String,
    pub definitive_rejection: bool,
}

/// Best-effort-Vorlauf ohne I/O im Raidpfad. Implementierungen dürfen hier
/// niemals auf Datenbank, Werbe-API oder einen Worker warten.
pub trait RaidAdProtection: Send + Sync {
    fn announce(&self, attempt_id: &str, from_id: &str, to_id: &str);
    fn complete(&self, attempt_id: &str, started: Option<bool>);
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
    Started { raid_history_id: Option<i64> },
    Failed(String),
}

pub struct RaidExecutor {
    api: Arc<dyn RaidApi>,
    token_provider: Arc<TokenProvider>,
    history: RaidHistoryStore,
    hard_bans: RaidBlacklistStore,
    ad_protection: Option<Arc<dyn RaidAdProtection>>,
}

impl RaidExecutor {
    pub fn new(
        api: Arc<dyn RaidApi>,
        token_provider: Arc<TokenProvider>,
        history: RaidHistoryStore,
        hard_bans: RaidBlacklistStore,
    ) -> Self {
        Self {
            api,
            token_provider,
            history,
            hard_bans,
            ad_protection: None,
        }
    }

    pub fn with_ad_protection(mut self, protection: Arc<dyn RaidAdProtection>) -> Self {
        self.ad_protection = Some(protection);
        self
    }

    /// Führt einen Raid aus. Schreibt in JEDEM Pfad (kein Token / API-Fehler /
    /// Erfolg) eine History-Zeile — wie Python.
    pub async fn execute(
        &self,
        req: &RaidRequest,
        now: DateTime<Utc>,
    ) -> Result<RaidOutcome, sqlx::Error> {
        match self
            .hard_bans
            .is_hard_banned(Some(&req.to_broadcaster_id), &req.to_broadcaster_login)
            .await
        {
            Ok(true) => {
                tracing::warn!(
                    from = %req.from_broadcaster_login,
                    to = %req.to_broadcaster_login,
                    to_id = %req.to_broadcaster_id,
                    "Raid-Ausführung hart blockiert: Ziel ist global gebannt"
                );
                self.record(req, false, Some("target_hard_banned".to_string()))
                    .await?;
                return Ok(RaidOutcome::Failed("target_hard_banned".to_string()));
            }
            Ok(false) => {}
            Err(error) => {
                tracing::error!(
                    %error,
                    from = %req.from_broadcaster_login,
                    to = %req.to_broadcaster_login,
                    to_id = %req.to_broadcaster_id,
                    "Raid-Ausführung hart blockiert: Global-Ban-Prüfung fehlgeschlagen"
                );
                self.record(req, false, Some("hard_ban_check_failed".to_string()))
                    .await?;
                return Ok(RaidOutcome::Failed("hard_ban_check_failed".to_string()));
            }
        }

        let token = self
            .token_provider
            .get_valid_token(&req.from_broadcaster_id, now)
            .await?;
        let Some(token) = token else {
            let error = format!("No valid token for {}", req.from_broadcaster_login);
            self.record(req, false, Some(error.clone())).await?;
            return Ok(RaidOutcome::Failed(error));
        };

        let attempt_id = tb_crypto::random_urlsafe_token(16);
        if let Some(protection) = &self.ad_protection {
            protection.announce(&attempt_id, &req.from_broadcaster_id, &req.to_broadcaster_id);
        }
        let result = self
            .api
            .start_raid_classified(&req.from_broadcaster_id, &req.to_broadcaster_id, &token)
            .await;
        if let Some(protection) = &self.ad_protection {
            let started = match &result {
                Ok(()) => Some(true),
                Err(error) if error.definitive_rejection => Some(false),
                Err(_) => None,
            };
            protection.complete(&attempt_id, started);
        }
        let result = result.map_err(|error| error.message);
        match result {
            Ok(()) => {
                let raid_history_id = match self.record(req, true, None).await {
                    Ok(id) => Some(id),
                    Err(error) => {
                        tracing::error!(
                            %error,
                            from = %req.from_broadcaster_login,
                            to = %req.to_broadcaster_login,
                            "Raid-History nach erfolgreichem Twitch-Raid nicht schreibbar"
                        );
                        None
                    }
                };
                Ok(RaidOutcome::Started { raid_history_id })
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
    ) -> Result<i64, sqlx::Error> {
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
            .await
    }
}

#[cfg(test)]
mod protection_tests {
    use super::*;

    struct UnknownApi;
    #[async_trait::async_trait]
    impl RaidApi for UnknownApi {
        async fn start_raid(&self, _: &str, _: &str, _: &str) -> Result<(), String> {
            Err("Timeout nach möglichem Versand".into())
        }
    }

    #[tokio::test]
    async fn bestehende_portimplementierung_behaelt_unbekannten_ausgang() {
        let error = UnknownApi.start_raid_classified("1", "2", "test").await.err().unwrap();
        assert!(!error.definitive_rejection);
        assert_eq!(error.message, "Timeout nach möglichem Versand");
    }
}
