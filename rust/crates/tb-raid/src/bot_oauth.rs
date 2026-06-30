//! Bot-OAuth-Kontext-Resolver für den Follower-/Recruitment-Pfad (P2.46).
//!
//! Port von `bot/raid/runtime_support.py:68-94` (`resolve_bot_oauth_context`).
//! Python löst den **Bot**-Token über den zentralen Token-Manager auf, strippt
//! ein führendes `oauth:`-IRC-Präfix, ermittelt die `bot_id` und liefert die
//! gewährten Scopes als **normalisierte, kleingeschriebene Menge** zurück. Bei
//! fehlendem Manager oder Fehler degradiert er sauber zu `(None, None, ∅)`.
//!
//! Dieser Kontext ist die Basis-Primitive, auf der das Follower-Total-Gating
//! aufsetzt (P2.48/P2.50): nur wenn der Bot-Token `moderator:read:followers`
//! trägt (oder die Scope-Liste unbekannt/leer ist), darf der Bot-Pfad den
//! `total`-Wert lesen; sonst greift der per-Streamer-Token-Fallback.
//!
//! Trennung: Die reine Normalisierung (`normalize_bot_oauth_context`) ist
//! DB-/IO-frei und voll unit-testbar; die Auflösung über den konkreten
//! Token-Manager läuft hinter dem [`BotOAuthSource`]-Trait, damit die
//! Composition-Root (`bin/tb-bot`) den `tb_chat::BotTokenManager` einhängen kann,
//! ohne dass tb-raid von tb-chat abhängt.

use std::collections::BTreeSet;

/// Aufgelöster Bot-OAuth-Kontext: gestrippter Token, Bot-User-ID und
/// normalisierte Scope-Menge. Alle Felder sind leer/`None`, wenn kein
/// Bot-Token verfügbar ist (sauberer Degrade, Python `(None, None, set())`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BotOAuthContext {
    /// Access-Token **ohne** `oauth:`-Präfix, getrimmt. `None`, wenn leer.
    pub token: Option<String>,
    /// Bot-User-ID, getrimmt. `None`, wenn leer.
    pub bot_id: Option<String>,
    /// Gewährte Scopes — getrimmt, kleingeschrieben, dedupliziert, leere raus.
    pub scopes: BTreeSet<String>,
}

impl BotOAuthContext {
    /// Leerer Kontext (kein Bot-Token verfügbar) — Python `(None, None, set())`.
    pub fn empty() -> Self {
        Self::default()
    }

    /// `true`, wenn der Bot-Token den `total`-Wert der Helix-Follower-Route
    /// lesen darf: Token vorhanden **und** (`moderator:read:followers` im
    /// Scope-Set **oder** das Scope-Set ist unbekannt/leer).
    ///
    /// Port von `bot/raid/services/followers.py:271-280` (`bot_can_read_followers`):
    /// Python lässt den Bot-Pfad auch bei leerer/None-Scope-Liste zu, weil der
    /// Validate-Endpoint die Scopes nicht immer zurückliefert.
    pub fn can_read_followers(&self) -> bool {
        self.token.is_some()
            && (self.scopes.is_empty() || self.scopes.contains("moderator:read:followers"))
    }
}

/// Normalisiert rohe Token-Manager-Werte zu einem [`BotOAuthContext`].
///
/// Reine Funktion (kein IO) — Port der Normalisierungs-Schritte aus
/// `runtime_support.py:85-93`:
/// - Token getrimmt; führendes `oauth:` (case-insensitiv) entfernt; leer → `None`.
/// - `bot_id`: bevorzugt der vom Token-Refresh gelieferte Wert, sonst der
///   Fallback (Manager-`bot_id`); getrimmt; leer → `None`.
/// - Scopes: getrimmt, kleingeschrieben, leere verworfen, dedupliziert.
pub fn normalize_bot_oauth_context(
    token: Option<&str>,
    bot_id_primary: Option<&str>,
    bot_id_fallback: Option<&str>,
    scopes: impl IntoIterator<Item = String>,
) -> BotOAuthContext {
    let token = token.map(strip_oauth_prefix).filter(|t| !t.is_empty());

    let bot_id = bot_id_primary
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| bot_id_fallback.map(str::trim).filter(|s| !s.is_empty()))
        .map(str::to_string);

    let scopes = scopes
        .into_iter()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect::<BTreeSet<String>>();

    BotOAuthContext {
        token,
        bot_id,
        scopes,
    }
}

/// Entfernt ein führendes `oauth:`-Präfix (case-insensitiv) und trimmt.
/// Spiegelt `runtime_support.py:85-87`.
fn strip_oauth_prefix(raw: &str) -> String {
    let trimmed = raw.trim();
    // Byte-basiert (panik-frei auch bei Mehrbyte-Inhalt): `oauth:` ist ASCII.
    match trimmed.as_bytes().get(..6) {
        Some(prefix) if prefix.eq_ignore_ascii_case(b"oauth:") => trimmed[6..].trim().to_string(),
        _ => trimmed.to_string(),
    }
}

/// Quelle der rohen Bot-OAuth-Daten (Token + ID + Scopes). Wird in der
/// Composition-Root vom `tb_chat::BotTokenManager` implementiert, damit tb-raid
/// nicht von tb-chat abhängt.
#[async_trait::async_trait]
pub trait BotOAuthSource: Send + Sync {
    /// Liefert `(token, bot_id, scopes)` best-effort. Bei Fehler/kein Token →
    /// `(None, None, vec![])`, damit der Resolver sauber zu [`BotOAuthContext::empty`]
    /// degradiert.
    async fn raw_bot_oauth(&self) -> (Option<String>, Option<String>, Vec<String>);
}

/// Löst den Bot-OAuth-Kontext über die gegebene Quelle auf und normalisiert ihn.
///
/// Port von `runtime_support.py:68-94`: `None`-Quelle → leerer Kontext; sonst
/// die rohen Werte holen und [`normalize_bot_oauth_context`] anwenden.
pub async fn resolve_bot_oauth_context(source: Option<&dyn BotOAuthSource>) -> BotOAuthContext {
    let Some(source) = source else {
        return BotOAuthContext::empty();
    };
    let (token, bot_id, scopes) = source.raw_bot_oauth().await;
    normalize_bot_oauth_context(token.as_deref(), bot_id.as_deref(), None, scopes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_oauth_prefix_and_lowercases_scopes() {
        let ctx = normalize_bot_oauth_context(
            Some("oauth:ABC123"),
            Some("  botid  "),
            None,
            vec![
                "Moderator:Read:Followers".to_string(),
                "  user:bot  ".to_string(),
                "".to_string(),
            ],
        );
        assert_eq!(ctx.token.as_deref(), Some("ABC123"));
        assert_eq!(ctx.bot_id.as_deref(), Some("botid"));
        assert!(ctx.scopes.contains("moderator:read:followers"));
        assert!(ctx.scopes.contains("user:bot"));
        // Leerer Scope verworfen.
        assert_eq!(ctx.scopes.len(), 2);
    }

    #[test]
    fn normalize_uses_fallback_bot_id_when_primary_empty() {
        let ctx = normalize_bot_oauth_context(Some("tok"), Some("   "), Some("fallback"), vec![]);
        assert_eq!(ctx.bot_id.as_deref(), Some("fallback"));
    }

    #[test]
    fn normalize_empty_token_becomes_none() {
        let ctx = normalize_bot_oauth_context(Some("  oauth:  "), None, None, vec![]);
        assert!(ctx.token.is_none());
    }

    #[test]
    fn can_read_followers_requires_scope_or_empty() {
        // Scope vorhanden → ok.
        let with_scope = normalize_bot_oauth_context(
            Some("tok"),
            None,
            None,
            vec!["moderator:read:followers".to_string()],
        );
        assert!(with_scope.can_read_followers());

        // Scope-Set leer (unbekannt) → Python lässt es zu.
        let no_scopes = normalize_bot_oauth_context(Some("tok"), None, None, vec![]);
        assert!(no_scopes.can_read_followers());

        // Andere Scopes, aber nicht der nötige → gesperrt.
        let wrong_scope =
            normalize_bot_oauth_context(Some("tok"), None, None, vec!["user:bot".to_string()]);
        assert!(!wrong_scope.can_read_followers());

        // Kein Token → nie.
        let no_token = normalize_bot_oauth_context(None, None, None, vec![]);
        assert!(!no_token.can_read_followers());
    }

    struct StubSource {
        token: Option<String>,
        bot_id: Option<String>,
        scopes: Vec<String>,
    }

    #[async_trait::async_trait]
    impl BotOAuthSource for StubSource {
        async fn raw_bot_oauth(&self) -> (Option<String>, Option<String>, Vec<String>) {
            (self.token.clone(), self.bot_id.clone(), self.scopes.clone())
        }
    }

    #[tokio::test]
    async fn resolve_with_scope_returns_normalized_context() {
        let source = StubSource {
            token: Some("oauth:secret-tok".to_string()),
            bot_id: Some("42".to_string()),
            scopes: vec![
                "Moderator:Read:Followers".to_string(),
                "user:bot".to_string(),
            ],
        };
        let ctx = resolve_bot_oauth_context(Some(&source)).await;
        assert_eq!(ctx.token.as_deref(), Some("secret-tok"));
        assert_eq!(ctx.bot_id.as_deref(), Some("42"));
        assert!(ctx.scopes.contains("moderator:read:followers"));
        assert!(ctx.can_read_followers());
    }

    #[tokio::test]
    async fn resolve_without_source_is_empty() {
        let ctx = resolve_bot_oauth_context(None).await;
        assert_eq!(ctx, BotOAuthContext::empty());
        assert!(ctx.token.is_none());
        assert!(ctx.bot_id.is_none());
        assert!(ctx.scopes.is_empty());
    }
}
