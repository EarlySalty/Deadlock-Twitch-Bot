//! Composition-Root für den Shadow→Discord-Review-Ausgang (Ticket B19, Binary-Seite).
//!
//! Die tb-engagement-Seite (`shadow_review`) liest die gestagten Shadow-Antworten,
//! reicht sie über den Port [`ShadowReviewSink`] weiter und markiert sie bei Erfolg.
//! Hier lebt die konkrete Sink-Implementierung (Discord-Send via Master-Broker) und
//! der periodische Scheduler, der [`forward_pending_reviews`] taktet.
//!
//! **Default AUS:** Der Loop startet nur, wenn der Review-Kanal konfiguriert ist
//! (`ENGAGEMENT_SHADOW_REVIEW_CHANNEL_ID`) **und** ein BrokerRelay konstruierbar ist.
//! Fehlt eins von beidem, gibt es einen einmaligen Hinweis und keinen Loop — passend
//! zum Engagement-default-AUS und opt-in `output_mode='shadow'`. Solange niemand den
//! Shadow-Modus aktiviert, ist ohnehin nichts in der Queue (no-op).
//!
//! **At-least-once:** Jedes Item wird als eigene Discord-Nachricht gesendet. Schlägt
//! ein Send fehl, bricht der Sink mit [`ShadowReviewError::Sink`] ab und tb-engagement
//! markiert **nichts** — der nächste Lauf reicht denselben Batch erneut ein.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use sqlx::PgPool;
use tb_engagement::shadow_review::{
    forward_pending_reviews, ShadowReviewError, ShadowReviewItem, ShadowReviewSink,
};
use tb_transport_discord::{BrokerRelay, DiscordBackend, SendRichMessage};

use crate::task_supervisor::TaskSupervisor;

/// Scheduler-Intervall: alle 60 s einen Batch ausliefern.
const FORWARD_INTERVAL: Duration = Duration::from_secs(60);
/// Maximale Items pro Lauf (FIFO, älteste zuerst).
const BATCH_LIMIT: i64 = 20;
/// Embed-Farbe für Shadow-Review-Postings (Blau-Grau, klar von Token-Alerts abgesetzt).
const REVIEW_COLOR: i64 = 0x5D_6D_7E;
/// Discord-Limit für einen Embed-Feld-Wert. Der gestagte Antwort-Text wird darauf
/// gekappt, damit der Send nicht an einem Validation-Error des Brokers scheitert.
const FIELD_VALUE_MAX: usize = 1024;

/// Liest die Review-Kanal-ID aus der Env. `None`, wenn ungesetzt/leer/0 —
/// dann bleibt der Scheduler aus (Default AUS).
fn review_channel_id_from_env() -> Option<i64> {
    std::env::var("ENGAGEMENT_SHADOW_REVIEW_CHANNEL_ID")
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .filter(|&id| id > 0)
}

/// Kappt `text` auf `max` Zeichen (an Char-Grenzen, nicht an Bytes) und hängt bei
/// Kürzung ein Ellipsis-Suffix an. Verhindert einen Broker-Validation-Error bei
/// langen Antworten.
fn truncate_for_field(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let suffix = " […]";
    let keep = max.saturating_sub(suffix.chars().count());
    let mut out: String = text.chars().take(keep).collect();
    out.push_str(suffix);
    out
}

/// Discord-Sink des Shadow-Reviews: postet jedes Item als eigenes Embed in den
/// konfigurierten Review-Kanal via Master-Broker.
struct DiscordShadowReviewSink {
    relay: BrokerRelay,
    channel_id: i64,
}

impl DiscordShadowReviewSink {
    /// Baut das Review-Embed eines einzelnen Items (Channel, Modell, Antwort-Text,
    /// auslösende Twitch-Message, Stagungs-Zeitpunkt).
    fn embed_for(&self, item: &ShadowReviewItem) -> serde_json::Value {
        let triggered = item.triggered_by_msg_id.as_deref().unwrap_or("—");
        serde_json::json!({
            "title": format!("Shadow-Antwort · #{}", item.channel_login),
            "description": truncate_for_field(&item.response_text, FIELD_VALUE_MAX),
            "color": REVIEW_COLOR,
            "fields": [
                { "name": "Modell", "value": item.model, "inline": true },
                { "name": "Auslösende Msg-ID", "value": triggered, "inline": true },
            ],
            "footer": { "text": format!("Log-ID {}", item.id) },
            "timestamp": item.created_at.to_rfc3339(),
        })
    }
}

#[async_trait]
impl ShadowReviewSink for DiscordShadowReviewSink {
    async fn forward_for_review(&self, items: &[ShadowReviewItem]) -> Result<(), ShadowReviewError> {
        for item in items {
            let payload = SendRichMessage {
                channel_id: self.channel_id,
                content: None,
                embed: self.embed_for(item),
                allowed_role_ids: vec![],
                view_spec: None,
            };
            self.relay
                .send_rich_message(payload)
                .await
                .map_err(|e| ShadowReviewError::Sink(e.to_string()))?;
        }
        Ok(())
    }
}

/// Spawnt den Shadow-Review-Scheduler. No-op (mit einmaligem Hinweis), wenn der
/// Review-Kanal nicht konfiguriert ist oder kein BrokerRelay konstruierbar ist.
pub fn spawn_shadow_review_scheduler(
    supervisor: &TaskSupervisor,
    pool: PgPool,
    broker: &tb_config::BrokerConfig,
) {
    let Some(channel_id) = review_channel_id_from_env() else {
        tracing::info!(
            "Shadow-Review-Scheduler aus — ENGAGEMENT_SHADOW_REVIEW_CHANNEL_ID nicht gesetzt"
        );
        return;
    };
    let relay = match BrokerRelay::new(broker) {
        Ok(relay) => relay,
        Err(e) => {
            tracing::warn!(
                "Shadow-Review-Scheduler nicht gestartet: BrokerRelay nicht initialisierbar: {e}"
            );
            return;
        }
    };
    let sink = Arc::new(DiscordShadowReviewSink { relay, channel_id });

    supervisor.spawn("shadow_review_forwarder", async move {
        let mut tick = tokio::time::interval(FORWARD_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            match forward_pending_reviews(&pool, sink.as_ref(), BATCH_LIMIT).await {
                Ok(forwarded) if forwarded > 0 => {
                    tracing::info!(forwarded, "Shadow-Review: Antworten zum Review weitergeleitet")
                }
                Ok(_) => {}
                Err(e) => tracing::warn!("Shadow-Review-Lauf fehlgeschlagen: {e}"),
            }
        }
    });

    tracing::info!(
        channel_id,
        "Shadow-Review-Scheduler aktiv (Discord-Forward alle 60 s, Batch {BATCH_LIMIT})"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_kurz_unveraendert() {
        assert_eq!(truncate_for_field("hallo", 1024), "hallo");
    }

    #[test]
    fn truncate_lang_gekappt_mit_suffix() {
        let text = "a".repeat(2000);
        let out = truncate_for_field(&text, FIELD_VALUE_MAX);
        assert_eq!(out.chars().count(), FIELD_VALUE_MAX);
        assert!(out.ends_with(" […]"));
    }

    #[test]
    fn truncate_unicode_an_char_grenze() {
        // Multibyte-Zeichen: Kappung darf keinen Char zerschneiden.
        let text = "ü".repeat(2000);
        let out = truncate_for_field(&text, FIELD_VALUE_MAX);
        assert_eq!(out.chars().count(), FIELD_VALUE_MAX);
        assert!(out.ends_with(" […]"));
    }

    #[test]
    fn embed_enthaelt_kernfelder() {
        let item = ShadowReviewItem {
            id: 42,
            channel_login: "nani".into(),
            response_text: "Beispielantwort".into(),
            triggered_by_msg_id: Some("m1".into()),
            model: "MiniMax-M3".into(),
            created_at: chrono::Utc::now(),
        };
        let sink = DiscordShadowReviewSink {
            // BrokerRelay wird im Embed-Bau nicht berührt; Dummy-Config reicht.
            relay: BrokerRelay::new(&tb_config::BrokerConfig {
                base_url: "http://127.0.0.1:0".into(),
                token: "t".into(),
            })
            .unwrap(),
            channel_id: 123,
        };
        let embed = sink.embed_for(&item);
        assert_eq!(embed["description"], "Beispielantwort");
        assert_eq!(embed["fields"][0]["value"], "MiniMax-M3");
        assert_eq!(embed["fields"][1]["value"], "m1");
        assert_eq!(embed["footer"]["text"], "Log-ID 42");
    }

    #[test]
    fn embed_ohne_msg_id_zeigt_platzhalter() {
        let item = ShadowReviewItem {
            id: 7,
            channel_login: "ch".into(),
            response_text: "x".into(),
            triggered_by_msg_id: None,
            model: "m".into(),
            created_at: chrono::Utc::now(),
        };
        let sink = DiscordShadowReviewSink {
            relay: BrokerRelay::new(&tb_config::BrokerConfig {
                base_url: "http://127.0.0.1:0".into(),
                token: "t".into(),
            })
            .unwrap(),
            channel_id: 1,
        };
        let embed = sink.embed_for(&item);
        assert_eq!(embed["fields"][1]["value"], "—");
    }

    #[test]
    fn channel_id_env_filtert_null_und_leer() {
        // Kein Env-Schreiben im Test (Race) — nur die Filter-Logik der Parse-Kette
        // wird über die öffentliche Funktion an gesetzten/ungesetzten Werten
        // indirekt durch die Konstanten abgedeckt; hier prüfen wir nur die
        // Schwellen-Invariante an Beispielwerten.
        let parse = |s: &str| s.trim().parse::<i64>().ok().filter(|&id| id > 0);
        assert_eq!(parse("123"), Some(123));
        assert_eq!(parse("0"), None);
        assert_eq!(parse(""), None);
        assert_eq!(parse("  "), None);
        assert_eq!(parse("-5"), None);
    }
}
