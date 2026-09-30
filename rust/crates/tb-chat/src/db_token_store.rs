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

pub struct DatabaseTokenStore {
    pool: PgPool,
    cipher: Arc<FieldCipher>,
    client_id: String,
    revision: Mutex<i64>,
    healthy: AtomicBool,
}
impl DatabaseTokenStore {
    pub fn new(pool: PgPool, cipher: Arc<FieldCipher>, client_id: String) -> Self {
        Self {
            pool,
            cipher,
            client_id,
            revision: Mutex::new(0),
            healthy: AtomicBool::new(false),
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
    pub async fn seed_and_load(&self, seed: SeedTokens) -> Result<SeedTokens, &'static str> {
        if let Some(refresh) = seed.refresh_token.as_deref().filter(|v| !v.is_empty()) {
            let refresh_enc = self
                .cipher
                .encrypt_field(refresh, &self.aad("refresh_token"))
                .map_err(|_| "Bot-Token-Verschlüsselung fehlgeschlagen")?;
            let access_enc = seed
                .access_token
                .as_deref()
                .map(|v| self.cipher.encrypt_field(v, &self.aad("access_token")))
                .transpose()
                .map_err(|_| "Bot-Token-Verschlüsselung fehlgeschlagen")?;
            sqlx::query("INSERT INTO twitch_bot_tokens(service_name,oauth_client_id,access_token_enc,refresh_token_enc) VALUES('twitch-chat',$1,$2,$3) ON CONFLICT(service_name) DO NOTHING")
                .bind(&self.client_id).bind(access_enc).bind(refresh_enc).execute(&self.pool).await.map_err(|_|"Bot-Token-Datenbank nicht verfügbar")?;
        }
        let row=sqlx::query("SELECT access_token_enc,refresh_token_enc,revision FROM twitch_bot_tokens WHERE service_name='twitch-chat' AND oauth_client_id=$1 AND revoked_at IS NULL")
            .bind(&self.client_id).fetch_optional(&self.pool).await.map_err(|_|"Bot-Token-Datenbank nicht verfügbar")?.ok_or("Bot-Zugang fehlt oder wurde widerrufen")?;
        let access: Option<Vec<u8>> = row.get("access_token_enc");
        let refresh: Vec<u8> = row.get("refresh_token_enc");
        let result = SeedTokens {
            access_token: access
                .map(|v| self.cipher.decrypt_field(&v, &self.aad("access_token")))
                .transpose()
                .map_err(|_| "Bot-Zugang nicht entschlüsselbar")?,
            refresh_token: Some(
                self.cipher
                    .decrypt_field(&refresh, &self.aad("refresh_token"))
                    .map_err(|_| "Bot-Zugang nicht entschlüsselbar")?,
            ),
        };
        *self.revision.lock().await = row.get("revision");
        self.healthy.store(true, Ordering::Release);
        Ok(result)
    }

    pub async fn bind_identity(&self, user_id: &str) -> Result<(), &'static str> {
        if user_id.is_empty() || !user_id.bytes().all(|b| b.is_ascii_digit()) {
            return Err("Ungültige Bot-Identität");
        }
        let n=sqlx::query("UPDATE twitch_bot_tokens SET twitch_user_id=$1 WHERE service_name='twitch-chat' AND oauth_client_id=$2 AND revoked_at IS NULL AND (twitch_user_id IS NULL OR twitch_user_id=$1)")
            .bind(user_id).bind(&self.client_id).execute(&self.pool).await.map_err(|_|"Bot-Identität nicht speicherbar")?.rows_affected();
        if n != 1 {
            return Err("Bot-Zugang gehört zu einem anderen Konto");
        }
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
