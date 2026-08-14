//! Key/Value-Settings für das Social-Media-Modul (Port von
//! `bot/social_media/settings.py`).
//!
//! Persistiert in `social_media_settings (key TEXT PK, value JSONB, …)`. Zentraler
//! Schalter ist `external_llm_consent`: ohne explizites `true` werden Daten nie an
//! einen externen LLM-Provider geschickt. JSONB wird wie im Rest der Codebase über
//! `value::text` gelesen und `$N::jsonb` geschrieben (kein sqlx-`json`-Feature).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;

pub const KEY_EXTERNAL_LLM_CONSENT: &str = "external_llm_consent";
pub const KEY_AUTO_APPROVE_YOUTUBE: &str = "auto_approve_youtube";
pub const KEY_AUTO_APPROVE_TIKTOK: &str = "auto_approve_tiktok";
pub const KEY_AUTO_APPROVE_INSTAGRAM: &str = "auto_approve_instagram";
pub const KEY_POSTING_SCHEDULE: &str = "posting_schedule";
pub const KEY_VOD_ARCHIVE_ENABLED: &str = "vod_archive_enabled";
pub const KEY_VOD_ARCHIVE_PRIVACY: &str = "vod_archive_privacy";
pub const KEY_FORMS_CONTACT_EMAIL: &str = "forms_contact_email";
pub const KEY_FORMS_SUBMIT_ENABLED: &str = "forms_submit_enabled";
pub const DEFAULT_FORMS_CONTACT_EMAIL: &str = "deadlockclips.dl@mailinator.com";

/// Auto-Approve-Flags je Plattform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoApprove {
    pub youtube: bool,
    pub tiktok: bool,
    pub instagram: bool,
}

/// Tägliche Posting-Slots in der angegebenen Zeitzone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostingSchedule {
    pub times: Vec<String>,
    pub timezone: String,
}

impl Default for PostingSchedule {
    fn default() -> Self {
        Self {
            times: vec![
                "14:00".to_string(),
                "18:00".to_string(),
                "21:00".to_string(),
            ],
            timezone: "Europe/Berlin".to_string(),
        }
    }
}

/// Liest einen Setting-Wert (JSONB → [`Value`]). `None` bei fehlendem Key, NULL
/// oder DB-/Parse-Fehler (Python `get_setting`-Fehlertoleranz).
pub async fn get_setting(pool: &PgPool, key: &str) -> Option<Value> {
    if key.is_empty() {
        return None;
    }
    let text = sqlx::query_scalar!(
        "SELECT value::text AS \"value?\" FROM social_media_settings WHERE key = $1",
        key
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .flatten()?;
    serde_json::from_str(&text).ok()
}

/// Upsert eines Setting-Werts (Python `set_setting`).
pub async fn set_setting(
    pool: &PgPool,
    key: &str,
    value: &Value,
    updated_by: Option<&str>,
) -> Result<(), sqlx::Error> {
    let payload = value.to_string();
    let updated_by = updated_by.map(str::trim).filter(|s| !s.is_empty());
    sqlx::query!(
        "INSERT INTO social_media_settings (key, value, updated_at, updated_by) \
         VALUES ($1, $2::text::jsonb, CURRENT_TIMESTAMP, $3) \
         ON CONFLICT (key) DO UPDATE SET \
             value = EXCLUDED.value, \
             updated_at = CURRENT_TIMESTAMP, \
             updated_by = EXCLUDED.updated_by",
        key,
        payload,
        updated_by
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Flexible Bool-Coercion (Python `_coerce_bool`): bool direkt, String
/// `true/1/yes/on`, Zahl ≠ 0; sonst `false`.
pub fn coerce_bool(value: &Value) -> bool {
    match value {
        Value::Bool(b) => *b,
        Value::String(s) => matches!(
            s.trim().to_lowercase().as_str(),
            "true" | "1" | "yes" | "on"
        ),
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        _ => false,
    }
}

async fn get_bool(pool: &PgPool, key: &str) -> bool {
    get_setting(pool, key)
        .await
        .as_ref()
        .map(coerce_bool)
        .unwrap_or(false)
}

/// `true`, wenn der Admin explizit `external_llm_consent=true` gesetzt hat.
pub async fn external_llm_consent(pool: &PgPool) -> bool {
    get_bool(pool, KEY_EXTERNAL_LLM_CONSENT).await
}

/// Auto-Approve-Flags aller Plattformen (Python `get_auto_approve_settings`).
pub async fn get_auto_approve_settings(pool: &PgPool) -> AutoApprove {
    AutoApprove {
        youtube: get_bool(pool, KEY_AUTO_APPROVE_YOUTUBE).await,
        tiktok: get_bool(pool, KEY_AUTO_APPROVE_TIKTOK).await,
        instagram: get_bool(pool, KEY_AUTO_APPROVE_INSTAGRAM).await,
    }
}

/// Setzt die Auto-Approve-Flags (Python `set_auto_approve_settings`).
pub async fn set_auto_approve_settings(
    pool: &PgPool,
    values: AutoApprove,
    updated_by: Option<&str>,
) -> Result<AutoApprove, sqlx::Error> {
    set_setting(
        pool,
        KEY_AUTO_APPROVE_YOUTUBE,
        &Value::Bool(values.youtube),
        updated_by,
    )
    .await?;
    set_setting(
        pool,
        KEY_AUTO_APPROVE_TIKTOK,
        &Value::Bool(values.tiktok),
        updated_by,
    )
    .await?;
    set_setting(
        pool,
        KEY_AUTO_APPROVE_INSTAGRAM,
        &Value::Bool(values.instagram),
        updated_by,
    )
    .await?;
    Ok(values)
}

/// Liest die tägliche Posting-Kadenz; fehlende oder ungültige Werte nutzen den Default.
pub async fn get_posting_schedule(pool: &PgPool) -> PostingSchedule {
    get_setting(pool, KEY_POSTING_SCHEDULE)
        .await
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

/// Setzt die tägliche Posting-Kadenz.
pub async fn set_posting_schedule(
    pool: &PgPool,
    values: PostingSchedule,
    updated_by: Option<&str>,
) -> Result<PostingSchedule, sqlx::Error> {
    set_setting(
        pool,
        KEY_POSTING_SCHEDULE,
        &serde_json::json!(&values),
        updated_by,
    )
    .await?;
    Ok(values)
}

/// Gueltige Sichtbarkeiten fuer den VOD-Upload. Solange das Google-Projekt
/// nicht auditiert ist, erzwingt YouTube ohnehin `private` und setzt alles
/// andere still zurueck; die Wahl bleibt trotzdem hier, damit sie nach dem
/// Audit ohne Codeaenderung greift.
pub const VOD_ARCHIVE_PRIVACY_VALUES: [&str; 3] = ["private", "unlisted", "public"];
pub const DEFAULT_VOD_ARCHIVE_PRIVACY: &str = "private";

/// Einstellung des VOD-Archivs: laeuft es, und wie sichtbar sind die Uploads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VodArchiveSettings {
    pub enabled: bool,
    pub privacy: String,
}

impl Default for VodArchiveSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            privacy: DEFAULT_VOD_ARCHIVE_PRIVACY.to_string(),
        }
    }
}

/// Liest die VOD-Archiv-Einstellung. Ein unbekannter Sichtbarkeitswert faellt
/// auf `private` zurueck statt den Upload zu verweigern.
pub async fn get_vod_archive_settings(pool: &PgPool) -> VodArchiveSettings {
    let privacy = get_setting(pool, KEY_VOD_ARCHIVE_PRIVACY)
        .await
        .and_then(|value| value.as_str().map(str::to_owned))
        .filter(|value| VOD_ARCHIVE_PRIVACY_VALUES.contains(&value.as_str()))
        .unwrap_or_else(|| DEFAULT_VOD_ARCHIVE_PRIVACY.to_string());
    VodArchiveSettings {
        enabled: get_bool(pool, KEY_VOD_ARCHIVE_ENABLED).await,
        privacy,
    }
}

/// Setzt die VOD-Archiv-Einstellung. Ungueltige Sichtbarkeit wird abgewiesen,
/// damit kein Tippfehler stillschweigend ein VOD oeffentlich stellt.
pub async fn set_vod_archive_settings(
    pool: &PgPool,
    values: VodArchiveSettings,
    updated_by: Option<&str>,
) -> Result<VodArchiveSettings, sqlx::Error> {
    if !VOD_ARCHIVE_PRIVACY_VALUES.contains(&values.privacy.as_str()) {
        return Err(sqlx::Error::Protocol(format!(
            "unbekannte Sichtbarkeit: {}",
            values.privacy
        )));
    }
    set_setting(
        pool,
        KEY_VOD_ARCHIVE_ENABLED,
        &Value::Bool(values.enabled),
        updated_by,
    )
    .await?;
    set_setting(
        pool,
        KEY_VOD_ARCHIVE_PRIVACY,
        &Value::String(values.privacy.clone()),
        updated_by,
    )
    .await?;
    Ok(values)
}

/// Kontaktadresse für externe Clip-Formulare.
pub async fn get_forms_contact_email(pool: &PgPool) -> String {
    get_setting(pool, KEY_FORMS_CONTACT_EMAIL)
        .await
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| DEFAULT_FORMS_CONTACT_EMAIL.to_string())
}

/// Setzt die Kontaktadresse für externe Clip-Formulare.
pub async fn set_forms_contact_email(
    pool: &PgPool,
    value: &str,
    updated_by: Option<&str>,
) -> Result<String, sqlx::Error> {
    set_setting(
        pool,
        KEY_FORMS_CONTACT_EMAIL,
        &Value::String(value.to_string()),
        updated_by,
    )
    .await?;
    Ok(value.to_string())
}

/// Externe Formular-Submits sind bis zur expliziten Aktivierung aus.
pub async fn forms_submit_enabled(pool: &PgPool) -> bool {
    get_bool(pool, KEY_FORMS_SUBMIT_ENABLED).await
}

/// Aktiviert oder deaktiviert externe Formular-Submits.
pub async fn set_forms_submit_enabled(
    pool: &PgPool,
    value: bool,
    updated_by: Option<&str>,
) -> Result<bool, sqlx::Error> {
    set_setting(
        pool,
        KEY_FORMS_SUBMIT_ENABLED,
        &Value::Bool(value),
        updated_by,
    )
    .await?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    #[test]
    fn coerce_bool_faelle() {
        assert!(coerce_bool(&json!(true)));
        assert!(!coerce_bool(&json!(false)));
        assert!(coerce_bool(&json!("true")));
        assert!(coerce_bool(&json!("On")));
        assert!(coerce_bool(&json!("1")));
        assert!(coerce_bool(&json!("yes")));
        assert!(!coerce_bool(&json!("nope")));
        assert!(coerce_bool(&json!(1)));
        assert!(!coerce_bool(&json!(0)));
        assert!(!coerce_bool(&json!(null)));
    }

    #[test]
    fn posting_schedule_default() {
        assert_eq!(
            PostingSchedule::default(),
            PostingSchedule {
                times: vec![
                    "14:00".to_string(),
                    "18:00".to_string(),
                    "21:00".to_string()
                ],
                timezone: "Europe/Berlin".to_string(),
            }
        );
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE social_media_settings (key TEXT PRIMARY KEY, value JSONB, \
             updated_at TIMESTAMPTZ, updated_by TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn get_set_roundtrip_und_consent() {
        let Some(pool) = make_pool("t_sm_settings").await else {
            return;
        };
        // Fehlend → None / Default false.
        assert!(get_setting(&pool, "fehlt").await.is_none());
        assert!(!external_llm_consent(&pool).await);

        // Consent setzen (als JSON-bool) → external_llm_consent true.
        set_setting(&pool, KEY_EXTERNAL_LLM_CONSENT, &json!(true), Some("admin"))
            .await
            .unwrap();
        assert!(external_llm_consent(&pool).await);
        // Roundtrip-Wert ist echtes JSON-bool.
        assert_eq!(
            get_setting(&pool, KEY_EXTERNAL_LLM_CONSENT).await,
            Some(json!(true))
        );

        // Upsert überschreibt + speichert komplexe JSON-Werte.
        set_setting(&pool, "obj", &json!({"a": 1, "b": [2, 3]}), None)
            .await
            .unwrap();
        assert_eq!(
            get_setting(&pool, "obj").await,
            Some(json!({"a": 1, "b": [2, 3]}))
        );
        set_setting(&pool, KEY_EXTERNAL_LLM_CONSENT, &json!(false), None)
            .await
            .unwrap();
        assert!(!external_llm_consent(&pool).await);
    }

    #[tokio::test]
    async fn auto_approve_roundtrip() {
        let Some(pool) = make_pool("t_sm_autoapprove").await else {
            return;
        };
        // Default alle false.
        assert_eq!(
            get_auto_approve_settings(&pool).await,
            AutoApprove {
                youtube: false,
                tiktok: false,
                instagram: false
            }
        );
        let set = AutoApprove {
            youtube: true,
            tiktok: false,
            instagram: true,
        };
        set_auto_approve_settings(&pool, set, Some("admin"))
            .await
            .unwrap();
        assert_eq!(get_auto_approve_settings(&pool).await, set);
    }

    #[tokio::test]
    async fn posting_schedule_roundtrip() {
        let Some(pool) = make_pool("t_sm_posting_schedule").await else {
            return;
        };
        assert_eq!(
            get_posting_schedule(&pool).await,
            PostingSchedule::default()
        );
        let schedule = PostingSchedule {
            times: vec!["09:30".to_string(), "16:45".to_string()],
            timezone: "UTC".to_string(),
        };
        set_posting_schedule(&pool, schedule.clone(), Some("admin"))
            .await
            .unwrap();
        assert_eq!(get_posting_schedule(&pool).await, schedule);
    }

    #[tokio::test]
    async fn forms_settings_have_safe_defaults_and_roundtrip() {
        let Some(pool) = make_pool("t_sm_forms_settings").await else {
            return;
        };

        assert_eq!(
            get_forms_contact_email(&pool).await,
            "deadlockclips.dl@mailinator.com"
        );
        assert!(!forms_submit_enabled(&pool).await);

        set_forms_contact_email(&pool, "forms@example.invalid", Some("admin"))
            .await
            .unwrap();
        set_forms_submit_enabled(&pool, true, Some("admin"))
            .await
            .unwrap();

        assert_eq!(
            get_forms_contact_email(&pool).await,
            "forms@example.invalid"
        );
        assert!(forms_submit_enabled(&pool).await);
    }
}
