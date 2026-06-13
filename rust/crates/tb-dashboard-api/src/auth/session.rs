//! Session-Lookup für Dashboard-Auth.
//!
//! Python-Referenz:
//! - `bot/storage/sessions_db.py` — `load_session`, `upsert_session`, Fernet-Decrypt
//! - `bot/dashboard/auth/state_store.py:254-313` — `load_dashboard_session`,
//!   `load_discord_admin_session`, `load_partner_access_session`
//! - `bot/dashboard/auth/auth_mixin.py:741-780` — `_is_partner_allowed`
//!
//! DB-Tabelle: `dashboard_sessions`
//! Spalten (prod_schema_twitch.txt):
//! - `session_id  TEXT`
//! - `session_type TEXT`
//! - `payload_enc  BYTEA` — Fernet-verschlüsseltes JSON
//! - `created_at   DOUBLE PRECISION` — Unix-Timestamp (float)
//! - `expires_at   DOUBLE PRECISION` — Unix-Timestamp (float)
//!
//! Session-Typen (state_store.py:17-19):
//! - `"twitch"` → Twitch-OAuth/Partner-Session (Cookie `twitch_dash_session`)
//! - `"discord_admin"` → Discord-Admin-Session (Cookie `master_dash_session`)
//!
//! Partner-Gate (auth_mixin.py:741-780):
//! - `twitch_partners.technical_pause_reason <> 'blocked'` (case-insensitive)
//! - Login oder User-ID muss matchen
//! - Blacklist: `twitch_token_blacklist` (Spalten: twitch_user_id, twitch_login)
//!
//! **Bewusste Abweichung von Python:** Python prüft das Partner-Gate nur beim
//! OAuth-Login (auth_mixin.py:1355); wir prüfen es bei JEDEM Request (mit 5s-Cache).
//! Strenger in die sichere Richtung — ein departnerter/geblockter Partner verliert
//! sofort Zugriff statt erst beim Session-Ablauf (bis zu 6h Fenster).
//!
//! **Sliding-Refresh (Parität zu Python):** Beide Session-Typen werden bei
//! Aktivität verlängert (`expires_at = now + TTL`), persistiert wird erst ab
//! über 1800 s Drift (services.py:222-231, auth_mixin.py:989-1003). Dafür wird der
//! Payload mit aktualisiertem `expires_at` neu Fernet-verschlüsselt.
//!
//! Der Fernet-Key kommt aus Env-Var `SESSIONS_ENCRYPTION_KEY` (Infisical lädt
//! sie in beide Services; Python liest sie seit dem Linux-Key-Fix ebenfalls).

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::PgPool;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use super::fernet;

/// Payload einer geladenen Twitch-Partner-Session.
#[derive(Debug, Clone, Default)]
pub struct PartnerSession {
    pub twitch_login: String,
    pub twitch_user_id: String,
}

/// Cache-Eintrag (Session-Payload + expires_at für Cache-Invalidierung).
#[derive(Debug, Clone)]
struct CacheEntry<T: Clone> {
    value: T,
    /// Wann dieser Eintrag invalidiert wird (Unix-Sekunden).
    expires_at: u64,
}

/// In-Memory-Cache mit 5-Sekunden-TTL.
///
/// Python: `DashboardAuthStateCache`, `state_store.py:31-112`.
/// Python nutzt keinen separaten Cache-TTL, sondern den Session-`expires_at`.
/// Wir nutzen 5 s als konservativen Cache-TTL — sichere Abweichung nach unten.
#[derive(Default)]
struct TimedCache<T: Clone> {
    entries: std::collections::HashMap<String, CacheEntry<T>>,
}

impl<T: Clone> TimedCache<T> {
    fn get(&self, key: &str, now: u64) -> Option<&T> {
        self.entries
            .get(key)
            .filter(|e| e.expires_at > now)
            .map(|e| &e.value)
    }

    fn insert(&mut self, key: String, value: T, cache_ttl_secs: u64, now: u64) {
        self.entries.insert(
            key,
            CacheEntry {
                value,
                expires_at: now + cache_ttl_secs,
            },
        );
    }

    /// Entfernt abgelaufene Einträge (opportunistisch).
    fn prune(&mut self, now: u64) {
        self.entries.retain(|_, e| e.expires_at > now);
    }
}

const CACHE_TTL_SECS: u64 = 5;

/// Admin-Session-TTL beim Sliding-Refresh (Python: server_v2.py:330, 14 Tage).
const ADMIN_SESSION_TTL_SECS: u64 = 14 * 24 * 3600;
/// Partner-Session-TTL beim Sliding-Refresh (Python: server_v2.py:183, min. 6h).
const PARTNER_SESSION_TTL_SECS: u64 = 6 * 3600;
/// Refresh wird erst persistiert wenn die Verlängerung diesen Drift übersteigt
/// (Python: services.py:224 / auth_mixin.py:994 — identische Schwelle).
const REFRESH_PERSIST_DRIFT_SECS: f64 = 1800.0;

/// Gemeinsamer Auth-State: Pool + Caches + Fernet-Key.
///
/// Als Extension in den Router injiziert — günstiger Clone via `Arc`.
#[derive(Clone)]
pub struct DashboardAuthState {
    pool: PgPool,
    fernet_key: String,
    /// Cache für Admin-Sessions (discord_admin).
    admin_cache: Arc<Mutex<TimedCache<bool>>>,
    /// Cache für Partner-Sessions (twitch).
    partner_cache: Arc<Mutex<TimedCache<PartnerSession>>>,
}

impl DashboardAuthState {
    /// Erzeugt einen neuen State.
    ///
    /// `fernet_key`: base64-urlsafe-kodierter 32-Byte-Key (Env-Var `SESSIONS_ENCRYPTION_KEY`).
    pub fn new(pool: PgPool, fernet_key: String) -> Self {
        Self {
            pool,
            fernet_key,
            admin_cache: Arc::new(Mutex::new(TimedCache::default())),
            partner_cache: Arc::new(Mutex::new(TimedCache::default())),
        }
    }

    /// Lädt den Fernet-Key aus der Env-Var `SESSIONS_ENCRYPTION_KEY`.
    ///
    /// Gibt `None` zurück wenn die Env-Var nicht gesetzt ist.
    pub fn fernet_key_from_env() -> Option<String> {
        std::env::var("SESSIONS_ENCRYPTION_KEY").ok()
    }

    /// Prüft ob eine `discord_admin`-Session gültig ist.
    ///
    /// Python-Pendant: `_get_discord_admin_session` (auth_mixin.py:956-1003) —
    /// DB-Lookup, Payload-`expires_at`-Prüfung (abgelaufen → Row löschen),
    /// Sliding-Refresh auf `now + 14d` (persistiert ab >1800s Drift).
    ///
    /// Gibt `Ok(Some(true))` wenn die Session gültig ist, `Ok(None)` wenn nicht gefunden
    /// oder abgelaufen, `Err` bei DB-Fehler.
    pub async fn load_admin_session(
        &self,
        session_id: &str,
    ) -> Result<Option<bool>, sqlx::Error> {
        let now = unix_now();

        {
            let cache = self.admin_cache.lock().await;
            if let Some(&valid) = cache.get(session_id, now) {
                return Ok(Some(valid));
            }
        }

        let Some(mut payload) = self
            .fetch_session_payload(session_id, "discord_admin", now)
            .await?
        else {
            return Ok(None);
        };

        // Payload-expires_at zusätzlich zur DB-Spalte prüfen (Python: 981-987)
        if payload_expired(&payload, now) {
            self.delete_session(session_id).await;
            return Ok(None);
        }

        // Sliding-Refresh: Python setzt zusätzlich last_seen_at + auth_type
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("last_seen_at".into(), serde_json::json!(now as f64));
            obj.entry("auth_type")
                .or_insert_with(|| serde_json::json!("discord_admin"));
        }
        self.maybe_refresh_session(
            session_id,
            "discord_admin",
            &mut payload,
            ADMIN_SESSION_TTL_SECS,
            now,
        )
        .await;

        {
            let mut cache = self.admin_cache.lock().await;
            cache.prune(now);
            cache.insert(session_id.to_string(), true, CACHE_TTL_SECS, now);
        }

        Ok(Some(true))
    }

    /// Prüft ob eine `twitch`-Session gültig ist UND der User ein aktiver Partner ist.
    ///
    /// Kaskade:
    /// 1. Session aus `dashboard_sessions` laden (Typ `twitch`)
    /// 2. `twitch_login` + `twitch_user_id` aus dem entschlüsselten Payload lesen
    /// 3. Partner-Gate prüfen (`twitch_partners`, auth_mixin.py:741-780)
    /// 4. Blacklist prüfen (`twitch_token_blacklist`)
    pub async fn load_partner_session(
        &self,
        session_id: &str,
    ) -> Result<Option<PartnerSession>, sqlx::Error> {
        let now = unix_now();

        {
            let cache = self.partner_cache.lock().await;
            if let Some(partner) = cache.get(session_id, now) {
                return Ok(Some(partner.clone()));
            }
        }

        let Some(mut payload) = self.fetch_session_payload(session_id, "twitch", now).await? else {
            return Ok(None);
        };

        // Payload-expires_at zusätzlich zur DB-Spalte prüfen (Python: services.py:213-220)
        if payload_expired(&payload, now) {
            self.delete_session(session_id).await;
            return Ok(None);
        }

        // Sliding-Refresh auf now + 6h (Python: services.py:222-231)
        self.maybe_refresh_session(
            session_id,
            "twitch",
            &mut payload,
            PARTNER_SESSION_TTL_SECS,
            now,
        )
        .await;

        let login = payload
            .get("twitch_login")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let user_id = payload
            .get("twitch_user_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        if login.is_empty() && user_id.is_empty() {
            return Ok(None);
        }

        // Partner-Gate (auth_mixin.py:741-780)
        let partner_row: Option<(String, String)> = sqlx::query_as(
            r#"
            SELECT p.twitch_login, p.twitch_user_id
            FROM twitch_partners p
            WHERE LOWER(COALESCE(p.technical_pause_reason, '')) <> 'blocked'
              AND (
                  LOWER(p.twitch_login) = LOWER($1)
                  OR ($2 <> '' AND p.twitch_user_id = $2)
              )
            ORDER BY CASE
                WHEN COALESCE(p.status, '') = 'active' THEN 0
                WHEN COALESCE(p.status, '') = 'archived' THEN 1
                WHEN COALESCE(p.status, '') = 'departnered' THEN 2
                ELSE 3
            END,
            COALESCE(p.departnered_at, p.admin_archived_at, p.partnered_at) DESC
            LIMIT 1
            "#,
        )
        .bind(&login)
        .bind(&user_id)
        .fetch_optional(&self.pool)
        .await?;

        let Some((db_login, db_user_id)) = partner_row else {
            return Ok(None);
        };

        // Blacklist prüfen (twitch_token_blacklist, auth_mixin.py context)
        let blacklisted: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM twitch_token_blacklist
                WHERE twitch_user_id = $1 OR LOWER(twitch_login) = LOWER($2)
            )
            "#,
        )
        .bind(&db_user_id)
        .bind(&db_login)
        .fetch_one(&self.pool)
        .await?;

        if blacklisted {
            debug!("Partner {} ist in token_blacklist — kein Zugriff", db_login);
            return Ok(None);
        }

        let partner = PartnerSession {
            twitch_login: db_login,
            twitch_user_id: db_user_id,
        };

        {
            let mut cache = self.partner_cache.lock().await;
            cache.prune(now);
            cache.insert(session_id.to_string(), partner.clone(), CACHE_TTL_SECS, now);
        }

        Ok(Some(partner))
    }

    /// Lädt einen nicht-abgelaufenen Session-Row und entschlüsselt den Payload.
    ///
    /// Gibt `None` zurück wenn nicht gefunden, abgelaufen oder Entschlüsselung fehlschlägt.
    async fn fetch_session_payload(
        &self,
        session_id: &str,
        session_type: &str,
        now: u64,
    ) -> Result<Option<serde_json::Value>, sqlx::Error> {
        // `expires_at` ist DOUBLE PRECISION (float8), `now` als float vergleichen
        let row: Option<(Vec<u8>,)> = sqlx::query_as(
            r#"
            SELECT payload_enc
            FROM dashboard_sessions
            WHERE session_id = $1
              AND session_type = $2
              AND expires_at > $3
            "#,
        )
        .bind(session_id)
        .bind(session_type)
        .bind(now as f64)
        .fetch_optional(&self.pool)
        .await?;

        let Some((payload_enc,)) = row else {
            return Ok(None);
        };

        // Fernet-Entschlüsselung (kein TTL-Check — DB-`expires_at` reicht)
        let plaintext = match fernet::decrypt(&self.fernet_key, &encode_b64(&payload_enc), None) {
            Ok(p) => p,
            Err(e) => {
                warn!("Fernet-Decrypt fehlgeschlagen für session {}: {}", session_id, e);
                return Ok(None);
            }
        };

        let json: serde_json::Value = match serde_json::from_slice(&plaintext) {
            Ok(v) => v,
            Err(e) => {
                warn!("Session-JSON ungültig für session {}: {}", session_id, e);
                return Ok(None);
            }
        };

        Ok(Some(json))
    }

    /// Löscht eine Session-Row (abgelaufener Payload). Fehler nur als Debug-Log —
    /// Auth-Entscheid hängt nicht davon ab (Python verhält sich identisch).
    async fn delete_session(&self, session_id: &str) {
        if let Err(e) = sqlx::query("DELETE FROM dashboard_sessions WHERE session_id = $1")
            .bind(session_id)
            .execute(&self.pool)
            .await
        {
            debug!("Session-Delete fehlgeschlagen für {}: {}", session_id, e);
        }
    }

    /// Sliding-Refresh: setzt `payload.expires_at = now + ttl` und persistiert
    /// die neu verschlüsselte Session, wenn die Verlängerung >1800s Drift hat.
    ///
    /// Python-Pendant: services.py:222-231 (Partner) / auth_mixin.py:989-1003
    /// (Admin). Persist-Fehler sind nicht fatal — nur Debug-Log wie in Python.
    async fn maybe_refresh_session(
        &self,
        session_id: &str,
        session_type: &str,
        payload: &mut serde_json::Value,
        ttl_secs: u64,
        now: u64,
    ) {
        let Some(obj) = payload.as_object_mut() else {
            return;
        };
        let old_expires = obj
            .get("expires_at")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0);
        let new_expires = now as f64 + ttl_secs as f64;
        obj.insert("expires_at".into(), serde_json::json!(new_expires));

        if new_expires - old_expires <= REFRESH_PERSIST_DRIFT_SECS {
            return;
        }

        let created_at = obj
            .get("created_at")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(now as f64);

        let token = match fernet::encrypt(&self.fernet_key, payload.to_string().as_bytes()) {
            Ok(t) => t,
            Err(e) => {
                debug!("Session-Refresh-Encrypt fehlgeschlagen für {}: {}", session_id, e);
                return;
            }
        };

        // Gleiche Semantik wie Python upsert_session (sessions_db.py:123-143):
        // bei Konflikt nur payload_enc + expires_at aktualisieren.
        if let Err(e) = sqlx::query(
            r#"
            INSERT INTO dashboard_sessions
                (session_id, session_type, payload_enc, created_at, expires_at)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (session_id) DO UPDATE SET
                payload_enc = EXCLUDED.payload_enc,
                expires_at  = EXCLUDED.expires_at
            "#,
        )
        .bind(session_id)
        .bind(session_type)
        .bind(token.as_bytes())
        .bind(created_at)
        .bind(new_expires)
        .execute(&self.pool)
        .await
        {
            debug!("Session-Refresh-Persist fehlgeschlagen für {}: {}", session_id, e);
        }
    }
}

/// Prüft das `expires_at`-Feld im entschlüsselten Payload (Python prüft es
/// zusätzlich zur DB-Spalte; fehlt das Feld, gilt die Session als abgelaufen —
/// identisch zu Pythons `float(session.get("expires_at", 0.0)) <= now`).
fn payload_expired(payload: &serde_json::Value, now: u64) -> bool {
    payload
        .get("expires_at")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0)
        <= now as f64
}

/// Aktueller Unix-Timestamp in Sekunden.
fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Fernet erwartet base64-urlsafe-String als Eingabe — wir haben BYTEA.
/// Der BYTEA-Inhalt *ist* schon der rohe Fernet-Token (inklusive base64-Encoding),
/// wie von Python `_encrypt` erzeugt: `fernet.encrypt()` gibt base64-Bytes zurück,
/// die direkt als BYTEA gespeichert werden.
///
/// Python: `sessions_db.py:87-88`:
/// ```python
/// def _encrypt(payload: dict) -> bytes:
///     return _get_fernet().encrypt(json.dumps(payload, ...).encode())
/// ```
/// `Fernet.encrypt()` gibt `bytes` zurück, die *bereits base64-urlsafe-kodiert* sind.
/// Diese bytes werden per `psycopg` als BYTEA in PG gespeichert.
/// Beim Lesen kommen sie als `bytes` zurück — wir müssen sie direkt als String
/// an unseren `fernet::decrypt` übergeben.
fn encode_b64(raw: &[u8]) -> String {
    // raw ist der base64-kodierte Fernet-Token als bytes (z.B. b"gAAAAAB...")
    // → einfach als UTF-8 interpretieren
    String::from_utf8_lossy(raw).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_now_ist_realistisch() {
        let now = unix_now();
        // Nach 2020-01-01 (1577836800) und vor 2100-01-01 (4102444800)
        assert!(now > 1_577_836_800);
        assert!(now < 4_102_444_800);
    }

    #[test]
    fn timed_cache_hit_and_miss() {
        let mut cache: TimedCache<String> = TimedCache::default();
        let now = unix_now();
        cache.insert("key1".into(), "value1".into(), 5, now);

        // Hit innerhalb TTL
        assert_eq!(cache.get("key1", now + 4), Some(&"value1".to_string()));
        // Miss nach TTL
        assert_eq!(cache.get("key1", now + 6), None);
        // Unbekannter Key
        assert_eq!(cache.get("unknown", now), None);
    }

    #[test]
    fn timed_cache_prune_entfernt_abgelaufene() {
        let mut cache: TimedCache<i32> = TimedCache::default();
        let now = unix_now();
        cache.insert("a".into(), 1, 1, now);
        cache.insert("b".into(), 2, 100, now);

        cache.prune(now + 2);

        assert_eq!(cache.get("a", now + 2), None);
        assert_eq!(cache.get("b", now + 2), Some(&2));
    }

    #[test]
    fn encode_b64_passthrough() {
        // BYTEA enthält bereits den Fernet-Token-String als bytes —
        // encode_b64 ist reiner Passthrough, Inhalt hier bewusst Dummy
        let token = b"gAAAAA-dummy-token-passthrough==";
        let result = encode_b64(token);
        assert_eq!(result, "gAAAAA-dummy-token-passthrough==");
    }

    #[test]
    fn partner_session_felder() {
        let ps = PartnerSession {
            twitch_login: "testuser".into(),
            twitch_user_id: "12345".into(),
        };
        assert_eq!(ps.twitch_login, "testuser");
        assert_eq!(ps.twitch_user_id, "12345");
    }

    #[test]
    fn fernet_key_from_env_liest_env_var() {
        // Nur wenn gesetzt — wir setzen eine Testvariable
        std::env::set_var("SESSIONS_ENCRYPTION_KEY", "testkey123");
        let key = DashboardAuthState::fernet_key_from_env();
        assert_eq!(key, Some("testkey123".to_string()));
        std::env::remove_var("SESSIONS_ENCRYPTION_KEY");
    }

    #[test]
    fn fernet_key_from_env_fehlt_gibt_none() {
        std::env::remove_var("SESSIONS_ENCRYPTION_KEY");
        let key = DashboardAuthState::fernet_key_from_env();
        assert_eq!(key, None);
    }
}

// --------------------------------------------------------------------------
// Integration-Tests (nur mit TB_TEST_DATABASE_URL + TB_TEST_REQUIRE_DB=1)
// --------------------------------------------------------------------------

#[cfg(test)]
mod integration_tests {
    use super::*;

    /// Gibt den Pool zurück wenn Integrations-Tests aktiv sind.
    async fn maybe_pool() -> Option<PgPool> {
        if std::env::var("TB_TEST_REQUIRE_DB").as_deref() != Ok("1") {
            return None;
        }
        let url = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        sqlx::PgPool::connect(&url).await.ok()
    }

    /// Fernet-Key für Integrations-Tests (Testkey, kein Prod-Secret).
    fn test_fernet_key() -> String {
        "dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU=".to_string()
    }

    /// Erzeugt einen Fernet-verschlüsselten Payload für Integrations-Tests via Python.
    /// Gibt base64-urlsafe-String zurück (wie Python `fernet.encrypt()` output).
    fn make_test_fernet_payload(json: &str) -> Vec<u8> {
        use std::process::Command;
        let payload_literal = format!("\"{}\"", json.replace('"', "\\\""));
        let script = format!(
            r#"
from cryptography.fernet import Fernet
import sys
key = b'{}'
f = Fernet(key)
payload = {}
print(f.encrypt(payload.encode()).decode(), end='')
"#,
            test_fernet_key(),
            payload_literal
        );
        let out = Command::new("python3")
            .arg("-c")
            .arg(&script)
            .output()
            .expect("python3 muss verfügbar sein");
        out.stdout
    }

    #[tokio::test]
    async fn session_nicht_gefunden_gibt_none() {
        let Some(pool) = maybe_pool().await else { return; };
        let state = DashboardAuthState::new(pool, test_fernet_key());
        let result = state.load_admin_session("nicht-existent-session-id-xyz").await;
        assert!(matches!(result, Ok(None)));
    }

    #[tokio::test]
    async fn partner_session_nicht_gefunden_gibt_none() {
        let Some(pool) = maybe_pool().await else { return; };
        let state = DashboardAuthState::new(pool, test_fernet_key());
        let result = state.load_partner_session("nicht-existent-partner-id-xyz").await;
        assert!(matches!(result, Ok(None)));
    }

    #[tokio::test]
    async fn admin_session_insert_und_laden() {
        let Some(pool) = maybe_pool().await else { return; };

        // DDL sicherstellen (prod-treue Spaltentypen aus prod_schema_twitch.txt)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS dashboard_sessions (
                session_id   TEXT NOT NULL PRIMARY KEY,
                session_type TEXT NOT NULL,
                payload_enc  BYTEA NOT NULL,
                created_at   DOUBLE PRECISION NOT NULL,
                expires_at   DOUBLE PRECISION NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let now = unix_now() as f64;
        let session_id = format!("test-admin-{}", now as u64);

        // Payload als Fernet-Token erzeugen (Python → Rust Roundtrip)
        let json_str = r#"{"auth_type":"discord_admin","expires_at":9999999999.0}"#;
        let fernet_bytes = make_test_fernet_payload(json_str);

        sqlx::query(
            "INSERT INTO dashboard_sessions (session_id, session_type, payload_enc, created_at, expires_at)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (session_id) DO NOTHING",
        )
        .bind(&session_id)
        .bind("discord_admin")
        .bind(fernet_bytes)
        .bind(now)
        .bind(now + 3600.0)
        .execute(&pool)
        .await
        .unwrap();

        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());
        let result = state.load_admin_session(&session_id).await.unwrap();
        assert_eq!(result, Some(true));

        // Cleanup
        sqlx::query("DELETE FROM dashboard_sessions WHERE session_id = $1")
            .bind(&session_id)
            .execute(&pool)
            .await
            .unwrap();
    }

    /// Sliding-Refresh: Session mit baldigem Ablauf wird beim Laden verlängert
    /// (Payload neu verschlüsselt + DB-`expires_at` auf now + 14d gesetzt).
    #[tokio::test]
    async fn admin_session_wird_beim_laden_verlaengert() {
        let Some(pool) = maybe_pool().await else { return; };

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS dashboard_sessions (
                session_id   TEXT NOT NULL PRIMARY KEY,
                session_type TEXT NOT NULL,
                payload_enc  BYTEA NOT NULL,
                created_at   DOUBLE PRECISION NOT NULL,
                expires_at   DOUBLE PRECISION NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let now = unix_now() as f64;
        let session_id = format!("test-refresh-{}", now as u64);

        // Läuft in 1h ab → Drift zur 14d-Verlängerung weit über 1800s → Persist
        let json_str = format!(
            r#"{{"auth_type":"discord_admin","created_at":{created},"expires_at":{expires}}}"#,
            created = now - 100.0,
            expires = now + 3600.0
        );
        let fernet_bytes = make_test_fernet_payload(&json_str);

        sqlx::query(
            "INSERT INTO dashboard_sessions (session_id, session_type, payload_enc, created_at, expires_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&session_id)
        .bind("discord_admin")
        .bind(fernet_bytes)
        .bind(now - 100.0)
        .bind(now + 3600.0)
        .execute(&pool)
        .await
        .unwrap();

        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());
        let result = state.load_admin_session(&session_id).await.unwrap();
        assert_eq!(result, Some(true));

        // DB-Spalte muss jetzt ~14 Tage in der Zukunft liegen
        let (payload_enc, db_expires): (Vec<u8>, f64) = sqlx::query_as(
            "SELECT payload_enc, expires_at FROM dashboard_sessions WHERE session_id = $1",
        )
        .bind(&session_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            db_expires > now + 13.0 * 24.0 * 3600.0,
            "expires_at muss auf ~14d verlängert sein, ist {db_expires} (now={now})"
        );

        // Neu verschlüsselter Payload muss mit dem Key lesbar sein und die
        // Refresh-Felder tragen (Python-Parität: last_seen_at + expires_at)
        let plaintext = fernet::decrypt(
            &test_fernet_key(),
            &String::from_utf8_lossy(&payload_enc),
            None,
        )
        .expect("neu verschlüsselter Payload muss entschlüsselbar sein");
        let payload: serde_json::Value = serde_json::from_slice(&plaintext).unwrap();
        assert_eq!(payload["auth_type"], "discord_admin");
        assert!(payload["last_seen_at"].as_f64().unwrap() >= now - 5.0);
        assert!(payload["expires_at"].as_f64().unwrap() > now + 13.0 * 24.0 * 3600.0);
        // created_at bleibt unangetastet (ON CONFLICT aktualisiert es nicht)
        assert!((payload["created_at"].as_f64().unwrap() - (now - 100.0)).abs() < 1.0);

        sqlx::query("DELETE FROM dashboard_sessions WHERE session_id = $1")
            .bind(&session_id)
            .execute(&pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn abgelaufene_session_gibt_none() {
        let Some(pool) = maybe_pool().await else { return; };

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS dashboard_sessions (
                session_id   TEXT NOT NULL PRIMARY KEY,
                session_type TEXT NOT NULL,
                payload_enc  BYTEA NOT NULL,
                created_at   DOUBLE PRECISION NOT NULL,
                expires_at   DOUBLE PRECISION NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let now = unix_now() as f64;
        let session_id = format!("test-expired-{}", now as u64);
        let fernet_bytes = make_test_fernet_payload(r#"{"auth_type":"discord_admin"}"#);

        sqlx::query(
            "INSERT INTO dashboard_sessions (session_id, session_type, payload_enc, created_at, expires_at)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (session_id) DO NOTHING",
        )
        .bind(&session_id)
        .bind("discord_admin")
        .bind(fernet_bytes)
        .bind(now - 7200.0)
        .bind(now - 1.0) // Abgelaufen
        .execute(&pool)
        .await
        .unwrap();

        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());
        let result = state.load_admin_session(&session_id).await.unwrap();
        assert_eq!(result, None);

        sqlx::query("DELETE FROM dashboard_sessions WHERE session_id = $1")
            .bind(&session_id)
            .execute(&pool)
            .await
            .unwrap();
    }
}
