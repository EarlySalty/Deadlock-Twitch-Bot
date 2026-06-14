//! Token-Refresh-Schreibpfad (`twitch_raid_auth`).
//!
//! Port von `RaidAuthManager.refresh_token` + `_write_token_refresh` +
//! `_acquire_refresh_db_lock`. **Security-Kern** — selbst gebaut.
//!
//! Saubere Trennung statt Python-Monolith: Die zwei Außenkopplungen sind Ports —
//! [`TwitchTokenClient`] (HTTP zu Twitch, echte Impl in `tb-transport-twitch`)
//! und [`TokenBlacklist`] (Lockout-Store, echte Impl in 6b). Der Refresher-Kern
//! (Advisory-Lock + verschlüsseltes Zurückschreiben) bleibt ohne Netz/Blacklist
//! testbar.
//!
//! Invarianten (1:1 zu Python, sonst Lockout/Leak):
//! - Refresh pro Broadcaster über `pg_advisory_xact_lock` serialisiert
//!   (blake2s-Key-Paar identisch zu Python `_refresh_advisory_lock_pair`) —
//!   kein paralleler Doppel-Refresh über Prozesse hinweg.
//! - Schlägt das Verschlüsseln fehl, wird **nichts** geschrieben (kein Lockout
//!   durch Klartext/leere Felder).
//! - Nur ein echtes `invalid_grant`/`invalid refresh token` blacklistet den
//!   Streamer; andere Fehler nicht.
//! - Neue Blobs mit `enc_version=1, enc_kid='v1'` (aktuelle Schreibversion).

use std::sync::Arc;

use blake2::digest::consts::U8;
use blake2::{Blake2s, Digest};
use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use tb_crypto::{aad, FieldCipher};

use crate::util::mask_log_identifier as mask;

/// Twitch-Token-Antwort (`/oauth2/token`).
#[derive(Debug, Clone)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    /// Gültigkeit in Sekunden (`expires_in`).
    pub expires_in: i64,
    /// Gewährte Scopes (Twitch `scope`-Array). Beim Refresh ungenutzt
    /// (Scopes bleiben erhalten), beim Exchange/Onboarding relevant.
    pub scopes: Vec<String>,
}

/// Fehlerklassen des Token-Endpoints — steuern Blacklist vs. nicht.
#[derive(Debug, Clone)]
pub enum RefreshError {
    /// `invalid_client` — Client-Credentials abgelehnt (NICHT den Streamer sperren).
    InvalidClient,
    /// `invalid_grant`/`invalid refresh token` — Streamer muss neu auth'en (Blacklist).
    InvalidGrant,
    /// Sonstiger Fehler (Netz, 5xx, andere 4xx) — kein Blacklist.
    Other(String),
}

/// HTTP-Port zum Twitch-Token-Endpoint (echte Impl in `tb-transport-twitch`).
#[async_trait::async_trait]
pub trait TwitchTokenClient: Send + Sync {
    /// Erneuert ein Access-Token via `grant_type=refresh_token`.
    async fn refresh(&self, refresh_token: &str) -> Result<TokenResponse, RefreshError>;

    /// Tauscht einen Authorization-Code gegen Tokens
    /// (`grant_type=authorization_code`, Python `exchange_code_for_token`).
    async fn exchange_code(&self, code: &str) -> Result<TokenResponse, RefreshError>;

    /// Ermittelt den Inhaber eines frischen Access-Tokens (Python
    /// `oauth_callback.py:130`: `GET /helix/users` mit dem User-Bearer ohne
    /// Parameter). Ohne diesen Schritt ist nach dem Code-Tausch unbekannt,
    /// WEM die Tokens gehören — der Persist-Pfad braucht User-ID + Login.
    async fn token_owner(&self, access_token: &str) -> Result<TokenOwnerInfo, RefreshError>;
}

/// Inhaber eines User-Access-Tokens (Login bereits lowercase-normalisiert).
#[derive(Debug, Clone)]
pub struct TokenOwnerInfo {
    pub twitch_user_id: String,
    pub twitch_login: String,
}

/// Lockout-Store-Port (`twitch_token_blacklist`, echte Impl `TokenBlacklistStore`).
#[async_trait::async_trait]
pub trait TokenBlacklist: Send + Sync {
    async fn is_blacklisted(&self, twitch_user_id: &str) -> bool;
    async fn has_recent_failure(&self, twitch_user_id: &str) -> bool;
    async fn add_to_blacklist(&self, twitch_user_id: &str, twitch_login: &str, error_message: &str);
    /// Nach erfolgreichem Refresh den Fehler-/Lockout-Zustand löschen.
    async fn clear_failure_count(&self, twitch_user_id: &str);
}

/// Ergebnis eines Refresh-Versuchs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshOutcome {
    /// Erfolgreich — neuer Access-Token (Wert NICHT loggen).
    Refreshed,
    /// Streamer geblacklistet (invalid_grant) — Refresh nicht möglich.
    Blacklisted,
    /// Übersprungen (geblacklistet/Cooldown/Client-blockiert) oder anderer Fehler.
    Skipped,
}

pub struct RaidTokenRefresher {
    pool: PgPool,
    cipher: Arc<FieldCipher>,
    client: Arc<dyn TwitchTokenClient>,
    blacklist: Arc<dyn TokenBlacklist>,
}

impl RaidTokenRefresher {
    pub fn new(
        pool: PgPool,
        cipher: Arc<FieldCipher>,
        client: Arc<dyn TwitchTokenClient>,
        blacklist: Arc<dyn TokenBlacklist>,
    ) -> Self {
        Self {
            pool,
            cipher,
            client,
            blacklist,
        }
    }

    /// Erneuert das Token eines Streamers und schreibt es verschlüsselt zurück.
    ///
    /// Reihenfolge:
    /// 1. Blacklist/Cooldown vorab prüfen (kein Lock nötig).
    /// 2. Transaktion beginnen + Advisory-Lock holen (serialisiert Zugriff
    ///    cross-process mit dem Python-Wartungssweep).
    /// 3. **Re-Read unter Lock**: frischesten `refresh_token` + `token_expires_at`
    ///    aus der DB lesen. Wenn ein paralleler Writer (Python-Sweep) den Token
    ///    bereits rotiert hat, verwenden wir seinen Refresh-Token — nicht den
    ///    vor dem Lock gelesenen. Ist das Token inzwischen frisch genug, überspringen.
    /// 4. HTTP-Refresh mit dem frischesten Token (unter Lock, damit kein dritter
    ///    Writer zwischen HTTP-Antwort und Write reinkommen kann).
    /// 5. Verschlüsseln + in dieselbe Transaktion schreiben → Commit + Lock frei.
    ///
    /// Lock-Dauer: schließt den HTTP-Call ein. Das ist der bewusste Trade-off —
    /// Korrektheit (kein Doppel-Refresh mit altem Token) geht vor kurzer Lock-Zeit.
    /// Der Lock ist pro Broadcaster; andere Broadcaster werden nicht blockiert.
    pub async fn refresh_and_store(
        &self,
        twitch_user_id: &str,
        twitch_login: &str,
        _current_refresh_token: &str,
        now: DateTime<Utc>,
    ) -> Result<RefreshOutcome, sqlx::Error> {
        if self.blacklist.is_blacklisted(twitch_user_id).await
            || self.blacklist.has_recent_failure(twitch_user_id).await
        {
            return Ok(RefreshOutcome::Skipped);
        }

        let mut tx = self.pool.begin().await?;
        let (lock_a, lock_b) = advisory_lock_pair(twitch_user_id);
        sqlx::query("SELECT pg_advisory_xact_lock($1, $2)")
            .bind(lock_a)
            .bind(lock_b)
            .execute(&mut *tx)
            .await?;

        // Re-Read unterm Lock: frischesten Stand holen, damit wir keinen bereits
        // invalidierten Refresh-Token von vor dem Lock verwenden (Twitch rotiert
        // Refresh-Tokens bei Nutzung — ein paralleler Python-Writer könnte ihn
        // inzwischen schon konsumiert haben).
        type RefreshRow = (Option<Vec<u8>>, Option<DateTime<Utc>>);
        let row: Option<RefreshRow> = sqlx::query_as(
            "SELECT refresh_token_enc, token_expires_at FROM twitch_raid_auth WHERE twitch_user_id = $1",
        )
        .bind(twitch_user_id)
        .fetch_optional(&mut *tx)
        .await?;

        let (fresh_refresh_token, fresh_expires_at) = match row {
            None => {
                // Zeile verschwunden — nichts zu tun.
                tx.commit().await?;
                return Ok(RefreshOutcome::Skipped);
            }
            Some((enc_bytes, expires_at)) => {
                let refresh_aad = aad::raid_auth("refresh_token", twitch_user_id, 1);
                let token = match enc_bytes {
                    Some(b) => match self.cipher.decrypt_field(&b, &refresh_aad) {
                        Ok(t) => t,
                        Err(_) => {
                            tracing::error!(
                                user = %mask(twitch_user_id),
                                "Re-Read unterm Lock: Entschlüsseln refresh_token fehlgeschlagen"
                            );
                            tx.commit().await?;
                            return Ok(RefreshOutcome::Skipped);
                        }
                    },
                    None => {
                        tx.commit().await?;
                        return Ok(RefreshOutcome::Skipped);
                    }
                };
                (token, expires_at)
            }
        };

        // Prüfen ob nach dem Lock inzwischen frisch genug — ein paralleler Writer
        // hat vielleicht schon refresht. Puffer 300 s (identisch zum Provider).
        const EXPIRY_PUFFER: i64 = 300;
        if let Some(exp) = fresh_expires_at {
            if now < exp - Duration::seconds(EXPIRY_PUFFER) {
                tx.commit().await?;
                return Ok(RefreshOutcome::Refreshed);
            }
        }

        let response = match self.client.refresh(&fresh_refresh_token).await {
            Ok(response) => response,
            Err(RefreshError::InvalidGrant) => {
                tx.commit().await?;
                self.blacklist
                    .add_to_blacklist(twitch_user_id, twitch_login, "invalid refresh grant")
                    .await;
                return Ok(RefreshOutcome::Blacklisted);
            }
            Err(RefreshError::InvalidClient | RefreshError::Other(_)) => {
                tx.commit().await?;
                return Ok(RefreshOutcome::Skipped);
            }
        };

        // Verschlüsseln — schlägt es fehl, NICHTS schreiben (Lockout-Schutz).
        let access_aad = aad::raid_auth("access_token", twitch_user_id, 1);
        let refresh_aad = aad::raid_auth("refresh_token", twitch_user_id, 1);
        let (Ok(access_enc), Ok(refresh_enc)) = (
            self.cipher
                .encrypt_field(&response.access_token, &access_aad),
            self.cipher
                .encrypt_field(&response.refresh_token, &refresh_aad),
        ) else {
            tracing::error!(
                user = %mask(twitch_user_id),
                "Refresh: Verschlüsseln fehlgeschlagen — Tokens NICHT geschrieben"
            );
            tx.commit().await?;
            return Ok(RefreshOutcome::Skipped);
        };

        // Floor gegen literal-0/negativ aus der Twitch-Antwort (fehlendes Feld
        // fängt bereits der serde-Default 3600 ab) — sonst sofort-stale-Token.
        let expires_at = now + Duration::seconds(response.expires_in.max(60));
        let result = sqlx::query(
            r#"
            UPDATE twitch_raid_auth
               SET access_token = 'ENC', refresh_token = 'ENC',
                   access_token_enc = $1, refresh_token_enc = $2,
                   enc_version = 1, enc_kid = 'v1',
                   token_expires_at = $3, last_refreshed_at = NOW()
             WHERE twitch_user_id = $4
            "#,
        )
        .bind(access_enc)
        .bind(refresh_enc)
        .bind(expires_at)
        .bind(twitch_user_id)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            // Keine Auth-Zeile getroffen → nichts geändert (wie Python).
            tx.commit().await?;
            tracing::error!(user = %mask(twitch_user_id), "Refresh: keine Auth-Zeile getroffen");
            return Ok(RefreshOutcome::Skipped);
        }
        tx.commit().await?;
        // Erfolgreicher Refresh → Fehler-/Lockout-Zustand löschen (Python).
        self.blacklist.clear_failure_count(twitch_user_id).await;
        Ok(RefreshOutcome::Refreshed)
    }
}

/// blake2s-Key-Paar für `pg_advisory_xact_lock` — byte-identisch zu Python
/// `_refresh_advisory_lock_pair` (digest_size=8, je 4 Byte big-endian signed).
pub fn advisory_lock_pair(twitch_user_id: &str) -> (i32, i32) {
    let mut hasher = Blake2s::<U8>::new();
    hasher.update(format!("twitch_raid_auth_refresh:{twitch_user_id}").as_bytes());
    let digest = hasher.finalize();
    let a = i32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]);
    let b = i32::from_be_bytes([digest[4], digest[5], digest[6], digest[7]]);
    (a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advisory_lock_pair_ist_deterministisch_und_stabil() {
        // Determinismus (gleicher Input → gleiches Paar).
        assert_eq!(advisory_lock_pair("42"), advisory_lock_pair("42"));
        // Verschiedene User → (praktisch immer) verschiedene Paare.
        assert_ne!(advisory_lock_pair("42"), advisory_lock_pair("43"));
    }

    #[test]
    fn advisory_lock_pair_ist_byte_identisch_zu_python() {
        // Referenzwerte aus hashlib.blake2s(..., digest_size=8) (Python).
        // Stellt sicher, dass Python-raid und Rust-raid bei Überlappung auf
        // DEMSELBEN Advisory-Lock serialisieren.
        assert_eq!(advisory_lock_pair("42"), (-369953205, 918188134));
        assert_eq!(advisory_lock_pair("abc123"), (1217983055, 837247636));
    }
}
