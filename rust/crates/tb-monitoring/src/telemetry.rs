//! Telemetrie-Schreibpfad der EventSub-Channel-Events
//! (`twitch_bits_events`, `_subscription_events`, `_follow_events`, …).
//!
//! Insert-only-Tabellen ohne Dedup (Schema-Vertrag, wie Python — Invariante 3).
//! Feld-Extraktion 1:1 zu den `_store_*_event`-Funktionen aus
//! `bot/analytics/mixin.py` bzw. den Webhook-Callbacks. Prod-Typen verifiziert
//! (2026-06-09): Timestamps timestamptz, Flags boolean, IDs bigint.
//! Die `session_id` wird über `twitch_live_state.active_session_id` aufgelöst.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use tb_chat::TimeoutGuard;

use crate::stream::parse_dt_utc;

#[derive(Clone)]
pub struct TelemetryStore {
    pool: PgPool,
    bot_timeout: Option<BotTimeoutRecorder>,
}

#[derive(Clone)]
struct BotTimeoutRecorder {
    bot_user_id: String,
    guard: Arc<TimeoutGuard>,
}

/// Hype-Train-Phase (Python: `begin` / `progress` / `end`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HypeTrainPhase {
    Begin,
    Progress,
    End,
}

/// Shoutout-Richtung (Python: `sent` / `received`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShoutoutDirection {
    Sent,
    Received,
}

impl TelemetryStore {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            bot_timeout: None,
        }
    }

    /// Verdrahtet die inbound EventSub-Erkennung von Bot-Self-Timeouts
    /// (`channel.ban` mit `ends_at`) an den tb-chat TimeoutGuard.
    ///
    /// Live verdrahtet in `bin/tb-bot` (main.rs, nach dem ChatRuntime-Aufbau via
    /// `runtime.bot_user_id()` + `runtime.timeout_guard()`); ohne diese Injection
    /// bleibt der Store wie bisher rein insert-only.
    pub fn with_bot_timeout_guard(
        mut self,
        bot_user_id: impl Into<String>,
        guard: Arc<TimeoutGuard>,
    ) -> Self {
        let bot_user_id = bot_user_id.into().trim().to_string();
        if !bot_user_id.is_empty() {
            self.bot_timeout = Some(BotTimeoutRecorder { bot_user_id, guard });
        }
        self
    }

    /// Aktive Session über den Live-State (Sessions tragen keine user_id).
    async fn session_id_for(&self, broadcaster_user_id: &str) -> Option<i64> {
        sqlx::query_scalar!(
            "SELECT active_session_id FROM twitch_live_state WHERE twitch_user_id = $1",
            broadcaster_user_id,
        )
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()
        .flatten()
    }

    /// channel.subscribe / channel.subscription.gift / channel.subscription.message.
    pub async fn store_subscription_event(
        &self,
        broadcaster_user_id: &str,
        event: &Value,
        event_type: &str,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        let user_login = str_lower(event, &["user_login", "user_name"]);
        let tier = str_field(event, &["tier"]).unwrap_or_else(|| "1000".to_string());
        let is_gift = event
            .get("is_gift")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let gifter_login = str_lower(event, &["gifter_login", "gifter_user_login"]);
        let cumulative_months =
            int_field(event, &["cumulative_months", "months"]).filter(|v| *v != 0);
        let streak_months = int_field(event, &["streak_months"]).filter(|v| *v != 0);
        let message = message_text(event);
        let gift_total_kind = str_field(event, &["gift_total_kind"])
            .map(|v| v.to_lowercase())
            .unwrap_or_default();
        let total_gifted = match gift_total_kind.as_str() {
            "batch_total" => int_field(event, &["gift_total", "total"]).filter(|v| *v != 0),
            "cumulative_total" => Some(int_field(event, &["total"]).unwrap_or(1)),
            _ => int_field(event, &["total", "gift_total"]).filter(|v| *v != 0),
        };
        let session_id = self.session_id_for(broadcaster_user_id).await;
        sqlx::query!(
            r#"
            INSERT INTO twitch_subscription_events
                (session_id, twitch_user_id, event_type, user_login, tier,
                 is_gift, gifter_login, cumulative_months, streak_months,
                 message, total_gifted, received_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            "#,
            session_id,
            broadcaster_user_id,
            event_type,
            user_login,
            tier,
            is_gift,
            gifter_login,
            cumulative_months,
            streak_months,
            message,
            total_gifted,
            now,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// channel.ad_break.begin.
    pub async fn store_ad_break_event(
        &self,
        broadcaster_user_id: &str,
        event: &Value,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        let duration_seconds = int_field(event, &["duration_seconds"]).filter(|v| *v != 0);
        let is_automatic = event
            .get("is_automatic")
            .map(|v| v.as_bool().unwrap_or_else(|| !v.is_null()))
            .unwrap_or(false);
        let session_id = self.session_id_for(broadcaster_user_id).await;
        sqlx::query!(
            "INSERT INTO twitch_ad_break_events
                (session_id, twitch_user_id, duration_seconds, is_automatic, started_at)
             VALUES ($1, $2, $3, $4, $5)",
            session_id,
            broadcaster_user_id,
            duration_seconds,
            is_automatic,
            now,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// channel.cheer. Events ohne Betrag werden (wie Python) verworfen.
    pub async fn store_bits_event(
        &self,
        broadcaster_user_id: &str,
        event: &Value,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        let donor_login = str_lower(event, &["user_login", "user_name"]);
        let amount = int_field(event, &["bits", "amount"]).unwrap_or(0);
        if amount == 0 {
            return Ok(());
        }
        let message = message_text(event);
        let session_id = self.session_id_for(broadcaster_user_id).await;
        sqlx::query!(
            "INSERT INTO twitch_bits_events
                (session_id, twitch_user_id, donor_login, amount, message, received_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
            session_id,
            broadcaster_user_id,
            donor_login,
            amount,
            message,
            now,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// channel.channel_points_custom_reward_redemption.add.
    pub async fn store_channel_points_event(
        &self,
        broadcaster_user_id: &str,
        event: &Value,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        let user_login = str_lower(event, &["user_login", "user_name"]);
        let reward = event.get("reward").cloned().unwrap_or(Value::Null);
        let reward_id = str_field(&reward, &["id"]).or_else(|| str_field(event, &["reward_id"]));
        let reward_title =
            str_field(&reward, &["title"]).or_else(|| str_field(event, &["reward_title"]));
        let reward_cost = int_field(&reward, &["cost"])
            .or_else(|| int_field(event, &["reward_cost"]))
            .filter(|v| *v != 0);
        let user_input = str_field(event, &["user_input"]);
        let redeemed_at = str_field(event, &["redeemed_at"])
            .and_then(|raw| parse_dt_utc(&raw))
            .unwrap_or(now);
        let session_id = self.session_id_for(broadcaster_user_id).await;
        sqlx::query!(
            "INSERT INTO twitch_channel_points_events
                (session_id, twitch_user_id, user_login, reward_id, reward_title,
                 reward_cost, user_input, redeemed_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            session_id,
            broadcaster_user_id,
            user_login,
            reward_id,
            reward_title,
            reward_cost,
            user_input,
            redeemed_at,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// channel.hype_train.begin/progress/end. `end` aktualisiert das passende
    /// offene begin-Event (gleicher Start), sonst Insert mit Phase.
    pub async fn store_hype_train_event(
        &self,
        broadcaster_user_id: &str,
        event: &Value,
        phase: HypeTrainPhase,
    ) -> Result<(), sqlx::Error> {
        let started_at = str_field(event, &["started_at"]).and_then(|raw| parse_dt_utc(&raw));
        let ended_at = if phase == HypeTrainPhase::End {
            str_field(event, &["ended_at"]).and_then(|raw| parse_dt_utc(&raw))
        } else {
            None
        };
        let level = int_field(event, &["level"]).filter(|v| *v != 0);
        let total_progress = int_field(event, &["total", "total_progress"]).filter(|v| *v != 0);
        let duration_seconds = match (started_at, ended_at) {
            (Some(start), Some(end)) => Some(((end - start).num_seconds().max(0)) as i32),
            _ => None,
        };
        let session_id = self.session_id_for(broadcaster_user_id).await;

        if phase == HypeTrainPhase::End {
            let updated = sqlx::query!(
                r#"
                UPDATE twitch_hype_train_events
                   SET ended_at = $1,
                       duration_seconds = $2,
                       level = COALESCE($3, level),
                       total_progress = COALESCE($4, total_progress)
                 WHERE twitch_user_id = $5
                   AND started_at IS NOT DISTINCT FROM $6
                   AND ended_at IS NULL
                "#,
                ended_at,
                duration_seconds,
                level,
                total_progress,
                broadcaster_user_id,
                started_at,
            )
            .execute(&self.pool)
            .await?;
            if updated.rows_affected() > 0 {
                return Ok(());
            }
        }
        let event_phase = match phase {
            HypeTrainPhase::Begin => "begin",
            HypeTrainPhase::Progress => "progress",
            HypeTrainPhase::End => "end",
        };
        sqlx::query!(
            "INSERT INTO twitch_hype_train_events
                (session_id, twitch_user_id, started_at, ended_at,
                 duration_seconds, level, total_progress, event_phase)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            session_id,
            broadcaster_user_id,
            started_at,
            ended_at,
            duration_seconds,
            level,
            total_progress,
            event_phase,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// channel.ban / channel.unban.
    pub async fn store_ban_event(
        &self,
        broadcaster_user_id: &str,
        broadcaster_login: Option<&str>,
        event: &Value,
        unbanned: bool,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        let event_type = if unbanned { "unban" } else { "ban" };
        let target_login = str_lower(event, &["user_login", "user_name"]);
        let target_id = str_field(event, &["user_id"]);
        let moderator_login = str_lower(event, &["moderator_user_login"]);
        let reason = str_field(event, &["reason"]);
        // None = permanent.
        let ends_at = str_field(event, &["ends_at"]).and_then(|raw| parse_dt_utc(&raw));
        let session_id = self.session_id_for(broadcaster_user_id).await;
        sqlx::query!(
            "INSERT INTO twitch_ban_events
                (session_id, twitch_user_id, event_type, target_login, target_id,
                 moderator_login, reason, ends_at, received_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            session_id,
            broadcaster_user_id,
            event_type,
            target_login,
            target_id.as_deref(),
            moderator_login,
            reason,
            ends_at,
            now,
        )
        .execute(&self.pool)
        .await?;
        self.record_inbound_bot_timeout(broadcaster_login, event, unbanned, ends_at, &target_id);
        Ok(())
    }

    fn record_inbound_bot_timeout(
        &self,
        broadcaster_login: Option<&str>,
        event: &Value,
        unbanned: bool,
        ends_at: Option<DateTime<Utc>>,
        target_id: &Option<String>,
    ) {
        let Some(recorder) = &self.bot_timeout else {
            return;
        };
        if unbanned || ends_at.is_none() {
            return;
        }
        let Some(target_id) = target_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
        else {
            return;
        };
        if target_id != recorder.bot_user_id {
            return;
        }
        let login = broadcaster_login
            .map(str::trim)
            .filter(|login| !login.is_empty())
            .map(str::to_lowercase)
            .or_else(|| {
                str_lower(
                    event,
                    &["broadcaster_user_login", "to_broadcaster_user_login"],
                )
            });
        if let Some(login) = login {
            recorder.guard.record_timeout(&login);
            tracing::info!(login = %login, "Bot-Timeout via channel.ban EventSub erkannt");
        }
    }

    /// channel.shoutout.create (`sent`) / channel.shoutout.receive (`received`).
    pub async fn store_shoutout_event(
        &self,
        broadcaster_user_id: &str,
        event: &Value,
        direction: ShoutoutDirection,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        let (other_id, other_login, moderator_login) = match direction {
            ShoutoutDirection::Sent => (
                str_field(event, &["to_broadcaster_user_id"]),
                str_lower(event, &["to_broadcaster_user_login"]),
                str_lower(event, &["moderator_user_login"]),
            ),
            ShoutoutDirection::Received => (
                str_field(event, &["from_broadcaster_user_id"]),
                str_lower(event, &["from_broadcaster_user_login"]),
                None,
            ),
        };
        let viewer_count = int_field(event, &["viewer_count"]).unwrap_or(0);
        let direction_text = match direction {
            ShoutoutDirection::Sent => "sent",
            ShoutoutDirection::Received => "received",
        };
        sqlx::query!(
            "INSERT INTO twitch_shoutout_events
                (twitch_user_id, direction, other_broadcaster_id, other_broadcaster_login,
                 moderator_login, viewer_count, received_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            broadcaster_user_id,
            direction_text,
            other_id,
            other_login,
            moderator_login,
            viewer_count,
            now,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// channel.follow (Webhook-Callback in Python).
    pub async fn store_follow_event(
        &self,
        broadcaster_user_id: &str,
        broadcaster_login: &str,
        event: &Value,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        let follower_login = str_lower(event, &["user_login", "user_name"]);
        let follower_id = str_field(event, &["user_id"]);
        let followed_at = str_field(event, &["followed_at"])
            .and_then(|raw| parse_dt_utc(&raw))
            .unwrap_or(now);
        sqlx::query!(
            "INSERT INTO twitch_follow_events
                (streamer_login, twitch_user_id, follower_login, follower_id, followed_at)
             VALUES ($1, $2, $3, $4, $5)",
            broadcaster_login,
            broadcaster_user_id,
            follower_login,
            follower_id,
            followed_at,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// channel.chat.user_first_message.
    pub async fn store_first_message_event(
        &self,
        broadcaster_user_id: &str,
        broadcaster_login: &str,
        event: &Value,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        // Python (eventsub_mixin.py:2437) bricht ohne Chatter-Login ab — kein Leer-Insert.
        let chatter_login = match str_lower(event, &["chatter_user_login", "user_login"]) {
            Some(c) if !c.is_empty() => c,
            _ => return Ok(()),
        };
        let chatter_id = str_field(event, &["chatter_user_id", "user_id"]);
        let message_id = str_field(event, &["message_id"]);
        let message_text = event
            .get("message")
            .and_then(|m| m.get("text"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string);

        // Python schreibt Insert + Session-Flag in EINER Transaktion (atomar).
        let mut tx = self.pool.begin().await?;
        sqlx::query!(
            "INSERT INTO twitch_first_message_events
                (streamer_login, broadcaster_id, chatter_login, chatter_id,
                 message_id, message_text, event_ts)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            broadcaster_login,
            broadcaster_user_id,
            &chatter_login,
            chatter_id,
            message_id,
            message_text,
            now,
        )
        .execute(&mut *tx)
        .await?;

        // confirmed_first_ever auf dem Session-Chatter setzen (Python
        // eventsub_mixin.py:2461-2469) — nur wenn eine offene Session existiert.
        // Der Session-Lookup steckt als Subquery im UPDATE: ohne offene Session
        // liefert sie NULL → keine Zeile getroffen (= Pythons `if session_id`).
        sqlx::query!(
            "UPDATE twitch_session_chatters
                SET confirmed_first_ever = TRUE
              WHERE chatter_login = $1
                AND session_id = (
                    SELECT id FROM twitch_stream_sessions
                     WHERE streamer_login = $2 AND ended_at IS NULL
                     ORDER BY started_at DESC
                     LIMIT 1
                )",
            &chatter_login,
            broadcaster_login,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    /// channel.update: Protokoll-Insert + Live-State-Update (nur wenn live).
    /// Läuft in einer Transaktion (Python `_persist_update`).
    pub async fn store_channel_update(
        &self,
        broadcaster_user_id: &str,
        event: &Value,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        let title = str_field(event, &["title"]);
        let game_name = str_field(event, &["category_name", "game_name"]);
        // EventSub channel.update liefert das Feld `language`;
        // `broadcaster_language` bleibt als Fallback für alte Payloads.
        let language = str_field(event, &["language", "broadcaster_language"]);
        let mut tx = self.pool.begin().await?;
        sqlx::query!(
            "INSERT INTO twitch_channel_updates (twitch_user_id, title, game_name, language, recorded_at)
             VALUES ($1, $2, $3, $4, $5)",
            broadcaster_user_id,
            title.as_deref(),
            game_name.as_deref(),
            language.as_deref(),
            now,
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "UPDATE twitch_live_state
                SET last_title = COALESCE($1, last_title),
                    last_game  = COALESCE($2, last_game)
              WHERE twitch_user_id = $3 AND is_live = 1",
            title.as_deref(),
            game_name.as_deref(),
            broadcaster_user_id,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

// ── Feld-Extraktion (Python-Semantik: strip, leer → None) ────────────────────

fn str_field(event: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(raw) = event.get(*key) {
            let text = match raw {
                Value::String(s) => s.trim().to_string(),
                Value::Null => continue,
                other => other.to_string(),
            };
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

fn str_lower(event: &Value, keys: &[&str]) -> Option<String> {
    str_field(event, keys).map(|v| v.to_lowercase())
}

fn int_field(event: &Value, keys: &[&str]) -> Option<i32> {
    for key in keys {
        if let Some(raw) = event.get(*key) {
            let value = match raw {
                Value::Number(n) => n.as_i64(),
                Value::String(s) => s.trim().parse::<i64>().ok(),
                _ => None,
            };
            if let Some(value) = value {
                return Some(value as i32);
            }
        }
    }
    None
}

/// `message` kann String oder `{"text": …}` sein (Python-Verhalten).
fn message_text(event: &Value) -> Option<String> {
    match event.get("message") {
        Some(Value::Object(map)) => map
            .get("text")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string),
        Some(Value::String(s)) => Some(s.trim().to_string()).filter(|t| !t.is_empty()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feld_extraktion_mit_fallbacks() {
        let event = serde_json::json!({
            "user_name": " Drag ",
            "bits": "250",
            "message": {"text": " gg "},
            "leer": ""
        });
        assert_eq!(
            str_lower(&event, &["user_login", "user_name"]).as_deref(),
            Some("drag")
        );
        assert_eq!(int_field(&event, &["bits"]), Some(250));
        assert_eq!(message_text(&event).as_deref(), Some("gg"));
        assert_eq!(str_field(&event, &["leer", "fehlt"]), None);
    }
}
