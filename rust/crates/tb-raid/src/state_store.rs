//! OAuth-State-Token-Store (`oauth_state_tokens`) — geteilt mit social-media,
//! deshalb **plattform-gated** auf `twitch_raid` (Python:
//! `_OAUTH_STATE_PLATFORM_RAID`). Ein State-Token überbrückt Authorize-Request
//! und Callback über Prozess-/Restart-Grenzen hinweg.
//!
//! Port von `RaidAuthManager._persist/_lookup/_consume_state_token`. Bewusste
//! Sauberkeit gegenüber dem Original:
//!
//! - Die DB-Spalte heißt `pkce_verifier`, speichert aber serialisierte
//!   State-Meta (Scope-Profil, erwarteter Login/User-ID, Discord-ID). Der
//!   Spaltenname ist Alt-Last; das Rust-Feld heißt ehrlich `state_meta`. Die
//!   Spalte bleibt (Schema-Vertrag), nur die Benennung wird geradegezogen.
//! - Meta-Format ist byte-kompatibel zu Python (`json.dumps(..., sort_keys,
//!   separators=(",",":"))`) — leere Felder werden weggelassen, damit
//!   bestehende Tokens und solche aus Python identisch round-trippen.

use chrono::{DateTime, Duration, Utc};
use serde_json::{Map, Value};
use sqlx::PgPool;

/// Plattform-Discriminator von raid in der geteilten Tabelle.
pub const PLATFORM_RAID: &str = "twitch_raid";
/// Gültigkeit eines State-Tokens (Python `_OAUTH_STATE_TTL_SECONDS`).
pub const STATE_TTL_SECONDS: i64 = 600;

/// Aufgelöster OAuth-State (Python `RaidOAuthState`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RaidOAuthState {
    pub requested_login: String,
    pub scope_profile: String,
    pub expected_twitch_login: Option<String>,
    pub expected_twitch_user_id: Option<String>,
    pub discord_user_id: Option<String>,
}

impl RaidOAuthState {
    /// Serialisiert die Meta (alles außer `requested_login`/`redirect_uri`) als
    /// kompaktes, sortiertes JSON — byte-identisch zu Pythons
    /// `_serialize_state_meta`. Leere Optionale werden ausgelassen.
    fn serialize_meta(&self) -> String {
        let mut map = Map::new();
        map.insert(
            "scope_profile".to_string(),
            Value::String(self.scope_profile.clone()),
        );
        // BTreeMap-Reihenfolge wäre nötig — serde_json::Map ist hier aber
        // bereits sortiert serialisierbar via to_string auf einer geordneten
        // Struktur. Wir bauen die Felder explizit und sortieren beim Dump.
        if let Some(login) = non_empty(&self.expected_twitch_login) {
            map.insert("expected_twitch_login".to_string(), Value::String(login));
        }
        if let Some(uid) = non_empty(&self.expected_twitch_user_id) {
            map.insert("expected_twitch_user_id".to_string(), Value::String(uid));
        }
        if let Some(did) = non_empty(&self.discord_user_id) {
            map.insert("discord_user_id".to_string(), Value::String(did));
        }
        dump_sorted_compact(&map)
    }

    /// Gegenstück zu Pythons `_parse_state_meta`: toleriert leeres/kaputtes
    /// JSON (dann nur Defaults). `requested_login` kommt aus der eigenen Spalte.
    fn from_row(requested_login: String, meta_raw: &str) -> Self {
        let parsed: Value = serde_json::from_str(meta_raw).unwrap_or(Value::Null);
        let get = |key: &str| {
            parsed
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        RaidOAuthState {
            requested_login,
            scope_profile: get("scope_profile").unwrap_or_default(),
            expected_twitch_login: get("expected_twitch_login"),
            expected_twitch_user_id: get("expected_twitch_user_id"),
            discord_user_id: get("discord_user_id"),
        }
    }
}

/// Zugriff auf `oauth_state_tokens`, plattform-gated auf `twitch_raid`.
#[derive(Clone)]
pub struct StateStore {
    pool: PgPool,
    redirect_uri: String,
}

impl StateStore {
    pub fn new(pool: PgPool, redirect_uri: impl Into<String>) -> Self {
        Self {
            pool,
            redirect_uri: redirect_uri.into(),
        }
    }

    /// Persistiert einen State-Token (Upsert), gültig für [`STATE_TTL_SECONDS`].
    pub async fn persist(
        &self,
        state_token: &str,
        state: &RaidOAuthState,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        let expires_at = now + Duration::seconds(STATE_TTL_SECONDS);
        let state_lookup_key = tb_crypto::token_lookup_key(state_token);
        sqlx::query!(
            r#"
            INSERT INTO oauth_state_tokens
                (state_token, platform, streamer_login, redirect_uri, pkce_verifier, expires_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (state_token) DO UPDATE SET
                platform = EXCLUDED.platform,
                streamer_login = EXCLUDED.streamer_login,
                redirect_uri = EXCLUDED.redirect_uri,
                pkce_verifier = EXCLUDED.pkce_verifier,
                expires_at = EXCLUDED.expires_at
            "#,
            state_lookup_key,
            PLATFORM_RAID,
            &state.requested_login,
            &self.redirect_uri,
            state.serialize_meta(),
            expires_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Liest einen noch gültigen State, ohne ihn zu verbrauchen. Leerer
    /// `streamer_login` → `None` (wie Python).
    pub async fn lookup(
        &self,
        state_token: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<RaidOAuthState>, sqlx::Error> {
        let state_lookup_key = tb_crypto::token_lookup_key(state_token);
        let row = sqlx::query!(
            r#"
            SELECT COALESCE(streamer_login, '') AS "streamer_login!",
                   pkce_verifier AS "pkce_verifier?"
            FROM oauth_state_tokens
            WHERE state_token = $1 AND platform = $2 AND expires_at > $3
            LIMIT 1
            "#,
            state_lookup_key,
            PLATFORM_RAID,
            now
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row_to_state(
            row.map(|row| (row.streamer_login, row.pkce_verifier)),
        ))
    }

    /// Verbraucht einen noch gültigen State atomar (DELETE … RETURNING).
    pub async fn consume(
        &self,
        state_token: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<RaidOAuthState>, sqlx::Error> {
        let state_lookup_key = tb_crypto::token_lookup_key(state_token);
        let row = sqlx::query!(
            r#"
            DELETE FROM oauth_state_tokens
            WHERE state_token = $1 AND platform = $2 AND expires_at > $3
            RETURNING COALESCE(streamer_login, '') AS "streamer_login!",
                      pkce_verifier AS "pkce_verifier?"
            "#,
            state_lookup_key,
            PLATFORM_RAID,
            now
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row_to_state(
            row.map(|row| (row.streamer_login, row.pkce_verifier)),
        ))
    }

    /// Räumt abgelaufene raid-State-Tokens ab; liefert die Anzahl.
    /// Nur eigene Plattform — social-media-Einträge bleiben unangetastet.
    pub async fn cleanup_expired(&self, now: DateTime<Utc>) -> Result<u64, sqlx::Error> {
        let result = sqlx::query!(
            "DELETE FROM oauth_state_tokens WHERE platform = $1 AND expires_at <= $2",
            PLATFORM_RAID,
            now
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }
}

fn row_to_state(row: Option<(String, Option<String>)>) -> Option<RaidOAuthState> {
    let (login, meta_raw) = row?;
    let login = login.trim().to_string();
    if login.is_empty() {
        return None;
    }
    Some(RaidOAuthState::from_row(
        login,
        meta_raw.as_deref().unwrap_or(""),
    ))
}

fn non_empty(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// `json.dumps(map, sort_keys=True, separators=(",",":"))`-Äquivalent.
/// serde_json sortiert Objektschlüssel nicht von sich aus — wir bauen die
/// String-Repräsentation deterministisch sortiert.
fn dump_sorted_compact(map: &Map<String, Value>) -> String {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    let mut out = String::from("{");
    for (i, key) in keys.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        // Werte sind ausschließlich Strings → serde_json escaped korrekt.
        out.push_str(&Value::String((*key).clone()).to_string());
        out.push(':');
        out.push_str(&map[*key].to_string());
    }
    out.push('}');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_serialisierung_ist_sortiert_kompakt_und_laesst_leere_weg() {
        let state = RaidOAuthState {
            requested_login: "drag".to_string(),
            scope_profile: "raid".to_string(),
            expected_twitch_login: Some("drag".to_string()),
            expected_twitch_user_id: None,
            discord_user_id: Some("123".to_string()),
        };
        // Sortiert: discord_user_id, expected_twitch_login, scope_profile.
        assert_eq!(
            state.serialize_meta(),
            r#"{"discord_user_id":"123","expected_twitch_login":"drag","scope_profile":"raid"}"#
        );
    }

    #[test]
    fn meta_roundtrip_und_toleranz_gegen_kaputtes_json() {
        let state = RaidOAuthState {
            requested_login: "drag".to_string(),
            scope_profile: "raid_full".to_string(),
            expected_twitch_login: Some("drag".to_string()),
            expected_twitch_user_id: Some("42".to_string()),
            discord_user_id: None,
        };
        let back = RaidOAuthState::from_row("drag".to_string(), &state.serialize_meta());
        assert_eq!(back, state);

        // Kaputtes/leeres Meta → Defaults, kein Panic.
        let fallback = RaidOAuthState::from_row("drag".to_string(), "{kaputt");
        assert_eq!(fallback.scope_profile, "");
        assert_eq!(fallback.expected_twitch_login, None);
    }
}
