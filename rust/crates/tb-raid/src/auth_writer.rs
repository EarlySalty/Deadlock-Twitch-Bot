//! Onboarding-/Re-Auth-Schreibpfad (`twitch_raid_auth`) — Port von
//! `RaidAuthManager.save_auth`. **Security-Kern** — selbst gebaut.
//!
//! Schreibt frische OAuth-Tokens verschlüsselt in die DB (UPSERT), validiert
//! die gewährten Scopes gegen das aufgelöste Profil und setzt `needs_reauth`
//! zurück. Bewusste Invarianten (1:1 zu Python):
//!
//! - Scope-Set muss exakt dem Profil entsprechen, sonst `ScopeMismatch`
//!   (kein Speichern halb-autorisierter Tokens).
//! - Verschlüsseln fehlgeschlagen → `EncryptionFailed`, nichts geschrieben.
//! - `raid_enabled` eines bestehenden Eintrags bleibt erhalten
//!   (`activate_raid_features OR existing`).
//! - Die Partner-Raid-Aktivierung (`set_partner_raid_bot_enabled`) ist ein
//!   nachgelagerter Effekt und wird mit dem Partner-Store (6b+) verdrahtet —
//!   hier bewusst nicht enthalten.

use std::collections::BTreeSet;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use tb_crypto::{aad, FieldCipher};

use crate::scope_profiles::scopes_for_profile;
use crate::util::mask_log_identifier as mask;

/// Eingabe für [`AuthWriter::store_new_auth`].
#[derive(Debug, Clone)]
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
    Db(sqlx::Error),
}

impl std::fmt::Display for AuthWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthWriteError::ScopeMismatch { profile } => {
                write!(f, "unexpected_scopes_for_profile:{profile}")
            }
            AuthWriteError::EncryptionFailed => write!(f, "encryption failed"),
            AuthWriteError::Db(e) => write!(f, "db error: {e}"),
        }
    }
}
impl std::error::Error for AuthWriteError {}
impl From<sqlx::Error> for AuthWriteError {
    fn from(e: sqlx::Error) -> Self {
        AuthWriteError::Db(e)
    }
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
        now: DateTime<Utc>,
    ) -> Result<(), AuthWriteError> {
        // Scope-Set gegen Profil prüfen (getrimmt, Reihenfolge egal).
        let expected: BTreeSet<&str> = scopes_for_profile(&new.resolved_scope_profile)
            .iter()
            .copied()
            .collect();
        let granted: BTreeSet<String> = new
            .granted_scopes
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let granted_refs: BTreeSet<&str> = granted.iter().map(String::as_str).collect();
        if granted_refs != expected {
            return Err(AuthWriteError::ScopeMismatch {
                profile: new.resolved_scope_profile.clone(),
            });
        }

        let uid = &new.twitch_user_id;
        let (Ok(access_enc), Ok(refresh_enc)) = (
            self.cipher
                .encrypt_field(&new.access_token, &aad::raid_auth("access_token", uid, 1)),
            self.cipher
                .encrypt_field(&new.refresh_token, &aad::raid_auth("refresh_token", uid, 1)),
        ) else {
            tracing::error!(user = %mask(uid), "save_auth: Verschlüsseln fehlgeschlagen — nicht gespeichert");
            return Err(AuthWriteError::EncryptionFailed);
        };

        let expires_at = now + Duration::seconds(new.expires_in);
        // Gespeichert werden die Profil-Scopes (Python: `" ".join(expected_scopes)`),
        // nicht die rohe Liste — die Validierung oben garantiert Gleichheit.
        let scopes_for_db = scopes_for_profile(&new.resolved_scope_profile).join(" ");

        let mut tx = self.pool.begin().await?;
        // Bestehenden raid_enabled-Status erhalten (per user_id ODER login).
        let existing_raid_enabled: Option<bool> = sqlx::query_scalar(
            r#"
            SELECT raid_enabled FROM twitch_raid_auth
            WHERE twitch_user_id = $1 OR LOWER(COALESCE(twitch_login, '')) = LOWER($2)
            LIMIT 1
            "#,
        )
        .bind(uid)
        .bind(&new.twitch_login)
        .fetch_optional(&mut *tx)
        .await?
        .flatten();
        let raid_enabled = new.activate_raid_features || existing_raid_enabled.unwrap_or(false);

        sqlx::query(
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
        )
        .bind(uid)
        .bind(&new.twitch_login)
        .bind(access_enc)
        .bind(refresh_enc)
        .bind(expires_at)
        .bind(&scopes_for_db)
        .bind(now)
        .bind(raid_enabled)
        .execute(&mut *tx)
        .await?;

        // Re-Auth abgeschlossen → needs_reauth zurücksetzen.
        sqlx::query(
            "UPDATE twitch_raid_auth SET needs_reauth = FALSE, reauth_notified_at = NULL
             WHERE twitch_user_id = $1",
        )
        .bind(uid)
        .execute(&mut *tx)
        .await?;

        // Token-Blacklist-Eintrag entfernen (Python: save_auth ruft
        // token_error_handler.remove_from_blacklist). Ohne dies bleibt ein wegen
        // invalid_grant (error_count ≥ 3) blacklisteter Streamer nach erfolgreicher
        // Re-Autorisierung DAUERHAFT gesperrt: der Blacklist-Check in get_valid_token
        // greift vor allem anderen und liefert None. Zuerst den Partner-Pause-Grund
        // 'token_error' aufheben, dann den Blacklist-Eintrag löschen.
        sqlx::query(
            "UPDATE twitch_partners
                SET technical_pause_reason = CASE
                        WHEN LOWER(COALESCE(technical_pause_reason, '')) = 'token_error' THEN NULL
                        ELSE technical_pause_reason
                    END
              WHERE twitch_user_id = $1",
        )
        .bind(uid)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM twitch_token_blacklist WHERE twitch_user_id = $1")
            .bind(uid)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }
}
