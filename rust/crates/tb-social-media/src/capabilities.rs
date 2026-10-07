use crate::credentials::SocialMediaCredentials;

#[derive(Debug, Clone, serde::Serialize)]
pub struct PlatformCapabilities {
    pub upload: bool,
    pub statistics: bool,
    pub upload_mode: &'static str,
    pub reason: Option<&'static str>,
}

pub fn platform_capabilities(platform: &str) -> PlatformCapabilities {
    let instagram_ready = std::env::var("INSTAGRAM_APP_APPROVED")
        .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        && ["INSTAGRAM_CLIENT_ID", "INSTAGRAM_CLIENT_SECRET"]
            .iter()
            .all(|key| std::env::var(key).is_ok_and(|value| !value.trim().is_empty()));
    capabilities_for(platform, instagram_ready)
}

fn capabilities_for(platform: &str, instagram_ready: bool) -> PlatformCapabilities {
    match platform {
        "tiktok" => PlatformCapabilities {
            upload: true,
            statistics: false,
            upload_mode: "direct_post",
            reason: None,
        },
        "youtube" => PlatformCapabilities {
            upload: true,
            statistics: true,
            upload_mode: "upload",
            reason: None,
        },
        "instagram" => PlatformCapabilities {
            upload: instagram_ready,
            statistics: instagram_ready,
            upload_mode: "reel",
            reason: (!instagram_ready)
                .then_some("Instagram ist in dieser Beta noch nicht verfügbar."),
        },
        _ => PlatformCapabilities {
            upload: false,
            statistics: false,
            upload_mode: "unavailable",
            reason: Some("Diese Plattform ist nicht verfügbar."),
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadWaitReason {
    PlatformUnavailable,
    ConnectionMissing,
    ConnectionIncomplete,
}

impl UploadWaitReason {
    pub fn code(self) -> &'static str {
        match self {
            Self::PlatformUnavailable => "platform_unavailable",
            Self::ConnectionMissing => "connection_missing",
            Self::ConnectionIncomplete => "connection_incomplete",
        }
    }
}

pub fn upload_wait_reason(
    platform: &str,
    creds: Option<&SocialMediaCredentials>,
) -> Option<String> {
    upload_wait_reason_kind(platform, creds).map(|reason| reason.code().to_string())
}

pub fn upload_wait_reason_kind(
    platform: &str,
    creds: Option<&SocialMediaCredentials>,
) -> Option<UploadWaitReason> {
    if !platform_capabilities(platform).upload {
        return Some(UploadWaitReason::PlatformUnavailable);
    }
    let Some(creds) = creds else {
        return Some(UploadWaitReason::ConnectionMissing);
    };
    let complete = !creds.access_token.trim().is_empty()
        && match platform {
            "tiktok" | "youtube" => creds
                .client_id
                .as_deref()
                .is_some_and(|id| !id.trim().is_empty()),
            "instagram" => creds
                .platform_user_id
                .as_deref()
                .is_some_and(|id| !id.trim().is_empty()),
            _ => false,
        };
    if !complete {
        return Some(UploadWaitReason::ConnectionIncomplete);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beta_capabilities_are_explicit() {
        let tiktok = capabilities_for("tiktok", false);
        assert!(tiktok.upload);
        assert!(!tiktok.statistics);
        assert_eq!(tiktok.upload_mode, "direct_post");
        assert!(!capabilities_for("instagram", false).upload);
        assert!(capabilities_for("instagram", true).upload);
        assert!(capabilities_for("youtube", false).statistics);
        assert_eq!(
            upload_wait_reason_kind("tiktok", None),
            Some(UploadWaitReason::ConnectionMissing)
        );
    }

    #[test]
    fn incomplete_connection_has_a_structured_reason() {
        let mut creds = SocialMediaCredentials {
            id: 1,
            platform: "youtube".into(),
            streamer_login: None,
            access_token: "synthetic".into(),
            refresh_token: None,
            client_id: None,
            client_secret: None,
            expires_at: None,
            scopes: None,
            platform_user_id: None,
            platform_username: None,
        };
        assert_eq!(
            upload_wait_reason_kind("youtube", Some(&creds)),
            Some(UploadWaitReason::ConnectionIncomplete)
        );
        creds.client_id = Some("client".into());
        assert_eq!(upload_wait_reason_kind("youtube", Some(&creds)), None);
        creds.access_token = " ".into();
        assert_eq!(
            upload_wait_reason_kind("youtube", Some(&creds)),
            Some(UploadWaitReason::ConnectionIncomplete)
        );
        assert_eq!(
            upload_wait_reason_kind("unknown", Some(&creds)),
            Some(UploadWaitReason::PlatformUnavailable)
        );
    }
}
