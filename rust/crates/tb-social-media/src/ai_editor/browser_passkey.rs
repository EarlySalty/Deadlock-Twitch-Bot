use std::time::Duration;

use sqlx::PgPool;
use tb_crypto::FieldCipher;
use tb_llm::video_observer::{self, BrowserPasskey, ObserverError, Session, VideoObserverConfig};
use zeroize::Zeroizing;

const NAME: &str = "gemini_observer";
const SEED_ENV: &str = "GOOGLE_STEEL_PASSKEY";
const INTERVAL: Duration = Duration::from_secs(300);

pub async fn keep_gemini_session(pool: PgPool, config: VideoObserverConfig) {
    loop {
        let outcome = match load(&pool).await {
            Ok(passkey) => video_observer::check_session(&config, &passkey).await,
            Err(error) => Err(error),
        };
        match outcome {
            Ok(Session::Ready) => tracing::debug!("gemini session ready"),
            Ok(Session::Relogged) => tracing::info!("gemini session signed in again via passkey"),
            Err(error) => tracing::warn!(%error, "gemini session keeper failed"),
        }
        tokio::time::sleep(INTERVAL).await;
    }
}

pub(super) async fn load(pool: &PgPool) -> Result<BrowserPasskey, ObserverError> {
    let cipher = FieldCipher::from_env().map_err(|_| ObserverError::Configuration)?;
    let blob = match stored(pool).await? {
        Some(blob) => blob,
        None => seed(pool, &cipher).await?,
    };
    let raw = Zeroizing::new(
        cipher
            .decrypt_field(&blob, &aad())
            .map_err(|_| ObserverError::Configuration)?,
    );
    BrowserPasskey::parse(&raw)
}

async fn stored(pool: &PgPool) -> Result<Option<Vec<u8>>, ObserverError> {
    sqlx::query_scalar("SELECT passkey_enc FROM public.social_ai_browser_passkeys WHERE name = $1")
        .bind(NAME)
        .fetch_optional(pool)
        .await
        .map_err(|_| ObserverError::Unavailable)
}

async fn seed(pool: &PgPool, cipher: &FieldCipher) -> Result<Vec<u8>, ObserverError> {
    let raw = Zeroizing::new(std::env::var(SEED_ENV).map_err(|_| ObserverError::Configuration)?);
    BrowserPasskey::parse(&raw)?;
    let blob = cipher
        .encrypt_field(&raw, &aad())
        .map_err(|_| ObserverError::Configuration)?;
    sqlx::query(
        "INSERT INTO public.social_ai_browser_passkeys (name, passkey_enc) VALUES ($1, $2) \
         ON CONFLICT (name) DO NOTHING",
    )
    .bind(NAME)
    .bind(&blob)
    .execute(pool)
    .await
    .map_err(|_| ObserverError::Unavailable)?;
    stored(pool).await?.ok_or(ObserverError::Unavailable)
}

fn aad() -> String {
    format!("social_ai_browser_passkeys:passkey_enc:{NAME}")
}
