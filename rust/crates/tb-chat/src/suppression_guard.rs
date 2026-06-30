//! Zentraler Outbound-Suppression-Guard fuer gezielte Chat-Sends.
//!
//! Der Guard ist ein lokaler [`ChatApi`]-Decorator: Er kennt den Ziel-Login
//! und den Python-`source`-Tag des Sendepfads und bricht `send_message` sowie
//! `send_announcement` vor dem echten Twitch-Call ab, wenn der Partner manuell
//! opt-out gesetzt hat oder die verdrahtete Suppression aktiv ist.

use crate::api::{BanOutcome, ChatApi};
use crate::promos::OutboundSuppressionCheck;
use crate::types::SendOutcome;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::sync::Arc;
use tracing::debug;

/// Prueft den manuellen Partner-Opt-out fuer einen Ziel-Login.
#[async_trait]
pub trait ManualPartnerOptOutCheck: Send + Sync {
    async fn is_manual_partner_opt_out(&self, target_login: &str) -> bool;
}

/// DB-Implementierung fuer [`ManualPartnerOptOutCheck`].
pub struct DbManualPartnerOptOutCheck {
    pool: PgPool,
}

impl DbManualPartnerOptOutCheck {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ManualPartnerOptOutCheck for DbManualPartnerOptOutCheck {
    async fn is_manual_partner_opt_out(&self, target_login: &str) -> bool {
        let login = target_login.trim();
        if login.is_empty() {
            return false;
        }

        let flag = sqlx::query_scalar!(
            "SELECT COALESCE(manual_partner_opt_out, 0) AS \"manual_partner_opt_out!\" \
             FROM twitch_streamers_partner_state \
             WHERE LOWER(twitch_login) = LOWER($1) LIMIT 1",
            login,
        )
        .fetch_optional(&self.pool)
        .await;

        match flag {
            Ok(Some(value)) => value != 0,
            Ok(None) => false,
            Err(error) => {
                debug!(
                    %error,
                    target_login = login,
                    "manual_partner_opt_out konnte fuer Outbound-Chat nicht geprueft werden"
                );
                false
            }
        }
    }
}

/// [`ChatApi`]-Decorator fuer source-/login-gebundene Outbound-Suppression.
pub struct SuppressionGuardChatApi {
    inner: Arc<dyn ChatApi>,
    suppression: Arc<dyn OutboundSuppressionCheck>,
    manual_opt_out: Arc<dyn ManualPartnerOptOutCheck>,
    source: String,
    target_login: String,
}

impl SuppressionGuardChatApi {
    pub fn new(
        inner: Arc<dyn ChatApi>,
        suppression: Arc<dyn OutboundSuppressionCheck>,
        manual_opt_out: Arc<dyn ManualPartnerOptOutCheck>,
        source: impl Into<String>,
        target_login: impl Into<String>,
    ) -> Self {
        Self {
            inner,
            suppression,
            manual_opt_out,
            source: source.into().trim().to_lowercase(),
            target_login: target_login.into().trim().trim_start_matches('#').to_lowercase(),
        }
    }

    pub fn with_pool(
        inner: Arc<dyn ChatApi>,
        suppression: Arc<dyn OutboundSuppressionCheck>,
        pool: PgPool,
        source: impl Into<String>,
        target_login: impl Into<String>,
    ) -> Self {
        Self::new(
            inner,
            suppression,
            Arc::new(DbManualPartnerOptOutCheck::new(pool)),
            source,
            target_login,
        )
    }

    async fn should_skip_send(&self) -> bool {
        if self.target_login.is_empty() {
            return false;
        }

        if self
            .manual_opt_out
            .is_manual_partner_opt_out(&self.target_login)
            .await
        {
            debug!(
                target_login = %self.target_login,
                source = %self.source,
                "Outbound-Chat wegen manual_partner_opt_out unterdrueckt"
            );
            return true;
        }

        if self.source == "promo" && self.suppression.is_muted(&self.target_login).await {
            debug!(
                target_login = %self.target_login,
                source = %self.source,
                "Outbound-Chat wegen aktiver Suppression unterdrueckt"
            );
            return true;
        }

        false
    }
}

#[async_trait]
impl ChatApi for SuppressionGuardChatApi {
    async fn send_message(
        &self,
        broadcaster_id: &str,
        message: &str,
    ) -> Result<SendOutcome, String> {
        if self.should_skip_send().await {
            return Ok(SendOutcome::Dropped {
                code: "outbound_suppressed".to_string(),
                message: String::new(),
            });
        }
        self.inner.send_message(broadcaster_id, message).await
    }

    async fn send_announcement(
        &self,
        broadcaster_id: &str,
        message: &str,
        color: &str,
    ) -> Result<bool, String> {
        if self.should_skip_send().await {
            return Ok(false);
        }
        self.inner
            .send_announcement(broadcaster_id, message, color)
            .await
    }

    async fn ban_user(
        &self,
        broadcaster_id: &str,
        target_user_id: &str,
        reason: &str,
    ) -> Result<BanOutcome, String> {
        self.inner
            .ban_user(broadcaster_id, target_user_id, reason)
            .await
    }

    async fn timeout_user(
        &self,
        broadcaster_id: &str,
        target_user_id: &str,
        duration_secs: u32,
        reason: &str,
    ) -> Result<BanOutcome, String> {
        self.inner
            .timeout_user(broadcaster_id, target_user_id, duration_secs, reason)
            .await
    }

    async fn unban_user(
        &self,
        broadcaster_id: &str,
        target_user_id: &str,
    ) -> Result<bool, String> {
        self.inner.unban_user(broadcaster_id, target_user_id).await
    }

    async fn delete_message(
        &self,
        broadcaster_id: &str,
        message_id: &str,
    ) -> Result<bool, String> {
        self.inner.delete_message(broadcaster_id, message_id).await
    }

    async fn user_created_at(
        &self,
        user_id: &str,
    ) -> Result<Option<DateTime<Utc>>, String> {
        self.inner.user_created_at(user_id).await
    }

    async fn resolve_user_id(&self, login: &str) -> Result<Option<String>, String> {
        self.inner.resolve_user_id(login).await
    }

    async fn bot_user_id(&self) -> String {
        self.inner.bot_user_id().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockApi {
        message_calls: AtomicUsize,
        announcement_calls: AtomicUsize,
    }

    impl MockApi {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                message_calls: AtomicUsize::new(0),
                announcement_calls: AtomicUsize::new(0),
            })
        }
    }

    #[async_trait]
    impl ChatApi for MockApi {
        async fn send_message(&self, _b: &str, _m: &str) -> Result<SendOutcome, String> {
            self.message_calls.fetch_add(1, Ordering::SeqCst);
            Ok(SendOutcome::Sent)
        }

        async fn send_announcement(&self, _b: &str, _m: &str, _c: &str) -> Result<bool, String> {
            self.announcement_calls.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        }

        async fn ban_user(&self, _b: &str, _u: &str, _r: &str) -> Result<BanOutcome, String> {
            unimplemented!()
        }

        async fn timeout_user(
            &self,
            _b: &str,
            _u: &str,
            _d: u32,
            _r: &str,
        ) -> Result<BanOutcome, String> {
            unimplemented!()
        }

        async fn unban_user(&self, _b: &str, _u: &str) -> Result<bool, String> {
            unimplemented!()
        }

        async fn delete_message(&self, _b: &str, _m: &str) -> Result<bool, String> {
            unimplemented!()
        }

        async fn user_created_at(&self, _u: &str) -> Result<Option<DateTime<Utc>>, String> {
            unimplemented!()
        }

        async fn resolve_user_id(&self, _l: &str) -> Result<Option<String>, String> {
            unimplemented!()
        }

        async fn bot_user_id(&self) -> String {
            "bot".to_string()
        }
    }

    struct FixedManualOptOut(bool);

    #[async_trait]
    impl ManualPartnerOptOutCheck for FixedManualOptOut {
        async fn is_manual_partner_opt_out(&self, _target_login: &str) -> bool {
            self.0
        }
    }

    struct FixedSuppression(bool);

    #[async_trait]
    impl OutboundSuppressionCheck for FixedSuppression {
        async fn is_muted(&self, _channel_login: &str) -> bool {
            self.0
        }
    }

    fn guard(
        inner: Arc<MockApi>,
        opt_out: bool,
        muted: bool,
    ) -> SuppressionGuardChatApi {
        SuppressionGuardChatApi::new(
            inner,
            Arc::new(FixedSuppression(muted)),
            Arc::new(FixedManualOptOut(opt_out)),
            "promo",
            "Kanal",
        )
    }

    #[tokio::test]
    async fn suppressed_message_wird_nicht_gesendet() {
        let inner = MockApi::new();
        let api = guard(Arc::clone(&inner), true, false);

        let outcome = api.send_message("bid", "text").await;

        assert!(matches!(outcome, Ok(SendOutcome::Dropped { ref code, .. }) if code == "outbound_suppressed"));
        assert_eq!(inner.message_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn suppressed_announcement_wird_nicht_gesendet() {
        let inner = MockApi::new();
        let api = guard(Arc::clone(&inner), false, true);

        let sent = api.send_announcement("bid", "text", "purple").await;

        assert_eq!(sent, Ok(false));
        assert_eq!(inner.announcement_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn allowed_message_und_announcement_delegieren() {
        let inner = MockApi::new();
        let api = guard(Arc::clone(&inner), false, false);

        let message = api.send_message("bid", "text").await;
        let announcement = api.send_announcement("bid", "text", "purple").await;

        assert_eq!(message, Ok(SendOutcome::Sent));
        assert_eq!(announcement, Ok(true));
        assert_eq!(inner.message_calls.load(Ordering::SeqCst), 1);
        assert_eq!(inner.announcement_calls.load(Ordering::SeqCst), 1);
    }
}
