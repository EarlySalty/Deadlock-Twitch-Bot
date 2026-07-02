//! Clip-Beschreibungs-Templates + zuletzt genutzte Hashtags (Port der
//! Template-Methoden aus `clip_manager.py`).
//!
//! Globale (Admin) + Streamer-Templates mit Platzhaltern
//! (`{{title}}`/`{{streamer}}`/`{{game}}`), angewendet auf einen Clip. Hashtags
//! liegen als JSON-String in TEXT-Spalten.

use sqlx::{PgPool, Row};

/// Globales Template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalTemplate {
    pub id: i64,
    pub template_name: String,
    pub description_template: String,
    pub hashtags: Vec<String>,
    pub category: Option<String>,
    pub usage_count: i32,
    pub created_at: Option<String>,
    pub created_by: Option<String>,
}

/// Streamer-Template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamerTemplate {
    pub id: i64,
    pub streamer_login: String,
    pub template_name: String,
    pub description_template: String,
    pub hashtags: Vec<String>,
    pub is_default: bool,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

fn parse_hashtags(raw: Option<&str>) -> Vec<String> {
    raw.and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
        .unwrap_or_default()
}

fn dump_hashtags(hashtags: &[String]) -> String {
    serde_json::to_string(hashtags).unwrap_or_else(|_| "[]".to_string())
}

/// Erstellt ein globales Template (Admin). Liefert die ID.
pub async fn create_global_template(
    pool: &PgPool,
    template_name: &str,
    description_template: &str,
    hashtags: &[String],
    category: Option<&str>,
    created_by: &str,
) -> Result<i64, sqlx::Error> {
    let id: i64 = sqlx::query_scalar!(
        "INSERT INTO clip_templates_global \
            (template_name, description_template, hashtags, category, created_by) \
         VALUES ($1, $2, $3, $4, $5) RETURNING id AS \"id!\"",
        template_name,
        description_template,
        dump_hashtags(hashtags),
        category,
        created_by
    )
    .fetch_one(pool)
    .await?;
    Ok(id)
}

/// Lädt globale Templates (optional kategorie-gefiltert).
pub async fn get_global_templates(pool: &PgPool, category: Option<&str>) -> Vec<GlobalTemplate> {
    let base = "SELECT id, template_name, description_template, hashtags, category, usage_count, \
                created_at::text, created_by FROM clip_templates_global";
    let rows = match category {
        Some(cat) => {
            sqlx::query(&format!(
                "{base} WHERE category = $1 ORDER BY usage_count DESC, template_name ASC"
            ))
            .bind(cat)
            .fetch_all(pool)
            .await
        }
        None => {
            sqlx::query(&format!(
                "{base} ORDER BY usage_count DESC, template_name ASC"
            ))
            .fetch_all(pool)
            .await
        }
    }
    .unwrap_or_default();
    rows.iter()
        .map(|r| GlobalTemplate {
            id: r.try_get("id").unwrap_or(0),
            template_name: r.try_get("template_name").unwrap_or_default(),
            description_template: r.try_get("description_template").unwrap_or_default(),
            hashtags: parse_hashtags(
                r.try_get::<Option<String>, _>("hashtags")
                    .unwrap_or(None)
                    .as_deref(),
            ),
            category: r.try_get("category").unwrap_or(None),
            usage_count: r.try_get("usage_count").unwrap_or(0),
            created_at: r.try_get("created_at").unwrap_or(None),
            created_by: r.try_get("created_by").unwrap_or(None),
        })
        .collect()
}

/// Erstellt oder aktualisiert ein Streamer-Template (Upsert nach
/// streamer_login+template_name). `is_default` setzt andere Defaults zurück.
pub async fn create_streamer_template(
    pool: &PgPool,
    streamer_login: &str,
    template_name: &str,
    description_template: &str,
    hashtags: &[String],
    is_default: bool,
) -> Result<i64, sqlx::Error> {
    let now = chrono::Utc::now().to_rfc3339();
    if is_default {
        sqlx::query!(
            "UPDATE clip_templates_streamer SET is_default = FALSE WHERE streamer_login = $1",
            streamer_login
        )
        .execute(pool)
        .await?;
    }
    let existing = sqlx::query_scalar!(
        "SELECT id AS \"id!\" FROM clip_templates_streamer WHERE streamer_login = $1 AND template_name = $2",
        streamer_login,
        template_name
    )
    .fetch_optional(pool)
    .await?;

    if let Some(id) = existing {
        sqlx::query!(
            "UPDATE clip_templates_streamer SET description_template = $1, hashtags = $2, \
             is_default = $3, updated_at = $4::text::timestamptz WHERE id = $5",
            description_template,
            dump_hashtags(hashtags),
            is_default,
            &now,
            id
        )
        .execute(pool)
        .await?;
        Ok(id)
    } else {
        let id: i64 = sqlx::query_scalar!(
            "INSERT INTO clip_templates_streamer \
                (streamer_login, template_name, description_template, hashtags, is_default) \
             VALUES ($1, $2, $3, $4, $5) RETURNING id AS \"id!\"",
            streamer_login,
            template_name,
            description_template,
            dump_hashtags(hashtags),
            is_default
        )
        .fetch_one(pool)
        .await?;
        Ok(id)
    }
}

/// Lädt alle Templates eines Streamers (Default zuerst).
pub async fn get_streamer_templates(pool: &PgPool, streamer_login: &str) -> Vec<StreamerTemplate> {
    let rows = sqlx::query!(
        "SELECT id, streamer_login, template_name, COALESCE(description_template, '') AS \"description_template!\", \
                hashtags, COALESCE(is_default, false) AS \"is_default!\", \
                created_at::text, updated_at::text FROM clip_templates_streamer \
         WHERE streamer_login = $1 ORDER BY is_default DESC, template_name ASC",
        streamer_login
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    rows.iter()
        .map(|r| StreamerTemplate {
            id: r.id,
            streamer_login: r.streamer_login.clone(),
            template_name: r.template_name.clone(),
            description_template: r.description_template.clone(),
            hashtags: parse_hashtags(Some(&r.hashtags)),
            is_default: r.is_default,
            created_at: r.created_at.clone(),
            updated_at: r.updated_at.clone(),
        })
        .collect()
}

/// Wendet ein Template auf einen Clip an (Platzhalter substituieren, Clip
/// updaten). Globale Templates erhöhen ihren `usage_count`.
pub async fn apply_template_to_clip(
    pool: &PgPool,
    clip_id: impl Into<i64>,
    template_id: impl Into<i64>,
    is_global: bool,
) -> bool {
    let clip_id = clip_id.into();
    let template_id = template_id.into();
    let clip = sqlx::query!(
        "SELECT clip_title, streamer_login, game_name FROM twitch_clips_social_media WHERE id = $1",
        clip_id
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(clip) = clip else {
        return false;
    };
    let title = clip.clip_title;
    let streamer = clip.streamer_login;
    let game = clip.game_name;

    let template: Option<(String, Option<String>)> = if is_global {
        let t = sqlx::query!(
            "SELECT COALESCE(description_template, '') AS \"description_template!\", hashtags AS \"hashtags!\" FROM clip_templates_global WHERE id = $1",
            template_id
        )
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .map(|row| (row.description_template, Some(row.hashtags)));
        let _ = sqlx::query!(
            "UPDATE clip_templates_global SET usage_count = usage_count + 1 WHERE id = $1",
            template_id
        )
        .execute(pool)
        .await;
        t
    } else {
        sqlx::query!(
            "SELECT COALESCE(description_template, '') AS \"description_template!\", hashtags AS \"hashtags!\" FROM clip_templates_streamer WHERE id = $1",
            template_id
        )
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .map(|row| (row.description_template, Some(row.hashtags)))
    };
    let Some((desc_template, hashtags_raw)) = template else {
        return false;
    };

    let title = title.unwrap_or_default();
    let game = game
        .filter(|g| !g.is_empty())
        .unwrap_or_else(|| "Unknown".to_string());
    let game_no_spaces = game.replace(' ', "");

    let description = desc_template
        .replace("{{title}}", &title)
        .replace("{{streamer}}", &streamer)
        .replace("{{game}}", &game);
    let hashtags: Vec<String> = parse_hashtags(hashtags_raw.as_deref())
        .into_iter()
        .map(|tag| tag.replace("{{game}}", &game_no_spaces))
        .collect();

    sqlx::query!(
        "UPDATE twitch_clips_social_media SET custom_description = $1, hashtags = $2 WHERE id = $3",
        &description,
        dump_hashtags(&hashtags),
        clip_id
    )
    .execute(pool)
    .await
    .is_ok()
}

/// Speichert zuletzt genutzte Hashtags eines Streamers (Upsert).
pub async fn save_last_hashtags(pool: &PgPool, streamer_login: &str, hashtags: &[String]) {
    let now = chrono::Utc::now().to_rfc3339();
    let _ = sqlx::query!(
        "INSERT INTO clip_last_hashtags (streamer_login, hashtags, last_used_at) VALUES ($1, $2, $3::text::timestamptz) \
         ON CONFLICT (streamer_login) DO UPDATE SET hashtags = EXCLUDED.hashtags, last_used_at = EXCLUDED.last_used_at",
        streamer_login,
        dump_hashtags(hashtags),
        &now
    )
    .execute(pool)
    .await;
}

/// Lädt zuletzt genutzte Hashtags eines Streamers (leer wenn keine).
pub async fn get_last_hashtags(pool: &PgPool, streamer_login: &str) -> Vec<String> {
    let raw: Option<String> = sqlx::query_scalar!(
        "SELECT hashtags FROM clip_last_hashtags WHERE streamer_login = $1",
        streamer_login
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    parse_hashtags(raw.as_deref())
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
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE clip_templates_global (id BIGSERIAL PRIMARY KEY, template_name TEXT NOT NULL UNIQUE, description_template TEXT NOT NULL, hashtags TEXT NOT NULL, category TEXT, usage_count INTEGER DEFAULT 0, created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP, created_by TEXT)",
            "CREATE TABLE clip_templates_streamer (id BIGSERIAL PRIMARY KEY, streamer_login TEXT NOT NULL, template_name TEXT NOT NULL, description_template TEXT NOT NULL, hashtags TEXT NOT NULL, is_default BOOLEAN DEFAULT FALSE, created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP, updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP, UNIQUE (streamer_login, template_name))",
            "CREATE TABLE clip_last_hashtags (streamer_login TEXT PRIMARY KEY, hashtags TEXT NOT NULL, last_used_at TIMESTAMPTZ NOT NULL)",
            "CREATE TABLE twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, clip_id TEXT NOT NULL, clip_url TEXT NOT NULL, clip_title TEXT, streamer_login TEXT NOT NULL, game_name TEXT, custom_description TEXT, hashtags TEXT, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), source_kind TEXT NOT NULL DEFAULT 'twitch')",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    fn tags(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[tokio::test]
    async fn global_template_crud_und_apply() {
        let Some(pool) = make_pool("t_sm_tpl").await else {
            return;
        };
        let tid = create_global_template(
            &pool,
            "gaming",
            "{{streamer}} spielt {{game}}: {{title}}",
            &tags(&["#deadlock", "#{{game}}"]),
            Some("Gaming"),
            "admin",
        )
        .await
        .unwrap();
        let list = get_global_templates(&pool, Some("Gaming")).await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, tid);
        assert_eq!(list[0].hashtags, tags(&["#deadlock", "#{{game}}"]));

        // Clip + apply.
        let clip: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, clip_title, streamer_login, game_name) VALUES ('tpl-1', 'https://clips.test/tpl-1', 'Insane 1v3', 'nani', 'Dead Lock') RETURNING id").fetch_one(&pool).await.unwrap();
        assert!(apply_template_to_clip(&pool, clip, tid, true).await);
        let (desc, ht): (Option<String>, Option<String>) = sqlx::query_as(
            "SELECT custom_description, hashtags FROM twitch_clips_social_media WHERE id = $1",
        )
        .bind(clip)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(desc.as_deref(), Some("nani spielt Dead Lock: Insane 1v3"));
        // {{game}} im Hashtag → ohne Leerzeichen.
        assert_eq!(
            parse_hashtags(ht.as_deref()),
            tags(&["#deadlock", "#DeadLock"])
        );
        // usage_count erhöht.
        let uc: i32 =
            sqlx::query_scalar("SELECT usage_count FROM clip_templates_global WHERE id = $1")
                .bind(tid)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(uc, 1);
    }

    #[tokio::test]
    async fn streamer_template_upsert_und_default() {
        let Some(pool) = make_pool("t_sm_tpl_streamer").await else {
            return;
        };
        let id1 = create_streamer_template(&pool, "nani", "main", "A", &tags(&["#a"]), true)
            .await
            .unwrap();
        // Zweites Default → erstes wird zurückgesetzt.
        let id2 = create_streamer_template(&pool, "nani", "alt", "B", &tags(&["#b"]), true)
            .await
            .unwrap();
        let list = get_streamer_templates(&pool, "nani").await;
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, id2); // Default zuerst
        assert!(list[0].is_default);
        assert!(!list.iter().find(|t| t.id == id1).unwrap().is_default);
        // Upsert (gleicher Name) → Update, gleiche ID.
        let id1b = create_streamer_template(&pool, "nani", "main", "A2", &tags(&["#a2"]), false)
            .await
            .unwrap();
        assert_eq!(id1, id1b);
        assert_eq!(
            get_streamer_templates(&pool, "nani")
                .await
                .iter()
                .find(|t| t.id == id1)
                .unwrap()
                .description_template,
            "A2"
        );
    }

    #[tokio::test]
    async fn last_hashtags_roundtrip() {
        let Some(pool) = make_pool("t_sm_lasttags").await else {
            return;
        };
        assert!(get_last_hashtags(&pool, "nani").await.is_empty());
        save_last_hashtags(&pool, "nani", &tags(&["#deadlock", "#haze"])).await;
        assert_eq!(
            get_last_hashtags(&pool, "nani").await,
            tags(&["#deadlock", "#haze"])
        );
        save_last_hashtags(&pool, "nani", &tags(&["#neu"])).await; // upsert
        assert_eq!(get_last_hashtags(&pool, "nani").await, tags(&["#neu"]));
    }
}
