//! Freiwillige Twitch-Abo-Erinnerung. Keine Laufzeitberechnung, keine KI.
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};

use crate::{api::ChatApi, types::ChatMessageEvent};

pub const SUB_SCOPE: &str = "channel:read:subscriptions";

/// Aufrufer hält dieselbe Transaktion wie der originale Telemetrie-Insert.
pub async fn record_subscription_event(
    connection: &mut sqlx::PgConnection,
    broadcaster_id: &str,
    viewer_id: &str,
    event_id: &str,
    timestamp: DateTime<Utc>,
    ended: bool,
) -> Result<(), sqlx::Error> {
    let enabled: Option<bool> = sqlx::query_scalar("SELECT sub_reminder_enabled=1 AND sub_reminder_enabled_at<=$2 FROM streamer_plans WHERE twitch_user_id=$1 FOR SHARE")
        .bind(broadcaster_id).bind(timestamp).fetch_optional(&mut *connection).await?;
    if enabled == Some(true) {
        sqlx::query("UPDATE twitch_sub_reminders SET last_event_at=$3, ended_at=CASE WHEN $4 THEN $3 ELSE NULL END, end_message_id=CASE WHEN $4 THEN $5 ELSE NULL END, retry_after=NULL WHERE broadcaster_user_id=$1 AND viewer_user_id=$2 AND enabled AND consent_at<$3 AND (last_event_at IS NULL OR last_event_at<$3)")
            .bind(broadcaster_id).bind(viewer_id).bind(timestamp).bind(ended).bind(event_id)
            .execute(connection).await?;
    }
    Ok(())
}

/// None bedeutet: Status nicht zuverlässig abrufbar, niemals „kein Abo“.
#[async_trait]
pub trait SubscriptionStatus: Send + Sync {
    async fn active(&self, broadcaster_id: &str, viewer_id: &str) -> Option<bool>;
}

pub struct SubReminder {
    pool: PgPool,
    api: Arc<dyn ChatApi>,
    status: Arc<dyn SubscriptionStatus>,
}

pub fn sub_link(login: &str) -> Option<String> {
    let login = login.trim();
    (!login.is_empty()
        && login
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_'))
    .then(|| format!("https://www.twitch.tv/subs/{}", login.to_ascii_lowercase()))
}

impl SubReminder {
    pub fn new(pool: PgPool, api: Arc<dyn ChatApi>, status: Arc<dyn SubscriptionStatus>) -> Self {
        Self { pool, api, status }
    }

    pub async fn command(&self, event: &ChatMessageEvent, args: &str) {
        let lang = crate::ui_lang::channel_ui_lang(&self.pool, &event.broadcaster_user_id).await;
        let text = match self.command_result(event, args, lang).await {
            Ok(text) => text,
            Err(error) => {
                tracing::warn!(%error, "Sub-Erinnerung konnte nicht gespeichert werden");
                lang.pick(
                    "Das hat gerade nicht geklappt. Versuch es bitte später nochmal.",
                    "That didn't work just now. Please try again later.",
                )
                .into()
            }
        };
        crate::api::send_reply(self.api.as_ref(), &event.broadcaster_user_id, &text).await;
    }

    async fn command_result(
        &self,
        event: &ChatMessageEvent,
        args: &str,
        lang: tb_domain::UiLang,
    ) -> Result<String, sqlx::Error> {
        let Some(link) = sub_link(&event.broadcaster_user_login) else {
            return Ok(lang
                .pick(
                    "Der Kanal konnte gerade nicht erkannt werden.",
                    "The channel could not be recognized just now.",
                )
                .into());
        };
        let args = args.trim().to_lowercase();
        if args.is_empty() {
            return Ok(match lang {
                tb_domain::UiLang::De => {
                    format!("Du möchtest den Kanal mit einem Abo unterstützen? {link}")
                }
                tb_domain::UiLang::En => {
                    format!("Want to support the channel with a sub? {link}")
                }
            });
        }
        let channel = &event.broadcaster_user_id;
        let viewer = &event.chatter_user_id;
        if channel.is_empty() || viewer.is_empty() {
            return Ok(lang
                .pick(
                    "Deine Twitch-ID fehlt gerade. Bitte versuch es später nochmal.",
                    "Your Twitch ID is missing right now. Please try again later.",
                )
                .into());
        }
        match args.as_str() {
            "erinnerung aus" | "reminder off" => {
                sqlx::query("UPDATE twitch_sub_reminders SET enabled=false, ended_at=NULL, end_message_id=NULL WHERE broadcaster_user_id=$1 AND viewer_user_id=$2")
                    .bind(channel).bind(viewer).execute(&self.pool).await?;
                Ok(lang
                    .pick(
                        "Alles klar, du bekommst hier keine Abo-Erinnerung mehr.",
                        "All set, you will not get a sub reminder here anymore.",
                    )
                    .into())
            }
            "erinnerung status" | "reminder status" => {
                let enabled: Option<bool> = sqlx::query_scalar("SELECT enabled FROM twitch_sub_reminders WHERE broadcaster_user_id=$1 AND viewer_user_id=$2")
                    .bind(channel).bind(viewer).fetch_optional(&self.pool).await?;
                Ok(if enabled == Some(true) {
                    lang.pick(
                        "Deine Abo-Erinnerung ist an. Abbestellen: !sub erinnerung aus",
                        "Your sub reminder is on. Turn it off: !sub reminder off",
                    )
                } else {
                    lang.pick(
                        "Deine Abo-Erinnerung ist aus. Einschalten: !sub erinnerung an",
                        "Your sub reminder is off. Turn it on: !sub reminder on",
                    )
                }
                .into())
            }
            "erinnerung an" | "reminder on" => {
                if !channel_enabled(&self.pool, channel).await? {
                    return Ok(lang
                        .pick(
                            "Abo-Erinnerungen sind in diesem Kanal noch ausgeschaltet.",
                            "Sub reminders are still turned off in this channel.",
                        )
                        .into());
                }
                if self.status.active(channel, viewer).await.is_none() {
                    return Ok(lang.pick(
                        "Abo-Erinnerungen sind gerade nicht verfügbar. Der Streamer muss Twitch in der Verwaltung neu verbinden oder es später nochmal versuchen.",
                        "Sub reminders are not available right now. The streamer needs to reconnect Twitch in settings, or try again later.",
                    ).into());
                }
                let mut tx = self.pool.begin().await?;
                let enabled: Option<i32> = sqlx::query_scalar("SELECT sub_reminder_enabled FROM streamer_plans WHERE twitch_user_id=$1 FOR SHARE")
                    .bind(channel).fetch_optional(&mut *tx).await?;
                if enabled != Some(1) {
                    return Ok(lang
                        .pick(
                            "Abo-Erinnerungen sind in diesem Kanal ausgeschaltet.",
                            "Sub reminders are turned off in this channel.",
                        )
                        .into());
                }
                sqlx::query("INSERT INTO twitch_sub_reminders (broadcaster_user_id,viewer_user_id,enabled) VALUES ($1,$2,true) ON CONFLICT (broadcaster_user_id,viewer_user_id) DO UPDATE SET enabled=true, consent_at=CASE WHEN twitch_sub_reminders.enabled THEN twitch_sub_reminders.consent_at ELSE now() END, ended_at=CASE WHEN twitch_sub_reminders.enabled THEN twitch_sub_reminders.ended_at ELSE NULL END, end_message_id=CASE WHEN twitch_sub_reminders.enabled THEN twitch_sub_reminders.end_message_id ELSE NULL END")
                    .bind(channel).bind(viewer).execute(&mut *tx).await?;
                tx.commit().await?;
                Ok(lang.pick(
                    "Ist an: Wenn Twitch dein Abo als beendet meldet, erinnere ich dich einmal bei deiner nächsten Chatnachricht. Die Erinnerung steht öffentlich im Chat. Abbestellen: !sub erinnerung aus",
                    "It's on: when Twitch reports your sub as ended, I will remind you once on your next chat message. The reminder is public in chat. Turn it off: !sub reminder off",
                ).into())
            }
            _ => Ok(lang
                .pick(
                    "Abo-Link: !sub | Erinnerung: !sub erinnerung an, aus oder status",
                    "Sub link: !sub | Reminder: !sub reminder on, off or status",
                )
                .into()),
        }
    }

    pub async fn on_message(&self, event: &ChatMessageEvent) {
        if event.text().trim_start().starts_with('!')
            || event.chatter_user_id.is_empty()
            || event.broadcaster_user_id.is_empty()
            || event
                .source_broadcaster_user_id
                .as_deref()
                .is_some_and(|id| id != event.broadcaster_user_id)
        {
            return;
        }
        if let Err(error) = self.try_remind(event).await {
            tracing::debug!(%error, "Sub-Erinnerung konnte nicht geprüft werden");
        }
    }

    async fn try_remind(&self, event: &ChatMessageEvent) -> Result<(), sqlx::Error> {
        let channel = &event.broadcaster_user_id;
        let viewer = &event.chatter_user_id;
        if !channel_enabled(&self.pool, channel).await? {
            return Ok(());
        }
        let Some(link) = sub_link(&event.broadcaster_user_login) else {
            return Ok(());
        };
        // Persistentes Claim vor Netzwerkzugriff: parallele Chatnachrichten lesen
        // dieselbe Fälligkeit, aber nur eine belegt den kurzen Prüfzeitraum.
        let claimed = sqlx::query("UPDATE twitch_sub_reminders SET retry_after=now()+interval '60 seconds' WHERE broadcaster_user_id=$1 AND viewer_user_id=$2 AND enabled AND end_message_id IS NOT NULL AND consumed_message_id IS DISTINCT FROM end_message_id AND (retry_after IS NULL OR retry_after<=now()) RETURNING end_message_id,consent_at")
            .bind(channel).bind(viewer).fetch_optional(&self.pool).await?;
        let Some(claimed) = claimed else {
            return Ok(());
        };
        let message_id: String = claimed.try_get("end_message_id")?;
        let consent_at: DateTime<Utc> = claimed.try_get("consent_at")?;
        let status = self.status.active(channel, viewer).await;
        let Some(active) = status else {
            return Ok(());
        };
        // Die Versandreservierung wird VOR dem Chat-Aufruf dauerhaft verbucht.
        // Bei unklarer Zustellung/Prozessabbruch kein zweiter Versuch desselben Endes.
        let consumed = sqlx::query("UPDATE twitch_sub_reminders SET consumed_message_id=$3 WHERE broadcaster_user_id=$1 AND viewer_user_id=$2 AND enabled AND end_message_id=$3 AND consent_at=$4 AND consumed_message_id IS DISTINCT FROM $3")
            .bind(channel).bind(viewer).bind(&message_id).bind(consent_at).execute(&self.pool).await?;
        if consumed.rows_affected() == 0 || active {
            return Ok(());
        }
        // Zustimmung und Kanalschalter bis zum Versand sperren: Abbestellen oder
        // Abschalten wird entweder vorher sichtbar oder nach dem Versand bestätigt.
        let mut tx = self.pool.begin().await?;
        let enabled: Option<i32> = sqlx::query_scalar(
            "SELECT sub_reminder_enabled FROM streamer_plans WHERE twitch_user_id=$1 FOR SHARE",
        )
        .bind(channel)
        .fetch_optional(&mut *tx)
        .await?;
        let ready: Option<bool> = sqlx::query_scalar("SELECT enabled AND end_message_id=$3 AND consent_at=$4 FROM twitch_sub_reminders WHERE broadcaster_user_id=$1 AND viewer_user_id=$2 FOR UPDATE")
            .bind(channel).bind(viewer).bind(&message_id).bind(consent_at).fetch_optional(&mut *tx).await?;
        if enabled == Some(1) && ready == Some(true) {
            let lang = crate::ui_lang::channel_ui_lang(&self.pool, channel).await;
            let text = match lang {
                tb_domain::UiLang::De => format!("@{} Wie gewünscht die Erinnerung: Dein Abo hier ist beendet. Wenn du wieder unterstützen möchtest: {link} | Erinnerung aus: !sub erinnerung aus", event.chatter_user_login),
                tb_domain::UiLang::En => format!("@{} As requested, a reminder: your sub here has ended. If you want to support again: {link} | Turn off: !sub reminder off", event.chatter_user_login),
            };
            match tokio::time::timeout(
                std::time::Duration::from_secs(8),
                self.api.send_message(channel, &text),
            )
            .await
            {
                Ok(Ok(crate::types::SendOutcome::Sent)) => {}
                _ => {
                    tracing::warn!(broadcaster_id=%channel, "Sub-Erinnerung nicht bestätigt; kein erneuter Versand dieses Abo-Endes")
                }
            }
        }
        tx.commit().await?;
        Ok(())
    }
}

async fn channel_enabled(pool: &PgPool, channel: &str) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query_scalar::<_, i32>(
        "SELECT sub_reminder_enabled FROM streamer_plans WHERE twitch_user_id=$1",
    )
    .bind(channel)
    .fetch_optional(pool)
    .await?
        == Some(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn link_nur_fuer_echten_kanallogin() {
        assert_eq!(
            sub_link("EarlySalty").as_deref(),
            Some("https://www.twitch.tv/subs/earlysalty")
        );
        for invalid in ["", "x/y", "x?foo=bar", "a b", "@all"] {
            assert_eq!(sub_link(invalid), None);
        }
    }
}
