use super::*;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TikTokDraft {
    caption: String,
    #[serde(default)]
    standard_description: String,
    privacy_level: String,
    allow_comment: bool,
    allow_duet: bool,
    allow_stitch: bool,
    #[serde(default)]
    commercial_content: bool,
    #[serde(default)]
    brand_organic_toggle: bool,
    #[serde(default)]
    brand_content_toggle: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    music_consent: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    music_consent_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    music_consent_platform_user_id: Option<String>,
}

impl TikTokDraft {
    fn validate(&self) -> Result<(), Response> {
        if combined_caption(&self.caption, &self.standard_description)
            .chars()
            .count()
            > 2200
            || !matches!(
                self.privacy_level.as_str(),
                "" | "PUBLIC_TO_EVERYONE"
                    | "MUTUAL_FOLLOW_FRIENDS"
                    | "FOLLOWER_OF_CREATOR"
                    | "SELF_ONLY"
            )
        {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "invalid_tiktok_draft"})),
            )
                .into_response());
        }
        Ok(())
    }

    fn without_music_consent(mut self) -> Self {
        self.music_consent = false;
        self.music_consent_at = None;
        self.music_consent_platform_user_id = None;
        self
    }

    fn for_account(self, account: Option<&str>) -> Self {
        if self.music_consent
            && self.music_consent_at.is_some()
            && account.is_some_and(|id| !id.is_empty())
            && self.music_consent_platform_user_id.as_deref() == account
        {
            self
        } else {
            self.without_music_consent()
        }
    }

    fn without_disclosure(mut self) -> Self {
        self.commercial_content = false;
        self.brand_organic_toggle = false;
        self.brand_content_toggle = false;
        self
    }
}

async fn owner(
    pool: &PgPool,
    auth: &DashboardAuthLevel,
    raw: String,
) -> Result<(i64, String), Response> {
    let scope = require_sm_access(auth, pool, None).await?;
    let clip_id = normalize_id(Some(&Value::String(raw))).ok_or_else(invalid_clip_db_id)?;
    require_clip_in_scope(pool, auth, clip_id, scope.as_deref()).await?;
    if let Some(response) = guard_partner_access_for_clip(pool, auth, clip_id).await {
        return Err(response);
    }
    let user: Option<String> =
        sqlx::query_scalar("SELECT twitch_user_id FROM twitch_clips_social_media WHERE id = $1")
            .bind(clip_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| clip_load_failed())?
            .flatten();
    let user = user
        .filter(|user| !user.is_empty() && user.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or_else(invalid_clip_db_id)?;
    Ok((clip_id, user))
}

fn draft_key(clip_id: i64, user: &str) -> String {
    format!("tiktok_draft:{user}:{clip_id}")
}

fn defaults_key(user: &str) -> String {
    format!("tiktok_defaults:{user}")
}

async fn load(pool: &PgPool, key: &str) -> Result<Option<TikTokDraft>, Response> {
    let raw: Option<String> =
        sqlx::query_scalar("SELECT value::text FROM social_media_settings WHERE key = $1")
            .bind(key)
            .fetch_optional(pool)
            .await
            .map_err(|_| clip_load_failed())?
            .flatten();
    raw.map(|raw| serde_json::from_str::<TikTokDraft>(&raw).map_err(|_| clip_load_failed()))
        .transpose()
}

pub(super) async fn saved_choices(
    pool: &PgPool,
    clip_id: i64,
    user: &str,
) -> Result<(Option<TikTokDraft>, Option<TikTokDraft>), Response> {
    let draft = load(pool, &draft_key(clip_id, user))
        .await?
        .map(TikTokDraft::without_music_consent);
    let defaults = load(pool, &defaults_key(user))
        .await?
        .map(TikTokDraft::without_disclosure)
        .map(TikTokDraft::without_music_consent);
    Ok((draft, defaults))
}

async fn tiktok_account(pool: &PgPool, user: &str) -> Result<Option<String>, Response> {
    sqlx::query_scalar::<_, Option<String>>(
        "SELECT platform_user_id FROM social_media_platform_auth WHERE platform = 'tiktok' AND enabled = 1 AND twitch_user_id = $1 ORDER BY authorized_at DESC, id DESC LIMIT 1",
    )
    .bind(user)
    .fetch_optional(pool)
    .await
    .map(Option::flatten)
    .map_err(|_| clip_load_failed())
}

pub async fn get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(raw): Path<String>,
) -> Response {
    let (clip_id, user) = match owner(&pool, &auth, raw).await {
        Ok(owner) => owner,
        Err(response) => return response,
    };
    let draft = match load(&pool, &draft_key(clip_id, &user)).await {
        Ok(draft) => draft.map(TikTokDraft::without_music_consent),
        Err(response) => return response,
    };
    let defaults = match load(&pool, &defaults_key(&user)).await {
        Ok(defaults) => defaults.map(TikTokDraft::without_disclosure),
        Err(response) => return response,
    };
    let account = match tiktok_account(&pool, &user).await {
        Ok(account) => account,
        Err(response) => return response,
    };
    let music_consent = match valid_music_consent(&pool, &user, account.as_deref()).await {
        Ok(valid) => valid,
        Err(response) => return response,
    };
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({"draft": draft, "defaults": defaults, "music_consent": music_consent, "music_consent_platform_user_id": account})),
    )
        .into_response()
}

async fn save(pool: &PgPool, key: String, user: &str, draft: TikTokDraft) -> Response {
    if let Err(response) = draft.validate() {
        return response;
    }
    let value = match serde_json::to_value(&draft) {
        Ok(value) => value,
        Err(_) => return invalid_payload(),
    };
    match tb_social_media::settings::set_setting(pool, &key, &value, Some(user)).await {
        Ok(()) => Json(json!({"success": true, "choices": draft})).into_response(),
        Err(error) => {
            tracing::warn!(%error, %key, "TikTok editor save failed");
            clip_load_failed()
        }
    }
}

pub async fn draft_put_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(raw): Path<String>,
    Json(draft): Json<TikTokDraft>,
) -> Response {
    let (clip_id, user) = match owner(&pool, &auth, raw).await {
        Ok(owner) => owner,
        Err(response) => return response,
    };
    save(
        &pool,
        draft_key(clip_id, &user),
        &user,
        draft.without_music_consent(),
    )
    .await
}

pub async fn defaults_put_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(raw): Path<String>,
    Json(draft): Json<TikTokDraft>,
) -> Response {
    let (_, user) = match owner(&pool, &auth, raw).await {
        Ok(owner) => owner,
        Err(response) => return response,
    };
    if let Err(response) = draft.validate() {
        return response;
    }
    let legacy = match load(&pool, &defaults_key(&user)).await {
        Ok(legacy) => legacy,
        Err(response) => return response,
    };
    let mut draft = draft.without_disclosure().without_music_consent();
    if let Some(legacy) = legacy {
        draft.music_consent = legacy.music_consent;
        draft.music_consent_at = legacy.music_consent_at;
        draft.music_consent_platform_user_id = legacy.music_consent_platform_user_id;
    }
    draft.caption.clear();
    save(&pool, defaults_key(&user), &user, draft).await
}

fn combined_caption(caption: &str, standard: &str) -> String {
    [caption.trim(), standard.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MusicConsent {
    platform_user_id: String,
    consented_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MusicConsentRequest {
    platform_user_id: String,
}

fn consent_key(user: &str, account: &str) -> String {
    format!("tiktok_music_consent:{user}:{account}")
}

pub(super) async fn valid_music_consent(
    pool: &PgPool,
    user: &str,
    account: Option<&str>,
) -> Result<bool, Response> {
    let Some(account) = account.filter(|account| !account.is_empty()) else {
        return Ok(false);
    };
    let raw: Option<String> =
        sqlx::query_scalar("SELECT value::text FROM social_media_settings WHERE key = $1")
            .bind(consent_key(user, account))
            .fetch_optional(pool)
            .await
            .map_err(|_| clip_load_failed())?
            .flatten();
    if let Some(raw) = raw {
        let consent: MusicConsent = serde_json::from_str(&raw).map_err(|_| clip_load_failed())?;
        if consent.platform_user_id == account && consent.consented_at.is_some() {
            return Ok(true);
        }
    }
    Ok(load(pool, &defaults_key(user))
        .await?
        .is_some_and(|draft| draft.for_account(Some(account)).music_consent))
}

pub async fn music_consent_put_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(raw): Path<String>,
    Json(request): Json<MusicConsentRequest>,
) -> Response {
    let (_, user) = match owner(&pool, &auth, raw).await {
        Ok(owner) => owner,
        Err(response) => return response,
    };
    let account = match tiktok_account(&pool, &user).await {
        Ok(account) => account,
        Err(response) => return response,
    };
    if request.platform_user_id.is_empty()
        || Some(request.platform_user_id.as_str()) != account.as_deref()
    {
        return (StatusCode::CONFLICT, Json(json!({"error": "tiktok_posting", "message": "Das TikTok-Konto hat sich geändert. Bitte öffne die Freigabe erneut."}))).into_response();
    }
    let key = consent_key(&user, &request.platform_user_id);
    let consent = MusicConsent {
        platform_user_id: request.platform_user_id,
        consented_at: Some(chrono::Utc::now()),
    };
    let value = match serde_json::to_value(consent) {
        Ok(value) => value,
        Err(_) => return invalid_payload(),
    };
    match tb_social_media::settings::set_setting(&pool, &key, &value, Some(&user)).await {
        Ok(()) => Json(json!({"success": true})).into_response(),
        Err(_) => clip_load_failed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn server_roundtrip_is_scoped_and_does_not_schedule() {
        let db = crate::test_database::Database::new().await;
        for statement in [
            "CREATE TABLE social_media_settings (key TEXT PRIMARY KEY, value JSONB, updated_at TIMESTAMPTZ, updated_by TEXT)",
            "CREATE TABLE twitch_clips_social_media (id BIGINT PRIMARY KEY, twitch_user_id TEXT, tiktok_post_options JSONB)",
            "CREATE TABLE social_media_partner_access (twitch_user_id TEXT PRIMARY KEY, granted BOOLEAN)",
            "CREATE TABLE IF NOT EXISTS social_media_platform_auth (id INTEGER PRIMARY KEY, platform TEXT, twitch_user_id TEXT, platform_user_id TEXT, enabled INTEGER, authorized_at TIMESTAMPTZ)",
            "INSERT INTO twitch_clips_social_media VALUES (1, '42', NULL), (2, '43', NULL), (3, '42', NULL)",
            "INSERT INTO social_media_partner_access VALUES ('42', TRUE), ('43', TRUE)",
        ] {
            sqlx::query(statement).execute(&db.pool).await.unwrap();
        }
        let partner = DashboardAuthLevel::Partner {
            twitch_login: "test".into(),
            twitch_user_id: "42".into(),
            display_name: "Test".into(),
        };
        let draft: TikTokDraft = serde_json::from_value(json!({"caption": "Eigener Entwurf ä", "privacy_level": "SELF_ONLY", "allow_comment": true, "allow_duet": true, "allow_stitch": false, "commercial_content": true, "brand_organic_toggle": true})).unwrap();
        assert_eq!(
            draft_put_handler(
                partner.clone(),
                State(db.pool.clone()),
                Path("1".into()),
                Json(draft.clone())
            )
            .await
            .status(),
            StatusCode::OK
        );
        assert_eq!(
            defaults_put_handler(
                partner.clone(),
                State(db.pool.clone()),
                Path("1".into()),
                Json(draft.clone())
            )
            .await
            .status(),
            StatusCode::OK
        );
        assert_eq!(
            load(&db.pool, &draft_key(1, "42")).await.unwrap(),
            Some(draft.clone())
        );
        assert_eq!(
            load(&db.pool, &defaults_key("42")).await.unwrap(),
            Some({
                let mut defaults = draft.clone().without_disclosure();
                defaults.caption.clear();
                defaults
            })
        );
        assert_eq!(
            draft_put_handler(
                partner.clone(),
                State(db.pool.clone()),
                Path("2".into()),
                Json(draft)
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            get_handler(partner.clone(), State(db.pool.clone()), Path("2".into()))
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            get_handler(
                DashboardAuthLevel::None,
                State(db.pool.clone()),
                Path("1".into())
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        assert!(load(&db.pool, &draft_key(3, "42")).await.unwrap().is_none());
        assert!(load(&db.pool, &defaults_key("43")).await.unwrap().is_none());
        let response = get_handler(partner.clone(), State(db.pool.clone()), Path("1".into())).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let published: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_clips_social_media WHERE tiktok_post_options IS NOT NULL",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(published, 0);
        sqlx::query(
            "UPDATE social_media_partner_access SET granted = FALSE WHERE twitch_user_id = '42'",
        )
        .execute(&db.pool)
        .await
        .unwrap();
        assert_eq!(
            get_handler(partner, State(db.pool.clone()), Path("1".into()))
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
        db.close().await;
    }

    #[test]
    fn choices_keep_post_consent_excluded_and_validate_incomplete_drafts() {
        let raw = json!({"caption": "Beschreibung", "privacy_level": "", "allow_comment": false, "allow_duet": true, "allow_stitch": false});
        let draft: TikTokDraft = serde_json::from_value(raw.clone()).unwrap();
        assert!(draft.validate().is_ok());
        assert!(!draft.music_consent);
        assert!(draft.music_consent_at.is_none());
        assert!(draft.music_consent_platform_user_id.is_none());
        let mut consent = raw;
        consent["consent"] = json!(true);
        assert!(serde_json::from_value::<TikTokDraft>(consent).is_err());
        let mut invalid = draft.clone();
        invalid.caption = "ä".repeat(2201);
        assert!(invalid.validate().is_err());
        invalid.caption.clear();
        invalid.privacy_level = "unknown".into();
        assert!(invalid.validate().is_err());
        assert_ne!(draft_key(1, "42"), draft_key(1, "43"));
        assert_ne!(defaults_key("42"), defaults_key("43"));
    }

    #[tokio::test]
    async fn legacy_consent_survives_saving_defaults_and_new_consent_is_account_bound() {
        let db = crate::test_database::Database::new().await;
        for statement in [
            "CREATE TABLE social_media_settings (key TEXT PRIMARY KEY, value JSONB, updated_at TIMESTAMPTZ, updated_by TEXT)",
            "CREATE TABLE twitch_clips_social_media (id BIGINT PRIMARY KEY, twitch_user_id TEXT)",
            "CREATE TABLE social_media_partner_access (twitch_user_id TEXT PRIMARY KEY, granted BOOLEAN)",
            "CREATE TABLE social_media_platform_auth (id INTEGER PRIMARY KEY, platform TEXT, twitch_user_id TEXT, platform_user_id TEXT, enabled INTEGER, authorized_at TIMESTAMPTZ)",
            "INSERT INTO twitch_clips_social_media VALUES (1, '42')",
            "INSERT INTO social_media_partner_access VALUES ('42', TRUE)",
            "INSERT INTO social_media_platform_auth VALUES (1, 'tiktok', '42', 'account-a', 1, NOW())",
        ] {
            sqlx::query(statement).execute(&db.pool).await.unwrap();
        }
        let partner = DashboardAuthLevel::Partner {
            twitch_login: "test".into(),
            twitch_user_id: "42".into(),
            display_name: "Test".into(),
        };
        let legacy: TikTokDraft = serde_json::from_value(json!({
            "caption": "Old clip", "privacy_level": "SELF_ONLY", "allow_comment": true,
            "allow_duet": false, "allow_stitch": false, "music_consent": true,
            "music_consent_at": "2026-01-01T00:00:00Z", "music_consent_platform_user_id": "account-a"
        })).unwrap();
        assert_eq!(
            save(&db.pool, defaults_key("42"), "42", legacy.clone())
                .await
                .status(),
            StatusCode::OK
        );
        assert!(valid_music_consent(&db.pool, "42", Some("account-a"))
            .await
            .unwrap());
        assert!(!valid_music_consent(&db.pool, "42", Some("account-b"))
            .await
            .unwrap());
        assert_eq!(
            defaults_put_handler(
                partner.clone(),
                State(db.pool.clone()),
                Path("1".into()),
                Json(legacy.without_music_consent())
            )
            .await
            .status(),
            StatusCode::OK
        );
        assert!(valid_music_consent(&db.pool, "42", Some("account-a"))
            .await
            .unwrap());
        assert!(load(&db.pool, &defaults_key("42"))
            .await
            .unwrap()
            .unwrap()
            .caption
            .is_empty());
        assert_eq!(
            music_consent_put_handler(
                partner.clone(),
                State(db.pool.clone()),
                Path("1".into()),
                Json(MusicConsentRequest {
                    platform_user_id: "account-b".into()
                })
            )
            .await
            .status(),
            StatusCode::CONFLICT
        );
        sqlx::query("UPDATE social_media_platform_auth SET platform_user_id = 'account-b'")
            .execute(&db.pool)
            .await
            .unwrap();
        assert!(!valid_music_consent(&db.pool, "42", Some("account-b"))
            .await
            .unwrap());
        assert_eq!(
            music_consent_put_handler(
                partner.clone(),
                State(db.pool.clone()),
                Path("1".into()),
                Json(MusicConsentRequest {
                    platform_user_id: "account-b".into()
                })
            )
            .await
            .status(),
            StatusCode::OK
        );
        assert!(valid_music_consent(&db.pool, "42", Some("account-b"))
            .await
            .unwrap());
        let raw: String = sqlx::query_scalar(
            "SELECT value::text FROM social_media_settings WHERE key = 'tiktok_music_consent:42:account-b'",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        let stored: MusicConsent = serde_json::from_str(&raw).unwrap();
        assert!(stored.consented_at.unwrap() > chrono::Utc::now() - chrono::Duration::minutes(1));
        let defaults = load(&db.pool, &defaults_key("42")).await.unwrap().unwrap();
        assert_eq!(
            defaults_put_handler(
                partner.clone(),
                State(db.pool.clone()),
                Path("1".into()),
                Json(defaults)
            )
            .await
            .status(),
            StatusCode::OK
        );
        assert!(valid_music_consent(&db.pool, "42", Some("account-b"))
            .await
            .unwrap());
        sqlx::query("UPDATE social_media_platform_auth SET platform_user_id = 'account-c'")
            .execute(&db.pool)
            .await
            .unwrap();
        assert_eq!(
            music_consent_put_handler(
                partner.clone(),
                State(db.pool.clone()),
                Path("1".into()),
                Json(MusicConsentRequest {
                    platform_user_id: "account-c".into()
                })
            )
            .await
            .status(),
            StatusCode::OK
        );
        assert!(valid_music_consent(&db.pool, "42", Some("account-b"))
            .await
            .unwrap());
        let response = get_handler(partner, State(db.pool.clone()), Path("1".into())).await;
        let body: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["music_consent"], true);
        assert_eq!(body["music_consent_platform_user_id"], "account-c");
        db.close().await;
    }

    #[test]
    fn combined_description_limit_counts_separator_and_unicode() {
        let mut draft: TikTokDraft = serde_json::from_value(json!({"caption": "ä".repeat(2197), "standard_description": "ß", "privacy_level": "", "allow_comment": false, "allow_duet": false, "allow_stitch": false})).unwrap();
        assert!(draft.validate().is_ok());
        draft.standard_description.push('ß');
        assert!(draft.validate().is_err());
        assert_eq!(combined_caption(" Clip ", " #tag "), "Clip\n\n#tag");
    }

    #[test]
    fn music_consent_requires_a_complete_matching_account_record() {
        let draft: TikTokDraft = serde_json::from_value(json!({
            "caption": "", "privacy_level": "", "allow_comment": false,
            "allow_duet": false, "allow_stitch": false,
            "music_consent": true, "music_consent_at": "2026-01-01T00:00:00Z",
            "music_consent_platform_user_id": "account-a"
        }))
        .unwrap();
        assert!(draft.clone().for_account(Some("account-a")).music_consent);
        for account in [None, Some(""), Some("account-b")] {
            assert_eq!(
                draft.clone().for_account(account),
                draft.clone().without_music_consent()
            );
        }
        let mut incomplete = draft.clone();
        incomplete.music_consent_at = None;
        assert!(!incomplete.for_account(Some("account-a")).music_consent);
        let mut incomplete = draft;
        incomplete.music_consent_platform_user_id = None;
        assert!(!incomplete.for_account(Some("account-a")).music_consent);
    }

    #[test]
    fn defaults_do_not_store_commercial_choices() {
        let draft: TikTokDraft = serde_json::from_value(json!({"caption": "", "privacy_level": "SELF_ONLY", "allow_comment": true, "allow_duet": false, "allow_stitch": false, "commercial_content": true, "brand_content_toggle": true, "brand_organic_toggle": true})).unwrap();
        let defaults = draft.without_disclosure();
        assert!(!defaults.commercial_content);
        assert!(!defaults.brand_content_toggle);
        assert!(!defaults.brand_organic_toggle);
    }
}
