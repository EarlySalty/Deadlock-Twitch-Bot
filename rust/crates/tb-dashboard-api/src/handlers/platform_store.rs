use std::future::Future;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tb_crypto::FieldCipher;

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

    pub async fn upsert(&self, verbindung: &PlatformConnection) -> Result<(), StoreFehler> {
        let aad = Self::aad(verbindung.streamer_id, &verbindung.platform);
        let access_enc = self
            .cipher
            .encrypt_field(&verbindung.access_token, &aad)
            .map_err(|e| StoreFehler::Crypto(e.to_string()))?;
        let refresh_enc = self
            .cipher
            .encrypt_field(&verbindung.refresh_token, &aad)
            .map_err(|e| StoreFehler::Crypto(e.to_string()))?;
        self.upsert_mit(
            &self.pool,
            verbindung,
            &access_enc,
            &refresh_enc,
        )
        .await
    }

    async fn upsert_mit<'e, E>(
        &self,
        exec: E,
        verbindung: &PlatformConnection,
        access_enc: &[u8],
        refresh_enc: &[u8],
    ) -> Result<(), StoreFehler>
    where
        E: sqlx::PgExecutor<'e>,
    {
        sqlx::query(
            "INSERT INTO platform_connections \
             (streamer_id, platform, platform_user_id, platform_login, \
              access_token_enc, refresh_token_enc, enc_kid, scopes, expires_at, needs_reauth, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, NOW()) \
             ON CONFLICT (streamer_id, platform) DO UPDATE SET \
                 platform_user_id = EXCLUDED.platform_user_id, \
                 platform_login = EXCLUDED.platform_login, \
                 access_token_enc = EXCLUDED.access_token_enc, \
                 refresh_token_enc = EXCLUDED.refresh_token_enc, \
                 enc_kid = EXCLUDED.enc_kid, \
                 scopes = EXCLUDED.scopes, \
                 expires_at = EXCLUDED.expires_at, \
                 needs_reauth = EXCLUDED.needs_reauth, \
                 updated_at = NOW()",
        )
        .bind(verbindung.streamer_id)
        .bind(&verbindung.platform)
        .bind(&verbindung.platform_user_id)
        .bind(&verbindung.platform_login)
        .bind(access_enc)
        .bind(refresh_enc)
        .bind(self.cipher.kid())
        .bind(&verbindung.scopes)
        .bind(verbindung.expires_at)
        .bind(verbindung.needs_reauth)
        .execute(exec)
        .await?;
        Ok(())
    }

    pub async fn delete(&self, streamer_id: i64, platform: &str) -> Result<bool, StoreFehler> {
        let ergebnis = sqlx::query(
            "DELETE FROM platform_connections WHERE streamer_id = $1 AND platform = $2",
        )
        .bind(streamer_id)
        .bind(platform)
        .execute(&self.pool)
        .await?;
        Ok(ergebnis.rows_affected() > 0)
    }

    pub async fn set_needs_reauth(
        &self,
        streamer_id: i64,
        platform: &str,
    ) -> Result<(), StoreFehler> {
        sqlx::query(
            "UPDATE platform_connections SET needs_reauth = TRUE, updated_at = NOW() \
             WHERE streamer_id = $1 AND platform = $2",
        )
        .bind(streamer_id)
        .bind(platform)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn faellige(
        &self,
        vorlauf: chrono::Duration,
        jetzt: DateTime<Utc>,
    ) -> Result<Vec<(i64, String)>, StoreFehler> {
        let grenze = jetzt + vorlauf;
        let zeilen: Vec<(i64, String)> = sqlx::query_as(
            "SELECT streamer_id, platform FROM platform_connections \
             WHERE needs_reauth = FALSE AND expires_at <= $1 ORDER BY streamer_id, platform",
        )
        .bind(grenze)
        .fetch_all(&self.pool)
        .await?;
        Ok(zeilen)
    }

    pub async fn refresh_and_store<F, Fut>(
        &self,
        streamer_id: i64,
        platform: &str,
        vorlauf: chrono::Duration,
        jetzt: DateTime<Utc>,
        refresh: F,
    ) -> Result<RefreshAusgang, StoreFehler>
    where
        F: FnOnce(String) -> Fut,
        Fut: Future<Output = Result<NeuerToken, RefreshAbbruch>>,
    {
        let mut tx = self.pool.begin().await?;
        let (lock_a, lock_b) = advisory_lock_pair(streamer_id, platform);
        sqlx::query("SELECT pg_advisory_xact_lock($1, $2)")
            .bind(lock_a)
            .bind(lock_b)
            .execute(&mut *tx)
            .await?;

        let Some(zeile) = self.load_mit(&mut *tx, streamer_id, platform, true).await? else {
            tx.commit().await?;
            return Ok(RefreshAusgang::NichtNoetig);
        };
        if zeile.needs_reauth {
            tx.commit().await?;
            return Ok(RefreshAusgang::NeuAnmeldungNoetig);
        }
        if jetzt + vorlauf < zeile.expires_at {
            tx.commit().await?;
            return Ok(RefreshAusgang::NichtNoetig);
        }
        if zeile.refresh_token.trim().is_empty() {
            self.set_needs_reauth_mit(&mut *tx, streamer_id, platform)
                .await?;
            tx.commit().await?;
            return Ok(RefreshAusgang::NeuAnmeldungNoetig);
        }

        match refresh(zeile.refresh_token.clone()).await {
            Ok(neu) => {
                let refresh_token = neu
                    .refresh_token
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(zeile.refresh_token);
                let scopes = if neu.scopes.is_empty() {
                    zeile.scopes
                } else {
                    neu.scopes
                };
                let aktualisiert = PlatformConnection {
                    streamer_id,
                    platform: platform.to_string(),
                    platform_user_id: zeile.platform_user_id,
                    platform_login: zeile.platform_login,
                    access_token: neu.access_token,
                    refresh_token,
                    scopes,
                    expires_at: neu.expires_at,
                    needs_reauth: false,
                };
                let aad = Self::aad(streamer_id, platform);
                let access_enc = self
                    .cipher
                    .encrypt_field(&aktualisiert.access_token, &aad)
                    .map_err(|e| StoreFehler::Crypto(e.to_string()))?;
                let refresh_enc = self
                    .cipher
                    .encrypt_field(&aktualisiert.refresh_token, &aad)
                    .map_err(|e| StoreFehler::Crypto(e.to_string()))?;
                self.upsert_mit(&mut *tx, &aktualisiert, &access_enc, &refresh_enc)
                    .await?;
                tx.commit().await?;
                Ok(RefreshAusgang::Erneuert)
            }
            Err(RefreshAbbruch::NeuAnmeldung) => {
                self.set_needs_reauth_mit(&mut *tx, streamer_id, platform)
                    .await?;
                tx.commit().await?;
                Ok(RefreshAusgang::NeuAnmeldungNoetig)
            }
            Err(RefreshAbbruch::Fehler) => {
                tx.commit().await?;
                Ok(RefreshAusgang::Fehlgeschlagen)
            }
        }
    }

    async fn set_needs_reauth_mit<'e, E>(
        &self,
        exec: E,
        streamer_id: i64,
        platform: &str,
    ) -> Result<(), StoreFehler>
    where
        E: sqlx::PgExecutor<'e>,
    {
        sqlx::query(
            "UPDATE platform_connections SET needs_reauth = TRUE, updated_at = NOW() \
             WHERE streamer_id = $1 AND platform = $2",
        )
        .bind(streamer_id)
        .bind(platform)
        .execute(exec)
        .await?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeuerToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshAbbruch {
    NeuAnmeldung,
    Fehler,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshAusgang {
    Erneuert,
    NichtNoetig,
    NeuAnmeldungNoetig,
    Fehlgeschlagen,
}

fn advisory_lock_pair(streamer_id: i64, platform: &str) -> (i32, i32) {
    let digest = Sha256::digest(format!("platform_connections_refresh:{streamer_id}:{platform}").as_bytes());
    let a = i32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]);
    let b = i32::from_be_bytes([digest[4], digest[5], digest[6], digest[7]]);
    (a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KEY_HEX: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    fn cipher() -> Arc<FieldCipher> {
        Arc::new(FieldCipher::from_hex_key(TEST_KEY_HEX, "v1").unwrap())
    }

    fn zeit(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    async fn maybe_pool() -> Option<PgPool> {
        if std::env::var("TB_TEST_REQUIRE_DB").as_deref() != Ok("1") {
            return None;
        }
        let url = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let schema = crate::auth::session::test_schema_name("platform_store");
        let admin = PgPool::connect(&url).await.ok()?;
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .ok()?;
        admin.close().await;
        let opts: sqlx::postgres::PgConnectOptions = url.parse().ok()?;
        let opts = opts.options([("search_path", schema.as_str())]);
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(3)
            .connect_with(opts)
            .await
            .ok()?;
        sqlx::query(
            "CREATE TABLE platform_connections (
                streamer_id BIGINT NOT NULL,
                platform TEXT NOT NULL,
                platform_user_id TEXT NOT NULL,
                platform_login TEXT NOT NULL,
                access_token_enc BYTEA NOT NULL,
                refresh_token_enc BYTEA NOT NULL,
                enc_kid TEXT NOT NULL DEFAULT 'v1',
                scopes TEXT[] NOT NULL DEFAULT '{}',
                expires_at TIMESTAMPTZ NOT NULL,
                needs_reauth BOOLEAN NOT NULL DEFAULT FALSE,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                PRIMARY KEY (streamer_id, platform)
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    macro_rules! pool_oder_ende {
        () => {
            match maybe_pool().await {
                Some(p) => p,
                None => {
                    assert!(
                        std::env::var("TB_TEST_REQUIRE_DB").as_deref() != Ok("1"),
                        "TB_TEST_REQUIRE_DB=1, aber keine Test-DB erreichbar"
                    );
                    return;
                }
            }
        };
    }

    fn verbindung(
        streamer_id: i64,
        platform: &str,
        expires_at: DateTime<Utc>,
    ) -> PlatformConnection {
        PlatformConnection {
            streamer_id,
            platform: platform.to_string(),
            platform_user_id: "u-1".into(),
            platform_login: "streamerin".into(),
            access_token: "acc-1".into(),
            refresh_token: "ref-1".into(),
            scopes: vec!["chat:write".into()],
            expires_at,
            needs_reauth: false,
        }
    }

    #[tokio::test]
    async fn upsert_und_load_verschluesseln_und_entschluesseln() {
        let pool = pool_oder_ende!();
        let store = PlatformConnectionStore::new(pool.clone(), cipher());
        let jetzt = zeit("2026-09-02T10:00:00Z");
        store
            .upsert(&verbindung(700, "kick", jetzt + chrono::Duration::hours(2)))
            .await
            .unwrap();
        let geladen = store.load(700, "kick").await.unwrap().unwrap();
        assert_eq!(geladen.access_token, "acc-1");
        assert_eq!(geladen.refresh_token, "ref-1");
        assert_eq!(geladen.platform_login, "streamerin");

        let roh: Vec<u8> =
            sqlx::query_scalar("SELECT access_token_enc FROM platform_connections WHERE streamer_id = 700")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_ne!(roh, b"acc-1", "Token darf nicht im Klartext liegen");
    }

    #[tokio::test]
    async fn delete_entfernt_die_zeile() {
        let pool = pool_oder_ende!();
        let store = PlatformConnectionStore::new(pool, cipher());
        let jetzt = zeit("2026-09-02T10:00:00Z");
        store
            .upsert(&verbindung(701, "kick", jetzt + chrono::Duration::hours(2)))
            .await
            .unwrap();
        assert!(store.delete(701, "kick").await.unwrap());
        assert!(!store.delete(701, "kick").await.unwrap());
        assert!(store.load(701, "kick").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn faellige_liefert_nur_ablaufende_und_ohne_reauth() {
        let pool = pool_oder_ende!();
        let store = PlatformConnectionStore::new(pool, cipher());
        let jetzt = zeit("2026-09-02T10:00:00Z");
        store
            .upsert(&verbindung(710, "kick", jetzt + chrono::Duration::minutes(2)))
            .await
            .unwrap();
        store
            .upsert(&verbindung(711, "youtube", jetzt + chrono::Duration::hours(5)))
            .await
            .unwrap();
        store.set_needs_reauth(710, "kick").await.unwrap();
        store
            .upsert(&verbindung(712, "kick", jetzt + chrono::Duration::minutes(1)))
            .await
            .unwrap();

        let faellig = store
            .faellige(chrono::Duration::minutes(10), jetzt)
            .await
            .unwrap();
        assert!(faellig.contains(&(712, "kick".to_string())));
        assert!(!faellig.contains(&(710, "kick".to_string())), "reauth raus");
        assert!(!faellig.contains(&(711, "youtube".to_string())), "noch lange gueltig");
    }

    #[tokio::test]
    async fn refresh_and_store_schreibt_frischen_token_zurueck() {
        let pool = pool_oder_ende!();
        let store = PlatformConnectionStore::new(pool, cipher());
        let jetzt = zeit("2026-09-02T10:00:00Z");
        store
            .upsert(&verbindung(720, "kick", jetzt + chrono::Duration::minutes(2)))
            .await
            .unwrap();
        let ausgang = store
            .refresh_and_store(720, "kick", chrono::Duration::minutes(10), jetzt, |rt| async move {
                assert_eq!(rt, "ref-1");
                Ok(NeuerToken {
                    access_token: "acc-2".into(),
                    refresh_token: Some("ref-2".into()),
                    expires_at: jetzt + chrono::Duration::hours(3),
                    scopes: vec!["chat:write".into()],
                })
            })
            .await
            .unwrap();
        assert_eq!(ausgang, RefreshAusgang::Erneuert);
        let geladen = store.load(720, "kick").await.unwrap().unwrap();
        assert_eq!(geladen.access_token, "acc-2");
        assert_eq!(geladen.refresh_token, "ref-2");
    }

    #[tokio::test]
    async fn refresh_and_store_setzt_reauth_bei_invalid_grant() {
        let pool = pool_oder_ende!();
        let store = PlatformConnectionStore::new(pool, cipher());
        let jetzt = zeit("2026-09-02T10:00:00Z");
        store
            .upsert(&verbindung(721, "kick", jetzt + chrono::Duration::minutes(2)))
            .await
            .unwrap();
        let ausgang = store
            .refresh_and_store(721, "kick", chrono::Duration::minutes(10), jetzt, |_rt| async move {
                Err(RefreshAbbruch::NeuAnmeldung)
            })
            .await
            .unwrap();
        assert_eq!(ausgang, RefreshAusgang::NeuAnmeldungNoetig);
        let geladen = store.load(721, "kick").await.unwrap().unwrap();
        assert!(geladen.needs_reauth);
    }

    #[tokio::test]
    async fn refresh_and_store_ueberspringt_frischen_token() {
        let pool = pool_oder_ende!();
        let store = PlatformConnectionStore::new(pool, cipher());
        let jetzt = zeit("2026-09-02T10:00:00Z");
        store
            .upsert(&verbindung(722, "kick", jetzt + chrono::Duration::hours(5)))
            .await
            .unwrap();
        let ausgang = store
            .refresh_and_store(722, "kick", chrono::Duration::minutes(10), jetzt, |_rt| async move {
                panic!("darf nicht refreshen, Token ist frisch");
            })
            .await
            .unwrap();
        assert_eq!(ausgang, RefreshAusgang::NichtNoetig);
    }
}
