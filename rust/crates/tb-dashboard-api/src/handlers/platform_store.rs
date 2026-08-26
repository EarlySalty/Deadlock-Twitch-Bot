//! Der plattformneutrale Token-Speicher `platform_connections`.
//!
//! Twitch liegt hier nicht mehr: dafuer gibt es den Streamer-OAuth und
//! `twitch_raid_auth` (siehe `platform_token.rs`). Zwei Token-Staende fuer
//! dasselbe Konto hiessen, dass einer irgendwann der falsche ist.
//!
//! Fuer Kick, YouTube und TikTok bleibt dieser Speicher stehen. Die haben
//! keinen Raid-Bot, an dessen Grant sich ein Chat-Zugang anhaengen liesse, und
//! wenn sie drankommen, ist die verschluesselte Ablage samt AAD schon gebaut
//! und geprueft. Heute ist die Tabelle leer; die Lesepfade liefern deshalb
//! ueberall "getrennt" beziehungsweise 404.
//!
//! Was hier steht, hat einen Aufrufer. Der Verbinden-Flow, der eigene Callback
//! und der Refresh-Job sind weg, und mit ihnen die Schreibseite: sie kommt
//! zurueck, wenn die erste dieser Plattformen gebaut wird, und steht bis dahin
//! in der Git-Historie statt als toter Code im Baum.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tb_crypto::FieldCipher;

// ───────────────────────────────────────────────────────────────────────────
// Speicher
// ───────────────────────────────────────────────────────────────────────────

/// Eine gespeicherte Verbindung, entschluesselt. Der Refresh-Token bleibt
/// absichtlich in diesem Typ und wandert nie in eine HTTP-Antwort.
#[derive(Debug, Clone)]
pub struct PlatformConnection {
    pub streamer_id: i64,
    pub platform: String,
    pub platform_user_id: String,
    pub platform_login: String,
    pub access_token: String,
    pub refresh_token: String,
    pub scopes: Vec<String>,
    pub expires_at: DateTime<Utc>,
    pub needs_reauth: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreFehler {
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
    #[error("crypto: {0}")]
    Crypto(String),
}

/// Zugriff auf `platform_connections` samt Feldverschluesselung.
#[derive(Clone)]
pub struct PlatformConnectionStore {
    pool: PgPool,
    cipher: Arc<FieldCipher>,
}

#[derive(sqlx::FromRow)]
struct Zeile {
    streamer_id: i64,
    platform: String,
    platform_user_id: String,
    platform_login: String,
    access_token_enc: Vec<u8>,
    refresh_token_enc: Vec<u8>,
    scopes: Vec<String>,
    expires_at: DateTime<Utc>,
    needs_reauth: bool,
}

const SELECT_ZEILE: &str = "SELECT streamer_id, platform, platform_user_id, platform_login, \
     access_token_enc, refresh_token_enc, scopes, expires_at, needs_reauth \
     FROM platform_connections";

impl PlatformConnectionStore {
    pub fn new(pool: PgPool, cipher: Arc<FieldCipher>) -> Self {
        Self { pool, cipher }
    }

    /// Zusatzdaten der Verschluesselung: bindet den Blob an Streamer und
    /// Plattform, damit ein umkopierter Blob in einer anderen Zeile nicht
    /// aufgeht.
    pub fn aad(streamer_id: i64, platform: &str) -> String {
        format!("platform_connections:{streamer_id}:{platform}")
    }

    fn entschluesseln(&self, zeile: Zeile) -> Result<PlatformConnection, StoreFehler> {
        let aad = Self::aad(zeile.streamer_id, &zeile.platform);
        let access_token = self
            .cipher
            .decrypt_field(&zeile.access_token_enc, &aad)
            .map_err(|e| StoreFehler::Crypto(e.to_string()))?;
        let refresh_token = self
            .cipher
            .decrypt_field(&zeile.refresh_token_enc, &aad)
            .map_err(|e| StoreFehler::Crypto(e.to_string()))?;
        Ok(PlatformConnection {
            streamer_id: zeile.streamer_id,
            platform: zeile.platform,
            platform_user_id: zeile.platform_user_id,
            platform_login: zeile.platform_login,
            access_token,
            refresh_token,
            scopes: zeile.scopes,
            expires_at: zeile.expires_at,
            needs_reauth: zeile.needs_reauth,
        })
    }

    pub async fn load(
        &self,
        streamer_id: i64,
        platform: &str,
    ) -> Result<Option<PlatformConnection>, StoreFehler> {
        self.load_mit(&self.pool, streamer_id, platform, false)
            .await
    }

    /// Wie [`Self::load`], auf einem Executor; mit `sperren` haelt die
    /// umgebende Transaktion die Zeile per `FOR UPDATE`, bis sie endet.
    async fn load_mit<'e, E>(
        &self,
        exec: E,
        streamer_id: i64,
        platform: &str,
        sperren: bool,
    ) -> Result<Option<PlatformConnection>, StoreFehler>
    where
        E: sqlx::PgExecutor<'e>,
    {
        let sperre = if sperren { " FOR UPDATE" } else { "" };
        let zeile: Option<Zeile> = sqlx::query_as(&format!(
            "{SELECT_ZEILE} WHERE streamer_id = $1 AND platform = $2{sperre}"
        ))
        .bind(streamer_id)
        .bind(platform)
        .fetch_optional(exec)
        .await?;
        zeile.map(|z| self.entschluesseln(z)).transpose()
    }

    /// Verbindungen eines Streamers als (Plattform, Status). Status ist
    /// `verbunden` oder `neu_verbinden`; was nicht in der Tabelle steht, ist
    /// `getrennt` und wird vom Aufrufer ergaenzt.
    pub async fn status_liste(
        &self,
        streamer_id: i64,
    ) -> Result<Vec<(String, &'static str)>, StoreFehler> {
        let zeilen: Vec<(String, bool)> = sqlx::query_as(
            "SELECT platform, needs_reauth FROM platform_connections \
             WHERE streamer_id = $1 ORDER BY platform",
        )
        .bind(streamer_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(zeilen
            .into_iter()
            .map(|(p, reauth)| (p, if reauth { "neu_verbinden" } else { "verbunden" }))
            .collect())
    }
}
