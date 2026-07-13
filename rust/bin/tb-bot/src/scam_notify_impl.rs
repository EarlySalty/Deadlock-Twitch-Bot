//! Discord-Notifier des Conversation-Scam-Guards (Sichtbarkeit + Revoke).
//!
//! Implementiert den tb-chat-Port [`ScamGuardNotifier`]: postet jedes Urteil als
//! deutsches Embed in den Aufsichts-Channel. Nur ausgeführte/vorgeschlagene
//! Aktionen erhalten einen `scam_revoke`-`view_spec` für den „Rückgängig"-Button.
//!
//! Sprache bewusst nur Deutsch (Entscheidung 2026-06-18: schlicht die
//! MiniMax-Begründung zeigen, kein i18n-Layer).

use std::sync::Arc;

use async_trait::async_trait;
use tb_chat::conversation_scam::{ScamGuardNotifier, ScamNotification};
use tb_transport_discord::{BrokerRelay, DiscordBackend, SendRichMessage};

/// Rot — ausgeführter Ban/Timeout.
const COLOR_BAN: u32 = 0xE74C3C;
/// Gelb — Moderationsvorschlag (kein automatischer Ban).
const COLOR_SUGGEST: u32 = 0xF1C40F;
/// Orange — Verdacht ohne Aktion.
const COLOR_UNSURE: u32 = 0xE67E22;
/// Grau — unauffällig, reine Protokollzeile.
const COLOR_CLEAN: u32 = 0x95A5A6;

struct ScamDiscordNotifier {
    backend: Arc<dyn DiscordBackend>,
    channel_id: i64,
}

#[async_trait]
impl ScamGuardNotifier for ScamDiscordNotifier {
    async fn notify(&self, n: ScamNotification) {
        let (icon, aktion, color) = match (n.verdict.as_str(), n.action_taken.as_str()) {
            (_, "banned") => ("🚨", "Gebannt", COLOR_BAN),
            (_, "timed_out") => ("🚨", "Stummgeschaltet (Timeout)", COLOR_BAN),
            (_, "ban_failed_no_mod") => (
                "🚨",
                "Ban nicht möglich, der Bot ist in diesem Kanal kein Mod",
                COLOR_BAN,
            ),
            (_, "suggested") => (
                "⚠️",
                "Moderationsvorschlag, keine automatische Aktion",
                COLOR_SUGGEST,
            ),
            ("clean", "none") => ("✅", "Unauffällig, keine Aktion", COLOR_CLEAN),
            _ => ("🔍", "Verdacht beobachtet, keine Aktion", COLOR_UNSURE),
        };
        let urteil = match n.verdict.as_str() {
            "scam" => "Betrug",
            "clean" => "Unbedenklich",
            _ => "Unklar",
        };
        let confidence_pct = (n.confidence * 100.0).round() as i64;

        let embed = serde_json::json!({
            "title": format!("{icon} Scam-Wächter — {}", n.chatter_login),
            "description": n.reasoning,
            "color": color,
            "fields": [
                {"name": "Kanal", "value": n.channel_login, "inline": true},
                {"name": "Kategorie", "value": n.category, "inline": true},
                {"name": "Konfidenz", "value": format!("{confidence_pct} %"), "inline": true},
                {"name": "Aktion", "value": aktion, "inline": true},
                {"name": "Urteil", "value": urteil, "inline": true},
            ],
        });

        // Die Discord-Seite (Master-Broker) interpretiert diesen Typ und rendert
        // den Revoke-Button; verdict_id adressiert exakt diesen Fall.
        let view_spec = (n.action_taken != "none").then(|| {
            serde_json::json!({
                "type": "scam_revoke",
                "verdict_id": n.verdict_id,
                "channel_login": n.channel_login,
                "chatter_login": n.chatter_login,
                "action_taken": n.action_taken,
            })
        });

        let payload = SendRichMessage {
            channel_id: self.channel_id,
            content: None,
            embed,
            components: None,
            allowed_role_ids: Vec::new(),
            view_spec,
        };

        if let Err(error) = self.backend.send_rich_message(payload).await {
            tracing::warn!(%error, verdict_id = n.verdict_id, "Scam-Discord-Post fehlgeschlagen");
        }
    }
}

/// Baut den Notifier aus der Broker-Config + Channel-ID. `None`, wenn kein
/// Broker konfigurierbar ist → der Wächter läuft dann ohne Discord-Sichtbarkeit.
pub fn build_scam_notifier(
    broker: &tb_config::BrokerConfig,
    channel_id: i64,
) -> Option<Arc<dyn ScamGuardNotifier>> {
    match BrokerRelay::new(broker) {
        Ok(relay) => Some(Arc::new(ScamDiscordNotifier {
            backend: Arc::new(relay),
            channel_id,
        })),
        Err(error) => {
            tracing::warn!(
                %error,
                "Scam-Discord-Notifier nicht initialisierbar — Sichtbarkeit aus"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tb_transport_discord::backend::{
        EditRichMessage, SendAlertEmbed, SendResult, SendResultInner, SendUserDm,
    };
    use tb_transport_discord::DiscordError;

    /// DiscordBackend-Stub, der das gesendete Rich-Message-Payload festhält.
    #[derive(Default)]
    struct CapturingBackend {
        last: Mutex<Option<SendRichMessage>>,
    }

    fn ok_result() -> SendResult {
        SendResult {
            ok: true,
            result: SendResultInner {
                message_id: "1".to_string(),
            },
        }
    }

    #[async_trait]
    impl DiscordBackend for CapturingBackend {
        async fn send_rich_message(
            &self,
            payload: SendRichMessage,
        ) -> Result<SendResult, DiscordError> {
            *self.last.lock().unwrap() = Some(payload);
            Ok(ok_result())
        }
        async fn edit_rich_message(&self, _: EditRichMessage) -> Result<(), DiscordError> {
            Ok(())
        }
        async fn send_user_dm(&self, _: SendUserDm) -> Result<SendResult, DiscordError> {
            Ok(ok_result())
        }
        async fn send_alert_embed(&self, _: SendAlertEmbed) -> Result<SendResult, DiscordError> {
            Ok(ok_result())
        }
        async fn remove_member_role(
            &self,
            _: u64,
            _: u64,
            _: u64,
            _: &str,
        ) -> Result<(), DiscordError> {
            Ok(())
        }
    }

    fn notification(verdict: &str, action: &str) -> ScamNotification {
        ScamNotification {
            verdict_id: 77,
            channel_login: "earlysalty".to_string(),
            chatter_login: "sophiaa_star".to_string(),
            category: "befriending_pivot".to_string(),
            reasoning: "Aufgesetzte Freundschafts-Masche mit Pivot zu Discord.".to_string(),
            confidence: 0.94,
            verdict: verdict.to_string(),
            action_taken: action.to_string(),
        }
    }

    async fn capture(verdict: &str, action: &str) -> SendRichMessage {
        let backend = Arc::new(CapturingBackend::default());
        let notifier = ScamDiscordNotifier {
            backend: backend.clone(),
            channel_id: 1374364800817303632,
        };
        notifier.notify(notification(verdict, action)).await;
        let captured = backend.last.lock().unwrap().clone();
        captured.expect("kein Rich-Message-Post abgesetzt")
    }

    #[tokio::test]
    async fn ban_post_traegt_revoke_vertrag_und_minimax_begruendung() {
        let p = capture("scam", "banned").await;

        assert_eq!(p.channel_id, 1374364800817303632);
        assert!(p.content.is_none(), "kein Plain-Content, nur Embed");

        // view_spec ist der Cross-Repo-Vertrag, aus dem der Python-Button rendert
        // und sein POST /scam-guard/revoke baut — exakte Feldnamen sind kritisch.
        let vs = p.view_spec.expect("view_spec fehlt");
        assert_eq!(vs["type"], "scam_revoke");
        assert_eq!(vs["verdict_id"].as_i64(), Some(77));
        assert_eq!(vs["channel_login"], "earlysalty");
        assert_eq!(vs["chatter_login"], "sophiaa_star");
        assert_eq!(vs["action_taken"], "banned");

        // Embed zeigt schlicht die MiniMax-Begründung (Entscheidung „nur Deutsch").
        assert_eq!(
            p.embed["description"],
            "Aufgesetzte Freundschafts-Masche mit Pivot zu Discord."
        );
        assert_eq!(p.embed["color"].as_u64(), Some(COLOR_BAN as u64));
        assert_eq!(p.embed["title"], "🚨 Scam-Wächter — sophiaa_star");
        // Feld-Reihenfolge: Kanal, Kategorie, Konfidenz, Aktion, Urteil.
        assert_eq!(p.embed["fields"][0]["value"], "earlysalty");
        assert_eq!(p.embed["fields"][1]["value"], "befriending_pivot");
        assert_eq!(p.embed["fields"][2]["value"], "94 %");
        assert_eq!(p.embed["fields"][3]["value"], "Gebannt");
        assert_eq!(p.embed["fields"][4]["value"], "Betrug");
    }

    #[tokio::test]
    async fn vorschlag_post_ist_gelb_und_traegt_action_suggested() {
        let p = capture("scam", "suggested").await;
        assert_eq!(p.embed["color"].as_u64(), Some(COLOR_SUGGEST as u64));
        assert_eq!(
            p.embed["fields"][3]["value"],
            "Moderationsvorschlag, keine automatische Aktion"
        );
        assert_eq!(p.embed["fields"][4]["value"], "Betrug");
        assert_eq!(p.view_spec.unwrap()["action_taken"], "suggested");
    }

    #[tokio::test]
    async fn timeout_post_ist_rot_und_traegt_timeout_label() {
        let p = capture("scam", "timed_out").await;
        assert_eq!(p.embed["color"].as_u64(), Some(COLOR_BAN as u64));
        assert_eq!(p.embed["fields"][3]["value"], "Stummgeschaltet (Timeout)");
        assert_eq!(p.embed["fields"][4]["value"], "Betrug");
        assert_eq!(p.view_spec.unwrap()["action_taken"], "timed_out");
    }

    #[tokio::test]
    async fn unsure_post_zeigt_hinweis_ohne_revoke_button() {
        let p = capture("unsure", "none").await;
        assert_eq!(p.embed["color"].as_u64(), Some(COLOR_UNSURE as u64));
        assert_eq!(
            p.embed["fields"][3]["value"],
            "Verdacht beobachtet, keine Aktion"
        );
        assert_eq!(p.embed["fields"][4]["value"], "Unklar");
        assert_eq!(p.embed["title"], "🔍 Scam-Wächter — sophiaa_star");
        assert!(p.view_spec.is_none());
    }

    #[tokio::test]
    async fn clean_post_ist_grau_und_hat_keinen_revoke_button() {
        let p = capture("clean", "none").await;
        assert_eq!(p.embed["color"].as_u64(), Some(COLOR_CLEAN as u64));
        assert_eq!(p.embed["fields"][3]["value"], "Unauffällig, keine Aktion");
        assert_eq!(p.embed["fields"][4]["value"], "Unbedenklich");
        assert_eq!(p.embed["title"], "✅ Scam-Wächter — sophiaa_star");
        assert!(p.view_spec.is_none());
    }

    #[tokio::test]
    async fn fehlgeschlagener_ban_wird_rot_mit_revoke_vertrag_gemeldet() {
        let p = capture("scam", "ban_failed_no_mod").await;
        assert_eq!(p.embed["color"].as_u64(), Some(COLOR_BAN as u64));
        assert_eq!(
            p.embed["fields"][3]["value"],
            "Ban nicht möglich, der Bot ist in diesem Kanal kein Mod"
        );
        assert_eq!(p.embed["fields"][4]["value"], "Betrug");
        assert_eq!(p.view_spec.unwrap()["action_taken"], "ban_failed_no_mod");
    }
}
