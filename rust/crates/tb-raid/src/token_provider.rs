//! `get_valid_token` — komponiert Lese-Store, Refresher und Blacklist zum
//! "gültigen Token holen, bei Ablauf erneuern". Port von
//! `RaidAuthManager.get_valid_token` (auth.py 1527).
//!
//! Reihenfolge (1:1 zu Python):
//! 1. Blacklist-Check + Recent-Failure-Cooldown → `None` (kein Refresh-Versuch).
//! 2. Token-Zeile lesen; fehlt sie oder `needs_reauth` → `None`.
//! 3. Noch gültig (jetzt < Ablauf − 300 s Puffer) → Access-Token zurück.
//! 4. Sonst refreshen; bei Erfolg den neu geschriebenen Token zurückgeben.

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};

use crate::token_refresher::{RaidTokenRefresher, RefreshOutcome, TokenBlacklist};
use crate::token_store::RaidAuthStore;

/// Sicherheitspuffer vor Ablauf (Python: `expires_at - 300`).
pub const EXPIRY_BUFFER_SECONDS: i64 = 300;

pub struct TokenProvider {
    store: RaidAuthStore,
    refresher: RaidTokenRefresher,
    blacklist: Arc<dyn TokenBlacklist>,
}

impl TokenProvider {
    pub fn new(
        store: RaidAuthStore,
        refresher: RaidTokenRefresher,
        blacklist: Arc<dyn TokenBlacklist>,
    ) -> Self {
        Self {
            store,
            refresher,
            blacklist,
        }
    }

    /// Holt einen gültigen Access-Token für den **Raid-Pfad** (`raid_enabled IS
    /// TRUE`), erneuert ihn bei Ablauf. `None`, wenn geblacklistet, im Cooldown,
    /// keine Zeile, `needs_reauth`, oder Refresh scheitert.
    pub async fn get_valid_token(
        &self,
        twitch_user_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<String>, sqlx::Error> {
        self.resolve(twitch_user_id, now, true).await
    }

    /// Wie [`get_valid_token`], aber **ohne** `raid_enabled`-Gate — Port von
    /// Python `get_tokens_for_user` (auth.py:1378). Für `!clip` & Co., die auch
    /// bei deaktivierten Raids den Broadcaster-Token nutzen dürfen.
    pub async fn get_valid_token_unrestricted(
        &self,
        twitch_user_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<String>, sqlx::Error> {
        self.resolve(twitch_user_id, now, false).await
    }

    /// Wie [`get_valid_token_unrestricted`], aber nur wenn die gespeicherte
    /// Scope-Liste den benoetigten Scope enthaelt. Das ist fuer Chatters/Presence
    /// wichtig: `moderator:read:chatters` ist am Helix-Endpunkt zwingend, ein
    /// Broadcaster-Token ohne Scope waere ein wirkungsloser 403-Pfad.
    pub async fn get_valid_token_unrestricted_with_scope(
        &self,
        twitch_user_id: &str,
        now: DateTime<Utc>,
        required_scope: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        let required_scope = required_scope.trim();
        if required_scope.is_empty() {
            return Ok(None);
        }

        let scopes = self.store.get_scopes(twitch_user_id).await?;
        if !scopes
            .iter()
            .any(|scope| scope.trim().eq_ignore_ascii_case(required_scope))
        {
            return Ok(None);
        }

        self.get_valid_token_unrestricted(twitch_user_id, now).await
    }

    /// Gemeinsamer „gültig holen, sonst refreshen"-Pfad. `require_raid_enabled`
    /// wählt nur die Lesevariante des Stores; Blacklist/Cooldown/needs_reauth/
    /// Refresh sind identisch. Der Re-Read nach Refresh nutzt dieselbe Variante.
    async fn resolve(
        &self,
        twitch_user_id: &str,
        now: DateTime<Utc>,
        require_raid_enabled: bool,
    ) -> Result<Option<String>, sqlx::Error> {
        if self.blacklist.is_blacklisted(twitch_user_id).await
            || self.blacklist.has_recent_failure(twitch_user_id).await
        {
            return Ok(None);
        }

        let Some(tokens) = self.load(twitch_user_id, require_raid_enabled).await? else {
            return Ok(None);
        };
        if tokens.needs_reauth {
            return Ok(None);
        }

        // Noch gültig (mit Puffer)?
        if let Some(expires_at) = tokens.token_expires_at {
            if now < expires_at - Duration::seconds(EXPIRY_BUFFER_SECONDS) {
                return Ok(Some(tokens.access_token));
            }
        }

        // Abgelaufen → refreshen (braucht den Refresh-Token).
        let Some(refresh_token) = tokens.refresh_token.as_deref() else {
            return Ok(None);
        };
        match self
            .refresher
            .refresh_and_store(twitch_user_id, &tokens.twitch_login, refresh_token, now)
            .await?
        {
            RefreshOutcome::Refreshed => {
                // Neu geschriebenen Token zurücklesen (entschlüsselt).
                Ok(self
                    .load(twitch_user_id, require_raid_enabled)
                    .await?
                    .map(|t| t.access_token))
            }
            RefreshOutcome::Blacklisted | RefreshOutcome::Skipped => Ok(None),
        }
    }

    /// Lese-Variante je nach `raid_enabled`-Anforderung (Helper für `resolve`).
    async fn load(
        &self,
        twitch_user_id: &str,
        require_raid_enabled: bool,
    ) -> Result<Option<crate::token_store::RaidTokens>, sqlx::Error> {
        if require_raid_enabled {
            self.store.load_decrypted(twitch_user_id).await
        } else {
            self.store.load_decrypted_unrestricted(twitch_user_id).await
        }
    }
}
