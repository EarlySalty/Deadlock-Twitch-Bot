use super::*;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TikTokDraft {
    caption: String,
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
        if self.caption.chars().count() > 2200
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
    let defaults = match defaults {
        Some(defaults) if defaults.music_consent => {
            let account = match tiktok_account(&pool, &user).await {
                Ok(account) => account,
                Err(response) => return response,
            };
            Some(defaults.for_account(account.as_deref()))
        }
        Some(defaults) => Some(defaults.without_music_consent()),
        None => None,
    };
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({"draft": draft, "defaults": defaults})),
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
    let mut draft = draft.without_disclosure();
    if draft.music_consent {
        let account = match tiktok_account(&pool, &user).await {
            Ok(account) => account,
            Err(response) => return response,
        };
        if account.as_deref().is_none_or(str::is_empty)
            || draft.music_consent_platform_user_id.as_deref() != account.as_deref()
        {
            return (
                StatusCode::CONFLICT,
                Json(json!({"error": "tiktok_posting", "message": "Das TikTok-Konto hat sich geändert. Bitte öffne die Freigabe erneut."})),
            )
                .into_response();
        }
        draft.music_consent_at = Some(chrono::Utc::now());
        draft.music_consent_platform_user_id = account;
    } else {
        draft = draft.without_music_consent();
    }
    save(&pool, defaults_key(&user), &user, draft).await
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
            Some(draft.clone().without_disclosure())
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
    async fn music_consent_is_server_dated_account_bound_and_revocable() {
        let db = crate::test_database::Database::new().await;
        for statement in [
            "CREATE TABLE social_media_settings (key TEXT PRIMARY KEY, value JSONB, updated_at TIMESTAMPTZ, updated_by TEXT)",
            "CREATE TABLE twitch_clips_social_media (id BIGINT PRIMARY KEY, twitch_user_id TEXT, tiktok_post_options JSONB)",
            "CREATE TABLE social_media_partner_access (twitch_user_id TEXT PRIMARY KEY, granted BOOLEAN)",
            "CREATE TABLE social_media_platform_auth (id INTEGER PRIMARY KEY, platform TEXT, twitch_user_id TEXT, platform_user_id TEXT, enabled INTEGER, authorized_at TIMESTAMPTZ)",
            "INSERT INTO twitch_clips_social_media VALUES (1, '42', NULL)",
            "INSERT INTO social_media_partner_access VALUES ('42', TRUE)",
            "INSERT INTO social_media_platform_auth VALUES (1, 'tiktok', '42', 'account-a', 1, '2026-01-01'), (2, 'tiktok', '42', 'disabled-account', 0, '2026-02-01'), (3, 'tiktok', NULL, 'global-account', 1, '2026-03-01'), (4, 'tiktok', '43', 'other-account', 1, '2026-04-01')",
        ] {
            sqlx::query(statement).execute(&db.pool).await.unwrap();
        }
        let partner = DashboardAuthLevel::Partner {
            twitch_login: "test".into(),
            twitch_user_id: "42".into(),
            display_name: "Test".into(),
        };
        let draft: TikTokDraft = serde_json::from_value(json!({
            "caption": "Beschreibung", "privacy_level": "SELF_ONLY",
            "allow_comment": true, "allow_duet": false, "allow_stitch": false,
            "commercial_content": true, "brand_content_toggle": true,
            "music_consent": true, "music_consent_at": "2000-01-01T00:00:00Z",
            "music_consent_platform_user_id": "account-a"
        }))
        .unwrap();
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
        let stored_draft = load(&db.pool, &draft_key(1, "42")).await.unwrap().unwrap();
        assert_eq!(stored_draft, draft.clone().without_music_consent());
        let raw_draft = serde_json::to_value(&stored_draft).unwrap();
        assert!(raw_draft.get("music_consent").is_none());
        assert!(raw_draft.get("music_consent_at").is_none());
        assert!(raw_draft.get("music_consent_platform_user_id").is_none());
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
        let stored = load(&db.pool, &defaults_key("42")).await.unwrap().unwrap();
        assert!(stored.music_consent);
        assert!(stored.music_consent_at.is_some());
        assert_ne!(stored.music_consent_at, draft.music_consent_at);
        assert_eq!(
            stored.music_consent_platform_user_id.as_deref(),
            Some("account-a")
        );
        assert!(!stored.commercial_content);
        assert!(!stored.brand_content_toggle);
        let response = get_handler(partner.clone(), State(db.pool.clone()), Path("1".into())).await;
        let body: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["defaults"]["music_consent"], true);
        let mut wrong_account = draft.clone();
        wrong_account.music_consent_platform_user_id = Some("other-account".into());
        assert_eq!(
            defaults_put_handler(
                partner.clone(),
                State(db.pool.clone()),
                Path("1".into()),
                Json(wrong_account)
            )
            .await
            .status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            load(&db.pool, &defaults_key("42")).await.unwrap(),
            Some(stored.clone())
        );
        sqlx::query(
            "UPDATE social_media_platform_auth SET platform_user_id = 'account-b' WHERE id = 1",
        )
        .execute(&db.pool)
        .await
        .unwrap();
        let response = get_handler(partner.clone(), State(db.pool.clone()), Path("1".into())).await;
        let body: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(body["defaults"].get("music_consent").is_none());
        assert_eq!(
            defaults_put_handler(
                partner.clone(),
                State(db.pool.clone()),
                Path("1".into()),
                Json(draft.clone())
            )
            .await
            .status(),
            StatusCode::CONFLICT
        );
        let revoked = draft.without_music_consent();
        assert_eq!(
            defaults_put_handler(
                partner,
                State(db.pool.clone()),
                Path("1".into()),
                Json(revoked.clone())
            )
            .await
            .status(),
            StatusCode::OK
        );
        let stored = load(&db.pool, &defaults_key("42")).await.unwrap().unwrap();
        assert_eq!(stored, revoked.without_disclosure());
        assert!(serde_json::to_value(stored)
            .unwrap()
            .get("music_consent_at")
            .is_none());
        let published: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_clips_social_media WHERE tiktok_post_options IS NOT NULL",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(published, 0);
        db.close().await;
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
