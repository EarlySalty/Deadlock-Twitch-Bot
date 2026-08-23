//! Dashboard-seitige Clip-Funktionen (Port des Rests von
//! `bot/social_media/clip_manager.py`).
//!
//! `register_manual_upload` legt einen manuell hochgeladenen Clip an (Quelle
//! `manual_upload`) und belegt das Default-Layout; `get_clips_for_dashboard`
//! liefert die Clip-Liste samt Anzahl offener Uploads für die Dashboard-Ansicht.
//! Die übrigen clip_manager-Teile (register_clip/fetch_recent_clips/Queue/
//! Templates/Analytics) sind bereits in eigenen Modulen.

use serde_json::{json, Value};
use sqlx::PgPool;

use crate::clip_queue::queue_upload;
use crate::layout::apply_default_layout;
use crate::retention::refresh_clip_publication_status;

#[derive(Debug, thiserror::Error)]
pub enum ManualUploadError {
    #[error("clip_id already exists")]
    AlreadyExists,
    #[error("unknown streamer")]
    UnknownStreamer,
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
}

/// Registriert einen manuell hochgeladenen Clip und belegt das Default-Layout.
/// Liefert (clip_db_id, retention_until als ISO-Text).
pub async fn register_manual_upload(
    pool: &PgPool,
    clip_id: &str,
    streamer_login: &str,
    title: Option<&str>,
    local_path: &str,
    duration_seconds: f64,
) -> Result<(i64, String), ManualUploadError> {
    let created_at = chrono::Utc::now().to_rfc3339();

    if sqlx::query_scalar!(
        "SELECT id AS \"id!\" FROM twitch_clips_social_media WHERE clip_id = $1",
        clip_id
    )
    .fetch_optional(pool)
    .await?
    .is_some()
    {
        return Err(ManualUploadError::AlreadyExists);
    }

    let streamer = sqlx::query!(
        "SELECT twitch_user_id FROM twitch_streamers WHERE LOWER(twitch_login) = LOWER($1) LIMIT 1",
        streamer_login
    )
    .fetch_optional(pool)
    .await?;
    let Some(row) = streamer else {
        return Err(ManualUploadError::UnknownStreamer);
    };
    let twitch_user_id = row.twitch_user_id;

    let row = sqlx::query!(
        "INSERT INTO twitch_clips_social_media \
            (clip_id, clip_url, clip_title, clip_thumbnail_url, streamer_login, twitch_user_id, \
             created_at, duration_seconds, view_count, game_name, status, source_kind, \
             upload_local_path, local_file_path) \
         VALUES ($1, $2, $3, NULL, $4, $5, $6::text::timestamptz, $7, 0, NULL, 'pending', 'manual_upload', $2, $2) \
         RETURNING id AS \"id!\", retention_until::text AS retention_until",
        clip_id,
        local_path,
        title,
        streamer_login,
        twitch_user_id.as_deref(),
        &created_at,
        duration_seconds
    )
    .fetch_one(pool)
    .await?;
    let clip_db_id = row.id;
    let retention_until = row.retention_until;

    if let Err(error) = apply_default_layout(pool, clip_db_id, streamer_login).await {
        tracing::warn!(
            %error,
            clip_db_id,
            streamer_login,
            "Social-Media-Clip: Default-Layout konnte nicht angewendet werden"
        );
    }
    Ok((clip_db_id, retention_until.unwrap_or_default()))
}

/// Clips für die Dashboard-Anzeige (volle Zeile + Anzahl offener Uploads),
/// optional nach Streamer/Status gefiltert.
pub async fn get_clips_for_dashboard(
    pool: &PgPool,
    streamer_login: Option<&str>,
    status: Option<&str>,
    limit: i64,
) -> Vec<Value> {
    let rows = match sqlx::query!(
        "SELECT to_jsonb(c)::text AS clip, \
                COALESCE((SELECT COUNT(*) FROM twitch_clips_upload_queue q \
                          WHERE q.clip_id = c.id AND q.status = 'pending'), 0) AS \"pending_uploads!\" \
           FROM twitch_clips_social_media c \
          WHERE ($1::text IS NULL OR LOWER(c.streamer_login) = LOWER($1)) \
            AND ($2::text IS NULL OR c.status = $2) \
          ORDER BY c.created_at DESC LIMIT $3",
        streamer_login,
        status,
        limit
    )
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows,
        Err(error) => {
            tracing::warn!(
                %error,
                streamer_login = streamer_login.unwrap_or(""),
                status = status.unwrap_or(""),
                "Social-Media-Clips: Dashboard-Liste nicht ladbar"
            );
            Vec::new()
        }
    };

    rows.iter()
        .map(|r| {
            let clip_text = r.clip.clone().unwrap_or_else(|| "{}".to_string());
            let pending = r.pending_uploads;
            let mut obj: Value = serde_json::from_str(&clip_text).unwrap_or_else(|_| json!({}));
            if let Some(map) = obj.as_object_mut() {
                map.insert("pending_uploads".to_string(), json!(pending));
            }
            obj
        })
        .collect()
}

/// Markiert einen Clip auf den gegebenen Plattformen als hochgeladen (manuell);
/// unbekannte Plattformen werden übersprungen. Liefert `false` bei DB-Fehler.
/// Frischt anschließend den Publication-Status auf.
pub async fn mark_clip_uploaded(
    pool: &PgPool,
    clip_db_id: impl Into<i64>,
    platforms: &[String],
    manual: bool,
) -> bool {
    let clip_db_id = clip_db_id.into();
    let now = chrono::Utc::now().to_rfc3339();
    let updates = async {
        let mut tx = pool.begin().await?;
        for platform in platforms {
            let sql = match platform.as_str() {
                "tiktok" => "UPDATE twitch_clips_social_media SET uploaded_tiktok = TRUE, tiktok_uploaded_at = $1::text::timestamptz WHERE id = $2",
                "youtube" => "UPDATE twitch_clips_social_media SET uploaded_youtube = TRUE, youtube_uploaded_at = $1::text::timestamptz WHERE id = $2",
                "instagram" => "UPDATE twitch_clips_social_media SET uploaded_instagram = TRUE, instagram_uploaded_at = $1::text::timestamptz WHERE id = $2",
                _ => continue,
            };
            sqlx::query(sql).bind(&now).bind(clip_db_id).execute(&mut *tx).await?;
        }
        tx.commit().await
    }
    .await;
    if updates.is_err() {
        return false;
    }
    tracing::info!(
        clip_db_id,
        ?platforms,
        manual,
        "Clip als hochgeladen markiert"
    );
    refresh_clip_publication_status(pool, clip_db_id).await;
    true
}

/// Stats eines Batch-Uploads (Python-Dict {queued, skipped, errors}).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchUploadStats {
    pub queued: i64,
    pub skipped: i64,
    pub errors: i64,
}

type PendingClipRow = (
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn parse_json_strings(raw: Option<&str>) -> Vec<String> {
    raw.and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
        .unwrap_or_default()
}

/// Reiht alle noch nicht hochgeladenen Clips eines Streamers je Plattform in die
/// Upload-Queue ein (Python `batch_upload_all_new`). Wendet — falls gewünscht und
/// die Clip-Beschreibung leer ist — das Default-Streamer-Template an
/// (Platzhalter {{title}}/{{streamer}}/{{game}}). `skipped` bleibt 0 (mirror).
pub async fn batch_upload_all_new(
    pool: &PgPool,
    streamer_login: &str,
    platforms: &[String],
    apply_default_template: bool,
) -> BatchUploadStats {
    let mut stats = BatchUploadStats {
        queued: 0,
        skipped: 0,
        errors: 0,
    };

    let default_template: Option<(String, Vec<String>)> = if apply_default_template {
        sqlx::query!(
            "SELECT description_template AS \"description_template!\", hashtags AS \"hashtags!\" \
             FROM clip_templates_streamer WHERE streamer_login = $1 AND is_default LIMIT 1",
            streamer_login
        )
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .map(|row| {
            (
                row.description_template,
                parse_json_strings(Some(&row.hashtags)),
            )
        })
    } else {
        None
    };

    for platform in platforms {
        let col = match platform.as_str() {
            "tiktok" => "uploaded_tiktok",
            "youtube" => "uploaded_youtube",
            "instagram" => "uploaded_instagram",
            _ => continue,
        };
        let clips: Vec<PendingClipRow> = sqlx::query_as(&format!(
            "SELECT id, clip_title, streamer_login, game_name, custom_description, hashtags \
             FROM twitch_clips_social_media WHERE streamer_login = $1 AND {col} = FALSE ORDER BY created_at DESC"
        ))
        .bind(streamer_login)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        for (id, clip_title, clip_streamer, game_name, custom_description, hashtags_str) in clips {
            let description_empty = custom_description
                .as_deref()
                .map(str::is_empty)
                .unwrap_or(true);
            let (description, hashtags): (Option<String>, Vec<String>) = match &default_template {
                Some((tmpl_desc, tmpl_tags)) if description_empty => {
                    let game_disp = game_name
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .unwrap_or("Unknown");
                    let desc = tmpl_desc
                        .replace("{{title}}", clip_title.as_deref().unwrap_or(""))
                        .replace("{{streamer}}", clip_streamer.as_deref().unwrap_or(""))
                        .replace("{{game}}", game_disp);
                    let game_no_space = game_disp.replace(' ', "");
                    let tags = tmpl_tags
                        .iter()
                        .map(|t| t.replace("{{game}}", &game_no_space))
                        .collect();
                    (Some(desc), tags)
                }
                _ => (
                    custom_description.clone(),
                    parse_json_strings(hashtags_str.as_deref()),
                ),
            };
            match queue_upload(
                pool,
                id,
                platform,
                clip_title.as_deref(),
                description.as_deref(),
                Some(&hashtags),
                None,
                0,
            )
            .await
            {
                Ok(_) => stats.queued += 1,
                Err(_) => stats.errors += 1,
            }
        }
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

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
            .max_connections(3)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE twitch_streamers (twitch_login TEXT PRIMARY KEY, twitch_user_id TEXT)",
            "CREATE TABLE twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, clip_id TEXT NOT NULL UNIQUE, clip_url TEXT NOT NULL, clip_title TEXT, clip_thumbnail_url TEXT, streamer_login TEXT NOT NULL, twitch_user_id TEXT, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), duration_seconds DOUBLE PRECISION, view_count INTEGER DEFAULT 0, game_name TEXT, custom_description TEXT, hashtags TEXT, status TEXT DEFAULT 'pending', source_kind TEXT NOT NULL DEFAULT 'twitch', upload_local_path TEXT, local_file_path TEXT, retention_until TIMESTAMPTZ DEFAULT (NOW() + INTERVAL '14 days'), discarded_at TIMESTAMPTZ, layout_override_json JSONB, uploaded_tiktok BOOLEAN DEFAULT FALSE, uploaded_youtube BOOLEAN DEFAULT FALSE, uploaded_instagram BOOLEAN DEFAULT FALSE, tiktok_uploaded_at TIMESTAMPTZ, youtube_uploaded_at TIMESTAMPTZ, instagram_uploaded_at TIMESTAMPTZ)",
            "CREATE TABLE social_media_streamer_layout (streamer_login TEXT PRIMARY KEY, layout_json JSONB NOT NULL, cam_enabled BOOLEAN NOT NULL DEFAULT TRUE, mode TEXT NOT NULL DEFAULT 'pip', updated_at TIMESTAMPTZ DEFAULT NOW(), updated_by TEXT)",
            "CREATE TABLE twitch_clips_upload_queue (id BIGSERIAL PRIMARY KEY, clip_id BIGINT NOT NULL, platform TEXT NOT NULL, status TEXT DEFAULT 'pending', priority INTEGER DEFAULT 0, title TEXT, description TEXT, hashtags TEXT, scheduled_at TIMESTAMPTZ, attempts INTEGER DEFAULT 0, quota_deferrals INTEGER NOT NULL DEFAULT 0, last_error TEXT, last_attempt_at TIMESTAMPTZ, created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP, completed_at TIMESTAMPTZ)",
            "CREATE TABLE social_media_platform_auth (id SERIAL PRIMARY KEY, platform TEXT, streamer_login TEXT, enabled INTEGER DEFAULT 1)",
            "CREATE TABLE clip_templates_streamer (id BIGSERIAL PRIMARY KEY, streamer_login TEXT NOT NULL, template_name TEXT NOT NULL, description_template TEXT NOT NULL, hashtags TEXT NOT NULL, is_default BOOLEAN DEFAULT FALSE, created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP, updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn manual_upload_und_dashboard_liste() {
        let Some(pool) = make_pool("t_sm_clip_manager").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('nani', '123')",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Registrierung.
        let (id, retention) = register_manual_upload(
            &pool,
            "m1",
            "nani",
            Some("Mein Upload"),
            "/data/v.mp4",
            30.0,
        )
        .await
        .unwrap();
        assert!(id > 0);
        assert!(!retention.is_empty());
        let (kind, path, status, layout): (String, String, String, Option<String>) = sqlx::query_as("SELECT source_kind, upload_local_path, status, layout_override_json::text FROM twitch_clips_social_media WHERE id = $1").bind(id).fetch_one(&pool).await.unwrap();
        assert_eq!(kind, "manual_upload");
        assert_eq!(path, "/data/v.mp4");
        assert_eq!(status, "pending");
        assert!(layout.is_some()); // apply_default_layout lief

        // Duplikat + unbekannter Streamer.
        assert!(matches!(
            register_manual_upload(&pool, "m1", "nani", None, "/x.mp4", 1.0).await,
            Err(ManualUploadError::AlreadyExists)
        ));
        assert!(matches!(
            register_manual_upload(&pool, "m2", "ghost", None, "/x.mp4", 1.0).await,
            Err(ManualUploadError::UnknownStreamer)
        ));

        // Dashboard-Liste.
        let clips = get_clips_for_dashboard(&pool, Some("nani"), None, 50).await;
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0]["clip_id"], "m1");
        assert_eq!(clips[0]["pending_uploads"], 0);
        // Status-Filter greift.
        assert_eq!(
            get_clips_for_dashboard(&pool, None, Some("processing"), 50)
                .await
                .len(),
            0
        );
        // Pending-Upload erhöht den Zähler.
        sqlx::query("INSERT INTO twitch_clips_upload_queue (clip_id, platform, status) VALUES ($1, 'tiktok', 'pending')").bind(id).execute(&pool).await.unwrap();
        assert_eq!(
            get_clips_for_dashboard(&pool, Some("nani"), None, 50).await[0]["pending_uploads"],
            1
        );
    }

    #[tokio::test]
    async fn mark_uploaded_setzt_flags_und_refresh() {
        let Some(pool) = make_pool("t_sm_mark_uploaded").await else {
            return;
        };
        // tiktok ist die einzige aktive Plattform für nani.
        sqlx::query("INSERT INTO social_media_platform_auth (platform, streamer_login) VALUES ('tiktok', 'nani')").execute(&pool).await.unwrap();
        let clip: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login) VALUES ('m1', 'https://clips.test/m1', 'nani') RETURNING id").fetch_one(&pool).await.unwrap();

        // tiktok markieren (+ unbekannte Plattform wird übersprungen).
        assert!(mark_clip_uploaded(&pool, clip, &["tiktok".into(), "snapchat".into()], true).await);
        let (up, at, status): (bool, Option<String>, String) = sqlx::query_as("SELECT uploaded_tiktok, tiktok_uploaded_at::text, status FROM twitch_clips_social_media WHERE id = $1").bind(clip).fetch_one(&pool).await.unwrap();
        assert!(up);
        assert!(at.is_some());
        // tiktok einzige aktive Plattform + jetzt hochgeladen → published_all.
        assert_eq!(status, "published_all");
    }

    #[tokio::test]
    async fn batch_upload_template_und_custom() {
        let Some(pool) = make_pool("t_sm_batch").await else {
            return;
        };
        // Default-Template für nani.
        sqlx::query("INSERT INTO clip_templates_streamer (streamer_login, template_name, description_template, hashtags, is_default) VALUES ('nani', 'def', 'Clip: {{title}} ({{game}})', '[\"#{{game}}\", \"#deadlock\"]', TRUE)").execute(&pool).await.unwrap();
        // Clip A: leere Beschreibung → Template greift.
        let a: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, clip_title, game_name, created_at) VALUES ('a', 'https://clips.test/a', 'nani', 'Toller Clip', 'Dead Lock', '2026-06-10') RETURNING id").fetch_one(&pool).await.unwrap();
        // Clip B: eigene Beschreibung + hashtags → Template NICHT greifen.
        let b: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, clip_title, custom_description, hashtags, created_at) VALUES ('b', 'https://clips.test/b', 'nani', 'B', 'Eigene Beschr', '[\"#own\"]', '2026-06-09') RETURNING id").fetch_one(&pool).await.unwrap();
        // Clip C: schon hochgeladen → nicht eingereiht.
        sqlx::query("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, uploaded_tiktok, created_at) VALUES ('c', 'https://clips.test/c', 'nani', TRUE, '2026-06-08')").execute(&pool).await.unwrap();

        let stats = batch_upload_all_new(&pool, "nani", &["tiktok".into()], true).await;
        assert_eq!(stats.queued, 2);
        assert_eq!(stats.errors, 0);
        assert_eq!(stats.skipped, 0);

        // A: Template substituiert ({{game}} → "Dead Lock" in Desc, "DeadLock" in Hashtag).
        let (desc_a, tags_a): (Option<String>, Option<String>) = sqlx::query_as(
            "SELECT description, hashtags FROM twitch_clips_upload_queue WHERE clip_id = $1",
        )
        .bind(a)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(desc_a.as_deref(), Some("Clip: Toller Clip (Dead Lock)"));
        assert_eq!(tags_a.as_deref(), Some("[\"#DeadLock\",\"#deadlock\"]"));
        // B: eigene Beschreibung + hashtags.
        let (desc_b, tags_b): (Option<String>, Option<String>) = sqlx::query_as(
            "SELECT description, hashtags FROM twitch_clips_upload_queue WHERE clip_id = $1",
        )
        .bind(b)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(desc_b.as_deref(), Some("Eigene Beschr"));
        assert_eq!(tags_b.as_deref(), Some("[\"#own\"]"));
    }
}
