//! Token-Lockout-Store (`twitch_token_blacklist`) — echte Impl des
//! [`crate::TokenBlacklist`]-Ports. Port von `api/token_error_handler.py`
//! (Blacklist-Teil).
//!
//! Prod-Schema-Eigenheit (Alt-Stil, verifiziert): `first_error_at`/
//! `last_error_at`/`grace_expires_at` sind **TEXT** (ISO), Flags **INTEGER**,
//! `error_count` DEFAULT 1. Entsprechend gebunden.
//!
//! Semantik (1:1 zu Python):
//! - **blacklisted** ⇔ `error_count >= 3` (`BLACKLIST_DISABLE_THRESHOLD`).
//! - **recent failure** ⇔ `error_count < 3` UND letzter Fehler < 2 h her
//!   (`RETRY_COOLDOWN_HOURS`) — Cooldown gegen Refresh-Sturm.
//! - **add**: bestehender Eintrag mit letztem Fehler < 12 h
//!   (`CONSECUTIVE_FAILURE_WINDOW_HOURS`) → Counter +1; sonst Reset auf 1.
//!   Neuer Eintrag → Grace-Period 7 Tage (`GRACE_PERIOD_DAYS`).
//! - **clear**: nach erfolgreichem Refresh den Eintrag löschen.
//! - **add** schreibt zusätzlich den Sofort-Lockout auf `twitch_raid_auth`
//!   (`raid_enabled=FALSE, needs_reauth=TRUE`) — Port von Python
//!   `_mark_reauth_required`, das `add_to_blacklist` **unbedingt ab dem ersten
//!   Fehler** aufruft. Ohne das bliebe der widerrufene Token bis `error_count>=3`
//!   nutzbar. Der Partner-Mirror + Discord-Hinweis (Python `_disable_raid_bot`,
//!   erst ab `error_count>=3`) gehört in die Partner-/Broker-Schicht und bleibt
//!   hier bewusst offen.

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use sqlx::PgPool;

use crate::token_refresher::TokenBlacklist;
use crate::util::mask_log_identifier as mask;

pub const BLACKLIST_DISABLE_THRESHOLD: i64 = 3;
pub const RETRY_COOLDOWN_HOURS: i64 = 2;
pub const CONSECUTIVE_FAILURE_WINDOW_HOURS: i64 = 12;
pub const GRACE_PERIOD_DAYS: i64 = 7;

#[derive(Clone)]
pub struct TokenBlacklistStore {
    pool: PgPool,
}

impl TokenBlacklistStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn iso(dt: DateTime<Utc>) -> String {
        dt.to_rfc3339_opts(SecondsFormat::Secs, false)
    }

    /// Parst einen TEXT-Timestamp (ISO, toleriert `Z`); naiv → UTC.
    fn parse_ts(raw: &str) -> Option<DateTime<Utc>> {
        crate::util::parse_iso_utc(raw)
    }
}

#[async_trait::async_trait]
impl TokenBlacklist for TokenBlacklistStore {
    async fn is_blacklisted(&self, twitch_user_id: &str) -> bool {
        let count: Result<Option<i32>, _> = sqlx::query_scalar!(
            r#"SELECT error_count AS "error_count?" FROM twitch_token_blacklist WHERE twitch_user_id = $1"#,
            twitch_user_id
        )
        .fetch_optional(&self.pool)
        .await
        .map(Option::flatten);
        match count {
            Ok(Some(c)) => i64::from(c) >= BLACKLIST_DISABLE_THRESHOLD,
            Ok(None) => false,
            Err(error) => {
                tracing::error!(%error, "Token-Blacklist-Check fehlgeschlagen");
                false
            }
        }
    }

    async fn has_recent_failure(&self, twitch_user_id: &str) -> bool {
        let row = sqlx::query!(
            r#"SELECT error_count AS "error_count?",
                      last_error_at AS "last_error_at?"
                 FROM twitch_token_blacklist
                WHERE twitch_user_id = $1"#,
            twitch_user_id
        )
        .fetch_optional(&self.pool)
        .await;
        let Ok(Some(row)) = row else {
            return false;
        };
        let Some(last) = row.last_error_at else {
            return false;
        };
        // Vollständig blacklisted → separat via is_blacklisted behandelt.
        if i64::from(row.error_count.unwrap_or(0)) >= BLACKLIST_DISABLE_THRESHOLD {
            return false;
        }
        let Some(last_dt) = Self::parse_ts(&last) else {
            return false;
        };
        (Utc::now() - last_dt) < Duration::hours(RETRY_COOLDOWN_HOURS)
    }

    async fn add_to_blacklist(
        &self,
        twitch_user_id: &str,
        twitch_login: &str,
        error_message: &str,
    ) {
        if let Err(error) = self
            .add_to_blacklist_inner(twitch_user_id, twitch_login, error_message, Utc::now())
            .await
        {
            tracing::error!(%error, user = %mask(twitch_user_id), "Token-Blacklist-Insert fehlgeschlagen");
        }
    }

    async fn clear_failure_count(&self, twitch_user_id: &str) {
        if let Err(error) = self.clear_failure_count_inner(twitch_user_id).await {
            tracing::debug!(%error, "Token-Blacklist-Clear fehlgeschlagen");
        }
    }
}

impl TokenBlacklistStore {
    /// Räumt nach erfolgreichem Refresh (ohne Re-Auth) sowohl den Pause-Grund
    /// `technical_pause_reason='token_error*'` als auch den Blacklist-Eintrag.
    ///
    /// Port von Python `clear_failure_count` (token_error_handler.py:852-867):
    /// erst `UPDATE twitch_partners` (nur `token_error*` → NULL, fremde Gründe wie
    /// `bot_banned` bleiben dank CASE unangetastet), dann `DELETE` aus der
    /// Blacklist. Ohne das UPDATE bliebe ein per Refresh genesener Partner in
    /// Dashboard-/Analytics-Gates als `token_error` pausiert, bis er voll
    /// neu autorisiert (die Re-Auth-Gegenrichtung in `auth_writer::store_new_auth`).
    async fn clear_failure_count_inner(&self, twitch_user_id: &str) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query!(
            "UPDATE twitch_partners
                SET technical_pause_reason = CASE
                        WHEN LOWER(TRIM(COALESCE(technical_pause_reason, ''))) LIKE 'token_error%' THEN NULL
                        ELSE technical_pause_reason
                    END
              WHERE twitch_user_id = $1",
            twitch_user_id
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "DELETE FROM twitch_token_blacklist WHERE twitch_user_id = $1",
            twitch_user_id
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn add_to_blacklist_inner(
        &self,
        twitch_user_id: &str,
        twitch_login: &str,
        error_message: &str,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        let now_iso = Self::iso(now);
        let mut tx = self.pool.begin().await?;
        let existing = sqlx::query!(
            r#"SELECT error_count AS "error_count?",
                      last_error_at AS "last_error_at?"
                 FROM twitch_token_blacklist
                WHERE twitch_user_id = $1"#,
            twitch_user_id
        )
        .fetch_optional(&mut *tx)
        .await?;

        match existing {
            Some(row) => {
                let prior = i64::from(row.error_count.unwrap_or(0));
                // Außerhalb des Consecutive-Fensters → Counter zurücksetzen.
                let reset = row
                    .last_error_at
                    .as_deref()
                    .and_then(Self::parse_ts)
                    .map(|dt| (now - dt) > Duration::hours(CONSECUTIVE_FAILURE_WINDOW_HOURS))
                    .unwrap_or(false);
                if reset {
                    sqlx::query!(
                        "UPDATE twitch_token_blacklist
                            SET error_count = 1, first_error_at = $1, last_error_at = $1,
                                error_message = $2, notified = 0
                          WHERE twitch_user_id = $3",
                        &now_iso,
                        error_message,
                        twitch_user_id
                    )
                    .execute(&mut *tx)
                    .await?;
                } else {
                    let new_count = (prior + 1).max(1) as i32;
                    sqlx::query!(
                        "UPDATE twitch_token_blacklist
                            SET error_count = $1, last_error_at = $2, error_message = $3
                          WHERE twitch_user_id = $4",
                        new_count,
                        &now_iso,
                        error_message,
                        twitch_user_id
                    )
                    .execute(&mut *tx)
                    .await?;
                }
            }
            None => {
                let grace = Self::iso(now + Duration::days(GRACE_PERIOD_DAYS));
                // error_count nutzt DEFAULT 1.
                sqlx::query!(
                    "INSERT INTO twitch_token_blacklist
                        (twitch_user_id, twitch_login, error_message,
                         first_error_at, last_error_at, grace_expires_at)
                     VALUES ($1, $2, $3, $4, $4, $5)",
                    twitch_user_id,
                    twitch_login,
                    error_message,
                    &now_iso,
                    &grace
                )
                .execute(&mut *tx)
                .await?;
            }
        }

        // Sofort-Lockout (Python add_to_blacklist -> _mark_reauth_required, unbedingt
        // ab dem ersten invalid_grant): raid_enabled=FALSE gated load_decrypted, der
        // Token wird ab sofort nicht mehr geladen/refresht; needs_reauth=TRUE
        // signalisiert dem Dashboard die nötige Neu-Autorisierung. twitch_login wird
        // wie in Python nur bei nicht-leerem Hint überschrieben.
        sqlx::query!(
            "UPDATE twitch_raid_auth
                SET raid_enabled = FALSE,
                    needs_reauth = TRUE,
                    twitch_login = COALESCE(NULLIF($1, ''), twitch_login)
              WHERE twitch_user_id = $2",
            twitch_login.trim().to_lowercase(),
            twitch_user_id
        )
        .execute(&mut *tx)
        .await?;

        // Partner-Mirror (Python _mark_reauth_required -> set_partner_raid_bot_enabled +
        // twitch_partners-UPDATE): ohne diesen Spiegel bleibt der Partner nach
        // invalid_grant auf raid_bot_enabled=1 / ohne technical_pause_reason, und
        // Dashboard-/Analytics-Gates, die auf 'token_error' reagieren, greifen nicht.
        // Guards wie Python: manueller Opt-out und 'bot_banned' werden NICHT
        // überschrieben. Die Gegenrichtung (Re-Auth) hebt 'token_error' in
        // auth_writer::store_new_auth wieder auf.
        sqlx::query!(
            "UPDATE twitch_partners
                SET technical_pause_reason = CASE
                        WHEN COALESCE(manual_partner_opt_out, 0) = 1 THEN technical_pause_reason
                        WHEN LOWER(COALESCE(technical_pause_reason, '')) = 'bot_banned' THEN technical_pause_reason
                        ELSE 'token_error'
                    END,
                    raid_bot_enabled = 0
              WHERE twitch_user_id = $1",
            twitch_user_id
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }
}
