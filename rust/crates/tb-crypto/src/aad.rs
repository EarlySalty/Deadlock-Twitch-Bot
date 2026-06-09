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
}
