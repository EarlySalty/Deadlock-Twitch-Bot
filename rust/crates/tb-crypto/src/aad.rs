//! AAD-Builder — byte-identisch zu den Python-AAD-Strings.
//!
//! Das AAD (Additional Authenticated Data) ist **nicht** Teil des gespeicherten Blobs.
//! Es wird beim Ver- und Entschlüsseln aus den Spaltenwerten rekonstruiert; weicht es
//! auch nur um ein Byte ab, schlägt die GCM-Tag-Prüfung fehl (`DecryptFailed`).
//!
//! `affiliate_pii` und `engagement_sender_auth` nutzen abweichende AAD-Formate und
//! werden mit ihren jeweiligen Feature-Crates ergänzt (nicht Teil von Phase 0a).

/// `twitch_raid_auth|<column>|<twitch_user_id>|<enc_version>`
pub fn raid_auth(column: &str, twitch_user_id: &str, enc_version: i64) -> String {
    format!("twitch_raid_auth|{column}|{twitch_user_id}|{enc_version}")
}

/// `social_media_platform_auth|<column>|<platform>|<streamer_login|global>|<enc_version>`
///
/// `streamer_login = None` ⇒ Literal `global` (entspricht `streamer_login or 'global'`).
pub fn social_media(
    column: &str,
    platform: &str,
    streamer_login: Option<&str>,
    enc_version: i64,
) -> String {
    let row = streamer_login.unwrap_or("global");
    format!("social_media_platform_auth|{column}|{platform}|{row}|{enc_version}")
}

/// `engagement_sender|<column>|<twitch_user_id>`
///
/// AAD des Engagement-Sende-Accounts (Smoke-Account). Anders als [`raid_auth`]
/// **ohne** `enc_version`-Suffix — byte-identisch zu `sender_auth._access_aad`
/// / `_refresh_aad` (Python `f"{PLATFORM}|{column}|{user_id}"`).
pub fn engagement_sender(column: &str, twitch_user_id: &str) -> String {
    format!("engagement_sender|{column}|{twitch_user_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raid_aad_matches_python_format() {
        assert_eq!(
            raid_auth("access_token", "123456", 1),
            "twitch_raid_auth|access_token|123456|1"
        );
    }

    #[test]
    fn social_aad_uses_global_when_no_streamer() {
        assert_eq!(
            social_media("refresh_token", "tiktok", None, 1),
            "social_media_platform_auth|refresh_token|tiktok|global|1"
        );
        assert_eq!(
            social_media("client_secret", "youtube", Some("dragskope"), 1),
            "social_media_platform_auth|client_secret|youtube|dragskope|1"
        );
    }

    #[test]
    fn engagement_sender_aad_matches_python_format() {
        assert_eq!(
            engagement_sender("access_token", "987654"),
            "engagement_sender|access_token|987654"
        );
        assert_eq!(
            engagement_sender("refresh_token", "987654"),
            "engagement_sender|refresh_token|987654"
        );
    }
}
