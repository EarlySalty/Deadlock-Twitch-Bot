//! TimeoutTracking — verdrahtet den [`TimeoutGuard`] an den ausgehenden
//! Sende-Pfad und an die Promo-Suppression.
//!
//! # Hintergrund
//!
//! Wird der Bot in einem Kanal getimed outed oder gebannt, verwirft Twitch die
//! ausgehende Nachricht serverseitig (`POST /chat/messages` → HTTP 200 mit
//! `is_sent=false` + `drop_reason.code` ∈ {`sender_banned`, `sender_timedout`}).
//! Python wertet genau diesen Fall in `moderation.py:1519–1546` aus und ruft
//! `get_timeout_guard().record_timeout(login)`. Nach 2 Drops/Tag bzw. 5/Woche
//! schaltet der Guard den Bot für 7 Tage stumm.
//!
//! Native bündelt das in zwei Bausteinen:
//!
//! - [`TimeoutTrackingChatApi`] — ein [`ChatApi`]-Decorator, der **nur**
//!   `send_message` instrumentiert: bei einem Bot-Timeout-Drop wird die
//!   `broadcaster_id` → `login` aufgelöst und `record_timeout` gerufen. Alle
//!   übrigen 8 Trait-Methoden delegieren unverändert.
//! - [`CombinedSuppression`] — kombiniert die bestehende DB-Suppression
//!   ([`OutboundSuppressionStore`]) mit dem In-Memory-Guard. Der Promo-Pfad
//!   prüft so beide Quellen (Python: `_send_promo_message` prüft erst
//!   `timeout_guard.is_muted`, promos.py:1137, und zusätzlich die DB-Suppression).
//!
//! Port: `bot/chat/moderation.py:1519–1546`, `bot/chat/promos.py:1132–1137`,
//! `bot/chat/timeout_guard.py`.

use crate::api::{AnnouncementOutcome, BanOutcome, ChatApi};
use crate::moderation::{TimeoutGuard, BOT_TIMEOUT_DROP_CODES};
use crate::types::SendOutcome;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::sync::Arc;
use tracing::debug;

// ---------------------------------------------------------------------------
// Reine Hilfsfunktion (ohne DB testbar)
// ---------------------------------------------------------------------------

/// Liefert den Drop-Code, falls `outcome` ein **Bot-Timeout-Drop** ist
/// (`SendOutcome::Dropped` mit `code` ∈ [`BOT_TIMEOUT_DROP_CODES`]).
///
/// `Sent`, `HttpError` und andere Drop-Codes (z. B. `channel_settings`)
/// liefern `None` → kein `record_timeout`.
///
/// Port: `moderation.py:1535` (`if drop_code in _BOT_TIMEOUT_DROP_CODES`).
pub fn is_bot_timeout_drop(outcome: &SendOutcome) -> Option<&str> {
    match outcome {
        SendOutcome::Dropped { code, .. } if BOT_TIMEOUT_DROP_CODES.contains(&code.as_str()) => {
            Some(code.as_str())
        }
        _ => None,
    }
}

/// Interner Grund fuer einen Kanal-seitigen Bot-Ban.
///
/// `sender_timedout` bleibt bewusst TimeoutGuard-only; der Bot-Ban-Lifecycle
/// reagiert nur auf `sender_banned` oder HTTP-Fehlerkoerper, die klar nach
/// Kanal-Ban aussehen.
pub fn bot_banned_reason(outcome: &SendOutcome) -> Option<String> {
    match outcome {
        SendOutcome::Dropped { code, message } if code == "sender_banned" => {
            Some(reason_with_detail("chat_bot_banned_in_channel", message))
        }
        SendOutcome::HttpError { status, body } if looks_like_bot_banned_error(*status, body) => {
            Some(reason_with_detail(
                &format!("chat_bot_banned_in_channel_http_{status}"),
                body,
            ))
        }
        _ => None,
    }
}

fn looks_like_bot_banned_error(status: u16, text: &str) -> bool {
    let lowered = text.to_lowercase();
    if lowered.contains("user is banned") || lowered.contains("sender is banned") {
        return true;
    }
    matches!(status, 400 | 401 | 403) && lowered.contains("ban")
}

fn reason_with_detail(code: &str, detail: &str) -> String {
    let compact = detail.replace('\n', " ");
    let trimmed = compact.trim();
    if trimmed.is_empty() {
        return code.to_string();
    }
    let snippet: String = trimmed.chars().take(180).collect();
    format!("{code}: {snippet}")
}

// ---------------------------------------------------------------------------
// TimeoutTrackingChatApi — ChatApi-Decorator
// ---------------------------------------------------------------------------

/// Signal aus `tb-chat`, wenn Twitch meldet, dass der Bot in einem Kanal
/// gebannt ist. Die Reaktion gehoert in den Composition-Root (`tb-bot`) und
/// weiter in `tb-raid`; `tb-chat` kennt diese Crate bewusst nicht.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotBannedChannelSignal {
    pub broadcaster_id: String,
    pub broadcaster_login: String,
    pub reason: String,
}

/// Port fuer den Bot-Ban-Lifecycle. Best-effort: Implementierungen duerfen
/// Fehler loggen und sollen den Chat-Sendepfad nicht abbrechen.
#[async_trait]
pub trait BotBannedChannelHandler: Send + Sync {
    async fn on_bot_banned_channel(&self, signal: BotBannedChannelSignal);
}

/// [`ChatApi`]-Decorator, der ausgehende Bot-Timeouts an den [`TimeoutGuard`]
/// meldet.
///
/// Nur [`TimeoutTrackingChatApi::send_message`] ist instrumentiert; alle
/// anderen Methoden delegieren unverändert an `inner`. Das Original-Ergebnis
/// wird in jedem Fall unverändert zurückgegeben — die Tracking-Logik ist ein
/// reiner Seiteneffekt.
///
/// Port: `moderation.py:1519–1546`.
pub struct TimeoutTrackingChatApi {
    inner: Arc<dyn ChatApi>,
    guard: Arc<TimeoutGuard>,
    pool: PgPool,
    bot_ban_handler: Option<Arc<dyn BotBannedChannelHandler>>,
}

impl TimeoutTrackingChatApi {
    /// Erstellt einen neuen Decorator.
    pub fn new(inner: Arc<dyn ChatApi>, guard: Arc<TimeoutGuard>, pool: PgPool) -> Self {
        Self {
            inner,
            guard,
            pool,
            bot_ban_handler: None,
        }
    }

    /// Verdrahtet optional den Bot-Ban-Lifecycle-Port.
    pub fn with_bot_ban_handler(
        mut self,
        handler: Option<Arc<dyn BotBannedChannelHandler>>,
    ) -> Self {
        self.bot_ban_handler = handler;
        self
    }

    /// Löst `broadcaster_id` → `login` auf (kanonische Identitäts-Tabelle).
    ///
    /// `twitch_streamer_identities` ist die robusteste vorhandene id→login-Quelle
    /// (PK `twitch_user_id`, vom Promo-Pfad ebenfalls als Identitäts-Join genutzt,
    /// promos.rs:1653). Findet die Abfrage keinen Login (oder DB-Fehler) → `None`.
    async fn resolve_login(&self, broadcaster_id: &str) -> Option<String> {
        let login: Option<String> = sqlx::query_scalar!(
            "SELECT twitch_login AS \"twitch_login?\" FROM twitch_streamer_identities WHERE twitch_user_id = $1",
            broadcaster_id,
        )
        .fetch_optional(&self.pool)
        .await
        .unwrap_or(None)
        .flatten();
        login.filter(|l| !l.trim().is_empty())
    }
}

#[async_trait]
impl ChatApi for TimeoutTrackingChatApi {
    /// Sendet die Nachricht über `inner` und meldet einen Bot-Timeout-Drop an
    /// den Guard. Das Original-Ergebnis bleibt unverändert.
    async fn send_message(
        &self,
        broadcaster_id: &str,
        message: &str,
    ) -> Result<SendOutcome, String> {
        let result = self.inner.send_message(broadcaster_id, message).await;

        // Nur im seltenen Bot-Timeout-Drop-Fall die DB für die id→login-Auflösung
        // bemühen (moderation.py:1535–1538).
        if let Ok(outcome) = &result {
            let timeout_drop = is_bot_timeout_drop(outcome).is_some();
            let bot_ban_reason = bot_banned_reason(outcome);
            if timeout_drop || bot_ban_reason.is_some() {
                match self.resolve_login(broadcaster_id).await {
                    Some(login) => {
                        if timeout_drop {
                            self.guard.record_timeout(&login);
                        }
                        if let (Some(handler), Some(reason)) =
                            (self.bot_ban_handler.as_ref(), bot_ban_reason)
                        {
                            handler
                                .on_bot_banned_channel(BotBannedChannelSignal {
                                    broadcaster_id: broadcaster_id.to_string(),
                                    broadcaster_login: login,
                                    reason,
                                })
                                .await;
                        }
                    }
                    None => debug!(
                        "Bot-Timeout/Ban-Signal, aber kein Login für broadcaster_id={broadcaster_id} \
                         auflösbar — Tracking übersprungen"
                    ),
                }
            }
        }

        result
    }

    async fn send_announcement(
        &self,
        broadcaster_id: &str,
        message: &str,
        color: &str,
    ) -> Result<bool, String> {
        self.inner
            .send_announcement(broadcaster_id, message, color)
            .await
    }

    async fn send_announcement_detailed(
        &self,
        broadcaster_id: &str,
        message: &str,
        color: &str,
    ) -> Result<AnnouncementOutcome, String> {
        self.inner
            .send_announcement_detailed(broadcaster_id, message, color)
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

    async fn unban_user(&self, broadcaster_id: &str, target_user_id: &str) -> Result<bool, String> {
        self.inner.unban_user(broadcaster_id, target_user_id).await
    }

    async fn delete_message(&self, broadcaster_id: &str, message_id: &str) -> Result<bool, String> {
        self.inner.delete_message(broadcaster_id, message_id).await
    }

    async fn user_created_at(&self, user_id: &str) -> Result<Option<DateTime<Utc>>, String> {
        self.inner.user_created_at(user_id).await
    }

    async fn resolve_user_id(&self, login: &str) -> Result<Option<String>, String> {
        self.inner.resolve_user_id(login).await
    }

    async fn bot_user_id(&self) -> String {
        self.inner.bot_user_id().await
    }
}

// ---------------------------------------------------------------------------
// CombinedSuppression — DB-Suppression ODER TimeoutGuard-Mute
// ---------------------------------------------------------------------------

/// Kombiniert die bestehende DB-Suppression mit dem In-Memory-[`TimeoutGuard`].
///
/// `is_muted` = `store.is_muted(login)` **ODER** `guard.is_muted(login)`.
/// So bleibt die DB-Suppression (`twitch_outbound_chat_suppressions`, Quelle
/// `promo`) erhalten und der Python-TimeoutGuard-Check kommt dazu.
///
/// Port: `promos.py:1132–1137` (erst `timeout_guard.is_muted`, dann
/// DB-Suppression).
pub struct CombinedSuppression {
    store: Arc<dyn crate::promos::OutboundSuppressionCheck>,
    guard: Arc<TimeoutGuard>,
}

impl CombinedSuppression {
    /// Erstellt eine neue kombinierte Suppression.
    pub fn new(
        store: Arc<dyn crate::promos::OutboundSuppressionCheck>,
        guard: Arc<TimeoutGuard>,
    ) -> Self {
        Self { store, guard }
    }
}

#[async_trait]
impl crate::promos::OutboundSuppressionCheck for CombinedSuppression {
    async fn is_muted(&self, channel_login: &str) -> bool {
        // Günstiger In-Memory-Check zuerst — spart bei aktivem Guard die DB-Abfrage.
        if self.guard.is_muted(channel_login) {
            return true;
        }
        self.store.is_muted(channel_login).await
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::promos::OutboundSuppressionCheck;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // -----------------------------------------------------------------------
    // Mock-ChatApi — liefert ein konfigurierbares send_message-Ergebnis.
    // Nur send_message zählt; die übrigen 8 Methoden sind Defaults/unimplemented.
    // -----------------------------------------------------------------------

    struct MockApi {
        send_calls: AtomicUsize,
        outcome: SendOutcome,
    }

    impl MockApi {
        fn with_outcome(outcome: SendOutcome) -> Arc<Self> {
            Arc::new(Self {
                send_calls: AtomicUsize::new(0),
                outcome,
            })
        }
    }

    #[async_trait]
    impl ChatApi for MockApi {
        async fn send_message(&self, _b: &str, _m: &str) -> Result<SendOutcome, String> {
            self.send_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.outcome.clone())
        }
        async fn send_announcement(&self, _b: &str, _m: &str, _c: &str) -> Result<bool, String> {
            unimplemented!()
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
            unimplemented!()
        }
    }

    /// Mock-Suppression mit fixem is_muted-Wert.
    struct FixedSuppression(bool);

    #[async_trait]
    impl OutboundSuppressionCheck for FixedSuppression {
        async fn is_muted(&self, _channel_login: &str) -> bool {
            self.0
        }
    }

    // -----------------------------------------------------------------------
    // is_bot_timeout_drop — reine Logik (kein DB)
    // -----------------------------------------------------------------------

    #[test]
    fn is_bot_timeout_drop_erkennt_sender_timedout() {
        let o = SendOutcome::Dropped {
            code: "sender_timedout".into(),
            message: "x".into(),
        };
        assert_eq!(is_bot_timeout_drop(&o), Some("sender_timedout"));
    }

    #[test]
    fn is_bot_timeout_drop_erkennt_sender_banned() {
        let o = SendOutcome::Dropped {
            code: "sender_banned".into(),
            message: String::new(),
        };
        assert_eq!(is_bot_timeout_drop(&o), Some("sender_banned"));
    }

    #[test]
    fn bot_banned_reason_erkennt_sender_banned_drop() {
        let o = SendOutcome::Dropped {
            code: "sender_banned".into(),
            message: "Sender is banned".into(),
        };
        let reason = bot_banned_reason(&o).expect("sender_banned muss Bot-Ban signalisieren");
        assert!(reason.contains("bot_banned"));
    }

    #[test]
    fn bot_banned_reason_ignoriert_sender_timedout_drop() {
        let o = SendOutcome::Dropped {
            code: "sender_timedout".into(),
            message: "Sender is timed out".into(),
        };
        assert_eq!(bot_banned_reason(&o), None);
    }

    #[test]
    fn bot_banned_reason_erkennt_401_user_is_banned() {
        let o = SendOutcome::HttpError {
            status: 401,
            body: "user is banned".into(),
        };
        let reason = bot_banned_reason(&o).expect("401 mit Ban-Body muss Bot-Ban signalisieren");
        assert!(reason.contains("http_401"));
    }

    #[test]
    fn is_bot_timeout_drop_ignoriert_channel_settings() {
        let o = SendOutcome::Dropped {
            code: "channel_settings".into(),
            message: String::new(),
        };
        assert_eq!(is_bot_timeout_drop(&o), None);
    }

    #[test]
    fn is_bot_timeout_drop_ignoriert_sent_und_httperror() {
        assert_eq!(is_bot_timeout_drop(&SendOutcome::Sent), None);
        assert_eq!(
            is_bot_timeout_drop(&SendOutcome::HttpError {
                status: 401,
                body: String::new()
            }),
            None
        );
    }

    // -----------------------------------------------------------------------
    // CombinedSuppression — Wahrheitstabelle
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn combined_store_false_guard_muted_gibt_true() {
        let guard = Arc::new(TimeoutGuard::new());
        // Tages-Schwelle erreichen → guard.is_muted("kanal") == true
        guard.record_timeout("kanal");
        guard.record_timeout("kanal");
        assert!(guard.is_muted("kanal"));

        let combined =
            CombinedSuppression::new(Arc::new(FixedSuppression(false)), Arc::clone(&guard));
        assert!(
            combined.is_muted("kanal").await,
            "store=false + guard muted → true"
        );
    }

    #[tokio::test]
    async fn combined_beide_false_gibt_false() {
        let guard = Arc::new(TimeoutGuard::new());
        let combined = CombinedSuppression::new(Arc::new(FixedSuppression(false)), guard);
        assert!(
            !combined.is_muted("kanal").await,
            "store=false + guard nicht muted → false"
        );
    }

    #[tokio::test]
    async fn combined_store_true_gibt_true() {
        let guard = Arc::new(TimeoutGuard::new());
        let combined = CombinedSuppression::new(Arc::new(FixedSuppression(true)), guard);
        assert!(
            combined.is_muted("kanal").await,
            "store=true (DB-Suppression) → true, auch ohne Guard-Mute"
        );
    }

    // -----------------------------------------------------------------------
    // Decorator — Verhalten ohne DB-Drop-Pfad (Sent triggert nichts)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn decorator_sent_triggert_kein_record() {
        let inner = MockApi::with_outcome(SendOutcome::Sent);
        let guard = Arc::new(TimeoutGuard::new());
        // connect_lazy: kein echter Pool. Bei Sent wird resolve_login nie gerufen,
        // also ist der tote Pool unkritisch.
        let pool = sqlx::PgPool::connect_lazy("postgres://x:x@127.0.0.1:1/x").unwrap();
        let api = TimeoutTrackingChatApi::new(inner.clone(), Arc::clone(&guard), pool);

        let out = api.send_message("bid", "hi").await.unwrap();
        assert_eq!(out, SendOutcome::Sent, "Original-Ergebnis unverändert");
        assert_eq!(inner.send_calls.load(Ordering::SeqCst), 1);
        assert!(!guard.is_muted("egal"), "Sent → kein record_timeout");
    }

    #[tokio::test]
    async fn decorator_channel_settings_drop_triggert_kein_record() {
        let inner = MockApi::with_outcome(SendOutcome::Dropped {
            code: "channel_settings".into(),
            message: String::new(),
        });
        let guard = Arc::new(TimeoutGuard::new());
        // channel_settings ist KEIN Bot-Timeout-Drop → resolve_login wird nie
        // gerufen, der tote Pool bleibt unberührt.
        let pool = sqlx::PgPool::connect_lazy("postgres://x:x@127.0.0.1:1/x").unwrap();
        let api = TimeoutTrackingChatApi::new(inner, Arc::clone(&guard), pool);

        let out = api.send_message("bid", "hi").await.unwrap();
        assert!(
            matches!(out, SendOutcome::Dropped { ref code, .. } if code == "channel_settings"),
            "Original-Ergebnis unverändert"
        );
        assert!(
            !guard.is_muted("egal"),
            "anderer Code → kein record_timeout"
        );
    }
}

// ---------------------------------------------------------------------------
// DB-Tests (gegen TB_TEST_DATABASE_URL)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::moderation::TIMEOUT_MUTE_DAILY_THRESHOLD;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use std::sync::atomic::{AtomicUsize, Ordering};

    macro_rules! pool_or_skip {
        ($schema:expr) => {{
            let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
                if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                    panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                }
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            };
            pool_in_schema(&dsn, $schema).await
        }};
    }

    async fn pool_in_schema(dsn: &str, schema: &str) -> PgPool {
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;

        let opts = PgConnectOptions::from_str(dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            r#"CREATE TABLE twitch_streamer_identities (
                twitch_user_id TEXT PRIMARY KEY,
                twitch_login TEXT
            )"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    /// Mock-ChatApi, das immer einen sender_timedout-Drop liefert.
    struct TimedOutApi {
        send_calls: AtomicUsize,
    }

    #[async_trait]
    impl ChatApi for TimedOutApi {
        async fn send_message(&self, _b: &str, _m: &str) -> Result<SendOutcome, String> {
            self.send_calls.fetch_add(1, Ordering::SeqCst);
            Ok(SendOutcome::Dropped {
                code: "sender_timedout".into(),
                message: "Bot ist getimed outed".into(),
            })
        }
        async fn send_announcement(&self, _b: &str, _m: &str, _c: &str) -> Result<bool, String> {
            unimplemented!()
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
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn decorator_timeout_drop_registriert_record_und_mutet() {
        let pool = pool_or_skip!("tt_decorator_mute");
        // Identität für die id→login-Auflösung anlegen.
        sqlx::query(
            "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login) \
             VALUES ('bcast-1', 'streamerlogin')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let inner = Arc::new(TimedOutApi {
            send_calls: AtomicUsize::new(0),
        });
        let guard = Arc::new(TimeoutGuard::new());
        let api = TimeoutTrackingChatApi::new(inner.clone(), Arc::clone(&guard), pool.clone());

        // Genug Drops, um die Tages-Schwelle zu erreichen.
        for _ in 0..TIMEOUT_MUTE_DAILY_THRESHOLD {
            let out = api.send_message("bcast-1", "promo").await.unwrap();
            assert!(
                matches!(out, SendOutcome::Dropped { ref code, .. } if code == "sender_timedout"),
                "Original-Ergebnis unverändert durchgereicht"
            );
        }
        assert_eq!(
            inner.send_calls.load(Ordering::SeqCst),
            TIMEOUT_MUTE_DAILY_THRESHOLD,
            "alle Sends an inner delegiert"
        );
        assert!(
            guard.is_muted("streamerlogin"),
            "nach Tages-Schwelle Bot-Timeout-Drops: Kanal ist stumm"
        );
    }

    #[tokio::test]
    async fn decorator_ohne_login_registriert_nichts() {
        let pool = pool_or_skip!("tt_decorator_no_login");
        // KEINE Identität angelegt → resolve_login findet nichts.
        let inner = Arc::new(TimedOutApi {
            send_calls: AtomicUsize::new(0),
        });
        let guard = Arc::new(TimeoutGuard::new());
        let api = TimeoutTrackingChatApi::new(inner, Arc::clone(&guard), pool);

        for _ in 0..(TIMEOUT_MUTE_DAILY_THRESHOLD + 2) {
            let _ = api.send_message("unbekannt", "promo").await.unwrap();
        }
        assert!(
            !guard.is_muted("unbekannt"),
            "kein Login auflösbar → kein record_timeout, kein Mute"
        );
    }
}
