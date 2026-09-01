//! OAuth-Flow-Hilfsfunktionen — Authorize-URL-Bau und State-Info-Aufbau.
//!
//! Python-Äquivalent: relevante Teile aus `bot/raid/auth.py` (Zeilen 121–130,
//! 433–494, 538–587) sowie Konstanten aus `bot/raid/auth.py` Zeilen 46/48.
//!
//! Kein DB-Zugriff in diesem Modul. Die einzige DB-Kopplung ist der Trait
//! [`StreamerContextResolver`], der für Tests mit einem Stub implementiert
//! werden kann.

use async_trait::async_trait;
use url::Url;

use crate::scope_profiles::{
    scopes_for_profile, BASE_SCOPE_PROFILE, DASHBOARD_REAUTH_SCOPE_PROFILE, UPLINK_SCOPE_PROFILE,
};
use crate::state_store::RaidOAuthState;

// ---------------------------------------------------------------------------
// Konstanten (Python: auth.py Zeilen 46/48)
// ---------------------------------------------------------------------------

/// Basis-URL für den Twitch Authorization-Code-Flow.
pub const TWITCH_AUTHORIZE_URL: &str = "https://id.twitch.tv/oauth2/authorize";

/// Pseudo-Login für Website-Onboarding ohne bekannten Streamer-Account.
pub const PUBLIC_WEBSITE_ONBOARDING_LOGIN: &str = "public:website_onboarding";

// ---------------------------------------------------------------------------
// DB-Abstraktions-Trait (Python: _has_existing_streamer_context +
//   _linked_twitch_identity_for_discord_user in RaidAuthManager)
// ---------------------------------------------------------------------------

/// Abstrahiert alle DB-Abfragen, die `build_state_info` benötigt.
///
/// Eine echte Implementierung würde via `sqlx::PgPool` auf die Tabellen
/// `twitch_partners`/`twitch_streamers` zugreifen. Im Test reicht ein
/// einfacher Stub.
#[async_trait]
pub trait StreamerContextResolver: Send + Sync {
    /// Gibt `true` zurück, wenn für `login` bereits ein aktiver Streamer-Kontext
    /// in der DB existiert (Token vorhanden, Partner-Eintrag aktiv o. ä.).
    ///
    /// Python: `_has_existing_streamer_context`.
    async fn has_existing_streamer_context(&self, login: &str) -> bool;

    /// Liefert `(twitch_login, twitch_user_id)` für eine Discord-User-ID,
    /// falls eine verknüpfte Twitch-Identität existiert.
    ///
    /// Python: `_linked_twitch_identity_for_discord_user`.
    async fn linked_twitch_identity_for_discord_user(
        &self,
        discord_user_id: &str,
    ) -> (Option<String>, Option<String>);
}

// ---------------------------------------------------------------------------
// Private Helfer (Python: _normalize_twitch_login, _normalize_state_discord_user_id)
// ---------------------------------------------------------------------------

/// Trimmt und lowercased einen Twitch-Login; gibt `None` zurück wenn leer.
///
/// Python: `_normalize_twitch_login` (auth.py Zeilen 121–124).
pub(crate) fn normalize_twitch_login(value: &str) -> Option<String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

/// Trimmt einen Discord-User-ID-String; gibt `None` zurück wenn nicht
/// ausschließlich aus Ziffern bestehend oder leer.
///
/// Python: `_normalize_state_discord_user_id` (auth.py Zeilen 126–130).
pub(crate) fn normalize_state_discord_user_id(value: &str) -> Option<String> {
    let normalized = value.trim().to_string();
    if normalized.is_empty() || !normalized.chars().all(|c| c.is_ascii_digit()) {
        None
    } else {
        Some(normalized)
    }
}

// ---------------------------------------------------------------------------
// Scope-Profil-Auflösung (Python: _resolve_scope_profile, auth.py 484–494)
// ---------------------------------------------------------------------------

/// Löst das endgültige Scope-Profil auf: explizite Profile (`base` /
/// `dashboard_reauth` / `uplink`) bleiben unverändert. Bei `auto` oder
/// unbekanntem Wert entscheidet der Resolver: existierender Streamer-Kontext →
/// `dashboard_reauth`, neuer Streamer → `base`.
///
/// `uplink` bekommt bewusst keinen Auto-Rückfall: wer den Uplink verbindet,
/// hat den Weg ausdrücklich gewählt, und ein stiller Rückfall auf `base`
/// hieße, dass der Twitch-Dialog die Chat- und Stream-Key-Rechte gar nicht
/// erst anfragt und "Verbinden" ohne sichtbaren Grund nichts bewirkt.
///
/// Python: `_resolve_scope_profile`.
async fn resolve_scope_profile(
    resolver: &dyn StreamerContextResolver,
    twitch_login: &str,
    requested_profile: &str,
) -> &'static str {
    let normalized = requested_profile.trim().to_ascii_lowercase();
    if normalized == DASHBOARD_REAUTH_SCOPE_PROFILE {
        return DASHBOARD_REAUTH_SCOPE_PROFILE;
    }
    if normalized == BASE_SCOPE_PROFILE {
        return BASE_SCOPE_PROFILE;
    }
    if normalized == UPLINK_SCOPE_PROFILE {
        return UPLINK_SCOPE_PROFILE;
    }
    // auto oder unbekannt → Kontext prüfen.
    if resolver.has_existing_streamer_context(twitch_login).await {
        DASHBOARD_REAUTH_SCOPE_PROFILE
    } else {
        BASE_SCOPE_PROFILE
    }
}

// ---------------------------------------------------------------------------
// Öffentliche API
// ---------------------------------------------------------------------------

/// Baut die Authorize-URL für den Twitch Authorization-Code-Flow.
///
/// Parameter (in dieser Reihenfolge in der URL):
/// `client_id`, `redirect_uri`, `response_type=code`, `scope` (Space-getrennt),
/// `state`, `force_verify=true`.
///
/// Python: `_build_authorize_url` (auth.py Zeilen 577–587).
///
/// # Hinweis
/// Die Scope-Liste wird mit [`scopes_for_profile`] aufgelöst. Wenn `scope_profile`
/// noch `"auto"` enthält, liefert das Basis-Satz — der Caller muss vorher
/// `build_state_info` aufgerufen haben, das `"auto"` bereits auflöst.
pub fn build_authorize_url(
    client_id: &str,
    redirect_uri: &str,
    scope_profile: &str,
    state: &str,
) -> String {
    let scopes = scopes_for_profile(scope_profile).join(" ");
    let mut url = Url::parse(TWITCH_AUTHORIZE_URL)
        .expect("TWITCH_AUTHORIZE_URL ist eine valide statische URL");
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("client_id", client_id);
        q.append_pair("redirect_uri", redirect_uri);
        q.append_pair("response_type", "code");
        q.append_pair("scope", &scopes);
        q.append_pair("state", state);
        q.append_pair("force_verify", "true");
    }
    url.to_string()
}

/// Baut den aufgelösten [`RaidOAuthState`] für einen OAuth-Request auf.
///
/// Enthält die gesamte Logik aus Python `_build_state_info` +
/// `_resolve_scope_profile` (auth.py Zeilen 538–576):
///
/// 1. Login lowercase/trim.
/// 2. Scope-Profil über [`resolve_scope_profile`] auflösen (`auto` →
///    `dashboard_reauth` wenn Kontext existiert, sonst `base`).
/// 3. `expected_twitch_login` normalisieren:
///    - `discord:<user_id>` → Discord-User-ID extrahieren + Twitch-Identity
///      per Resolver nachschlagen.
///    - `public:website_onboarding` → `expected_twitch_login = None`.
///    - Sonst → `expected_twitch_login = requested_login`.
/// 4. Fehlende `discord_user_id` aus dem `discord:`-Prefix ableiten.
///
/// Python: `_build_state_info` (auth.py Zeilen 538–576).
pub async fn build_state_info(
    resolver: &dyn StreamerContextResolver,
    twitch_login: &str,
    scope_profile: &str,
    expected_twitch_login: Option<&str>,
    expected_twitch_user_id: Option<&str>,
    discord_user_id: Option<&str>,
) -> RaidOAuthState {
    let requested_login = twitch_login.trim().to_ascii_lowercase();

    let resolved_profile = resolve_scope_profile(resolver, &requested_login, scope_profile).await;

    let mut normalized_expected_login = expected_twitch_login.and_then(normalize_twitch_login);
    let mut normalized_expected_user_id = expected_twitch_user_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let mut normalized_discord_user_id = discord_user_id.and_then(normalize_state_discord_user_id);

    if normalized_expected_login.is_none() {
        if requested_login.starts_with("discord:") {
            // Discord-User-ID aus dem Prefix ableiten, falls noch nicht gesetzt.
            if normalized_discord_user_id.is_none() {
                let suffix = requested_login
                    .strip_prefix("discord:")
                    .unwrap_or("")
                    .trim()
                    .to_string();
                normalized_discord_user_id = normalize_state_discord_user_id(&suffix);
            }
            // Twitch-Identity per Resolver nachschlagen.
            if let Some(ref did) = normalized_discord_user_id {
                let (linked_login, linked_user_id) =
                    resolver.linked_twitch_identity_for_discord_user(did).await;
                normalized_expected_login = linked_login;
                if normalized_expected_user_id.is_none() {
                    normalized_expected_user_id = linked_user_id;
                }
            }
        } else if requested_login == PUBLIC_WEBSITE_ONBOARDING_LOGIN {
            // Website-Onboarding: kein konkreter Streamer-Account erwartet.
            normalized_expected_login = None;
        } else {
            normalized_expected_login = normalize_twitch_login(&requested_login);
        }
    }

    RaidOAuthState {
        requested_login,
        scope_profile: resolved_profile.to_string(),
        expected_twitch_login: normalized_expected_login,
        expected_twitch_user_id: normalized_expected_user_id,
        discord_user_id: normalized_discord_user_id,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Stub-Resolver für Tests (kein DB-Zugriff)
    // -----------------------------------------------------------------------

    struct StubResolver {
        /// Logins, für die ein existierender Kontext simuliert wird.
        existing: Vec<String>,
        /// Optionale feste Twitch-Identity für einen Discord-User.
        discord_identity: Option<(String, String, String)>, // (discord_id, login, user_id)
    }

    impl StubResolver {
        fn new(existing: &[&str]) -> Self {
            Self {
                existing: existing.iter().map(|s| s.to_string()).collect(),
                discord_identity: None,
            }
        }

        fn with_discord_identity(mut self, discord_id: &str, login: &str, user_id: &str) -> Self {
            self.discord_identity = Some((
                discord_id.to_string(),
                login.to_string(),
                user_id.to_string(),
            ));
            self
        }
    }

    #[async_trait]
    impl StreamerContextResolver for StubResolver {
        async fn has_existing_streamer_context(&self, login: &str) -> bool {
            self.existing.iter().any(|e| e == login)
        }

        async fn linked_twitch_identity_for_discord_user(
            &self,
            discord_user_id: &str,
        ) -> (Option<String>, Option<String>) {
            if let Some((ref did, ref login, ref uid)) = self.discord_identity {
                if did == discord_user_id {
                    return (Some(login.clone()), Some(uid.clone()));
                }
            }
            (None, None)
        }
    }

    // -----------------------------------------------------------------------
    // normalize_twitch_login
    // -----------------------------------------------------------------------

    #[test]
    fn normalize_login_trimmt_und_lowercased() {
        assert_eq!(
            normalize_twitch_login("  DragScope  "),
            Some("dragscope".to_string())
        );
        assert_eq!(normalize_twitch_login(""), None);
        assert_eq!(normalize_twitch_login("   "), None);
    }

    // -----------------------------------------------------------------------
    // normalize_state_discord_user_id
    // -----------------------------------------------------------------------

    #[test]
    fn normalize_discord_id_nur_ziffern_gueltig() {
        assert_eq!(
            normalize_state_discord_user_id("123456789"),
            Some("123456789".to_string())
        );
        assert_eq!(normalize_state_discord_user_id("abc123"), None);
        assert_eq!(normalize_state_discord_user_id(""), None);
        assert_eq!(normalize_state_discord_user_id("  "), None);
        assert_eq!(
            normalize_state_discord_user_id("  789  "),
            Some("789".to_string())
        );
    }

    // -----------------------------------------------------------------------
    // build_authorize_url
    // -----------------------------------------------------------------------

    #[test]
    fn authorize_url_enthaelt_alle_pflicht_parameter() {
        let url = build_authorize_url(
            "my_client_id",
            "https://example.com/callback",
            "base",
            "my_state_token",
        );
        assert!(url.starts_with(TWITCH_AUTHORIZE_URL));
        assert!(url.contains("client_id=my_client_id"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("force_verify=true"));
        assert!(url.contains("state=my_state_token"));
    }

    #[test]
    fn authorize_url_basis_profil_hat_keine_dashboard_scopes() {
        let url = build_authorize_url("cid", "https://example.com/cb", "base", "tok");
        // channel:read:subscriptions darf im Basis-Profil nicht vorkommen.
        assert!(!url.contains("channel%3Aread%3Asubscriptions"));
        // channel:manage:raids muss enthalten sein.
        assert!(url.contains("channel%3Amanage%3Araids"));
    }

    #[test]
    fn authorize_url_dashboard_reauth_enthaelt_erweiterte_scopes() {
        let url = build_authorize_url("cid", "https://example.com/cb", "dashboard_reauth", "tok");
        assert!(url.contains("channel%3Aread%3Asubscriptions"));
        assert!(url.contains("channel%3Aread%3Ahype_train"));
    }

    #[test]
    fn authorize_url_sonderzeichen_im_state_werden_encodiert() {
        let url = build_authorize_url(
            "cid",
            "https://example.com/cb",
            "base",
            "state with spaces & special=chars",
        );
        // Leerzeichen müssen percent-encodiert sein.
        assert!(!url.contains("state with spaces"));
    }

    // -----------------------------------------------------------------------
    // build_state_info
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn basis_login_bekommt_base_profil() {
        let resolver = StubResolver::new(&[]);
        let state = build_state_info(&resolver, "dragscope", "base", None, None, None).await;
        assert_eq!(state.scope_profile, "base");
        assert_eq!(state.requested_login, "dragscope");
        assert_eq!(state.expected_twitch_login, Some("dragscope".to_string()));
    }

    #[tokio::test]
    async fn explizites_dashboard_reauth_bleibt_erhalten() {
        let resolver = StubResolver::new(&[]); // kein existierender Kontext nötig
        let state =
            build_state_info(&resolver, "dragscope", "dashboard_reauth", None, None, None).await;
        assert_eq!(state.scope_profile, "dashboard_reauth");
    }

    #[tokio::test]
    async fn neuer_streamer_bekommt_base_profil_bei_auto() {
        let resolver = StubResolver::new(&[]); // kein Kontext vorhanden
        let state = build_state_info(&resolver, "neuer_streamer", "auto", None, None, None).await;
        assert_eq!(state.scope_profile, "base");
    }

    #[tokio::test]
    async fn existierender_streamer_bekommt_dashboard_reauth_bei_auto() {
        let resolver = StubResolver::new(&["bekannter_streamer"]);
        let state =
            build_state_info(&resolver, "bekannter_streamer", "auto", None, None, None).await;
        assert_eq!(state.scope_profile, "dashboard_reauth");
    }

    #[tokio::test]
    async fn discord_prefix_loest_twitch_identity_auf() {
        let resolver =
            StubResolver::new(&[]).with_discord_identity("123456789", "linked_streamer", "uid_999");
        let state =
            build_state_info(&resolver, "discord:123456789", "auto", None, None, None).await;
        assert_eq!(state.discord_user_id, Some("123456789".to_string()));
        assert_eq!(
            state.expected_twitch_login,
            Some("linked_streamer".to_string())
        );
        assert_eq!(state.expected_twitch_user_id, Some("uid_999".to_string()));
    }

    #[tokio::test]
    async fn discord_prefix_ohne_identity_ergibt_none_expected() {
        let resolver = StubResolver::new(&[]);
        let state =
            build_state_info(&resolver, "discord:999000999", "base", None, None, None).await;
        assert_eq!(state.discord_user_id, Some("999000999".to_string()));
        assert_eq!(state.expected_twitch_login, None);
    }

    #[tokio::test]
    async fn public_website_onboarding_ergibt_kein_expected_login() {
        let resolver = StubResolver::new(&[]);
        let state = build_state_info(
            &resolver,
            PUBLIC_WEBSITE_ONBOARDING_LOGIN,
            "base",
            None,
            None,
            None,
        )
        .await;
        assert_eq!(state.expected_twitch_login, None);
        assert_eq!(state.requested_login, PUBLIC_WEBSITE_ONBOARDING_LOGIN);
    }

    #[tokio::test]
    async fn explizites_expected_login_wird_nicht_ueberschrieben() {
        let resolver = StubResolver::new(&[]);
        let state = build_state_info(
            &resolver,
            "streamer_a",
            "base",
            Some("streamer_b"),
            None,
            None,
        )
        .await;
        // Ein explizit übergebenes expected_login bleibt unverändert.
        assert_eq!(state.expected_twitch_login, Some("streamer_b".to_string()));
    }

    #[tokio::test]
    async fn login_wird_lowercase_normalisiert() {
        let resolver = StubResolver::new(&[]);
        let state = build_state_info(&resolver, "  DragScope  ", "base", None, None, None).await;
        assert_eq!(state.requested_login, "dragscope");
        assert_eq!(state.expected_twitch_login, Some("dragscope".to_string()));
    }

    #[tokio::test]
    async fn resolve_uplink_ohne_kontext_bleibt_uplink() {
        // Kein Streamer-Kontext: `auto` faellt hier auf `base` zurueck. Das
        // Uplink-Profil darf das nicht tun, sonst fragt der Twitch-Dialog die
        // Chat- und Stream-Key-Rechte gar nicht erst an.
        let resolver = StubResolver::new(&[]);
        let state = build_state_info(&resolver, "neulingin", "uplink", None, None, None).await;
        assert_eq!(state.scope_profile, "uplink");

        // Und mit Kontext ebenso: kein stilles Hochstufen auf dashboard_reauth.
        let resolver = StubResolver::new(&["altgediente"]);
        let state = build_state_info(&resolver, "altgediente", "uplink", None, None, None).await;
        assert_eq!(state.scope_profile, "uplink");

        // Gegenprobe: `auto` verhaelt sich unveraendert.
        let state = build_state_info(&resolver, "altgediente", "auto", None, None, None).await;
        assert_eq!(state.scope_profile, "dashboard_reauth");
    }

    /// `auto` ist der Weg, den der Discord-Knopf und das Onboarding nehmen.
    /// Er darf nie beim Uplink-Profil landen, auch nicht bei einem Streamer,
    /// der den Uplink schon verbunden hat: sonst faengt eine Re-Autorisierung
    /// des Raid-Bots still an, Stream-Key und Chat mit anzufragen.
    #[tokio::test]
    async fn resolve_auto_liefert_nie_uplink() {
        for kontext in [vec![], vec!["altgediente"]] {
            let resolver = StubResolver::new(&kontext);
            for roh in ["auto", "", "unbekannt", "AUTO"] {
                let state =
                    build_state_info(&resolver, "altgediente", roh, None, None, None).await;
                assert_ne!(state.scope_profile, "uplink", "Profil {roh}");
                assert!(
                    state.scope_profile == "base" || state.scope_profile == "dashboard_reauth",
                    "Profil {roh} ergab {}",
                    state.scope_profile
                );
            }
        }
    }

    #[test]
    fn authorize_url_uplink_traegt_alle_scopes() {
        let url = build_authorize_url("cid", "https://x.test/callback/twitch", "uplink", "st");
        assert!(url.starts_with(TWITCH_AUTHORIZE_URL));
        for scope in crate::scope_profiles::UPLINK_SCOPES {
            let kodiert = scope.replace(':', "%3A");
            assert!(url.contains(&kodiert), "{scope} fehlt in {url}");
        }
        assert!(url.contains("force_verify=true"));
        // Der Raid-Reauth-Weg nutzt den bewusst erweiterten Satz mit zwölf Rechten.
        let raid = build_authorize_url(
            "cid",
            "https://x.test/callback/twitch",
            "dashboard_reauth",
            "st",
        );
        assert!(!raid.contains("stream_key"));
        assert!(!raid.contains("user%3Aread%3Achat"));
    }
}
