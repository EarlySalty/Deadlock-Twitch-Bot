//! Verschlüsselter Token-Lesepfad (`twitch_raid_auth`).
//!
//! Port von `RaidAuthManager._resolve_token` + dem Lese-Teil von
//! `get_valid_token`/`get_tokens_for_user`/`get_scopes`. **Security-Kern** —
//! deshalb bewusst nicht delegiert.
//!
//! Invarianten (1:1 zu Python, sonst Lockout/Leak):
//!
//! - Tokens werden **ausschließlich** aus den verschlüsselten `_enc`-bytea-Spalten
//!   gelesen. Die Klartext-Spalten `access_token`/`refresh_token` sind Legacy
//!   (`'ENC'`-Platzhalter) und werden NIE als Fallback verwendet.
//! - Die AAD nutzt das **`enc_version` aus der Zeile** (dynamisch, nicht fix 1):
//!   `twitch_raid_auth|<column>|<twitch_user_id>|<enc_version>`
//!   (siehe [`tb_crypto::aad::raid_auth`]).
//! - Entschlüsselungs-Fehler (Tag-Mismatch, falsche AAD, NULL-Blob) → der Token
//!   gilt als **nicht verfügbar** (`None`), kein Fehler nach oben — wie Pythons
//!   `_try_decrypt`, das bei Misserfolg `None` liefert.
//! - Klartext-Tokens landen **niemals** in Logs (nur maskierte user_ids).

use std::sync::Arc;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tb_crypto::{aad, FieldCipher};

use crate::util::mask_log_identifier as mask;

/// Entschlüsseltes Token-Bündel eines Streamers.
#[derive(Debug, Clone)]
pub struct RaidTokens {
    pub twitch_user_id: String,
    pub twitch_login: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_expires_at: Option<DateTime<Utc>>,
    pub needs_reauth: bool,
}

/// Roh-Zeile aus `twitch_raid_auth` (prod-verifizierte Typen: `_enc` bytea,
/// `enc_version` int, Timestamps timestamptz, Flags boolean).
#[derive(sqlx::FromRow)]
struct RaidAuthRow {
    twitch_login: String,
    access_token_enc: Option<Vec<u8>>,
    refresh_token_enc: Option<Vec<u8>>,
    enc_version: Option<i32>,
    token_expires_at: Option<DateTime<Utc>>,
    needs_reauth: Option<bool>,
}

/// Lesezugriff auf den verschlüsselten Raid-Token-Store.
#[derive(Clone)]
pub struct RaidAuthStore {
    pool: PgPool,
    cipher: Arc<FieldCipher>,
}

impl RaidAuthStore {
    pub fn new(pool: PgPool, cipher: Arc<FieldCipher>) -> Self {
        Self { pool, cipher }
    }

    /// Lädt und entschlüsselt die Tokens eines **raid-aktivierten** Streamers
    /// (`raid_enabled IS TRUE`). Port von Python `get_valid_token` (auth.py:1567,
    /// raid-Pfad). Für API-Nutzungen, die auch bei `raid_enabled=0` greifen
    /// müssen (z. B. `!clip`), siehe [`load_decrypted_unrestricted`].
    ///
    /// `None` wenn: keine Zeile, `raid_enabled` falsch, oder der **Access-Token**
    /// nicht entschlüsselbar ist (Python: kein nutzbares Token → kein Refresh).
    /// Der Refresh-Token darf fehlen (z. B. wenn nur Access neu verschlüsselt
    /// wurde) — dann `refresh_token = None`.
    pub async fn load_decrypted(
        &self,
        twitch_user_id: &str,
    ) -> Result<Option<RaidTokens>, sqlx::Error> {
        self.load_inner(twitch_user_id, true).await
    }

    /// Wie [`load_decrypted`], aber **ohne** `raid_enabled`-Gate. Port von Python
    /// `get_tokens_for_user` (auth.py:1378), das laut Docstring „bewusst auch
    /// genutzt wird, wenn raid_enabled=0 (Chat-Bot/Moderation)" — etwa für
    /// `!clip`, das einem Streamer auch dann gehört, wenn er Raids deaktiviert
    /// hat. `needs_reauth` wird über das Rückgabefeld vom Aufrufer geprüft.
    pub async fn load_decrypted_unrestricted(
        &self,
        twitch_user_id: &str,
    ) -> Result<Option<RaidTokens>, sqlx::Error> {
        self.load_inner(twitch_user_id, false).await
    }

    /// Gemeinsamer Lese-/Entschlüsselungspfad. `require_raid_enabled` steuert
    /// nur die `WHERE`-Klausel — alles andere ist identisch.
    async fn load_inner(
        &self,
        twitch_user_id: &str,
        require_raid_enabled: bool,
    ) -> Result<Option<RaidTokens>, sqlx::Error> {
        // Zwei statische Statements statt format!() — keine SQL-Konkatenation.
        let sql = if require_raid_enabled {
            r#"
            SELECT twitch_login, access_token_enc, refresh_token_enc,
                   enc_version, token_expires_at, needs_reauth
            FROM twitch_raid_auth
            WHERE twitch_user_id = $1 AND raid_enabled IS TRUE
            "#
        } else {
            r#"
            SELECT twitch_login, access_token_enc, refresh_token_enc,
                   enc_version, token_expires_at, needs_reauth
            FROM twitch_raid_auth
            WHERE twitch_user_id = $1
            "#
        };
        let row: Option<RaidAuthRow> = sqlx::query_as(sql)
            .bind(twitch_user_id)
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };

        let enc_version = i64::from(row.enc_version.unwrap_or(1));
        let access_token = self.resolve_token(
            "access_token",
            row.access_token_enc.as_deref(),
            twitch_user_id,
            enc_version,
        );
        let Some(access_token) = access_token else {
            // Access nicht lesbar → Token nicht verfügbar (kein Plaintext-Fallback).
            return Ok(None);
        };
        let refresh_token = self.resolve_token(
            "refresh_token",
            row.refresh_token_enc.as_deref(),
            twitch_user_id,
            enc_version,
        );

        Ok(Some(RaidTokens {
            twitch_user_id: twitch_user_id.to_string(),
            twitch_login: row.twitch_login,
            access_token,
            refresh_token,
            token_expires_at: row.token_expires_at,
            needs_reauth: row.needs_reauth.unwrap_or(false),
        }))
    }

    /// Scopes eines Streamers als Liste (`scopes` ist Space-getrennter Text).
    pub async fn get_scopes(&self, twitch_user_id: &str) -> Result<Vec<String>, sqlx::Error> {
        let scopes: Option<Option<String>> =
            sqlx::query_scalar("SELECT scopes FROM twitch_raid_auth WHERE twitch_user_id = $1")
                .bind(twitch_user_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(scopes
            .flatten()
            .map(|raw| {
                raw.split_whitespace()
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default())
    }

    /// Entschlüsselt ein einzelnes `_enc`-Feld; `None` bei NULL oder Misserfolg
    /// (Python `_resolve_token`/`_try_decrypt`). Loggt nur maskiert, nie Klartext.
    fn resolve_token(
        &self,
        column: &str,
        enc_blob: Option<&[u8]>,
        twitch_user_id: &str,
        enc_version: i64,
    ) -> Option<String> {
        let blob = enc_blob.filter(|b| !b.is_empty())?;
        let aad = aad::raid_auth(column, twitch_user_id, enc_version);
        match self.cipher.decrypt_field(blob, &aad) {
            Ok(plaintext) if !plaintext.is_empty() => Some(plaintext),
            Ok(_) => None,
            Err(_) => {
                tracing::warn!(
                    column,
                    user = %mask(twitch_user_id),
                    "Verschlüsseltes Token-Feld unlesbar — wird ignoriert"
                );
                None
            }
        }
    }
}
