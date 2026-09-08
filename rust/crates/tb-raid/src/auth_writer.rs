//! Onboarding-/Re-Auth-Schreibpfad (`twitch_raid_auth`) — Port von
//! `RaidAuthManager.save_auth`. **Security-Kern** — selbst gebaut.
//!
//! Schreibt frische OAuth-Tokens verschlüsselt in die DB (UPSERT), validiert
//! die gewährten Scopes gegen das aufgelöste Profil und setzt `needs_reauth`
//! zurück. Bewusste Invarianten (1:1 zu Python):
//!
//! - Der validierte Token muss alle Rechte des Profils besitzen, sonst `ScopeMismatch`
//!   (kein Speichern halb-autorisierter Tokens).
//! - Verschlüsseln fehlgeschlagen → `EncryptionFailed`, nichts geschrieben.
//! - `raid_enabled` eines bestehenden Eintrags bleibt erhalten
//!   (`activate_raid_features OR existing`).
//! - Re-Auth heilt technische Token-Pausen (`token_error*`) inklusive
//!   `raid_bot_enabled`, bevor der Blacklist-Eintrag entfernt wird.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::token_refresher::{
    advisory_lock_pair, RefreshError, TokenValidation, TwitchTokenClient,
};
use chrono::{DateTime, Duration, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use tb_crypto::{aad, FieldCipher};

use crate::scope_profiles::scopes_for_profile;
use crate::util::mask_log_identifier as mask;

/// Eingabe für [`AuthWriter::store_new_auth`].
#[derive(Clone)]
pub struct NewAuth {
    pub twitch_user_id: String,
    pub twitch_login: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    /// Tatsächlich gewährte Scopes (aus der Twitch-Token-Antwort).
    pub granted_scopes: Vec<String>,
    /// Bereits aufgelöstes Scope-Profil (über `oauth_flow::build_state_info`).
    pub resolved_scope_profile: String,
    pub activate_raid_features: bool,
    /// Vertrauenswürdiges created_at des einmalig verbrauchten OAuth-States.
    pub state_created_at: DateTime<Utc>,
}
impl std::fmt::Debug for NewAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewAuth")
            .field("twitch_user_id", &self.twitch_user_id)
            .field("profile", &self.resolved_scope_profile)
            .field("tokens", &"[redacted]")
            .finish_non_exhaustive()
    }
}

/// Fehler des Onboarding-Writes.
#[derive(Debug)]
pub enum AuthWriteError {
    /// Gewährte Scopes passen nicht zum Profil (Python `unexpected_scopes_for_profile`).
    ScopeMismatch {
        profile: String,
    },
    /// Verschlüsseln fehlgeschlagen — nichts geschrieben (Sicherheits-Policy).
    EncryptionFailed,
    InvalidIdentity,
    InvalidToken,
    ValidationUnavailable,
    ConflictingGrant,
    DisconnectedSinceAuthorization,
    Timeout,
    Db(sqlx::Error),
}

impl std::fmt::Display for AuthWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthWriteError::ScopeMismatch { profile } => {
                write!(f, "unexpected_scopes_for_profile:{profile}")
            }
            AuthWriteError::EncryptionFailed => write!(f, "encryption failed"),
            AuthWriteError::InvalidIdentity => write!(f, "Ungültige Plattform-ID"),
            AuthWriteError::InvalidToken => write!(f, "Twitch-Zugang ist nicht gültig"),
            AuthWriteError::ValidationUnavailable => {
                write!(f, "Twitch-Zugang konnte nicht geprüft werden")
            }
            AuthWriteError::ConflictingGrant => {
                write!(f, "Die neue Freigabe würde bestehende Rechte verlieren")
            }
            AuthWriteError::DisconnectedSinceAuthorization => {
                write!(f, "Uplink wurde nach Beginn dieser Anmeldung getrennt")
            }
            AuthWriteError::Timeout => write!(f, "Zugangsänderung überschritt die Frist"),
            AuthWriteError::Db(_) => write!(f, "Zugangsänderung konnte nicht gespeichert werden"),
        }
    }
}
impl std::error::Error for AuthWriteError {}
impl From<sqlx::Error> for AuthWriteError {
    fn from(e: sqlx::Error) -> Self {
        AuthWriteError::Db(e)
    }
}

#[derive(sqlx::FromRow)]
struct ExistingGrant {
    access_token_enc: Option<Vec<u8>>,
    refresh_token_enc: Option<Vec<u8>>,
    enc_version: Option<i32>,
    raid_enabled: Option<bool>,
}

#[derive(Clone)]
pub struct AuthWriter {
    pool: PgPool,
    cipher: Arc<FieldCipher>,
}

impl AuthWriter {
    pub fn new(pool: PgPool, cipher: Arc<FieldCipher>) -> Self {
        Self { pool, cipher }
    }

    /// Speichert frische Tokens verschlüsselt (UPSERT) + `needs_reauth`-Reset.
    pub async fn store_new_auth(
        &self,
        new: &NewAuth,
        client: &dyn TwitchTokenClient,
        now: DateTime<Utc>,
    ) -> Result<(), AuthWriteError> {
        tokio::time::timeout(
            std::time::Duration::from_secs(35),
            self.store_new_auth_inner(new, client, now),
        )
        .await
        .map_err(|_| AuthWriteError::Timeout)?
    }
    async fn store_new_auth_inner(
        &self,
        new: &NewAuth,
        client: &dyn TwitchTokenClient,
        now: DateTime<Utc>,
    ) -> Result<(), AuthWriteError> {
        if !valid_uid(&new.twitch_user_id) {
            return Err(AuthWriteError::InvalidIdentity);
        }
        let uid = &new.twitch_user_id;
        let mut tx = auth_transaction(&self.pool, uid).await?;
        let disconnected: Option<Option<DateTime<Utc>>> = sqlx::query_scalar(
            "SELECT last_disconnected_at FROM twitch_uplink_auth_intent WHERE twitch_user_id=$1",
        )
        .bind(uid)
        .fetch_optional(&mut *tx)
        .await?;
        if new.resolved_scope_profile == "uplink"
            && disconnected
                .flatten()
                .is_some_and(|at| new.state_created_at <= at)
        {
            return Err(AuthWriteError::DisconnectedSinceAuthorization);
        }
        if new.refresh_token.is_empty() || new.refresh_token.len() > 4096 {
            return Err(AuthWriteError::InvalidToken);
        }
        let verified_at = Utc::now();
        let verified = verify(client, &new.access_token, uid).await?;
        // Scope-Set gegen Profil prüfen (getrimmt, Reihenfolge egal).
        let expected: BTreeSet<&str> = scopes_for_profile(&new.resolved_scope_profile)
            .iter()
            .copied()
            .collect();
        let granted: BTreeSet<String> = verified
            .scopes
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let granted_refs: BTreeSet<&str> = granted.iter().map(String::as_str).collect();
        if !expected.is_subset(&granted_refs) {
            return Err(AuthWriteError::ScopeMismatch {
                profile: new.resolved_scope_profile.clone(),
            });
        }

        let existing: Option<ExistingGrant> = sqlx::query_as("SELECT access_token_enc,refresh_token_enc,enc_version,raid_enabled FROM twitch_raid_auth WHERE twitch_user_id=$1 FOR UPDATE")
            .bind(uid).fetch_optional(&mut *tx).await?;
        let raid_enabled = new.activate_raid_features
            || existing
                .as_ref()
                .and_then(|r| r.raid_enabled)
                .unwrap_or(false);
        let mut access = new.access_token.clone();
        let mut refresh = Some(new.refresh_token.clone());
        let mut selected_scopes = granted;
        let mut expires_at = validated_expiry(verified_at, verified.expires_in)?;
        if let Some(ExistingGrant {
            access_token_enc: Some(old_access),
            refresh_token_enc: old_refresh,
            enc_version: version,
            ..
        }) = existing
        {
            let version = i64::from(version.unwrap_or(1));
            if let Ok(old_token) = self
                .cipher
                .decrypt_field(&old_access, &aad::raid_auth("access_token", uid, version))
            {
                let old_verified_at = Utc::now();
                match verify(client, &old_token, uid).await {
                    Ok(old) => {
                        let old_scopes: BTreeSet<String> = old.scopes.into_iter().collect();
                        if !old_scopes.is_subset(&selected_scopes) {
                            if !selected_scopes.is_subset(&old_scopes) {
                                return Err(AuthWriteError::ConflictingGrant);
                            }
                            access = old_token;
                            refresh = old_refresh.and_then(|blob| {
                                self.cipher
                                    .decrypt_field(
                                        &blob,
                                        &aad::raid_auth("refresh_token", uid, version),
                                    )
                                    .ok()
                            });
                            selected_scopes = old_scopes;
                            expires_at = validated_expiry(old_verified_at, old.expires_in)?;
                        }
                    }
                    Err(AuthWriteError::InvalidToken) => (),
                    Err(error) => return Err(error),
                }
            }
        }
        let (Ok(access_enc), Ok(refresh_enc)) = (
            self.cipher
                .encrypt_field(&access, &aad::raid_auth("access_token", uid, 1)),
            refresh
                .as_deref()
                .map(|token| {
                    self.cipher
                        .encrypt_field(token, &aad::raid_auth("refresh_token", uid, 1))
                })
                .transpose(),
        ) else {
            tracing::error!(user = %mask(uid), "save_auth: Verschlüsseln fehlgeschlagen — nicht gespeichert");
            return Err(AuthWriteError::EncryptionFailed);
        };

        // Genau die validierten Rechte des ausgewählten Tokens, keine Profilvereinigung.
        let scopes_for_db = selected_scopes.into_iter().collect::<Vec<_>>().join(" ");

        sqlx::query!(
            r#"
            INSERT INTO twitch_raid_auth
                (twitch_user_id, twitch_login, access_token, refresh_token,
                 access_token_enc, refresh_token_enc, enc_version, enc_kid,
                 token_expires_at, scopes, authorized_at, raid_enabled)
            VALUES ($1, $2, 'ENC', 'ENC', $3, $4, 1, 'v1', $5, $6, $7, $8)
            ON CONFLICT (twitch_user_id) DO UPDATE SET
                twitch_login      = EXCLUDED.twitch_login,
                access_token_enc  = EXCLUDED.access_token_enc,
                refresh_token_enc = EXCLUDED.refresh_token_enc,
                enc_version       = EXCLUDED.enc_version,
                enc_kid           = EXCLUDED.enc_kid,
                token_expires_at  = EXCLUDED.token_expires_at,
                scopes            = EXCLUDED.scopes,
                authorized_at     = EXCLUDED.authorized_at,
                raid_enabled      = EXCLUDED.raid_enabled
            "#,
            uid,
            &new.twitch_login,
            access_enc,
            refresh_enc,
            expires_at,
            &scopes_for_db,
            now,
            raid_enabled
        )
        .execute(&mut *tx)
        .await?;

        // Re-Auth abgeschlossen → needs_reauth zurücksetzen.
        sqlx::query!(
            "UPDATE twitch_raid_auth SET needs_reauth = FALSE, reauth_notified_at = NULL
             WHERE twitch_user_id = $1",
            uid
        )
        .execute(&mut *tx)
        .await?;

        // Token-Blacklist-Eintrag entfernen (Python: save_auth ruft
        // token_error_handler.remove_from_blacklist). Ohne dies bleibt ein wegen
        // invalid_grant (error_count ≥ 3) blacklisteter Streamer nach erfolgreicher
        // Re-Autorisierung DAUERHAFT gesperrt: der Blacklist-Check in get_valid_token
        // greift vor allem anderen und liefert None. Nur technische Partner-
        // Pause-Gründe `token_error*` aufheben, den technischen Opt-out resetten
        // und Raid reaktivieren, dann den Blacklist-Eintrag löschen.
        sqlx::query(
            "UPDATE twitch_partners
                SET manual_partner_opt_out = CASE
                        WHEN LOWER(TRIM(COALESCE(technical_pause_reason, ''))) LIKE 'token_error%' THEN 0
                        ELSE manual_partner_opt_out
                    END,
                    technical_pause_reason = CASE
                        WHEN LOWER(TRIM(COALESCE(technical_pause_reason, ''))) LIKE 'token_error%' THEN NULL
                        ELSE technical_pause_reason
                    END,
                    raid_bot_enabled = CASE
                        WHEN LOWER(TRIM(COALESCE(technical_pause_reason, ''))) LIKE 'token_error%' THEN 1
                        WHEN $2
                             AND COALESCE(manual_partner_opt_out, 0) = 0
                             AND LOWER(TRIM(COALESCE(technical_pause_reason, ''))) NOT IN ('blocked', 'bot_banned')
                        THEN 1
                        ELSE raid_bot_enabled
                    END
              WHERE twitch_user_id = $1",
        )
        .bind(uid)
        .bind(new.activate_raid_features)
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "DELETE FROM twitch_token_blacklist WHERE twitch_user_id = $1",
            uid
        )
        .execute(&mut *tx)
        .await?;

        if new.resolved_scope_profile == "uplink" {
            crate::target_generation::activate_callback(
                &mut tx,
                uid,
                "twitch",
                new.state_created_at,
            )
            .await?;
            sqlx::query("INSERT INTO twitch_uplink_auth_intent(twitch_user_id,enabled) VALUES($1,TRUE) ON CONFLICT(twitch_user_id) DO UPDATE SET enabled=TRUE")
                .bind(uid).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Nimmt die gespeicherten Tokens zurück, ohne die Zeile zu löschen.
    ///
    /// Explizite Gesamttrennung: die Blobs werden geleert und `needs_reauth` gesetzt,
    /// damit jeder Lesepfad sofort erkennt, dass hier nichts mehr zu holen ist.
    /// Ein DELETE der Zeile wäre falsch: sie trägt auch `raid_enabled`,
    /// `authorized_at` und die Verknüpfung zur Partnerhistorie, und die
    /// gehören nicht dem Uplink. Uplink verwendet ausschließlich disconnect_uplink.
    ///
    /// `reauth_notified_at` wird bewusst mitgesetzt. Die Erinnerungs-DM prüft
    /// dieses Feld; ohne den Stempel bekäme der Streamer eine Mahnung, seinen
    /// Zugang zu erneuern, direkt nachdem er ihn selbst getrennt hat.
    ///
    /// Kein Fehler, wenn es die Zeile nicht gibt: Trennen soll wiederholbar
    /// sein.
    pub async fn clear_tokens(
        &self,
        twitch_user_id: &str,
        now: DateTime<Utc>,
    ) -> Result<(), AuthWriteError> {
        tokio::time::timeout(
            std::time::Duration::from_secs(35),
            self.clear_tokens_inner(twitch_user_id, now),
        )
        .await
        .map_err(|_| AuthWriteError::Timeout)?
    }
    async fn clear_tokens_inner(
        &self,
        twitch_user_id: &str,
        now: DateTime<Utc>,
    ) -> Result<(), AuthWriteError> {
        if !valid_uid(twitch_user_id) {
            return Err(AuthWriteError::InvalidIdentity);
        }
        let mut tx = auth_transaction(&self.pool, twitch_user_id).await?;
        disconnect_intent(&mut tx, twitch_user_id).await?;
        sqlx::query!(
            "UPDATE twitch_raid_auth
                SET access_token_enc = NULL,
                    refresh_token_enc = NULL,
                    needs_reauth = TRUE,
                    reauth_notified_at = $2,
                    last_refreshed_at = $2
              WHERE twitch_user_id = $1",
            twitch_user_id,
            now
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Beendet nur die Uplink-Nutzung. Gemeinsame Raid-/Bot-Grants bleiben
    /// bestehen; ein Widerruf bei Twitch wird ausdrücklich nicht behauptet.
    pub async fn disconnect_uplink(
        &self,
        uid: &str,
        _now: DateTime<Utc>,
    ) -> Result<(), AuthWriteError> {
        tokio::time::timeout(
            std::time::Duration::from_secs(35),
            self.disconnect_uplink_inner(uid),
        )
        .await
        .map_err(|_| AuthWriteError::Timeout)?
    }
    /// Nur nach bestätigtem Relay-DELETE derselben Generation abschließen.
    /// Ein inzwischen neuer Connect bleibt erhalten (false = überholt).
    pub async fn finish_uplink_disconnect(
        &self,
        uid: &str,
        generation: i64,
    ) -> Result<bool, AuthWriteError> {
        tokio::time::timeout(std::time::Duration::from_secs(15),async{
            let Some(mut tx)=crate::target_generation::disconnect_transaction(&self.pool,uid,"twitch",generation).await? else { return Ok(false) };
            sqlx::query("INSERT INTO twitch_uplink_auth_intent(twitch_user_id,enabled,last_disconnected_at) SELECT twitch_user_id,false,last_disconnected_at FROM uplink_target_generations WHERE twitch_user_id=$1 AND platform='twitch' AND generation=$2 ON CONFLICT(twitch_user_id) DO UPDATE SET enabled=false,last_disconnected_at=GREATEST(twitch_uplink_auth_intent.last_disconnected_at,EXCLUDED.last_disconnected_at)").bind(uid).bind(generation).execute(&mut *tx).await?;
            tx.commit().await?;
            Ok(true)
        }).await.map_err(|_|AuthWriteError::Timeout)?
    }
    async fn disconnect_uplink_inner(&self, uid: &str) -> Result<(), AuthWriteError> {
        if !valid_uid(uid) {
            return Err(AuthWriteError::InvalidIdentity);
        }
        let mut tx = auth_transaction(&self.pool, uid).await?;
        disconnect_intent(&mut tx, uid).await?;
        tx.commit().await?;
        Ok(())
    }
}

pub(crate) fn valid_uid(uid: &str) -> bool {
    !uid.is_empty()
        && uid.len() <= 32
        && !uid.starts_with('0')
        && uid.bytes().all(|b| b.is_ascii_digit())
}
// Lockreihenfolge für Callback, Refresh, Blacklist und Trennen:
// Pool-Verbindung → UID-Advisory-Lock → Auth-/Intent-/Partner-/Blacklist-Zeilen.
// Unter dem Lock keine zweite Poolanforderung; alle Schreibpfade verwenden
// dieselbe Transaktion. HTTP bleibt zeitlich begrenzt, Gesamtoperation 35 s.
pub(crate) async fn auth_transaction<'a>(
    pool: &'a PgPool,
    uid: &str,
) -> Result<Transaction<'a, Postgres>, sqlx::Error> {
    let mut tx = tokio::time::timeout(std::time::Duration::from_secs(5), pool.begin())
        .await
        .map_err(|_| sqlx::Error::PoolTimedOut)??;
    sqlx::query("SET LOCAL lock_timeout='5s'")
        .execute(&mut *tx)
        .await?;
    sqlx::query("SET LOCAL statement_timeout='10s'")
        .execute(&mut *tx)
        .await?;
    sqlx::query("SET LOCAL idle_in_transaction_session_timeout='30s'")
        .execute(&mut *tx)
        .await?;
    let (a, b) = advisory_lock_pair(uid);
    sqlx::query("SELECT pg_advisory_xact_lock($1,$2)")
        .bind(a)
        .bind(b)
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}
async fn disconnect_intent(
    tx: &mut Transaction<'_, Postgres>,
    uid: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO twitch_uplink_auth_intent(twitch_user_id,enabled,last_disconnected_at) VALUES($1,FALSE,clock_timestamp()) ON CONFLICT(twitch_user_id) DO UPDATE SET enabled=FALSE,last_disconnected_at=GREATEST(twitch_uplink_auth_intent.last_disconnected_at,clock_timestamp())")
        .bind(uid).execute(&mut **tx).await?;
    Ok(())
}
pub(crate) async fn verify(
    client: &dyn TwitchTokenClient,
    token: &str,
    uid: &str,
) -> Result<TokenValidation, AuthWriteError> {
    if token.is_empty() || token.len() > 4096 {
        return Err(AuthWriteError::InvalidToken);
    }
    let verified = tokio::time::timeout(
        std::time::Duration::from_secs(9),
        client.validate_token(token, uid),
    )
    .await
    .map_err(|_| AuthWriteError::ValidationUnavailable)?
    .map_err(|error| match error {
        RefreshError::InvalidGrant => AuthWriteError::InvalidToken,
        _ => AuthWriteError::ValidationUnavailable,
    })?;
    if verified.twitch_user_id != uid
        || verified.client_id.is_empty()
        || verified.expires_in <= 0
        || verified.scopes.len() > 128
        || verified.scopes.iter().any(|s| {
            s.is_empty()
                || s.len() > 128
                || !s.is_ascii()
                || s.bytes()
                    .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
        })
    {
        return Err(AuthWriteError::InvalidToken);
    }
    validated_expiry(Utc::now(), verified.expires_in)?;
    Ok(verified)
}

pub(crate) fn validated_expiry(
    at: DateTime<Utc>,
    seconds: i64,
) -> Result<DateTime<Utc>, AuthWriteError> {
    let duration = Duration::try_seconds(seconds)
        .filter(|d| *d > Duration::zero())
        .ok_or(AuthWriteError::InvalidToken)?;
    at.checked_add_signed(duration)
        .ok_or(AuthWriteError::InvalidToken)
}
