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
//! - KEINE `twitch_token_blacklist`-Prüfung: Python gated die Session nur über
//!   `twitch_partners`; ein Blacklist-Eintrag (token_error) wirkt auf den
//!   partner_status/die Gnadenfrist, nicht auf den Dashboard-Zugang.
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

use chrono::Utc;
use sqlx::PgPool;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use super::fernet;

/// Payload einer geladenen Twitch-Partner-Session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PartnerSession {
    pub twitch_login: String,
    pub twitch_user_id: String,
    /// Twitch-`display_name` aus dem Helix-`/users`-Snapshot vom Login (im
    /// verschlüsselten Session-Payload, Python `session["display_name"]`,
    /// auth_mixin.py:906). Leer → Login-Fallback (Python api_v2.py:1826).
    pub display_name: String,
}

/// Payload einer geladenen Affiliate-Session (`twitch_affiliate_session`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AffiliateSession {
    pub twitch_login: String,
    pub twitch_user_id: String,
    pub display_name: String,
    pub email: String,
}

/// Sicherheitsrelevante Bindungs-Felder einer geladenen Admin-Session
/// (`master_dash_session` / `discord_admin`), für den Forward-Auth-Check (P1.39).
///
/// Python-Referenz: `server_v2.py:435-472` (`validate_admin_session`). Die Felder
/// werden beim Discord-Admin-Login geschrieben (`auth_mixin.py:1587-1648`).
///
/// Seit dem nativen Discord-Admin-Login werden auch `js_fp`/`fp_pending` geschrieben;
/// damit kann der harte Python-Zweig wieder abgebildet werden:
/// `source != "discord_dashboard"` UND leerer `js_fp` → 401.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdminSessionFingerprint {
    /// Beim Login gebundene Client-IP (`session["client_ip"]`). Leer → IP-Check aus.
    pub client_ip: String,
    /// Beim Login gebundener Passive-Fingerprint-Hash (`session["passive_fp"]`).
    /// Leer → Passive-FP-Check aus.
    pub passive_fp: String,
    /// `true` solange der JS-Fingerprint-Schritt nach dem Login noch aussteht
    /// (`session["fp_pending"]`). `true` → 401 (Schritt unvollständig).
    pub fp_pending: bool,
    /// Herkunft der Session (`source`). `"discord_dashboard"` überspringt Pythons
    /// harten JS-Fingerprint-Pflichtzweig.
    pub source: String,
    /// JS-Fingerprint-Hash (`session["js_fp"]`), nach `/twitch/auth/fingerprint`.
    pub js_fp: String,
    /// Anzeigbarer Admin-Name (`username`/`display_name`, sonst `"admin"`).
    pub username: String,
}

impl AdminSessionFingerprint {
    /// Spiegelt Pythons konditionale Bindungs-Checks aus `validate_admin_session`
    /// (`server_v2.py:435-466`). Gibt `true` zurück, wenn die Session den aktuellen
    /// Request akzeptieren darf, `false` bei einer Bindungs-Verletzung (→ 401).
    ///
    /// - `current_ip`: aus dem Request abgeleitete Client-IP (X-Forwarded-For hinter
    ///   dem Loopback-Proxy bzw. Peer-IP). Leer → IP-Check übersprungen (Caddy liefert
    ///   die Client-IP auf dem Auth-Subrequest nicht zuverlässig — Python-Parität).
    /// - `current_passive_fp`: aus `ua|lang|platform` SHA-256-gehashter Wert (32 hex).
    ///
    /// Konstant-zeitlicher Vergleich gegen Timing-Seitenkanäle.
    pub fn verify(&self, current_ip: &str, current_passive_fp: &str) -> bool {
        let stored_ip = self.client_ip.trim();
        if !stored_ip.is_empty() {
            let current_ip = current_ip.trim();
            // Nur erzwingen, wenn eine aktuelle Client-IP vorliegt (Python: Caddy-
            // forward_auth liefert sie nicht zuverlässig → kein Lockout bei Fehlen).
            if !current_ip.is_empty()
                && !tb_crypto::constant_time_eq(stored_ip.as_bytes(), current_ip.as_bytes())
            {
                warn!("AUDIT admin session IP mismatch");
                return false;
            }
        }

        let stored_fp = self.passive_fp.trim();
        if !stored_fp.is_empty()
            && !tb_crypto::constant_time_eq(
                stored_fp.as_bytes(),
                current_passive_fp.trim().as_bytes(),
            )
        {
            warn!("AUDIT admin session passive FP mismatch");
            return false;
        }

        if self.fp_pending {
            warn!("AUDIT admin session fp_pending - fingerprint step incomplete");
            return false;
        }

        if self.source.trim() != "discord_dashboard" && self.js_fp.trim().is_empty() {
            warn!("AUDIT admin session missing JS fingerprint");
            return false;
        }

        true
    }
}

/// Berechnet den Passive-Fingerprint-Hash aus den stabilen Request-Headern, exakt
/// wie Python `_build_passive_fp` (`auth_mixin.py:134-140`) und der Recompute in
/// `validate_admin_session` (`server_v2.py:454-460`):
/// `sha256("{ua}|{lang}|{platform}")[:32]`, wobei `lang` der erste
/// `Accept-Language`-Eintrag ist und `platform` der entquotete
/// `Sec-CH-UA-Platform`-Wert.
pub fn build_passive_fp(
    user_agent: &str,
    accept_language: &str,
    sec_ch_ua_platform: &str,
) -> String {
    use sha2::{Digest, Sha256};
    let ua = user_agent.trim();
    let lang = accept_language.split(',').next().unwrap_or("").trim();
    let platform = sec_ch_ua_platform.trim().trim_matches('"').trim();
    let raw = format!("{ua}|{lang}|{platform}");
    let digest = Sha256::digest(raw.as_bytes());
    let hex = digest
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    hex.chars().take(32).collect()
}

/// Ergebnis einer Session-Erstellung: die Session-ID (Cookie-Wert) und das an
/// die Session gebundene CSRF-Token (für `X-CSRF-Token` bei Write-Actions).
#[derive(Debug, Clone)]
pub struct SessionCreation {
    /// Opaker 32-Byte-CSPRNG-Wert (url-safe), wandert in das Session-Cookie.
    pub session_id: String,
    /// Sessiongebundenes CSRF-Token (im verschlüsselten Payload gespeichert).
    pub csrf_token: String,
}

/// Persistierter Zustand eines laufenden Twitch-OAuth-Login-Flows
/// (Python `auth_login` state_payload, auth_mixin.py:1239-1244). Überbrückt den
/// Authorize-Request und den Callback (über Prozess-/Restart-Grenzen hinweg) und
/// bindet die exakte beim Authorize verwendete `redirect_uri` (Twitch verlangt
/// beim Code-Tausch dieselbe URI) sowie das normalisierte Post-Login-Ziel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthLoginState {
    /// Whitelist-validiertes Post-Login-Redirect-Ziel (z. B. `/twitch/dashboard`).
    pub next_path: String,
    /// Beim Authorize verwendete Redirect-URI — muss beim Code-Tausch exakt
    /// wiederholt werden, sonst lehnt Twitch ab.
    pub redirect_uri: String,
    /// CSRF-Kontext-Token (P2.139): wird beim Login-Start als HttpOnly-Cookie
    /// gesetzt und im Callback gegen den hier persistierten Wert geprüft. Bindet
    /// den Callback an denselben Browser → ein cookieloser/fremder Callback (CSRF-
    /// Login) wird abgelehnt. Leer = keine Bindung (Abwärtskompatibilität).
    pub context_token: String,
}

/// Persistierter Zustand des Affiliate-Twitch-OAuth-Flows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AffiliateOAuthState {
    pub redirect_uri: String,
}

/// Persistierter Zustand des Affiliate-Stripe-Connect-Flows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AffiliateConnectState {
    pub redirect_uri: String,
    pub twitch_login: String,
}

/// `SameSite`-Politik eines Session-Cookies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SameSite {
    /// `SameSite=Lax` — Standard für Session-Cookies (Python: services.py:62).
    Lax,
    /// `SameSite=Strict` — strenger, für hochsensible Cookies.
    Strict,
}

impl SameSite {
    fn as_str(self) -> &'static str {
        match self {
            SameSite::Lax => "Lax",
            SameSite::Strict => "Strict",
        }
    }
}

/// Baut einen `Set-Cookie`-Header-Wert für eine Dashboard-Session.
///
/// Setzt die sicherheitsrelevanten Flags wie das Python-Pendant
/// (`services.py:53-63`): `HttpOnly` (kein JS-Zugriff → XSS kann das Cookie nicht
/// stehlen), `Secure` (nur über HTTPS, abhängig vom Request), `SameSite=Lax`
/// (CSRF-Grundschutz), `Path=/`, `Max-Age=<ttl>`. `secure` wird vom Aufrufer aus
/// dem Request abgeleitet (Python: `_is_secure_request`) — hinter dem HTTPS-Proxy
/// `true`, im lokalen HTTP-Test `false`.
pub fn build_session_cookie(
    name: &str,
    value: &str,
    secure: bool,
    same_site: SameSite,
    max_age_secs: u64,
) -> String {
    let mut cookie = format!(
        "{name}={value}; Path=/; Max-Age={max_age_secs}; HttpOnly; SameSite={}",
        same_site.as_str()
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

/// Baut ein Browser-Session-Cookie ohne persistente Ablaufzeit.
///
/// Ohne `Max-Age`/`Expires` verwirft der Browser das Cookie beim Ende der
/// Browser-Session. Die übrigen Sicherheitsattribute entsprechen den normalen
/// Dashboard-Session-Cookies.
pub fn build_transient_session_cookie(
    name: &str,
    value: &str,
    secure: bool,
    same_site: SameSite,
) -> String {
    let mut cookie = format!(
        "{name}={value}; Path=/; HttpOnly; SameSite={}",
        same_site.as_str()
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

/// Baut einen `Set-Cookie`-Header-Wert, der eine Session-Cookie **löscht**
/// (Logout). `Max-Age=0` + leerer Wert (Python: `clear_session_cookie`).
pub fn clear_session_cookie(name: &str, secure: bool, same_site: SameSite) -> String {
    let mut cookie = format!(
        "{name}=; Path=/; Max-Age=0; HttpOnly; SameSite={}",
        same_site.as_str()
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
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
pub const ADMIN_SESSION_TTL_SECS: u64 = 14 * 24 * 3600;
/// Partner-Session-TTL beim Sliding-Refresh (Python: server_v2.py:183, min. 6h).
const PARTNER_SESSION_TTL_SECS: u64 = 6 * 3600;

/// TTL einer **neu erstellten** Twitch-Partner-Session — hartkodiert 6 Stunden.
///
/// Block-11-Entscheidung: KEIN Env-Override (`SESSION_TTL_SECONDS` o. ä.). Python
/// erlaubte eine Konfiguration mit Mindestwert 6h (`server_v2.py:183`); im Rust-
/// Cutover ist der Wert fixiert, damit die Session-Lebensdauer nicht versehentlich
/// hochgedreht werden kann. Deckt sich mit dem Default und dem Sliding-Refresh-TTL.
pub const SESSION_CREATE_TTL_SECS: u64 = 6 * 3600;

/// Cookie-Name der Twitch-Partner-Session (Python: `server_v2.py:185`).
pub const PARTNER_COOKIE_NAME: &str = "twitch_dash_session";
/// Cookie-Name der Discord-Admin-Session (Python: state_store.py:18).
pub const ADMIN_COOKIE_NAME: &str = "master_dash_session";
/// Cookie-Name der Affiliate-Session (Python `_AFFILIATE_COOKIE`).
pub const AFFILIATE_COOKIE_NAME: &str = "twitch_affiliate_session";

/// Cookie-Name der durablen Partner-Access-Session (B3-9, Python
/// `services.py:36` — `{base}_partner`, mit `base = "twitch_dash_session"`).
/// Wird nach einem Partner-Einmal-Login ausgestellt und überdauert die kurzlebige
/// `twitch_dash_session`. Trägt eine grobe Request-Fingerprint-Bindung
/// (User-Agent-Familie + Plattform) gegen Cookie-Diebstahl.
pub const PARTNER_ACCESS_COOKIE_NAME: &str = "twitch_dash_session_partner";

/// Session-Typ der Partner-Access-Session in `dashboard_sessions`
/// (Python `session["auth_type"] = "partner_token"`, services.py:603/622).
pub const PARTNER_ACCESS_SESSION_TYPE: &str = "partner_token";

/// Session-Typ der kurzlebigen OAuth-Login-State-Rows in `dashboard_sessions`
/// (Python: `state_store.py:12`, `_OAUTH_STATE_TYPE_TWITCH = "oauth_state:twitch"`).
/// Der CSRF-State des Twitch-OAuth-Login-Flows wird als eigene Row mit diesem Typ
/// abgelegt — atomar einmal verbraucht (DELETE … RETURNING) beim Callback.
pub const OAUTH_STATE_SESSION_TYPE: &str = "oauth_state:twitch";
/// Persistierter Session-Typ der Affiliate-Portal-Session.
pub const AFFILIATE_SESSION_TYPE: &str = "affiliate";
/// Persistierter State-Typ des Affiliate-Twitch-OAuth-Flows.
pub const AFFILIATE_OAUTH_STATE_SESSION_TYPE: &str = "oauth_state:affiliate";
/// Persistierter State-Typ des Affiliate-Stripe-Connect-Flows.
pub const AFFILIATE_CONNECT_STATE_SESSION_TYPE: &str = "oauth_state:affiliate_connect";

/// Plattform-Discriminator des geteilten Raid-OAuth-State-Stores
/// (`oauth_state_tokens`), Python `_OAUTH_STATE_PLATFORM_RAID`.
const RAID_OAUTH_STATE_PLATFORM: &str = "twitch_raid";

/// Gültigkeit eines OAuth-Login-State-Tokens (Python: `server_v2.py:189`,
/// `_oauth_state_ttl_seconds = 600`). Hartkodiert — kein Env-Override.
pub const OAUTH_STATE_TTL_SECS: u64 = 600;
/// Affiliate-Session-TTL (Python `_AFFILIATE_SESSION_TTL`: 7 Tage).
pub const AFFILIATE_SESSION_TTL_SECS: u64 = 7 * 24 * 3600;
/// Affiliate OAuth-/Connect-State-TTL (Python: 600s).
pub const AFFILIATE_STATE_TTL_SECS: u64 = 600;

/// Session-Typ der Partner-Einmal-Login-State-Rows (B3-8, Python
/// `state_store.py`, `"oauth_state:partner_login"`). Atomar einmal verbraucht.
pub const PARTNER_LOGIN_STATE_TYPE: &str = "oauth_state:partner_login";

/// Länge der Session-ID/CSRF-Zufallsbytes (Python: `secrets.token_urlsafe(32)`).
const SESSION_ID_BYTES: usize = 32;
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

    /// Referenz auf den DB-Pool (für Resolver, die ihn brauchen, z. B. der
    /// Access-State-Lookup im Partner-Gate, P2.85).
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Lädt den Fernet-Key aus der Env-Var `SESSIONS_ENCRYPTION_KEY`.
    ///
    /// Gibt `None` zurück wenn die Env-Var nicht gesetzt ist.
    pub fn fernet_key_from_env() -> Option<String> {
        std::env::var("SESSIONS_ENCRYPTION_KEY").ok()
    }

    /// Legt eine **neue Twitch-Partner-Session** beim OAuth-Login an
    /// (Pendant zu Python `DashboardSessionService.create`, services.py:236-258).
    ///
    /// Ablauf:
    /// 1. `session_id` = 32-Byte-CSPRNG, url-safe (Python `secrets.token_urlsafe(32)`).
    /// 2. Sessiongebundenes `csrf_token` = 32-Byte-CSPRNG (Härtung über Python hinaus,
    ///    Block-11-Entscheidung — Python ließ CSRF leer).
    /// 3. Payload (`twitch_login`, `twitch_user_id`, `display_name`, `is_partner`,
    ///    `csrf_token`, `created_at`, `expires_at = now + 6h`) Fernet-verschlüsseln
    ///    und in `dashboard_sessions` (Typ `twitch`) persistieren.
    ///
    /// TTL ist hartkodiert 6h ([`SESSION_CREATE_TTL_SECS`]) — kein Env-Override.
    /// Bei DB-Fehler `Err`: anders als der Sliding-Refresh (Komfort) ist das Anlegen
    /// der Session der Login selbst — ohne persistierte Session kann sich der User
    /// nicht anmelden, also fail-closed statt stiller Erfolg.
    pub async fn create_partner_session(
        &self,
        twitch_login: &str,
        twitch_user_id: &str,
        display_name: &str,
    ) -> Result<SessionCreation, sqlx::Error> {
        let now = unix_now();
        let session_id = tb_crypto::random_urlsafe_token(SESSION_ID_BYTES);
        let csrf_token = tb_crypto::random_urlsafe_token(SESSION_ID_BYTES);
        let expires_at = now as f64 + SESSION_CREATE_TTL_SECS as f64;
        let display = if display_name.trim().is_empty() {
            twitch_login
        } else {
            display_name
        };

        let payload = serde_json::json!({
            "twitch_login": twitch_login,
            "twitch_user_id": twitch_user_id,
            "display_name": display,
            "is_partner": true,
            "csrf_token": csrf_token,
            "created_at": now as f64,
            "expires_at": expires_at,
        });

        self.persist_new_session(&session_id, "twitch", &payload, now as f64, expires_at)
            .await?;

        Ok(SessionCreation {
            session_id,
            csrf_token,
        })
    }

    /// Legt eine neue Affiliate-Portal-Session an (Python
    /// `_create_affiliate_session`, Typ `"affiliate"`, TTL 7 Tage).
    pub async fn create_affiliate_session(
        &self,
        twitch_login: &str,
        twitch_user_id: &str,
        display_name: &str,
        email: &str,
    ) -> Result<SessionCreation, sqlx::Error> {
        let now = unix_now();
        let session_id = tb_crypto::random_urlsafe_token(SESSION_ID_BYTES);
        let csrf_token = tb_crypto::random_urlsafe_token(SESSION_ID_BYTES);
        let expires_at = now as f64 + AFFILIATE_SESSION_TTL_SECS as f64;
        let login = twitch_login.trim().to_lowercase();
        let display = if display_name.trim().is_empty() {
            login.clone()
        } else {
            display_name.trim().to_string()
        };

        let payload = serde_json::json!({
            "twitch_login": login,
            "twitch_user_id": twitch_user_id.trim(),
            "display_name": display,
            "email": email.trim(),
            "created_at": now as f64,
            "expires_at": expires_at,
        });

        self.persist_new_session(
            &session_id,
            AFFILIATE_SESSION_TYPE,
            &payload,
            now as f64,
            expires_at,
        )
        .await?;

        Ok(SessionCreation {
            session_id,
            csrf_token,
        })
    }

    /// Legt eine **durable, geräte-gebundene Partner-Access-Session** an (P1.54,
    /// Pendant zu Python `PartnerAccessService.create`, services.py:540-566).
    ///
    /// Anders als [`Self::create_partner_session`] (kurzlebiges
    /// `twitch_dash_session`) wird hier eine eigene Row mit Typ `partner_token`
    /// (Cookie `twitch_dash_session_partner`) geschrieben, die den Einmal-Login
    /// überdauert. Der Payload trägt zusätzlich die Request-Fingerprint-Bindung
    /// (User-Agent-Familie + Plattform), gegen die [`Self::load_partner_access_session`]
    /// jeden späteren Request prüft — ein gestohlenes Cookie auf einem anderen
    /// Gerät/Browser wird so abgewiesen. TTL hartkodiert 6h.
    pub async fn create_partner_access_session(
        &self,
        twitch_login: &str,
        twitch_user_id: &str,
        display_name: &str,
        request_user_agent: &str,
    ) -> Result<SessionCreation, sqlx::Error> {
        let now = unix_now();
        let session_id = tb_crypto::random_urlsafe_token(SESSION_ID_BYTES);
        let csrf_token = tb_crypto::random_urlsafe_token(SESSION_ID_BYTES);
        let expires_at = now as f64 + SESSION_CREATE_TTL_SECS as f64;
        let display = if display_name.trim().is_empty() {
            twitch_login
        } else {
            display_name
        };
        let fp = RequestFingerprint::capture(request_user_agent);

        let payload = serde_json::json!({
            "twitch_login": twitch_login,
            "twitch_user_id": twitch_user_id,
            "display_name": display,
            "is_partner": true,
            "auth_type": PARTNER_ACCESS_SESSION_TYPE,
            "csrf_token": csrf_token,
            "user_agent_family": fp.family_str(),
            "user_agent_platform": fp.platform_str(),
            "created_at": now as f64,
            "expires_at": expires_at,
        });

        self.persist_new_session(
            &session_id,
            PARTNER_ACCESS_SESSION_TYPE,
            &payload,
            now as f64,
            expires_at,
        )
        .await?;

        Ok(SessionCreation {
            session_id,
            csrf_token,
        })
    }

    /// Legt eine einfache synthetische Discord-Admin-Session an (Test-/Interop-
    /// Helper). Der echte native Discord-Admin-OAuth-Flow nutzt
    /// [`Self::create_discord_admin_session`], weil dort Python-paritär 14 Tage TTL,
    /// Passive-Fingerprint und `fp_pending` geschrieben werden.
    pub async fn create_admin_session(
        &self,
        user_id: &str,
        display_name: &str,
    ) -> Result<SessionCreation, sqlx::Error> {
        let now = unix_now();
        let session_id = tb_crypto::random_urlsafe_token(SESSION_ID_BYTES);
        let csrf_token = tb_crypto::random_urlsafe_token(SESSION_ID_BYTES);
        let expires_at = now as f64 + SESSION_CREATE_TTL_SECS as f64;

        let payload = serde_json::json!({
            "auth_type": "discord_admin",
            "user_id": user_id,
            "display_name": display_name,
            "csrf_token": csrf_token,
            "source": "discord_dashboard",
            "fp_pending": false,
            "js_fp": "discord_validated",
            "created_at": now as f64,
            "last_seen_at": now as f64,
            "expires_at": expires_at,
        });

        self.persist_new_session(
            &session_id,
            "discord_admin",
            &payload,
            now as f64,
            expires_at,
        )
        .await?;

        Ok(SessionCreation {
            session_id,
            csrf_token,
        })
    }

    /// Spiegelt eine vom zentralen Discord-Dashboard validierte Admin-Session.
    pub async fn import_central_admin_session(
        &self,
        session_id: &str,
        user_id: &str,
        username: &str,
        display_name: &str,
        expires_at: f64,
    ) -> Result<SessionCreation, sqlx::Error> {
        let now = unix_now() as f64;
        let expires_at = expires_at.min(now + ADMIN_SESSION_TTL_SECS as f64);
        let csrf_token = tb_crypto::random_urlsafe_token(SESSION_ID_BYTES);
        let payload = serde_json::json!({
            "auth_type": "discord_admin",
            "user_id": user_id.trim(),
            "username": username.trim(),
            "display_name": display_name.trim(),
            "reason": "discord_dashboard",
            "source": "discord_dashboard",
            "csrf_token": csrf_token,
            "created_at": now,
            "last_seen_at": now,
            "expires_at": expires_at,
            "client_ip": "",
            "passive_fp": "",
            "fp_pending": false,
            "js_fp": "discord_validated",
        });

        self.persist_new_session(session_id, "discord_admin", &payload, now, expires_at)
            .await?;
        self.admin_cache.lock().await.entries.remove(session_id);

        Ok(SessionCreation {
            session_id: session_id.to_string(),
            csrf_token,
        })
    }

    /// Liefert den CSRF-Token einer gültigen lokalen Admin-Session.
    pub async fn admin_csrf_token(&self, session_id: &str) -> Result<Option<String>, sqlx::Error> {
        self.csrf_token_for_type(session_id, "discord_admin").await
    }

    /// Liefert den CSRF-Token einer gültigen Twitch-Partner-Session.
    pub async fn partner_csrf_token(&self, session_id: &str) -> Result<Option<String>, sqlx::Error> {
        self.csrf_token_for_type(session_id, "twitch").await
    }

    async fn csrf_token_for_type(
        &self,
        session_id: &str,
        session_type: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        Ok(self
            .fetch_session_payload(session_id, session_type, unix_now())
            .await?
            .and_then(|payload| {
                payload
                    .get("csrf_token")
                    .and_then(serde_json::Value::as_str)
                    .filter(|token| !token.is_empty())
                    .map(str::to_string)
            }))
    }

    /// Legt eine neue native Discord-Admin-Session an (Python
    /// `discord_auth_complete`, auth_mixin.py:1584-1648).
    ///
    /// TTL, Payload-Felder und `fp_pending` entsprechen Python: die Session wird mit
    /// 14 Tagen Laufzeit gemintet und erst nach `/twitch/auth/fingerprint` im
    /// Forward-Auth-Pfad akzeptiert.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_discord_admin_session(
        &self,
        user_id: u64,
        username: &str,
        display_name: &str,
        reason: &str,
        client_ip: &str,
        passive_fp: &str,
        post_fp_destination: &str,
    ) -> Result<SessionCreation, sqlx::Error> {
        let now = unix_now();
        let session_id = tb_crypto::random_urlsafe_token(SESSION_ID_BYTES);
        let csrf_token = tb_crypto::random_urlsafe_token(SESSION_ID_BYTES);
        let expires_at = now as f64 + ADMIN_SESSION_TTL_SECS as f64;
        let username = username.trim();
        let display = if display_name.trim().is_empty() {
            format!("User {user_id}")
        } else {
            display_name.trim().to_string()
        };

        let payload = serde_json::json!({
            "auth_type": "discord_admin",
            "user_id": user_id,
            "username": username,
            "display_name": display,
            "reason": reason.trim(),
            "csrf_token": csrf_token,
            "created_at": now as f64,
            "last_seen_at": now as f64,
            "expires_at": expires_at,
            "client_ip": client_ip.trim(),
            "passive_fp": passive_fp.trim(),
            "fp_pending": true,
            "post_fp_destination": post_fp_destination.trim(),
        });

        self.persist_new_session(
            &session_id,
            "discord_admin",
            &payload,
            now as f64,
            expires_at,
        )
        .await?;

        Ok(SessionCreation {
            session_id,
            csrf_token,
        })
    }

    /// Schließt den JS-Fingerprint-Schritt einer nativen Discord-Admin-Session ab.
    /// Gibt das gespeicherte Post-Fingerprint-Ziel zurück, wenn die Session gültig war.
    pub async fn complete_admin_session_fingerprint(
        &self,
        session_id: &str,
        js_fp: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        let now = unix_now();
        let Some(mut payload) = self
            .fetch_session_payload(session_id, "discord_admin", now)
            .await?
        else {
            return Ok(None);
        };
        if payload_expired(&payload, now) {
            return Ok(None);
        }

        let destination = payload
            .get("post_fp_destination")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("/twitch/admin")
            .trim()
            .to_string();
        let created_at = payload
            .get("created_at")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(now as f64);
        let expires_at = payload
            .get("expires_at")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(now as f64);

        if let Some(obj) = payload.as_object_mut() {
            obj.insert("js_fp".into(), serde_json::json!(js_fp.trim()));
            obj.insert("fp_pending".into(), serde_json::json!(false));
            obj.insert("last_seen_at".into(), serde_json::json!(now as f64));
        }

        self.persist_new_session(
            session_id,
            "discord_admin",
            &payload,
            created_at,
            expires_at,
        )
        .await?;
        {
            let mut cache = self.admin_cache.lock().await;
            cache.entries.remove(session_id);
        }
        Ok(Some(destination))
    }

    /// Verschlüsselt einen frischen Session-Payload und schreibt ihn in
    /// `dashboard_sessions`. Gemeinsame Logik von Partner- und Admin-Erstellung.
    async fn persist_new_session(
        &self,
        session_id: &str,
        session_type: &str,
        payload: &serde_json::Value,
        created_at: f64,
        expires_at: f64,
    ) -> Result<(), sqlx::Error> {
        let token = fernet::encrypt(&self.fernet_key, payload.to_string().as_bytes())
            .map_err(|e| sqlx::Error::Encode(Box::new(SessionEncryptError(e.to_string()))))?;

        sqlx::query!(
            r#"
            INSERT INTO dashboard_sessions
                (session_id, session_type, payload_enc, created_at, expires_at)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (session_id) DO UPDATE SET
                payload_enc = EXCLUDED.payload_enc,
                expires_at  = EXCLUDED.expires_at
            "#,
            session_id,
            session_type,
            token.as_bytes(),
            created_at,
            expires_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Persistiert einen frischen OAuth-Login-State (Pendant zu Python
    /// `save_twitch_oauth_state`, state_store.py:103-113). Ablage als eigene Row
    /// in `dashboard_sessions` mit Typ [`OAUTH_STATE_SESSION_TYPE`], TTL
    /// [`OAUTH_STATE_TTL_SECS`]. Der Payload trägt `next_path`, `redirect_uri` und
    /// `created_at` (Fernet-verschlüsselt wie jede Session). Bei DB-Fehler `Err` —
    /// ohne persistierten State darf der Login nicht starten (fail-closed).
    pub async fn save_oauth_login_state(
        &self,
        state_token: &str,
        state: &OAuthLoginState,
    ) -> Result<(), sqlx::Error> {
        let now = unix_now();
        let expires_at = now as f64 + OAUTH_STATE_TTL_SECS as f64;
        let payload = serde_json::json!({
            "next_path": state.next_path,
            "redirect_uri": state.redirect_uri,
            "context_token": state.context_token,
            "created_at": now as f64,
            "expires_at": expires_at,
        });
        self.persist_new_session(
            state_token,
            OAUTH_STATE_SESSION_TYPE,
            &payload,
            now as f64,
            expires_at,
        )
        .await
    }

    /// Verbraucht einen OAuth-Login-State atomar und einmalig (Pendant zu Python
    /// `consume_twitch_oauth_state` → `pop_session`, single-use `DELETE … RETURNING`).
    /// Ein zweiter Aufruf mit demselben Token liefert `None` (Replay-Schutz). Nur
    /// nicht-abgelaufene Tokens (`expires_at > now`) werden zurückgegeben; der
    /// Payload wird entschlüsselt und in [`OAuthLoginState`] geparst. Fehlt/kaputt
    /// → `None` (fail-closed), DB-Fehler → `Err`.
    pub async fn consume_oauth_login_state(
        &self,
        state_token: &str,
    ) -> Result<Option<OAuthLoginState>, sqlx::Error> {
        let now = unix_now();
        let row = sqlx::query!(
            r#"
            DELETE FROM dashboard_sessions
            WHERE session_id = $1
              AND session_type = $2
              AND expires_at > $3
            RETURNING payload_enc
            "#,
            state_token,
            OAUTH_STATE_SESSION_TYPE,
            now as f64
        )
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };
        let payload_enc = row.payload_enc;
        let plaintext = match fernet::decrypt(&self.fernet_key, &encode_b64(&payload_enc), None) {
            Ok(p) => p,
            Err(e) => {
                warn!("OAuth-State-Decrypt fehlgeschlagen: {e}");
                return Ok(None);
            }
        };
        let payload: serde_json::Value = match serde_json::from_slice(&plaintext) {
            Ok(v) => v,
            Err(e) => {
                warn!("OAuth-State-JSON ungültig: {e}");
                return Ok(None);
            }
        };
        // Defensiver TTL-Re-Check im Payload (Python: auth_mixin.py:1324).
        if payload_expired(&payload, now) {
            return Ok(None);
        }
        let next_path = payload
            .get("next_path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let redirect_uri = payload
            .get("redirect_uri")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if redirect_uri.is_empty() {
            return Ok(None);
        }
        let context_token = payload
            .get("context_token")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Ok(Some(OAuthLoginState {
            next_path,
            redirect_uri,
            context_token,
        }))
    }

    /// Persistiert einen Affiliate-Twitch-OAuth-State (Python
    /// `_affiliate_save_oauth_state`).
    pub async fn save_affiliate_oauth_state(
        &self,
        state_token: &str,
        state: &AffiliateOAuthState,
    ) -> Result<(), sqlx::Error> {
        let now = unix_now();
        let expires_at = now as f64 + AFFILIATE_STATE_TTL_SECS as f64;
        let payload = serde_json::json!({
            "redirect_uri": state.redirect_uri,
            "created_at": now as f64,
            "expires_at": expires_at,
        });
        self.persist_new_session(
            state_token,
            AFFILIATE_OAUTH_STATE_SESSION_TYPE,
            &payload,
            now as f64,
            expires_at,
        )
        .await
    }

    /// Verbraucht einen Affiliate-Twitch-OAuth-State atomar und einmalig.
    pub async fn consume_affiliate_oauth_state(
        &self,
        state_token: &str,
    ) -> Result<Option<AffiliateOAuthState>, sqlx::Error> {
        let Some(payload) = self
            .consume_affiliate_state_payload(state_token, AFFILIATE_OAUTH_STATE_SESSION_TYPE)
            .await?
        else {
            return Ok(None);
        };
        let redirect_uri = payload
            .get("redirect_uri")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if redirect_uri.is_empty() {
            return Ok(None);
        }
        Ok(Some(AffiliateOAuthState { redirect_uri }))
    }

    /// Persistiert einen Affiliate-Stripe-Connect-State (Python
    /// `_affiliate_save_connect_state`).
    pub async fn save_affiliate_connect_state(
        &self,
        state_token: &str,
        state: &AffiliateConnectState,
    ) -> Result<(), sqlx::Error> {
        let now = unix_now();
        let expires_at = now as f64 + AFFILIATE_STATE_TTL_SECS as f64;
        let payload = serde_json::json!({
            "redirect_uri": state.redirect_uri,
            "twitch_login": state.twitch_login,
            "created_at": now as f64,
            "expires_at": expires_at,
        });
        self.persist_new_session(
            state_token,
            AFFILIATE_CONNECT_STATE_SESSION_TYPE,
            &payload,
            now as f64,
            expires_at,
        )
        .await
    }

    /// Verbraucht einen Affiliate-Stripe-Connect-State atomar und einmalig.
    pub async fn consume_affiliate_connect_state(
        &self,
        state_token: &str,
    ) -> Result<Option<AffiliateConnectState>, sqlx::Error> {
        let Some(payload) = self
            .consume_affiliate_state_payload(state_token, AFFILIATE_CONNECT_STATE_SESSION_TYPE)
            .await?
        else {
            return Ok(None);
        };
        let redirect_uri = payload
            .get("redirect_uri")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let twitch_login = payload
            .get("twitch_login")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_lowercase();
        if redirect_uri.is_empty() || twitch_login.is_empty() {
            return Ok(None);
        }
        Ok(Some(AffiliateConnectState {
            redirect_uri,
            twitch_login,
        }))
    }

    async fn consume_affiliate_state_payload(
        &self,
        state_token: &str,
        session_type: &str,
    ) -> Result<Option<serde_json::Value>, sqlx::Error> {
        let now = unix_now();
        let row = sqlx::query!(
            r#"
            DELETE FROM dashboard_sessions
            WHERE session_id = $1
              AND session_type = $2
              AND expires_at > $3
            RETURNING payload_enc
            "#,
            state_token,
            session_type,
            now as f64
        )
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };
        let plaintext = match fernet::decrypt(&self.fernet_key, &encode_b64(&row.payload_enc), None)
        {
            Ok(p) => p,
            Err(e) => {
                warn!("Affiliate-State-Decrypt fehlgeschlagen: {e}");
                return Ok(None);
            }
        };
        let payload: serde_json::Value = match serde_json::from_slice(&plaintext) {
            Ok(v) => v,
            Err(e) => {
                warn!("Affiliate-State-JSON ungültig: {e}");
                return Ok(None);
            }
        };
        if payload_expired(&payload, now) {
            return Ok(None);
        }
        Ok(Some(payload))
    }

    /// Prüft, ob ein noch gültiger Raid-OAuth-State existiert, ohne ihn zu
    /// verbrauchen. Das ist das Rust-Pendant zu Pythons `has_state_details`
    /// beim geteilten `/callback/twitch`-Dispatch.
    pub async fn has_raid_oauth_state(&self, state_token: &str) -> Result<bool, sqlx::Error> {
        let row = sqlx::query!(
            r#"
            SELECT streamer_login
            FROM oauth_state_tokens
            WHERE state_token = $1
              AND platform = $2
              AND expires_at > $3
            LIMIT 1
            "#,
            state_token,
            RAID_OAUTH_STATE_PLATFORM,
            Utc::now()
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row
            .map(|row| {
                row.streamer_login
                    .as_deref()
                    .is_some_and(|login| !login.trim().is_empty())
            })
            .unwrap_or(false))
    }

    /// Persistiert einen Partner-Einmal-Login-State (B3-8). `state_id` = `sid` aus
    /// dem HMAC-Token; Payload trägt `next_path` + den Ziel-`login` (welcher
    /// Partner sich anmeldet). TTL via `ttl_secs`.
    pub async fn save_partner_login_state(
        &self,
        state_id: &str,
        login: &str,
        next_path: &str,
        ttl_secs: u64,
    ) -> Result<(), sqlx::Error> {
        let now = unix_now();
        let expires_at = now as f64 + ttl_secs as f64;
        let payload = serde_json::json!({
            "login": login,
            "next_path": next_path,
            "created_at": now as f64,
            "expires_at": expires_at,
        });
        self.persist_new_session(
            state_id,
            PARTNER_LOGIN_STATE_TYPE,
            &payload,
            now as f64,
            expires_at,
        )
        .await
    }

    /// Verbraucht einen Partner-Login-State atomar + einmalig (DELETE … RETURNING,
    /// Replay-Schutz). Liefert `(login, next_path)` zurück; ein zweiter Aufruf
    /// oder ein abgelaufener State → `None`.
    pub async fn consume_partner_login_state(
        &self,
        state_id: &str,
    ) -> Result<Option<(String, String)>, sqlx::Error> {
        let now = unix_now();
        let row = sqlx::query!(
            r#"
            DELETE FROM dashboard_sessions
            WHERE session_id = $1
              AND session_type = $2
              AND expires_at > $3
            RETURNING payload_enc
            "#,
            state_id,
            PARTNER_LOGIN_STATE_TYPE,
            now as f64
        )
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };
        let payload_enc = row.payload_enc;
        let plaintext = match fernet::decrypt(&self.fernet_key, &encode_b64(&payload_enc), None) {
            Ok(p) => p,
            Err(e) => {
                warn!("Partner-Login-State-Decrypt fehlgeschlagen: {e}");
                return Ok(None);
            }
        };
        let payload: serde_json::Value = match serde_json::from_slice(&plaintext) {
            Ok(v) => v,
            Err(e) => {
                warn!("Partner-Login-State-JSON ungültig: {e}");
                return Ok(None);
            }
        };
        if payload_expired(&payload, now) {
            return Ok(None);
        }
        let login = payload
            .get("login")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let next_path = payload
            .get("next_path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Ok(Some((login, next_path)))
    }

    /// Schlägt den kanonischen `twitch_partners`-Eintrag für einen Login bzw.
    /// User-ID nach (Pendant zu Python `_is_partner_allowed`, auth_mixin.py:741-780).
    /// Liefert `(twitch_login, twitch_user_id)` aus der DB, wenn ein nicht-`blocked`
    /// Partner existiert; bevorzugt `active` vor `archived`/`departnered`. Dient als
    /// Partner-Gate beim OAuth-Login — kein Treffer → kein Dashboard-Zugang (403).
    pub async fn find_partner_for_login(
        &self,
        login: &str,
        user_id: &str,
    ) -> Result<Option<PartnerSession>, sqlx::Error> {
        let login = login.trim().to_lowercase();
        let user_id = user_id.trim();
        if login.is_empty() && user_id.is_empty() {
            return Ok(None);
        }
        let row = sqlx::query!(
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
            login,
            user_id
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| PartnerSession {
            twitch_login: row.twitch_login,
            twitch_user_id: row.twitch_user_id,
            // Login-Gate (vor Session-Erstellung) kennt keinen display_name; der
            // echte Helix-Snapshot landet erst beim create_partner_session-Payload.
            display_name: String::new(),
        }))
    }

    /// Self-Heal beim OAuth-Login (P1.56): reaktiviert einen departnered/archived/
    /// paused Partner. Setzt `status='active'`, löscht `departnered_at`/
    /// `admin_archived_at`, `manual_partner_opt_out=0` und einen reinen
    /// `token_error`-Pausengrund (`technical_pause_reason`) — räumt also die
    /// vorübergehenden Sperren auf, sobald sich der Streamer selbst wieder per
    /// OAuth anmeldet.
    ///
    /// **Hart geschützt:** `blocked` und `bot_banned` werden NICHT angefasst (das
    /// sind administrative Hard-Kills, keine Selbstheilung). Liefert `true`, wenn
    /// eine Zeile reaktiviert wurde. clean-SQL: Zeitspalten via `NULL` bzw. NOW().
    pub async fn reactivate_partner(
        &self,
        login: &str,
        user_id: &str,
    ) -> Result<bool, sqlx::Error> {
        let login = login.trim().to_lowercase();
        let user_id = user_id.trim();
        if login.is_empty() && user_id.is_empty() {
            return Ok(false);
        }
        let result = sqlx::query!(
            r#"
            UPDATE twitch_partners
            SET status = 'active',
                departnered_at = NULL,
                admin_archived_at = NULL,
                manual_partner_opt_out = 0,
                technical_pause_reason = CASE
                    WHEN LOWER(TRIM(COALESCE(technical_pause_reason, ''))) LIKE 'token_error%'
                    THEN NULL ELSE technical_pause_reason
                END
            WHERE (
                    LOWER(twitch_login) = $1
                    OR ($2 <> '' AND twitch_user_id = $2)
                )
              AND LOWER(COALESCE(technical_pause_reason, '')) NOT IN ('blocked', 'bot_banned')
              AND COALESCE(status, '') <> 'active'
            "#,
            login,
            user_id
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Invalidiert eine Session beim Logout: löscht die Row aus `dashboard_sessions`
    /// und entfernt den Partner-Cache-Eintrag (Pendant zu Python `auth_logout` →
    /// `delete_session`, routes_entry.py:225). Idempotent — ein leerer/unbekannter
    /// `session_id` ist ein No-Op. DB-Fehler werden nur als Debug geloggt; der
    /// Logout-Erfolg hängt nicht am DB-Delete (das Cookie wird ohnehin gelöscht).
    pub async fn invalidate_session(&self, session_id: &str) {
        if session_id.is_empty() {
            return;
        }
        self.delete_session(session_id).await;
        {
            let mut cache = self.partner_cache.lock().await;
            cache.entries.remove(session_id);
        }
        {
            let mut cache = self.admin_cache.lock().await;
            cache.entries.remove(session_id);
        }
    }

    /// Validiert ein CSRF-Token gegen die Session — konstant-zeitlicher Vergleich.
    ///
    /// Lädt den Session-Payload (Partner `twitch` oder Admin `discord_admin`),
    /// liest das gespeicherte `csrf_token` und vergleicht es timing-sicher mit dem
    /// vom Client präsentierten Wert. Gibt `true` nur bei exaktem Match zurück.
    /// Fehlende Session, fehlendes Token-Feld oder leeres präsentiertes Token →
    /// `false` (fail-closed). DB-Fehler werden hochgereicht.
    pub async fn validate_csrf(
        &self,
        session_id: &str,
        session_type: &str,
        presented_token: &str,
    ) -> Result<bool, sqlx::Error> {
        if presented_token.is_empty() {
            return Ok(false);
        }
        let now = unix_now();
        let Some(payload) = self
            .fetch_session_payload(session_id, session_type, now)
            .await?
        else {
            return Ok(false);
        };
        let stored = payload.get("csrf_token").and_then(|v| v.as_str());
        Ok(match stored {
            Some(stored) if !stored.is_empty() => {
                tb_crypto::constant_time_eq(stored.as_bytes(), presented_token.as_bytes())
            }
            _ => false,
        })
    }

    /// Prüft ob eine `discord_admin`-Session gültig ist.
    ///
    /// Python-Pendant: `_get_discord_admin_session` (auth_mixin.py:956-1003) —
    /// DB-Lookup, Payload-`expires_at`-Prüfung (abgelaufen → Row löschen),
    /// Sliding-Refresh auf `now + 14d` (persistiert ab >1800s Drift).
    ///
    /// Gibt `Ok(Some(true))` wenn die Session gültig ist, `Ok(None)` wenn nicht gefunden
    /// oder abgelaufen, `Err` bei DB-Fehler.
    pub async fn load_admin_session(&self, session_id: &str) -> Result<Option<bool>, sqlx::Error> {
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

    /// Liest die Discord-`user_id` aus einer gültigen `discord_admin`-Session.
    ///
    /// Python-Pendant: `_get_discord_admin_user_id` (live.py:83-92) →
    /// `_get_discord_admin_session(...).get("user_id")`. Wird vom Owner-Gate der
    /// Admin-Chat-Aktion (P2.120/P2.119) gebraucht, um den freigeschalteten
    /// Discord-Owner zu prüfen. `Ok(None)` wenn die Session fehlt, abgelaufen ist
    /// oder kein `user_id`-Feld trägt (fail-closed). DB-Fehler werden hochgereicht.
    pub async fn load_admin_session_user_id(
        &self,
        session_id: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        let now = unix_now();
        let Some(payload) = self
            .fetch_session_payload(session_id, "discord_admin", now)
            .await?
        else {
            return Ok(None);
        };
        if payload_expired(&payload, now) {
            return Ok(None);
        }
        let Some(user_id) = payload.get("user_id") else {
            return Ok(None);
        };
        if let Some(value) = user_id.as_str().map(str::trim).filter(|s| !s.is_empty()) {
            return Ok(Some(value.to_string()));
        }
        if let Some(value) = user_id.as_u64() {
            return Ok(Some(value.to_string()));
        }
        if let Some(value) = user_id.as_i64().filter(|v| *v > 0) {
            return Ok(Some(value.to_string()));
        }
        Ok(None)
    }

    /// Lädt die sicherheitsrelevanten Bindungs-Felder einer gültigen
    /// `discord_admin`-Session für den Forward-Auth-Check (P1.39).
    ///
    /// Gibt `Ok(Some(..))` mit [`AdminSessionFingerprint`] zurück, wenn die Session
    /// existiert und nicht abgelaufen ist; `Ok(None)` wenn fehlend/abgelaufen;
    /// `Err` bei DB-Fehler. Anders als [`load_admin_session`] kein Sliding-Refresh
    /// und kein Cache — der Forward-Auth-Pfad braucht den frischen Payload-Inhalt.
    pub async fn load_admin_session_fingerprint(
        &self,
        session_id: &str,
    ) -> Result<Option<AdminSessionFingerprint>, sqlx::Error> {
        let now = unix_now();
        let Some(payload) = self
            .fetch_session_payload(session_id, "discord_admin", now)
            .await?
        else {
            return Ok(None);
        };
        if payload_expired(&payload, now) {
            return Ok(None);
        }

        let read = |key: &str| -> String {
            payload
                .get(key)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let username = {
            let u = read("username");
            if !u.is_empty() {
                u
            } else {
                let d = read("display_name");
                if d.is_empty() {
                    "admin".to_string()
                } else {
                    d
                }
            }
        };

        Ok(Some(AdminSessionFingerprint {
            client_ip: read("client_ip"),
            passive_fp: read("passive_fp"),
            fp_pending: payload
                .get("fp_pending")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            source: read("source"),
            js_fp: read("js_fp"),
            username,
        }))
    }

    /// Lädt eine gültige Affiliate-Session (`session_type='affiliate'`).
    /// Kein Partner-Gate und kein Sliding-Refresh, exakt wie Pythons
    /// `_get_affiliate_session`: gültig bis `expires_at`, sonst `None`.
    pub async fn load_affiliate_session(
        &self,
        session_id: &str,
    ) -> Result<Option<AffiliateSession>, sqlx::Error> {
        let now = unix_now();
        let Some(payload) = self
            .fetch_session_payload(session_id, AFFILIATE_SESSION_TYPE, now)
            .await?
        else {
            return Ok(None);
        };

        if payload_expired(&payload, now) {
            self.delete_session(session_id).await;
            return Ok(None);
        }

        let read = |key: &str| -> String {
            payload
                .get(key)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let twitch_login = read("twitch_login").to_lowercase();
        let twitch_user_id = read("twitch_user_id");
        if twitch_login.is_empty() && twitch_user_id.is_empty() {
            return Ok(None);
        }
        let display_name = {
            let display = read("display_name");
            if display.is_empty() {
                twitch_login.clone()
            } else {
                display
            }
        };
        Ok(Some(AffiliateSession {
            twitch_login,
            twitch_user_id,
            display_name,
            email: read("email"),
        }))
    }

    /// Prüft ob eine `twitch`-Session gültig ist UND der User ein aktiver Partner ist.
    ///
    /// Kaskade:
    /// 1. Session aus `dashboard_sessions` laden (Typ `twitch`)
    /// 2. `twitch_login` + `twitch_user_id` aus dem entschlüsselten Payload lesen
    /// 3. Partner-Gate prüfen (`twitch_partners`, auth_mixin.py:741-780) — KEINE
    ///    `twitch_token_blacklist`-Prüfung (die beeinflusst nur partner_status/Grace,
    ///    nicht die Session-Gültigkeit; Python-Parität).
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

        let Some(mut payload) = self
            .fetch_session_payload(session_id, "twitch", now)
            .await?
        else {
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
        // display_name aus dem Login-Snapshot (Python api_v2.py:1826: session
        // display_name → sonst Login). Wird unten in die PartnerSession übernommen.
        let display_name = payload
            .get("display_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        if login.is_empty() && user_id.is_empty() {
            return Ok(None);
        }

        // Partner-Gate (auth_mixin.py:741-780)
        let partner_row = sqlx::query!(
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
            login,
            user_id
        )
        .fetch_optional(&self.pool)
        .await?;

        let Some(partner_row) = partner_row else {
            return Ok(None);
        };

        // KEINE twitch_token_blacklist-Prüfung hier: Python `_is_partner_allowed`
        // (auth_mixin.py:741-780) gated die Session-Gültigkeit ausschließlich über
        // `twitch_partners`. Ein Blacklist-Eintrag (i.d.R. nur token_error-bedingt)
        // beeinflusst den partner_status/die Gnadenfrist — NICHT den Dashboard-
        // Zugang. Der frühere EXISTS-Check sperrte solche Partner fälschlich komplett
        // aus (DashboardAuthLevel::None → Login-Redirect), statt sie wie Python
        // einzulassen und nur den Re-Auth-Hinweis anzuzeigen.

        let partner = PartnerSession {
            twitch_login: partner_row.twitch_login,
            twitch_user_id: partner_row.twitch_user_id,
            display_name,
        };

        {
            let mut cache = self.partner_cache.lock().await;
            cache.prune(now);
            cache.insert(session_id.to_string(), partner.clone(), CACHE_TTL_SECS, now);
        }

        Ok(Some(partner))
    }

    /// Lädt die durable **Partner-Access-Session** (B3-9, Cookie
    /// `twitch_dash_session_partner`, Typ `partner_token`).
    ///
    /// Port von `PartnerAccessService.load` (services.py:568-615) +
    /// `PartnerAccessBinding.matches` (services.py:288-310). Kaskade:
    /// 1. Session-Row laden + Payload entschlüsseln (Typ `partner_token`),
    ///    `expires_at` prüfen.
    /// 2. **Fingerprint-Bindung**: die im Payload gespeicherte User-Agent-Familie/
    ///    -Plattform muss zum aktuellen Request-User-Agent passen (sonst Row löschen
    ///    + `None` — schützt vor Cookie-Diebstahl auf fremdem Gerät).
    /// 3. Partner-Gate (`twitch_partners`, identisch zu `load_partner_session`).
    /// 4. Sliding-Refresh auf `now + 6h`.
    ///
    /// `request_user_agent` ist der rohe `User-Agent`-Header (leer erlaubt — dann
    /// greift die Bindung nicht, wie in Python, wenn beide Seiten leer sind).
    pub async fn load_partner_access_session(
        &self,
        session_id: &str,
        request_user_agent: &str,
    ) -> Result<Option<PartnerSession>, sqlx::Error> {
        let now = unix_now();

        let Some(mut payload) = self
            .fetch_session_payload(session_id, PARTNER_ACCESS_SESSION_TYPE, now)
            .await?
        else {
            return Ok(None);
        };

        if payload_expired(&payload, now) {
            self.delete_session(session_id).await;
            return Ok(None);
        }

        // Fingerprint-Bindung prüfen (Python services.py:591-598).
        let request_fp = RequestFingerprint::capture(request_user_agent);
        if !request_fp.matches_payload(&payload) {
            self.delete_session(session_id).await;
            return Ok(None);
        }

        // Sliding-Refresh auf now + 6h (Python services.py:600-614).
        self.maybe_refresh_session(
            session_id,
            PARTNER_ACCESS_SESSION_TYPE,
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
        let display_name = payload
            .get("display_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        if login.is_empty() && user_id.is_empty() {
            return Ok(None);
        }

        // Partner-Gate (auth_mixin.py:741-780) — identisch zu load_partner_session.
        let partner_row = sqlx::query!(
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
            login,
            user_id
        )
        .fetch_optional(&self.pool)
        .await?;

        let Some(partner_row) = partner_row else {
            return Ok(None);
        };

        Ok(Some(PartnerSession {
            twitch_login: partner_row.twitch_login,
            twitch_user_id: partner_row.twitch_user_id,
            display_name,
        }))
    }

    /// `true` wenn der Partner **aktiv** geführt ist. Kanonische Definition
    /// wie `twitch_partners_all_state.is_partner_active`: active =
    /// `status='active'` UND `manual_partner_opt_out=0` UND
    /// `technical_pause_reason=''` UND `admin_archived_at IS NULL`.
    /// Keine Zeile ODER DB-Fehler → `false` (passive) — 1:1 Python (gibt dort
    /// „passive" zurück). Der `partner_status_gate` nutzt das für active-only-Routen.
    pub async fn is_partner_active(&self, login: &str, user_id: &str) -> bool {
        let login = login.trim().to_lowercase();
        let user_id = user_id.trim();
        if login.is_empty() && user_id.is_empty() {
            return false;
        }
        let row = sqlx::query_scalar!(
            r#"
            SELECT CASE
                WHEN COALESCE(p.status, '') = 'active'
                     AND COALESCE(p.manual_partner_opt_out, 0) = 0
                     AND COALESCE(p.technical_pause_reason, '') = ''
                     AND p.admin_archived_at IS NULL
                THEN 1 ELSE 0
            END AS "is_active!"
            FROM twitch_partners p
            WHERE (
                  LOWER(p.twitch_login) = $1
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
            login,
            user_id
        )
        .fetch_optional(&self.pool)
        .await;
        match row {
            Ok(Some(is_active)) => is_active == 1,
            Ok(None) => false,
            Err(error) => {
                debug!(%error, "is_partner_active-Query fehlgeschlagen → passive");
                false
            }
        }
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
        let row = sqlx::query!(
            r#"
            SELECT payload_enc
            FROM dashboard_sessions
            WHERE session_id = $1
              AND session_type = $2
              AND expires_at > $3
            "#,
            session_id,
            session_type,
            now as f64
        )
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };
        let payload_enc = row.payload_enc;

        // Fernet-Entschlüsselung (kein TTL-Check — DB-`expires_at` reicht)
        let plaintext = match fernet::decrypt(&self.fernet_key, &encode_b64(&payload_enc), None) {
            Ok(p) => p,
            Err(e) => {
                warn!(
                    "Fernet-Decrypt fehlgeschlagen für session {}: {}",
                    session_id, e
                );
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
        if let Err(e) = sqlx::query!(
            "DELETE FROM dashboard_sessions WHERE session_id = $1",
            session_id
        )
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
                debug!(
                    "Session-Refresh-Encrypt fehlgeschlagen für {}: {}",
                    session_id, e
                );
                return;
            }
        };

        // Gleiche Semantik wie Python upsert_session (sessions_db.py:123-143):
        // bei Konflikt nur payload_enc + expires_at aktualisieren.
        if let Err(e) = sqlx::query!(
            r#"
            INSERT INTO dashboard_sessions
                (session_id, session_type, payload_enc, created_at, expires_at)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (session_id) DO UPDATE SET
                payload_enc = EXCLUDED.payload_enc,
                expires_at  = EXCLUDED.expires_at
            "#,
            session_id,
            session_type,
            token.as_bytes(),
            created_at,
            new_expires
        )
        .execute(&self.pool)
        .await
        {
            debug!(
                "Session-Refresh-Persist fehlgeschlagen für {}: {}",
                session_id, e
            );
        }
    }
}

/// Fehler-Wrapper, um einen Fernet-Encrypt-Fehler als `sqlx::Error::Encode` zu
/// transportieren (die Session-Erstellung gibt `sqlx::Error` zurück, damit der
/// Aufrufer nur einen Fehlertyp behandeln muss). Die Klartext-Nachricht enthält
/// KEINE Secrets — nur die Fernet-Fehlerart (z. B. ungültige Key-Länge).
#[derive(Debug)]
struct SessionEncryptError(String);

impl std::fmt::Display for SessionEncryptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Session-Payload-Verschlüsselung fehlgeschlagen: {}",
            self.0
        )
    }
}

impl std::error::Error for SessionEncryptError {}

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

#[cfg(test)]
static TEST_SCHEMA_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[cfg(test)]
pub(crate) fn test_schema_name(prefix: &str) -> String {
    let seq = TEST_SCHEMA_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    format!("{prefix}_{}_{}", unix_now(), seq)
}

/// Grobe, robuste Request-Fingerprint-Bindung für Partner-Access-Sessions (B3-9).
///
/// Port von `PartnerAccessBinding` (services.py:275-334): leitet aus dem
/// User-Agent eine *Familie* (erstes Token-Wort, klein, max 32 Zeichen) und eine
/// *Plattform* (ios/android/windows/macos/linux) ab. Die Bindung ist absichtlich
/// grob (übersteht Versions-Bumps), bricht aber bei Geräte-/Browser-Wechsel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RequestFingerprint {
    family: String,
    platform: String,
}

impl RequestFingerprint {
    /// Erfasst Familie + Plattform aus dem rohen `User-Agent`-Header.
    pub(crate) fn capture(user_agent: &str) -> Self {
        let ua: String = user_agent.trim().chars().take(256).collect();
        Self {
            family: Self::family(&ua),
            platform: Self::platform(&ua),
        }
    }

    /// Erfasste User-Agent-Familie (für die Session-Payload-Bindung).
    pub(crate) fn family_str(&self) -> &str {
        &self.family
    }

    /// Erfasste Plattform (für die Session-Payload-Bindung).
    pub(crate) fn platform_str(&self) -> &str {
        &self.platform
    }

    /// Erstes `[A-Za-z][A-Za-z0-9_-]{1,31}`-Token, kleingeschrieben (services.py:312-317).
    fn family(user_agent: &str) -> String {
        if user_agent.is_empty() {
            return String::new();
        }
        // Iterativer Scan statt Regex-Compile pro Call.
        let bytes = user_agent.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let c = bytes[i];
            if c.is_ascii_alphabetic() {
                let start = i;
                i += 1;
                while i < bytes.len() {
                    let d = bytes[i];
                    if d.is_ascii_alphanumeric() || d == b'_' || d == b'-' {
                        i += 1;
                    } else {
                        break;
                    }
                }
                // Mindestlänge 2 (Regex verlangt {1,31} nach dem ersten Zeichen).
                if i - start >= 2 {
                    return user_agent[start..i.min(start + 32)].to_ascii_lowercase();
                }
                // Sonst weitersuchen.
            } else {
                i += 1;
            }
        }
        String::new()
    }

    /// Plattform-Klassifikation (services.py:319-334).
    fn platform(user_agent: &str) -> String {
        let c = user_agent.to_ascii_lowercase();
        if c.is_empty() {
            return String::new();
        }
        if c.contains("iphone") || c.contains("ipad") || c.contains("ios") {
            "ios"
        } else if c.contains("android") {
            "android"
        } else if c.contains("windows") {
            "windows"
        } else if c.contains("mac os") || c.contains("macintosh") {
            "macos"
        } else if c.contains("linux") {
            "linux"
        } else {
            ""
        }
        .to_string()
    }

    /// Vergleicht gegen die im Session-Payload gespeicherte Bindung
    /// (services.py:288-310). Sind beide Seiten je Achse gesetzt, reicht ein
    /// Match auf einer Achse; ist eine Achse leer, wird nur die andere geprüft;
    /// sind beide leer, gilt es als Match (fail-open auf fehlender Information,
    /// 1:1 Python).
    fn matches_payload(&self, payload: &serde_json::Value) -> bool {
        let expected_family = payload
            .get("user_agent_family")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let expected_platform = payload
            .get("user_agent_platform")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();

        let family_both = !expected_family.is_empty() && !self.family.is_empty();
        let platform_both = !expected_platform.is_empty() && !self.platform.is_empty();
        let family_ok = family_both
            && tb_crypto::constant_time_eq(expected_family.as_bytes(), self.family.as_bytes());
        let platform_ok = platform_both
            && tb_crypto::constant_time_eq(expected_platform.as_bytes(), self.platform.as_bytes());

        if family_both && platform_both {
            family_ok || platform_ok
        } else if family_both {
            family_ok
        } else if platform_both {
            platform_ok
        } else {
            true
        }
    }
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
    use std::sync::Mutex;

    /// Serialisiert die Tests, die `SESSIONS_ENCRYPTION_KEY` per
    /// `set_var`/`remove_var` anfassen. Ohne den Lock racet ein Test, der die
    /// Var setzt, mit einem, der sie entfernt und `None` erwartet → flaky.
    /// Konvention wie in `tb-llm` (`keys.rs`/`ledger.rs`).
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn unix_now_ist_realistisch() {
        let now = unix_now();
        // Nach 2020-01-01 (1577836800) und vor 2100-01-01 (4102444800)
        assert!(now > 1_577_836_800);
        assert!(now < 4_102_444_800);
    }

    // ── B3-9: Partner-Access-Fingerprint ───────────────────────────────────
    use serde_json::json as sjson;

    #[test]
    fn fingerprint_family_und_plattform_erkannt() {
        let fp = RequestFingerprint::capture(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        );
        // Erstes Wort-Token mit >=2 Zeichen = "mozilla".
        assert_eq!(fp.family, "mozilla");
        assert_eq!(fp.platform, "windows");

        let ios = RequestFingerprint::capture("Mozilla/5.0 (iPhone; CPU iPhone OS 17_0)");
        assert_eq!(ios.platform, "ios");
        let android = RequestFingerprint::capture("Mozilla/5.0 (Linux; Android 14; Pixel)");
        // android-Check vor linux → android gewinnt.
        assert_eq!(android.platform, "android");
        let leer = RequestFingerprint::capture("");
        assert_eq!(leer.family, "");
        assert_eq!(leer.platform, "");
    }

    #[test]
    fn fingerprint_matches_payload_regeln() {
        // Beide Achsen gesetzt → ein Treffer reicht.
        let fp = RequestFingerprint {
            family: "mozilla".into(),
            platform: "windows".into(),
        };
        // Familie passt, Plattform nicht → ok (OR).
        assert!(fp.matches_payload(&sjson!({
            "user_agent_family": "mozilla", "user_agent_platform": "linux"
        })));
        // Beide passen nicht → false.
        assert!(!fp.matches_payload(&sjson!({
            "user_agent_family": "safari", "user_agent_platform": "linux"
        })));
        // Nur Familie im Payload gesetzt → nur Familie zählt.
        assert!(fp.matches_payload(&sjson!({ "user_agent_family": "mozilla" })));
        assert!(!fp.matches_payload(&sjson!({ "user_agent_family": "safari" })));
        // Payload ohne Bindung → fail-open (true), wie Python.
        assert!(fp.matches_payload(&sjson!({})));
        // Request ohne UA, Payload mit Bindung → keine beidseitige Achse → true.
        let empty = RequestFingerprint::default();
        assert!(empty.matches_payload(&sjson!({ "user_agent_family": "mozilla" })));
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
            display_name: "TestUser".into(),
        };
        assert_eq!(ps.twitch_login, "testuser");
        assert_eq!(ps.twitch_user_id, "12345");
        assert_eq!(ps.display_name, "TestUser");
    }

    #[test]
    fn fernet_key_from_env_liest_env_var() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        // Nur wenn gesetzt — wir setzen eine Testvariable
        std::env::set_var("SESSIONS_ENCRYPTION_KEY", "testkey123");
        let key = DashboardAuthState::fernet_key_from_env();
        assert_eq!(key, Some("testkey123".to_string()));
        std::env::remove_var("SESSIONS_ENCRYPTION_KEY");
    }

    #[test]
    fn fernet_key_from_env_fehlt_gibt_none() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::remove_var("SESSIONS_ENCRYPTION_KEY");
        let key = DashboardAuthState::fernet_key_from_env();
        assert_eq!(key, None);
    }

    // ── F5: Cookie-Builder ──────────────────────────────────────────────────

    #[test]
    fn session_cookie_traegt_security_flags() {
        let c = build_session_cookie(
            PARTNER_COOKIE_NAME,
            "abc123",
            true,
            SameSite::Lax,
            SESSION_CREATE_TTL_SECS,
        );
        assert!(c.starts_with("twitch_dash_session=abc123"));
        assert!(c.contains("HttpOnly"), "HttpOnly muss gesetzt sein");
        assert!(
            c.contains("Secure"),
            "Secure muss bei secure=true gesetzt sein"
        );
        assert!(c.contains("SameSite=Lax"));
        assert!(c.contains("Path=/"));
        assert!(c.contains("Max-Age=21600"), "6h-TTL = 21600s");
    }

    #[test]
    fn session_cookie_ohne_secure_bei_http() {
        // Lokaler HTTP-Request (kein HTTPS-Proxy) → kein Secure-Flag, sonst
        // würde der Browser das Cookie über http:// nie senden.
        let c = build_session_cookie(PARTNER_COOKIE_NAME, "v", false, SameSite::Lax, 21600);
        assert!(!c.contains("Secure"), "kein Secure bei secure=false");
        assert!(c.contains("HttpOnly"));
    }

    #[test]
    fn transient_session_cookie_hat_keine_ablaufzeit() {
        let c = build_transient_session_cookie("tb_admin_mode", "2", true, SameSite::Lax);
        assert!(c.starts_with("tb_admin_mode=2;"));
        assert!(c.contains("Path=/"));
        assert!(c.contains("HttpOnly"));
        assert!(c.contains("SameSite=Lax"));
        assert!(c.contains("Secure"));
        assert!(!c.contains("Max-Age"));
        assert!(!c.contains("Expires"));
    }

    #[test]
    fn session_cookie_samesite_strict() {
        let c = build_session_cookie("x", "y", true, SameSite::Strict, 60);
        assert!(c.contains("SameSite=Strict"));
    }

    #[test]
    fn clear_cookie_setzt_max_age_null() {
        let c = clear_session_cookie(PARTNER_COOKIE_NAME, true, SameSite::Lax);
        assert!(c.contains("twitch_dash_session=;"));
        assert!(c.contains("Max-Age=0"));
        assert!(c.contains("HttpOnly"));
    }

    #[test]
    fn session_create_ttl_ist_sechs_stunden() {
        // Block-11: hartkodiert, kein Env-Override.
        assert_eq!(SESSION_CREATE_TTL_SECS, 6 * 3600);
    }

    // ── P1.39: Admin-Session-Fingerprint / Forward-Auth-Bindung ─────────────

    #[test]
    fn passive_fp_matcht_python_format() {
        // sha256("ua|de|Windows")[:32]. Wert gegen die Python-Formel verifiziert.
        let fp = build_passive_fp("ua", "de", "Windows");
        assert_eq!(fp.len(), 32);
        assert_eq!(fp, "1acf759d3fa2005e852d13227cc88189");
        // Quote-Stripping bei Sec-CH-UA-Platform + erster Accept-Language-Eintrag.
        let fp_quoted = build_passive_fp("ua", "de,en;q=0.9", "\"Windows\"");
        assert_eq!(fp, fp_quoted);
    }

    #[test]
    fn verify_ohne_bindung_akzeptiert() {
        // Native Admin-Session ohne client_ip/passive_fp/fp_pending → kein Lockout.
        let fp = AdminSessionFingerprint {
            js_fp: "abc12345".into(),
            ..Default::default()
        };
        assert!(fp.verify("203.0.113.7", "irgendwas"));
    }

    #[test]
    fn verify_ip_mismatch_lehnt_ab() {
        let fp = AdminSessionFingerprint {
            client_ip: "203.0.113.7".into(),
            js_fp: "abc12345".into(),
            ..Default::default()
        };
        assert!(!fp.verify("198.51.100.2", ""));
        assert!(fp.verify("203.0.113.7", ""));
        // Fehlende aktuelle IP (Caddy-Subrequest) → nicht erzwungen (kein Lockout).
        assert!(fp.verify("", ""));
    }

    #[test]
    fn verify_passive_fp_mismatch_lehnt_ab() {
        let stored = build_passive_fp("ua-a", "de", "Windows");
        let fp = AdminSessionFingerprint {
            passive_fp: stored.clone(),
            js_fp: "abc12345".into(),
            ..Default::default()
        };
        assert!(!fp.verify("", &build_passive_fp("ua-b", "de", "Windows")));
        assert!(fp.verify("", &stored));
    }

    #[test]
    fn verify_fp_pending_lehnt_ab() {
        let fp = AdminSessionFingerprint {
            fp_pending: true,
            js_fp: "abc12345".into(),
            ..Default::default()
        };
        assert!(!fp.verify("", ""));
    }

    #[test]
    fn verify_missing_js_fp_lehnt_ab_ausser_discord_dashboard_source() {
        assert!(!AdminSessionFingerprint::default().verify("", ""));
        let fp = AdminSessionFingerprint {
            source: "discord_dashboard".into(),
            ..Default::default()
        };
        assert!(fp.verify("", ""));
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
        let schema = test_schema_name("auth_session");
        let admin_pool = sqlx::PgPool::connect(&url).await.ok()?;
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin_pool)
            .await
            .ok()?;
        admin_pool.close().await;

        let opts: sqlx::postgres::PgConnectOptions = url.parse().ok()?;
        let opts = opts.options([("search_path", schema.as_str())]);
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .ok()
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
        let Some(pool) = maybe_pool().await else {
            return;
        };
        ensure_sessions_table(&pool).await;
        let state = DashboardAuthState::new(pool, test_fernet_key());
        let result = state
            .load_admin_session("nicht-existent-session-id-xyz")
            .await;
        assert!(matches!(result, Ok(None)));
    }

    #[tokio::test]
    async fn import_central_admin_session_behaelt_id_und_erzeugt_csrf() {
        let Some(pool) = maybe_pool().await else {
            return;
        };
        ensure_sessions_table(&pool).await;
        let state = DashboardAuthState::new(pool, test_fernet_key());

        let mirrored = state
            .import_central_admin_session("central-id", "42", "admin", "Admin", 9_999_999_999.0)
            .await
            .unwrap();

        assert_eq!(mirrored.session_id, "central-id");
        assert!(!mirrored.csrf_token.is_empty());
        assert_eq!(
            state
                .admin_csrf_token("central-id")
                .await
                .unwrap()
                .as_deref(),
            Some(mirrored.csrf_token.as_str())
        );
    }

    #[tokio::test]
    async fn partner_session_nicht_gefunden_gibt_none() {
        let Some(pool) = maybe_pool().await else {
            return;
        };
        ensure_sessions_table(&pool).await;
        let state = DashboardAuthState::new(pool, test_fernet_key());
        let result = state
            .load_partner_session("nicht-existent-partner-id-xyz")
            .await;
        assert!(matches!(result, Ok(None)));
    }

    #[tokio::test]
    async fn affiliate_session_wird_per_session_type_geladen() {
        let Some(pool) = maybe_pool().await else {
            return;
        };
        ensure_sessions_table(&pool).await;
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());
        let created = state
            .create_affiliate_session("Partner_One", "1001", "Partner One", "p@example.test")
            .await
            .unwrap();

        let loaded = state
            .load_affiliate_session(&created.session_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.twitch_login, "partner_one");
        assert_eq!(loaded.twitch_user_id, "1001");
        assert_eq!(loaded.display_name, "Partner One");
        assert_eq!(loaded.email, "p@example.test");

        let session_type: String =
            sqlx::query_scalar("SELECT session_type FROM dashboard_sessions WHERE session_id = $1")
                .bind(&created.session_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(session_type, AFFILIATE_SESSION_TYPE);

        sqlx::query("DELETE FROM dashboard_sessions WHERE session_id = $1")
            .bind(&created.session_id)
            .execute(&pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn admin_session_insert_und_laden() {
        let Some(pool) = maybe_pool().await else {
            return;
        };

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
        let Some(pool) = maybe_pool().await else {
            return;
        };

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
        let Some(pool) = maybe_pool().await else {
            return;
        };

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

    /// F5/F6 End-to-End: Partner-Session erstellen → Cookie-ID lädt die Session
    /// zurück (Round-Trip ver-/entschlüsselt) → CSRF-Token validiert nur exakt.
    #[tokio::test]
    async fn partner_session_create_roundtrip_und_csrf() {
        let Some(pool) = maybe_pool().await else {
            return;
        };

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
        ensure_partners_table(&pool).await;

        // Partner-Row, damit load_partner_session das Gate passiert.
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, twitch_user_id, status)
             VALUES ($1, $2, 'active')
             ON CONFLICT DO NOTHING",
        )
        .bind("csrftest_user")
        .bind("777001")
        .execute(&pool)
        .await
        .ok();

        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());
        let created = state
            .create_partner_session("csrftest_user", "777001", "CSRF Tester")
            .await
            .expect("Session-Erstellung muss klappen");

        // Round-Trip: Cookie-ID lädt die verschlüsselte Session zurück.
        let loaded = state
            .load_partner_session(&created.session_id)
            .await
            .expect("Laden darf nicht fehlschlagen")
            .expect("Session muss existieren");
        assert_eq!(loaded.twitch_login, "csrftest_user");

        // DB-Spalte expires_at muss ~6h in der Zukunft liegen (hartkodierte TTL).
        let db_expires: f64 =
            sqlx::query_scalar("SELECT expires_at FROM dashboard_sessions WHERE session_id = $1")
                .bind(&created.session_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let now = unix_now() as f64;
        assert!(
            (db_expires - (now + 6.0 * 3600.0)).abs() < 120.0,
            "TTL muss ~6h sein, ist {}",
            db_expires - now
        );

        // CSRF: korrektes Token akzeptiert.
        assert!(state
            .validate_csrf(&created.session_id, "twitch", &created.csrf_token)
            .await
            .unwrap());
        assert_eq!(
            state
                .partner_csrf_token(&created.session_id)
                .await
                .unwrap()
                .as_deref(),
            Some(created.csrf_token.as_str())
        );
        // CSRF: falsches Token abgelehnt.
        assert!(!state
            .validate_csrf(&created.session_id, "twitch", "falsch")
            .await
            .unwrap());
        // CSRF: leeres Token abgelehnt.
        assert!(!state
            .validate_csrf(&created.session_id, "twitch", "")
            .await
            .unwrap());
        // CSRF: unbekannte Session abgelehnt.
        assert!(!state
            .validate_csrf("nicht-existent-xyz", "twitch", &created.csrf_token)
            .await
            .unwrap());

        sqlx::query("DELETE FROM dashboard_sessions WHERE session_id = $1")
            .bind(&created.session_id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM twitch_partners WHERE twitch_login = 'csrftest_user'")
            .execute(&pool)
            .await
            .ok();
    }

    async fn ensure_sessions_table(pool: &PgPool) {
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
        .execute(pool)
        .await
        .unwrap();
    }

    async fn ensure_partners_table(pool: &PgPool) {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_partners (
                twitch_login TEXT,
                twitch_user_id TEXT,
                status TEXT,
                technical_pause_reason TEXT,
                manual_partner_opt_out INTEGER,
                departnered_at TIMESTAMPTZ,
                admin_archived_at TIMESTAMPTZ,
                partnered_at TIMESTAMPTZ
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
    }

    /// B3-1: OAuth-Login-State persistieren → genau EINMAL konsumieren
    /// (Replay-Schutz). Zweiter Consume liefert None.
    #[tokio::test]
    async fn oauth_login_state_single_use() {
        let Some(pool) = maybe_pool().await else {
            return;
        };
        ensure_sessions_table(&pool).await;
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());

        let token = format!("st-{}", unix_now());
        let login_state = OAuthLoginState {
            next_path: "/twitch/stats".to_string(),
            redirect_uri: "https://x.test/twitch/auth/callback".to_string(),
            context_token: String::new(),
        };
        state
            .save_oauth_login_state(&token, &login_state)
            .await
            .unwrap();

        // Erster Consume liefert den State zurück.
        let got = state.consume_oauth_login_state(&token).await.unwrap();
        assert_eq!(got, Some(login_state));
        // Zweiter Consume (Replay) → None.
        let again = state.consume_oauth_login_state(&token).await.unwrap();
        assert_eq!(again, None);
    }

    /// B3-1: Unbekannter/abgelaufener State → None (KEIN Login möglich).
    #[tokio::test]
    async fn oauth_login_state_unbekannt_und_abgelaufen_geben_none() {
        let Some(pool) = maybe_pool().await else {
            return;
        };
        ensure_sessions_table(&pool).await;
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());

        // Unbekannt.
        assert_eq!(
            state
                .consume_oauth_login_state("gibt-es-nicht")
                .await
                .unwrap(),
            None
        );

        // Abgelaufen: Row mit expires_at in der Vergangenheit direkt einschleusen.
        let token = format!("st-exp-{}", unix_now());
        let now = unix_now() as f64;
        let fernet_bytes = make_test_fernet_payload(
            r#"{"next_path":"/analyse","redirect_uri":"https://x.test/cb","expires_at":1.0}"#,
        );
        sqlx::query(
            "INSERT INTO dashboard_sessions (session_id, session_type, payload_enc, created_at, expires_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&token)
        .bind(OAUTH_STATE_SESSION_TYPE)
        .bind(fernet_bytes)
        .bind(now - 700.0)
        .bind(now - 1.0)
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(state.consume_oauth_login_state(&token).await.unwrap(), None);

        sqlx::query("DELETE FROM dashboard_sessions WHERE session_id = $1")
            .bind(&token)
            .execute(&pool)
            .await
            .ok();
    }

    /// B3-2-Gate: aktiver Partner wird gefunden; unbekannter Login nicht.
    #[tokio::test]
    async fn find_partner_for_login_findet_aktiven_partner() {
        let Some(pool) = maybe_pool().await else {
            return;
        };
        ensure_sessions_table(&pool).await;
        ensure_partners_table(&pool).await;
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());

        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, twitch_user_id, status)
             VALUES ($1, $2, 'active')
             ON CONFLICT DO NOTHING",
        )
        .bind("gatetest_user")
        .bind("888001")
        .execute(&pool)
        .await
        .ok();

        let found = state
            .find_partner_for_login("GateTest_User", "")
            .await
            .unwrap();
        assert_eq!(
            found,
            Some(PartnerSession {
                twitch_login: "gatetest_user".to_string(),
                twitch_user_id: "888001".to_string(),
                // find_partner_for_login (Login-Gate) trägt keinen display_name.
                display_name: String::new(),
            })
        );
        // Unbekannter Login → None (→ 403 im Callback).
        let none = state
            .find_partner_for_login("kein_partner_xyz", "")
            .await
            .unwrap();
        assert_eq!(none, None);

        sqlx::query("DELETE FROM twitch_partners WHERE twitch_login = 'gatetest_user'")
            .execute(&pool)
            .await
            .ok();
    }

    /// B3-2-Gate: `technical_pause_reason='blocked'` → kein Treffer (Hard-Kill).
    #[tokio::test]
    async fn find_partner_for_login_blocked_wird_abgelehnt() {
        let Some(pool) = maybe_pool().await else {
            return;
        };
        ensure_sessions_table(&pool).await;
        ensure_partners_table(&pool).await;
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());

        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, twitch_user_id, status, technical_pause_reason)
             VALUES ($1, $2, 'active', 'blocked')
             ON CONFLICT DO NOTHING",
        )
        .bind("blocked_user")
        .bind("888002")
        .execute(&pool)
        .await
        .ok();

        assert_eq!(
            state
                .find_partner_for_login("blocked_user", "")
                .await
                .unwrap(),
            None
        );

        sqlx::query("DELETE FROM twitch_partners WHERE twitch_login = 'blocked_user'")
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    async fn is_partner_active_folgt_kanonischer_view_logik() {
        let Some(pool) = maybe_pool().await else {
            return;
        };
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS twitch_partners (
                twitch_login TEXT, twitch_user_id TEXT, status TEXT,
                technical_pause_reason TEXT, manual_partner_opt_out INTEGER,
                departnered_at TIMESTAMPTZ, admin_archived_at TIMESTAMPTZ,
                partnered_at TIMESTAMPTZ, raid_bot_enabled INTEGER,
                inactivity_flagged_at TIMESTAMPTZ)",
        )
        .execute(&pool)
        .await
        .ok();
        for ddl in [
            "ALTER TABLE twitch_partners ADD COLUMN IF NOT EXISTS technical_pause_reason TEXT",
            "ALTER TABLE twitch_partners ADD COLUMN IF NOT EXISTS manual_partner_opt_out INTEGER",
            "ALTER TABLE twitch_partners ADD COLUMN IF NOT EXISTS departnered_at TIMESTAMPTZ",
            "ALTER TABLE twitch_partners ADD COLUMN IF NOT EXISTS admin_archived_at TIMESTAMPTZ",
            "ALTER TABLE twitch_partners ADD COLUMN IF NOT EXISTS partnered_at TIMESTAMPTZ",
            "ALTER TABLE twitch_partners ADD COLUMN IF NOT EXISTS raid_bot_enabled INTEGER",
            "ALTER TABLE twitch_partners ADD COLUMN IF NOT EXISTS inactivity_flagged_at TIMESTAMPTZ",
        ] {
            sqlx::query(ddl).execute(&pool).await.ok();
        }
        sqlx::query(
            "DELETE FROM twitch_partners
              WHERE twitch_login IN ('active_raid_off','admin_archived_gate','tech_pause_gate','inactive_info_gate')",
        )
        .execute(&pool)
        .await
        .ok();

        sqlx::query(
            "INSERT INTO twitch_partners
                (twitch_login, twitch_user_id, status, manual_partner_opt_out,
                 technical_pause_reason, admin_archived_at, raid_bot_enabled,
                 inactivity_flagged_at)
             VALUES
                ('active_raid_off', '901001', 'active', 0, NULL, NULL, 0, NULL),
                ('admin_archived_gate', '901002', 'active', 0, NULL, '2026-06-01T00:00:00Z', 1, NULL),
                ('tech_pause_gate', '901003', 'active', 0, 'maintenance', NULL, 1, NULL),
                ('inactive_info_gate', '901004', 'active', 0, NULL, NULL, 1, '2026-06-01T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());
        assert!(
            state.is_partner_active("active_raid_off", "").await,
            "raid_bot_enabled=0 darf nicht deaktivieren"
        );
        assert!(
            !state.is_partner_active("admin_archived_gate", "").await,
            "admin_archived_at ist Operator-Deaktivierung"
        );
        assert!(
            !state.is_partner_active("tech_pause_gate", "").await,
            "jede technical_pause_reason deaktiviert"
        );
        assert!(
            state.is_partner_active("inactive_info_gate", "").await,
            "Inaktivitaet ist nur Anzeigezustand"
        );

        sqlx::query(
            "DELETE FROM twitch_partners
              WHERE twitch_login IN ('active_raid_off','admin_archived_gate','tech_pause_gate','inactive_info_gate')",
        )
        .execute(&pool)
        .await
        .ok();
    }

    /// P1.54: durable Partner-Access-Session anlegen → mit gleichem User-Agent
    /// ladbar; Replay mit fremdem Fingerprint (anderer Plattform+Familie) → None.
    #[tokio::test]
    async fn partner_access_session_create_und_fingerprint_bindung() {
        let Some(pool) = maybe_pool().await else {
            return;
        };
        ensure_sessions_table(&pool).await;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS twitch_partners (
                twitch_login TEXT, twitch_user_id TEXT, status TEXT,
                technical_pause_reason TEXT, manual_partner_opt_out INTEGER,
                departnered_at TIMESTAMPTZ, admin_archived_at TIMESTAMPTZ,
                partnered_at TIMESTAMPTZ)",
        )
        .execute(&pool)
        .await
        .ok();

        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, twitch_user_id, status)
             VALUES ($1, $2, 'active') ON CONFLICT DO NOTHING",
        )
        .bind("paccess_user")
        .bind("888777")
        .execute(&pool)
        .await
        .ok();

        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());
        let ua_windows = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";
        let created = state
            .create_partner_access_session("paccess_user", "888777", "PAccess", ua_windows)
            .await
            .expect("Partner-Access-Session muss anlegbar sein");

        // Row trägt den partner_token-Typ.
        let session_type: String =
            sqlx::query_scalar("SELECT session_type FROM dashboard_sessions WHERE session_id = $1")
                .bind(&created.session_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(session_type, super::PARTNER_ACCESS_SESSION_TYPE);

        // Gleicher User-Agent → ladbar.
        let loaded = state
            .load_partner_access_session(&created.session_id, ua_windows)
            .await
            .unwrap();
        assert!(loaded.is_some(), "gleicher Fingerprint muss laden");
        assert_eq!(loaded.unwrap().twitch_login, "paccess_user");

        // Fremdes Gerät (andere Familie UND Plattform) → abgewiesen (Row gelöscht).
        let foreign_ua = "curlbot/9 (iPhone; CPU iPhone OS 17_0 like Mac OS X)";
        let rejected = state
            .load_partner_access_session(&created.session_id, foreign_ua)
            .await
            .unwrap();
        assert!(
            rejected.is_none(),
            "fremder Fingerprint muss abgewiesen werden"
        );

        sqlx::query("DELETE FROM twitch_partners WHERE twitch_login = 'paccess_user'")
            .execute(&pool)
            .await
            .ok();
    }

    /// P1.56: departnered/token_error-Partner wird beim Self-Heal reaktiviert;
    /// blocked bleibt unangetastet.
    #[tokio::test]
    async fn reactivate_partner_self_heal() {
        let Some(pool) = maybe_pool().await else {
            return;
        };
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS twitch_partners (
                twitch_login TEXT, twitch_user_id TEXT, status TEXT,
                technical_pause_reason TEXT, manual_partner_opt_out INTEGER,
                departnered_at TIMESTAMPTZ, admin_archived_at TIMESTAMPTZ,
                partnered_at TIMESTAMPTZ)",
        )
        .execute(&pool)
        .await
        .ok();
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());

        // departnered + token_error → wird geheilt.
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, twitch_user_id, status, technical_pause_reason, manual_partner_opt_out, departnered_at)
             VALUES ('healme', '770001', 'departnered', 'token_error', 1, NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(state.reactivate_partner("HealMe", "").await.unwrap());
        let (status, reason, optout, departed): (String, Option<String>, Option<i32>, Option<chrono::DateTime<Utc>>) =
            sqlx::query_as("SELECT status, technical_pause_reason, manual_partner_opt_out, departnered_at FROM twitch_partners WHERE twitch_login='healme'")
                .fetch_one(&pool).await.unwrap();
        assert_eq!(status, "active");
        assert_eq!(reason, None);
        assert_eq!(optout, Some(0));
        assert_eq!(departed, None);

        // blocked → bleibt unangetastet, kein Update.
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, twitch_user_id, status, technical_pause_reason)
             VALUES ('blockme', '770002', 'departnered', 'blocked')",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(!state.reactivate_partner("blockme", "").await.unwrap());
        let blocked_status: String =
            sqlx::query_scalar("SELECT status FROM twitch_partners WHERE twitch_login='blockme'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(blocked_status, "departnered");

        sqlx::query("DELETE FROM twitch_partners WHERE twitch_login IN ('healme','blockme')")
            .execute(&pool)
            .await
            .ok();
    }

    /// Logout: invalidate_session löscht die Row → load_partner_session → None.
    #[tokio::test]
    async fn invalidate_session_loescht_partner_session() {
        let Some(pool) = maybe_pool().await else {
            return;
        };
        ensure_sessions_table(&pool).await;
        ensure_partners_table(&pool).await;

        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, twitch_user_id, status)
             VALUES ($1, $2, 'active')
             ON CONFLICT DO NOTHING",
        )
        .bind("logout_user")
        .bind("888003")
        .execute(&pool)
        .await
        .ok();

        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());
        let created = state
            .create_partner_session("logout_user", "888003", "Logout Tester")
            .await
            .unwrap();
        // Session ist ladbar.
        assert!(state
            .load_partner_session(&created.session_id)
            .await
            .unwrap()
            .is_some());

        // Logout invalidiert.
        state.invalidate_session(&created.session_id).await;
        assert!(state
            .load_partner_session(&created.session_id)
            .await
            .unwrap()
            .is_none());

        sqlx::query("DELETE FROM twitch_partners WHERE twitch_login = 'logout_user'")
            .execute(&pool)
            .await
            .ok();
    }

    /// P1.39: lädt die Bindungs-Felder einer Discord-Admin-Session mit
    /// `client_ip`/`passive_fp`/`fp_pending` korrekt aus dem Fernet-Payload.
    /// Schema-isoliert (eigenes `CREATE SCHEMA`), um die Shared-Schema-Race der
    /// parallelen Integrations-Tests zu vermeiden.
    #[tokio::test]
    async fn admin_fingerprint_aus_payload_geladen() {
        if std::env::var("TB_TEST_REQUIRE_DB").as_deref() != Ok("1") {
            return;
        }
        let Ok(url) = std::env::var("TB_TEST_DATABASE_URL") else {
            return;
        };

        // Eigene Pool-Verbindung mit fixiertem search_path: jede Connection im Pool
        // sieht das Test-Schema (schema-isoliert, keine Shared-Schema-Race). Die
        // unqualifizierten Table-Refs in fetch_session_payload landen so im Schema.
        let schema = format!("fp_test_{}", unix_now());
        let admin_pool = sqlx::PgPool::connect(&url).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
            .execute(&admin_pool)
            .await
            .unwrap();

        let opts: sqlx::postgres::PgConnectOptions = url.parse().unwrap();
        let opts = opts.options([("search_path", schema.as_str())]);
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();

        sqlx::query(&format!(
            r#"CREATE TABLE {schema}.dashboard_sessions (
                session_id   TEXT NOT NULL PRIMARY KEY,
                session_type TEXT NOT NULL,
                payload_enc  BYTEA NOT NULL,
                created_at   DOUBLE PRECISION NOT NULL,
                expires_at   DOUBLE PRECISION NOT NULL
            )"#
        ))
        .execute(&pool)
        .await
        .unwrap();

        let now = unix_now() as f64;
        let session_id = format!("admin-fp-{}", now as u64);
        let json_str = format!(
            r#"{{"auth_type":"discord_admin","username":"earlysalty","client_ip":"203.0.113.7","passive_fp":"abc123","fp_pending":false,"js_fp":"feedface","expires_at":{}}}"#,
            now + 3600.0
        );
        let fernet_bytes = make_test_fernet_payload(&json_str);
        sqlx::query(&format!(
            "INSERT INTO {schema}.dashboard_sessions (session_id, session_type, payload_enc, created_at, expires_at) VALUES ($1,$2,$3,$4,$5)"
        ))
        .bind(&session_id)
        .bind("discord_admin")
        .bind(fernet_bytes)
        .bind(now)
        .bind(now + 3600.0)
        .execute(&pool)
        .await
        .unwrap();

        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());
        let fp = state
            .load_admin_session_fingerprint(&session_id)
            .await
            .unwrap()
            .expect("Admin-Session-Fingerprint muss laden");
        assert_eq!(fp.client_ip, "203.0.113.7");
        assert_eq!(fp.passive_fp, "abc123");
        assert!(!fp.fp_pending);
        assert_eq!(fp.username, "earlysalty");

        // Bindungs-Verifikation greift: gleiche IP + gleicher Passive-FP ok,
        // fremde IP bzw. fremder Passive-FP abgelehnt.
        assert!(fp.verify("203.0.113.7", "abc123"));
        assert!(!fp.verify("198.51.100.2", "abc123"));
        assert!(!fp.verify("203.0.113.7", "wrong-fp"));

        drop(pool);
        sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
            .execute(&admin_pool)
            .await
            .ok();
    }
}
