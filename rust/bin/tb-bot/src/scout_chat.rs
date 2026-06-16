//! Scout-Chat-Sink-Adapter (B17-SCOUT-PRIME).
//!
//! Brücke zwischen dem `ScoutTask` (tb-monitoring) und der Chat-Runtime. Der
//! Scout entdeckt live Deadlock-Streamer und registriert sie als
//! `is_monitored_only = 1`; danach ruft er über den [`ScoutChatSink`]-Port
//! `set_monitored_channels` → `join_channels` (neu + heal) → `part_channels`
//! (entfernt) und beantwortet die Heal-Prüfungen `is_monitored_only` /
//! `is_subscription_ready`.
//!
//! # Architektur-Befund (Python-IRC vs. Rust-EventSub)
//!
//! Der **Python**-Chat-Bot (`bot/chat/connection.py`) ist ein twitchio-Bot mit
//! anonymem IRC-/WebSocket-Read: `join_channels`/`part_channels` abonnieren bzw.
//! verlassen `monitored-only`-Kanäle **ohne** Broadcaster-Autorisierung
//! (read-only Lurker). `is_channel_subscription_ready` prüft den
//! In-Process-WS-Subscription-State.
//!
//! Der **Rust**-Chat (`chat_wiring.rs`) ist dagegen rein EventSub-Webhook-
//! basiert: eine Chat-Subscription (`channel.chat.message`) verlangt den
//! `channel:bot`-Grant des Broadcasters (`reconcile_chat_subscriptions` filtert
//! exakt darauf). Für `monitored-only`-Kanäle existiert dieser Grant per
//! Definition **nicht** — sie sind passive Lurker (`lurker_policy.rs`). Ein
//! anonymer Read-Pfad existiert in Rust nur über den DB-getriebenen
//! [`tb_engagement::irc_reader::EngagementIrcReader`] (`twitch_engagement_settings
//! .irc_read = TRUE`), der seine Kanalliste selbst pflegt und **keinen**
//! Live-Join/Part-Handle nach außen gibt.
//!
//! # Folge für diesen Adapter
//!
//! - `is_monitored_only` → `true`: Scout-Kanäle sind per Definition
//!   monitoring-only. Über [`should_attempt_runtime_heal`] heißt das: **nie**
//!   Heal-Ziel (`base.py:1135`-Parität) — exakt korrekt für den EventSub-Modus,
//!   in dem ein grant-loser Kanal ohnehin nicht abonniert werden kann.
//! - `is_subscription_ready` → `true`: greift wegen `is_monitored_only=true`
//!   nicht ins Heal ein (Kurzschluss), bleibt aber semantisch passend (für
//!   monitoring-only Kanäle ist „bereit" der erwartete passive Endzustand).
//! - `set_monitored_channels`/`join_channels`/`part_channels`: kein nativer
//!   anonymer Read-Membership-Handle vorhanden → bewusst No-op mit einmaligem
//!   Hinweis. Das entspricht dem bisherigen `NoopScoutChatSink`-An/Aus-Zustand
//!   (kein Chat-Effekt) und vermeidet 403-Spam durch grant-lose
//!   EventSub-Subscribe-Versuche.
//!
//! Damit ist der wertschöpfende Teil — das **Session-Priming** neu entdeckter
//! Kanäle (`with_session_tracker`) — voll verdrahtet, während die anonyme
//! Read-Membership der monitoring-only Kanäle als präziser Handoff offenbleibt
//! (siehe Report). Der Scout selbst ist über `TB_SCOUT_ENABLED` gegated.

use std::sync::atomic::{AtomicBool, Ordering};

use tb_chat::should_attempt_runtime_heal;
use tb_monitoring::scout::ScoutChatSink;

/// Chat-Sink des Scout-Tasks im EventSub-Modell.
///
/// Hält keinen Live-Join/Part-Handle (existiert in Rust nicht, s. Modul-Doku);
/// beantwortet die Heal-Prädikate so, dass monitoring-only Kanäle nie geheilt
/// werden (Parität zu `lurker_policy.should_attempt_runtime_heal`).
pub struct ScoutChatAdapter {
    /// Stellt sicher, dass der Handoff-Hinweis nur einmal geloggt wird, statt
    /// jeden Scout-Zyklus zu wiederholen.
    notice_logged: AtomicBool,
}

impl ScoutChatAdapter {
    pub fn new() -> Self {
        Self {
            notice_logged: AtomicBool::new(false),
        }
    }

    /// Scout-Heal-Gate (B8-07) über die **kanonische** Policy aus `tb-chat`
    /// (`lurker_policy.should_attempt_runtime_heal`, Port von `base.py:1135`):
    /// monitoring-only Kanäle sind nie Heal-Ziele, sonst heilt nur ein
    /// runtime-nicht-bereiter Kanal. Der Adapter ist der bin/tb-bot-seitige
    /// Heal-Pfad; das Gate liegt hier zentral, damit ein künftiger echter
    /// Runtime-Heal nicht versehentlich monitoring-only Kanäle anfasst.
    fn wants_runtime_heal(is_monitored_only: bool, is_ready: bool) -> bool {
        should_attempt_runtime_heal(is_monitored_only, is_ready)
    }

    /// Loggt den Handoff-Hinweis genau einmal pro Prozess.
    fn log_missing_runtime_once(&self, action: &str, count: usize) {
        if self
            .notice_logged
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            tracing::info!(
                action,
                count,
                "scout-chat: kein nativer anonymer Read-Membership-Handle (EventSub-Modus) — \
                 monitoring-only Kanäle werden nicht via Chat-Runtime gejoint/gepartet \
                 (Handoff: anonymer Lurker-Read offen)"
            );
        }
    }
}

impl Default for ScoutChatAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ScoutChatSink for ScoutChatAdapter {
    async fn set_monitored_channels(&self, logins: &[String]) {
        if !logins.is_empty() {
            self.log_missing_runtime_once("set_monitored_channels", logins.len());
        }
    }

    async fn join_channels(&self, logins: &[String]) {
        if logins.is_empty() {
            return;
        }
        // bin/tb-bot-Heal-Gate (B8-07): jede Join-Zielmenge enthält neue +
        // Heal-Kanäle. Bevor ein (künftiger) echter Runtime-Heal anliefe, das
        // kanonische `tb_chat::should_attempt_runtime_heal` anwenden — mit den
        // Adapter-Prädikaten. Für monitoring-only Scout-Kanäle ist das immer
        // `false`, also kein Heal-Versuch (1:1 zu `base.py:1135`).
        let heal_due = logins.iter().any(|login| {
            Self::wants_runtime_heal(self.is_monitored_only(login), self.is_subscription_ready(login))
        });
        if heal_due {
            tracing::debug!("scout-chat: Runtime-Heal-Gate offen für mindestens einen Kanal");
        }
        self.log_missing_runtime_once("join_channels", logins.len());
    }

    async fn part_channels(&self, logins: &[String]) {
        if !logins.is_empty() {
            self.log_missing_runtime_once("part_channels", logins.len());
        }
    }

    /// Scout-entdeckte Kanäle sind per Definition monitoring-only — explizit,
    /// statt sich auf den Trait-Default zu verlassen. Über [`wants_runtime_heal`]
    /// (`tb_chat::should_attempt_runtime_heal`) heißt das: nie Heal-Ziel.
    fn is_monitored_only(&self, _login: &str) -> bool {
        true
    }

    /// Greift wegen `is_monitored_only == true` nicht ins Heal-Gate ein
    /// (Kurzschluss in [`wants_runtime_heal`]); `true` ist der passende passive
    /// Endzustand für monitoring-only Kanäle.
    fn is_subscription_ready(&self, _login: &str) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heal_gate_uebernimmt_kanonische_policy() {
        // bin/tb-bot-Heal-Gate delegiert 1:1 an tb_chat::should_attempt_runtime_heal.
        // monitoring-only ⇒ nie heilen (egal ob bereit).
        assert!(!ScoutChatAdapter::wants_runtime_heal(true, false));
        assert!(!ScoutChatAdapter::wants_runtime_heal(true, true));
        // nicht-monitoring-only ⇒ nur heilen wenn nicht bereit.
        assert!(ScoutChatAdapter::wants_runtime_heal(false, false));
        assert!(!ScoutChatAdapter::wants_runtime_heal(false, true));
    }

    #[test]
    fn scout_kanaele_sind_monitoring_only_und_kein_heal() {
        let adapter = ScoutChatAdapter::new();
        // Scout-Kanäle: monitoring-only ⇒ aus den Adapter-Prädikaten folgt kein Heal.
        let mon = adapter.is_monitored_only("irgendwer");
        let ready = adapter.is_subscription_ready("irgendwer");
        assert!(mon);
        assert!(!ScoutChatAdapter::wants_runtime_heal(mon, ready));
    }

    #[tokio::test]
    async fn join_part_set_sind_noop_ohne_panik() {
        // Kein nativer Runtime-Handle: die drei Membership-Aktionen sind No-ops
        // (sie loggen höchstens einmalig) und dürfen nie paniken.
        let adapter = ScoutChatAdapter::new();
        let logins = vec!["a".to_string(), "b".to_string()];
        adapter.set_monitored_channels(&logins).await;
        adapter.join_channels(&logins).await;
        adapter.part_channels(&logins).await;
        // Leere Eingabe ⇒ ebenfalls No-op.
        adapter.join_channels(&[]).await;
    }
}
