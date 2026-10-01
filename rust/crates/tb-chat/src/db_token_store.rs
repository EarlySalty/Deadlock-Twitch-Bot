//! Verschlüsselter Runtime-Speicher des eigenen Chat-Bot-Kontos.
//! Infrastruktur (OAuth-App und Master-Key) bleibt im Secret-Manager.
use crate::{secret_sink::SecretSink, token::SeedTokens};
use sqlx::{PgPool, Row};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tb_crypto::FieldCipher;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapError {
    DatabaseUnavailable,
    StorageConfiguration,
    Encryption,
    MissingOrRevoked,
    Decryption,
    Identity,
}
impl std::fmt::Display for BootstrapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::DatabaseUnavailable => "Bot-Token-Datenbank vorübergehend nicht verfügbar",
            Self::StorageConfiguration => "Bot-Token-Datenbankvertrag ungültig",
            Self::Encryption => "Bot-Token-Verschlüsselung fehlgeschlagen",
            Self::MissingOrRevoked => "Bot-Zugang fehlt oder wurde widerrufen",
            Self::Decryption => "Bot-Zugang nicht entschlüsselbar",
            Self::Identity => "Bot-Zugang gehört zu einem anderen Konto",
        })
    }
}
impl std::error::Error for BootstrapError {}
fn bootstrap_database_error(error: sqlx::Error) -> BootstrapError {
    let transient = match error {
        sqlx::Error::Io(_)
        | sqlx::Error::Tls(_)
        | sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::WorkerCrashed => true,
        sqlx::Error::Database(error) => error
            .code()
            .is_some_and(|code| retryable_database_code(&code)),
        _ => false,
    };
    if transient {
        BootstrapError::DatabaseUnavailable
    } else {
        BootstrapError::StorageConfiguration
    }
}
fn retryable_database_code(code: &str) -> bool {
    code.starts_with("08")
        || code.starts_with("40")
        || matches!(
            code,
            "57P01" | "57P02" | "57P03" | "53300" | "55P03" | "57014"
        )
}

pub struct DatabaseTokenStore {
    pool: PgPool,
    cipher: Arc<FieldCipher>,
    client_id: String,
    revision: Mutex<i64>,
    identity: Mutex<Option<String>>,
    healthy: AtomicBool,
    terminal: AtomicBool,
}
impl DatabaseTokenStore {
    pub fn new(pool: PgPool, cipher: Arc<FieldCipher>, client_id: String) -> Self {
        Self {
            pool,
            cipher,
            client_id,
            revision: Mutex::new(0),
            identity: Mutex::new(None),
            healthy: AtomicBool::new(false),
            terminal: AtomicBool::new(false),
        }
    }
    fn aad(&self, field: &str) -> String {
        format!("twitch_bot_tokens|{field}|twitch-chat|{}|1", self.client_id)
    }
    pub fn healthy(&self) -> bool {
        self.healthy.load(Ordering::Acquire)
    }

    /// Nur beim ersten Cutover einen Infisical-Seed übernehmen. Ein bestehender
    /// Datensatz, insbesondere ein widerrufener, gewinnt immer vor dem Alt-Seed.
    pub async fn seed_and_load(&self, seed: SeedTokens) -> Result<SeedTokens, BootstrapError> {
        if let Some(refresh) = seed.refresh_token.as_deref().filter(|v| !v.is_empty()) {
            let refresh_enc = self
                .cipher
                .encrypt_field(refresh, &self.aad("refresh_token"))
                .map_err(|_| BootstrapError::Encryption)?;
            let access_enc = seed
                .access_token
                .as_deref()
                .map(|v| self.cipher.encrypt_field(v, &self.aad("access_token")))
                .transpose()
                .map_err(|_| BootstrapError::Encryption)?;
            sqlx::query("INSERT INTO twitch_bot_tokens(service_name,oauth_client_id,access_token_enc,refresh_token_enc) VALUES('twitch-chat',$1,$2,$3) ON CONFLICT(service_name) DO NOTHING")
                .bind(&self.client_id).bind(access_enc).bind(refresh_enc).execute(&self.pool).await.map_err(bootstrap_database_error)?;
        }
        let row=sqlx::query("SELECT access_token_enc,refresh_token_enc,revision,twitch_user_id FROM twitch_bot_tokens WHERE service_name='twitch-chat' AND oauth_client_id=$1 AND revoked_at IS NULL")
            .bind(&self.client_id).fetch_optional(&self.pool).await.map_err(bootstrap_database_error)?.ok_or(BootstrapError::MissingOrRevoked)?;
        let access: Option<Vec<u8>> = row.get("access_token_enc");
        let refresh: Vec<u8> = row.get("refresh_token_enc");
        let result = SeedTokens {
            access_token: access
                .map(|v| self.cipher.decrypt_field(&v, &self.aad("access_token")))
                .transpose()
                .map_err(|_| BootstrapError::Decryption)?,
            refresh_token: Some(
                self.cipher
                    .decrypt_field(&refresh, &self.aad("refresh_token"))
                    .map_err(|_| BootstrapError::Decryption)?,
            ),
        };
        *self.revision.lock().await = row.get("revision");
        *self.identity.lock().await = row.get("twitch_user_id");
        self.healthy.store(true, Ordering::Release);
        Ok(result)
    }

    pub async fn bind_identity(&self, user_id: &str) -> Result<(), BootstrapError> {
        if user_id.is_empty() || !user_id.bytes().all(|b| b.is_ascii_digit()) {
            return Err(BootstrapError::Identity);
        }
        let n=sqlx::query("UPDATE twitch_bot_tokens SET twitch_user_id=$1 WHERE service_name='twitch-chat' AND oauth_client_id=$2 AND revoked_at IS NULL AND (twitch_user_id IS NULL OR twitch_user_id=$1)")
            .bind(user_id).bind(&self.client_id).execute(&self.pool).await.map_err(bootstrap_database_error)?.rows_affected();
        if n != 1 {
            return Err(BootstrapError::Identity);
        }
        *self.identity.lock().await = Some(user_id.to_string());
        Ok(())
    }

    pub async fn persist(&self, access: &str, refresh: Option<&str>) -> Result<(), &'static str> {
        let access_enc = self
            .cipher
            .encrypt_field(access, &self.aad("access_token"))
            .map_err(|_| "Bot-Token-Verschlüsselung fehlgeschlagen")?;
        let refresh_enc = refresh
            .map(|v| self.cipher.encrypt_field(v, &self.aad("refresh_token")))
            .transpose()
            .map_err(|_| "Bot-Token-Verschlüsselung fehlgeschlagen")?;
        let mut revision = self.revision.lock().await;
        let n=sqlx::query("UPDATE twitch_bot_tokens SET access_token_enc=$1,refresh_token_enc=COALESCE($2,refresh_token_enc),revision=revision+1,updated_at=now() WHERE service_name='twitch-chat' AND oauth_client_id=$3 AND revision=$4 AND revoked_at IS NULL")
            .bind(access_enc).bind(refresh_enc).bind(&self.client_id).bind(*revision).execute(&self.pool).await.map_err(|_|"Bot-Token-Datenbank nicht verfügbar")?.rows_affected();
        if n != 1 {
            // Ein verlorenes Commit-ACK lässt die lokale Revision unverändert.
            // Nur genau die nächste Revision mit denselben logischen Werten
            // darf als eigene bereits bestätigte Rotation gelten.
            let row = sqlx::query("SELECT revision,access_token_enc,refresh_token_enc,twitch_user_id FROM twitch_bot_tokens WHERE service_name='twitch-chat' AND oauth_client_id=$1 AND revoked_at IS NULL")
                .bind(&self.client_id).fetch_optional(&self.pool).await
                .map_err(|_| "Bot-Token-Datenbank nicht verfügbar")?;
            if let Some(row) = row {
                let stored_access: Option<Vec<u8>> = row.get("access_token_enc");
                let stored_refresh: Vec<u8> = row.get("refresh_token_enc");
                let same_access = stored_access
                    .as_deref()
                    .and_then(|v| self.cipher.decrypt_field(v, &self.aad("access_token")).ok())
                    .is_some_and(|v| v == access);
                let same_refresh = refresh.is_some_and(|expected| {
                    self.cipher
                        .decrypt_field(&stored_refresh, &self.aad("refresh_token"))
                        .is_ok_and(|v| v == expected)
                });
                let same_identity =
                    row.get::<Option<String>, _>("twitch_user_id") == *self.identity.lock().await;
                if row.get::<i64, _>("revision") == *revision + 1
                    && same_access
                    && same_refresh
                    && same_identity
                {
                    *revision += 1;
                    return Ok(());
                }
            }
            self.terminal.store(true, Ordering::Release);
            return Err("Bot-Zugang wurde gleichzeitig geändert oder widerrufen");
        }
        *revision += 1;
        Ok(())
    }
}
#[cfg(test)]
#[path = "db_token_store_tests.rs"]
mod tests;

#[async_trait::async_trait]
impl SecretSink for DatabaseTokenStore {
    fn terminal_failure(&self) -> bool {
        self.terminal.load(Ordering::Acquire)
    }
    async fn prepare_refresh(&self) -> Result<(), ()> {
        let revision = self.revision.lock().await;
        let result = sqlx::query("UPDATE twitch_bot_tokens SET updated_at=updated_at WHERE service_name='twitch-chat' AND oauth_client_id=$1 AND revision=$2 AND revoked_at IS NULL")
            .bind(&self.client_id).bind(*revision).execute(&self.pool).await;
        let healthy = match result {
            Ok(result) => {
                let healthy = result.rows_affected() == 1;
                if !healthy {
                    self.terminal.store(true, Ordering::Release);
                }
                healthy
            }
            Err(_) => false,
        };
        self.healthy.store(healthy, Ordering::Release);
        healthy.then_some(()).ok_or(())
    }
    async fn persist_checked(
        &self,
        access_token: &str,
        refresh_token: Option<&str>,
    ) -> Result<(), ()> {
        let result = self.persist(access_token, refresh_token).await;
        self.healthy.store(result.is_ok(), Ordering::Release);
        result.map_err(|_| ())
    }
    async fn persist_bot_tokens(&self, access_token: &str, refresh_token: Option<&str>) {
        let result = self.persist(access_token, refresh_token).await;
        self.healthy.store(result.is_ok(), Ordering::Release);
        if let Err(reason) = result {
            tracing::error!(
                reason,
                "Bot-Token-Rückschreibung fehlgeschlagen; kein Datei-Fallback"
            );
        }
    }
}
