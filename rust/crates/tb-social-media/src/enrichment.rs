//! Persistenz der Clip-Enrichment (Port der Storage-Seite von
//! `bot/social_media/enrichment.py`).
//!
//! CRUD über `social_media_clip_enrichment`: Transkript + korrigierter Text +
//! erkannte Begriffe + LLM-Titel/Beschreibung/Hashtags je Plattform + Status.
//! JSONB-Spalten (segments/detected_terms/hashtags_*) via `::text` / `$N::jsonb`.
//! Die LLM-Anreicherung + der Orchestrator folgen separat.

use serde_json::Value;
use sqlx::{postgres::PgRow, PgPool, Postgres, QueryBuilder, Row};

pub const STATUS_PENDING: &str = "pending";
pub const STATUS_TRANSCRIBING: &str = "transcribing";
pub const STATUS_CORRECTING: &str = "correcting";
pub const STATUS_LLM: &str = "llm";
pub const STATUS_DONE: &str = "done";
pub const STATUS_FAILED: &str = "failed";
pub const STATUS_SKIPPED_NO_KEY: &str = "skipped_no_key";

/// LLM-Anreicherung einer Plattform (Titel/Beschreibung/Hashtags).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlatformEnrichment {
    pub title: Option<String>,
    pub description: Option<String>,
    pub hashtags: Vec<String>,
}

/// Eine Zeile aus `social_media_clip_enrichment`.
#[derive(Debug, Clone, PartialEq)]
pub struct EnrichmentRecord {
    pub clip_db_id: i32,
    pub transcript_raw: Option<String>,
    pub transcript_corrected: Option<String>,
    pub transcript_segments: Vec<Value>,
    pub transcript_lang: Option<String>,
    pub detected_terms: Vec<String>,
    pub title_youtube: Option<String>,
    pub title_tiktok: Option<String>,
    pub title_instagram: Option<String>,
    pub description_youtube: Option<String>,
    pub description_tiktok: Option<String>,
    pub description_instagram: Option<String>,
    pub hashtags_youtube: Vec<String>,
    pub hashtags_tiktok: Vec<String>,
    pub hashtags_instagram: Vec<String>,
    pub llm_provider: Option<String>,
    pub llm_model: Option<String>,
    pub cost_usd_estimate: Option<f64>,
    pub status: String,
    pub error_message: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub edited_by: Option<String>,
    pub updated_at: Option<String>,
}

/// JSON-Text → Vec<Value> (Segmente; fehlertolerant).
fn decode_array(raw: Option<String>) -> Vec<Value> {
    raw.and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| {
            if let Value::Array(a) = v {
                Some(a)
            } else {
                None
            }
        })
        .unwrap_or_default()
}

/// JSON-Text → Vec<String> (detected_terms / hashtags; fehlertolerant).
fn decode_strings(raw: Option<String>) -> Vec<String> {
    decode_array(raw)
        .into_iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

/// Alle Spalten — JSONB als `::text`, Timestamps als `::text`.
const SELECT_SQL: &str = "SELECT clip_db_id, transcript_raw, transcript_corrected, \
    transcript_segments::text, transcript_lang, detected_terms::text, \
    title_youtube, title_tiktok, title_instagram, \
    description_youtube, description_tiktok, description_instagram, \
    hashtags_youtube::text, hashtags_tiktok::text, hashtags_instagram::text, \
    llm_provider, llm_model, cost_usd_estimate, status, error_message, \
    started_at::text, completed_at::text, edited_by, updated_at::text \
    FROM social_media_clip_enrichment WHERE clip_db_id = $1";

fn row_to_record(row: &PgRow) -> EnrichmentRecord {
    EnrichmentRecord {
        clip_db_id: row.try_get("clip_db_id").unwrap_or(0),
        transcript_raw: row.try_get("transcript_raw").unwrap_or(None),
        transcript_corrected: row.try_get("transcript_corrected").unwrap_or(None),
        transcript_segments: decode_array(row.try_get("transcript_segments").unwrap_or(None)),
        transcript_lang: row.try_get("transcript_lang").unwrap_or(None),
        detected_terms: decode_strings(row.try_get("detected_terms").unwrap_or(None)),
        title_youtube: row.try_get("title_youtube").unwrap_or(None),
        title_tiktok: row.try_get("title_tiktok").unwrap_or(None),
        title_instagram: row.try_get("title_instagram").unwrap_or(None),
        description_youtube: row.try_get("description_youtube").unwrap_or(None),
        description_tiktok: row.try_get("description_tiktok").unwrap_or(None),
        description_instagram: row.try_get("description_instagram").unwrap_or(None),
        hashtags_youtube: decode_strings(row.try_get("hashtags_youtube").unwrap_or(None)),
        hashtags_tiktok: decode_strings(row.try_get("hashtags_tiktok").unwrap_or(None)),
        hashtags_instagram: decode_strings(row.try_get("hashtags_instagram").unwrap_or(None)),
        llm_provider: row.try_get("llm_provider").unwrap_or(None),
        llm_model: row.try_get("llm_model").unwrap_or(None),
        cost_usd_estimate: row.try_get("cost_usd_estimate").unwrap_or(None),
        status: row
            .try_get::<Option<String>, _>("status")
            .unwrap_or(None)
            .unwrap_or_else(|| STATUS_PENDING.to_string()),
        error_message: row.try_get("error_message").unwrap_or(None),
        started_at: row.try_get("started_at").unwrap_or(None),
        completed_at: row.try_get("completed_at").unwrap_or(None),
        edited_by: row.try_get("edited_by").unwrap_or(None),
        updated_at: row.try_get("updated_at").unwrap_or(None),
    }
}

/// Lädt die Enrichment-Zeile eines Clips.
pub async fn get_enrichment(pool: &PgPool, clip_db_id: i32) -> Option<EnrichmentRecord> {
    match sqlx::query(SELECT_SQL)
        .bind(clip_db_id)
        .fetch_optional(pool)
        .await
    {
        Ok(row) => row.as_ref().map(row_to_record),
        Err(error) => {
            tracing::warn!(
                %error,
                clip_db_id,
                "Social-Media-Enrichment: Enrichment-Zeile nicht ladbar"
            );
            None
        }
    }
}

/// Stellt sicher, dass eine (pending-)Zeile existiert; liefert sie.
pub async fn ensure_enrichment_row(pool: &PgPool, clip_db_id: i32) -> EnrichmentRecord {
    if let Some(existing) = get_enrichment(pool, clip_db_id).await {
        return existing;
    }
    if let Err(error) = sqlx::query!(
        "INSERT INTO social_media_clip_enrichment (clip_db_id, status, updated_at) \
         VALUES ($1, $2, CURRENT_TIMESTAMP) ON CONFLICT (clip_db_id) DO NOTHING",
        clip_db_id,
        STATUS_PENDING
    )
    .execute(pool)
    .await
    {
        tracing::warn!(
            %error,
            clip_db_id,
            "Social-Media-Enrichment: Pending-Zeile konnte nicht sichergestellt werden"
        );
    }
    match get_enrichment(pool, clip_db_id).await {
        Some(record) => record,
        None => {
            tracing::warn!(
                clip_db_id,
                "Social-Media-Enrichment: Pending-Zeile nicht ladbar, Fallback-Record"
            );
            EnrichmentRecord {
                clip_db_id,
                transcript_raw: None,
                transcript_corrected: None,
                transcript_segments: Vec::new(),
                transcript_lang: None,
                detected_terms: Vec::new(),
                title_youtube: None,
                title_tiktok: None,
                title_instagram: None,
                description_youtube: None,
                description_tiktok: None,
                description_instagram: None,
                hashtags_youtube: Vec::new(),
                hashtags_tiktok: Vec::new(),
                hashtags_instagram: Vec::new(),
                llm_provider: None,
                llm_model: None,
                cost_usd_estimate: None,
                status: STATUS_PENDING.to_string(),
                error_message: None,
                started_at: None,
                completed_at: None,
                edited_by: None,
                updated_at: None,
            }
        }
    }
}

/// Setzt den Status (+ optionale Felder). Outer `None` = unverändert lassen
/// (Python-Sentinel `...`); `Some(None)` = auf NULL setzen.
pub async fn update_enrichment_status(
    pool: &PgPool,
    clip_db_id: i32,
    status: &str,
    error_message: Option<Option<String>>,
    started_at: Option<Option<String>>,
    completed_at: Option<Option<String>>,
) -> Result<(), sqlx::Error> {
    ensure_enrichment_row(pool, clip_db_id).await;
    let mut qb = QueryBuilder::<Postgres>::new("UPDATE social_media_clip_enrichment SET status = ");
    qb.push_bind(status.to_string())
        .push(", updated_at = CURRENT_TIMESTAMP");
    if let Some(em) = error_message {
        qb.push(", error_message = ").push_bind(em);
    }
    if let Some(sa) = started_at {
        qb.push(", started_at = ")
            .push_bind(sa)
            .push("::timestamptz");
    }
    if let Some(ca) = completed_at {
        qb.push(", completed_at = ")
            .push_bind(ca)
            .push("::timestamptz");
    }
    qb.push(" WHERE clip_db_id = ").push_bind(clip_db_id);
    qb.build().execute(pool).await?;
    Ok(())
}

/// Speichert den Roh-Transkript + Segmente + Sprache.
pub async fn save_transcript(
    pool: &PgPool,
    clip_db_id: i32,
    transcript_raw: Option<&str>,
    transcript_segments: &[Value],
    transcript_lang: Option<&str>,
) -> Result<(), sqlx::Error> {
    let payload = if transcript_segments.is_empty() {
        None
    } else {
        Some(serde_json::to_string(transcript_segments).unwrap_or_else(|_| "[]".to_string()))
    };
    sqlx::query!(
        "UPDATE social_media_clip_enrichment SET transcript_raw = $1, \
         transcript_segments = $2::text::jsonb, transcript_lang = $3, updated_at = CURRENT_TIMESTAMP \
         WHERE clip_db_id = $4",
        transcript_raw,
        payload.as_deref(),
        transcript_lang,
        clip_db_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Speichert den korrigierten Text + erkannte Begriffe.
pub async fn save_corrected(
    pool: &PgPool,
    clip_db_id: i32,
    transcript_corrected: Option<&str>,
    detected_terms: &[String],
) -> Result<(), sqlx::Error> {
    let payload = serde_json::to_string(detected_terms).unwrap_or_else(|_| "[]".to_string());
    sqlx::query!(
        "UPDATE social_media_clip_enrichment SET transcript_corrected = $1, \
         detected_terms = $2::text::jsonb, updated_at = CURRENT_TIMESTAMP WHERE clip_db_id = $3",
        transcript_corrected,
        &payload,
        clip_db_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Speichert die LLM-Ausgabe je Plattform + Provider/Modell/Kosten.
#[allow(clippy::too_many_arguments)]
pub async fn save_llm_output(
    pool: &PgPool,
    clip_db_id: i32,
    youtube: &PlatformEnrichment,
    tiktok: &PlatformEnrichment,
    instagram: &PlatformEnrichment,
    provider: &str,
    model: Option<&str>,
    cost_usd_estimate: Option<f64>,
) -> Result<(), sqlx::Error> {
    let json_tags = |p: &PlatformEnrichment| {
        serde_json::to_string(&p.hashtags).unwrap_or_else(|_| "[]".to_string())
    };
    let youtube_tags = json_tags(youtube);
    let tiktok_tags = json_tags(tiktok);
    let instagram_tags = json_tags(instagram);
    sqlx::query!(
        "UPDATE social_media_clip_enrichment SET \
            title_youtube = $1, title_tiktok = $2, title_instagram = $3, \
            description_youtube = $4, description_tiktok = $5, description_instagram = $6, \
            hashtags_youtube = $7::text::jsonb, hashtags_tiktok = $8::text::jsonb, hashtags_instagram = $9::text::jsonb, \
            llm_provider = $10, llm_model = $11, cost_usd_estimate = $12::double precision, updated_at = CURRENT_TIMESTAMP \
         WHERE clip_db_id = $13",
        youtube.title.as_deref(),
        tiktok.title.as_deref(),
        instagram.title.as_deref(),
        youtube.description.as_deref(),
        tiktok.description.as_deref(),
        instagram.description.as_deref(),
        &youtube_tags,
        &tiktok_tags,
        &instagram_tags,
        provider,
        model,
        cost_usd_estimate,
        clip_db_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Clip-IDs, die Enrichment brauchen (Python `iter_pending_enrichments`):
/// nicht verworfen, mit lokaler Datei, ohne Enrichment-Zeile ODER Status
/// `pending`/`failed` — neueste zuerst.
///
/// Kategorie-Gate: angereichert wird nur, was in einer Kategorie mit
/// `enrichment_enabled` liegt (heute allein Deadlock). Clips anderer Spiele
/// bekommen das nackte Auto-Posting ohne LLM und laufen ueber
/// [`crate::approval::iter_clips_ohne_enrichment`] in den Approval-Workflow.
pub async fn iter_pending_enrichments(pool: &PgPool, limit: i64) -> Vec<i32> {
    let rows = sqlx::query!(
        "SELECT c.id AS \"id!\" FROM twitch_clips_social_media c \
         LEFT JOIN social_media_clip_enrichment e ON e.clip_db_id = c.id \
         JOIN social_media_category k ON k.category_key = c.category_key \
         WHERE c.discarded_at IS NULL \
           AND k.enrichment_enabled \
           AND COALESCE(c.upload_local_path, c.local_file_path) IS NOT NULL \
           AND (e.status IS NULL OR e.status IN ('pending', 'failed')) \
           AND c.id BETWEEN 0 AND 2147483647 \
         ORDER BY c.created_at DESC LIMIT $1",
        limit.max(1)
    )
    .fetch_all(pool)
    .await
    .unwrap_or_else(|error| {
        // Ein Fehler hier sah frueher aus wie "keine Clips offen": die
        // Enrichment-Strecke waere still stehen geblieben, ohne dass irgendwo
        // etwas auffaellt.
        tracing::error!(%error, "Offene Enrichments konnten nicht gelesen werden");
        Vec::new()
    });
    let mut ids = Vec::with_capacity(rows.len());
    for row in rows {
        let Ok(id) = i32::try_from(row.id) else {
            tracing::warn!(
                clip_db_id = row.id,
                "pending enrichment clip id is out of int4 range; skipping"
            );
            continue;
        };
        ids.push(id);
    }
    ids
}

/// Speichert manuelle Enrichment-Edits aus dem Admin-UI (Python
/// `update_manual_edit`). title/description-Felder nutzen `Option<Option<&str>>`:
/// `None` = unverändert lassen, `Some(None)` = auf NULL setzen, `Some(Some(v))`
/// = Wert setzen. hashtags: `None` = unverändert, `Some(list)` = setzen.
#[allow(clippy::too_many_arguments)]
pub async fn update_manual_edit(
    pool: &PgPool,
    clip_db_id: i32,
    edited_by: Option<&str>,
    title_youtube: Option<Option<&str>>,
    title_tiktok: Option<Option<&str>>,
    title_instagram: Option<Option<&str>>,
    description_youtube: Option<Option<&str>>,
    description_tiktok: Option<Option<&str>>,
    description_instagram: Option<Option<&str>>,
    hashtags_youtube: Option<&[String]>,
    hashtags_tiktok: Option<&[String]>,
    hashtags_instagram: Option<&[String]>,
) -> Result<(), sqlx::Error> {
    // Zeile sicherstellen.
    sqlx::query!(
        "INSERT INTO social_media_clip_enrichment (clip_db_id, status, updated_at, edited_by) \
         VALUES ($1, $2, CURRENT_TIMESTAMP, $3) ON CONFLICT (clip_db_id) DO NOTHING",
        clip_db_id,
        STATUS_PENDING,
        edited_by
    )
    .execute(pool)
    .await?;

    let mut qb = QueryBuilder::<Postgres>::new(
        "UPDATE social_media_clip_enrichment SET updated_at = CURRENT_TIMESTAMP, edited_by = ",
    );
    qb.push_bind(edited_by.map(str::to_string));
    for (col, value) in [
        ("title_youtube", title_youtube),
        ("title_tiktok", title_tiktok),
        ("title_instagram", title_instagram),
        ("description_youtube", description_youtube),
        ("description_tiktok", description_tiktok),
        ("description_instagram", description_instagram),
    ] {
        if let Some(v) = value {
            qb.push(", ")
                .push(col)
                .push(" = ")
                .push_bind(v.map(str::to_string));
        }
    }
    for (col, value) in [
        ("hashtags_youtube", hashtags_youtube),
        ("hashtags_tiktok", hashtags_tiktok),
        ("hashtags_instagram", hashtags_instagram),
    ] {
        if let Some(list) = value {
            qb.push(", ")
                .push(col)
                .push(" = ")
                .push_bind(serde_json::to_string(list).unwrap_or_else(|_| "[]".to_string()))
                .push("::jsonb");
        }
    }

    qb.push(" WHERE clip_db_id = ").push_bind(clip_db_id);
    qb.build().execute(pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = crate::test_support::test_dsn()?;
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
        // Minimaltabelle (Spalten wie schema.rs, ohne FK).
        sqlx::query(
            "CREATE TABLE social_media_clip_enrichment (clip_db_id INTEGER PRIMARY KEY, \
             transcript_raw TEXT, transcript_corrected TEXT, transcript_segments JSONB, \
             transcript_lang TEXT, detected_terms JSONB DEFAULT '[]'::jsonb, \
             title_youtube TEXT, title_tiktok TEXT, title_instagram TEXT, \
             description_youtube TEXT, description_tiktok TEXT, description_instagram TEXT, \
             hashtags_youtube JSONB DEFAULT '[]'::jsonb, hashtags_tiktok JSONB DEFAULT '[]'::jsonb, \
             hashtags_instagram JSONB DEFAULT '[]'::jsonb, llm_provider TEXT, llm_model TEXT, \
             cost_usd_estimate NUMERIC(10,6), status TEXT NOT NULL DEFAULT 'pending', \
             error_message TEXT, started_at TIMESTAMPTZ, completed_at TIMESTAMPTZ, edited_by TEXT, \
             updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP)",
        )
        .execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn persistenz_roundtrip() {
        let Some(pool) = make_pool("t_sm_enrich").await else {
            return;
        };
        // ensure legt pending an.
        let rec = ensure_enrichment_row(&pool, 7).await;
        assert_eq!(rec.clip_db_id, 7);
        assert_eq!(rec.status, "pending");
        // Idempotent.
        ensure_enrichment_row(&pool, 7).await;

        // Transkript + Segmente.
        let segs = vec![json!({"start": 0.0, "end": 2.0, "text": "haze"})];
        save_transcript(&pool, 7, Some("haze ist stark"), &segs, Some("de"))
            .await
            .unwrap();
        // Korrektur.
        save_corrected(&pool, 7, Some("Haze ist stark"), &["Haze".to_string()])
            .await
            .unwrap();
        // LLM-Ausgabe.
        let yt = PlatformEnrichment {
            title: Some("YT Titel".into()),
            description: Some("desc".into()),
            hashtags: vec!["deadlock".into(), "haze".into()],
        };
        let tk = PlatformEnrichment {
            title: Some("TK".into()),
            description: None,
            hashtags: vec!["dl".into()],
        };
        let ig = PlatformEnrichment::default();
        save_llm_output(&pool, 7, &yt, &tk, &ig, "ollama", Some("llama3"), Some(0.0))
            .await
            .unwrap();

        let got = get_enrichment(&pool, 7).await.unwrap();
        assert_eq!(got.transcript_raw.as_deref(), Some("haze ist stark"));
        assert_eq!(got.transcript_corrected.as_deref(), Some("Haze ist stark"));
        assert_eq!(got.transcript_lang.as_deref(), Some("de"));
        assert_eq!(got.detected_terms, vec!["Haze".to_string()]);
        assert_eq!(got.transcript_segments.len(), 1);
        assert_eq!(got.title_youtube.as_deref(), Some("YT Titel"));
        assert_eq!(
            got.hashtags_youtube,
            vec!["deadlock".to_string(), "haze".to_string()]
        );
        assert_eq!(got.title_instagram, None);
        assert_eq!(got.hashtags_instagram, Vec::<String>::new());
        assert_eq!(got.llm_provider.as_deref(), Some("ollama"));
        assert_eq!(got.llm_model.as_deref(), Some("llama3"));
    }

    #[tokio::test]
    async fn status_sentinel() {
        let Some(pool) = make_pool("t_sm_enrich_status").await else {
            return;
        };
        // failed + error setzen, started_at unverändert (None), completed_at = NULL setzen.
        update_enrichment_status(
            &pool,
            1,
            STATUS_FAILED,
            Some(Some("boom".into())),
            None,
            Some(None),
        )
        .await
        .unwrap();
        let r = get_enrichment(&pool, 1).await.unwrap();
        assert_eq!(r.status, "failed");
        assert_eq!(r.error_message.as_deref(), Some("boom"));
        assert!(r.completed_at.is_none());

        // Status auf done, error NICHT anfassen (None) → bleibt "boom".
        update_enrichment_status(&pool, 1, STATUS_DONE, None, None, None)
            .await
            .unwrap();
        let r = get_enrichment(&pool, 1).await.unwrap();
        assert_eq!(r.status, "done");
        assert_eq!(r.error_message.as_deref(), Some("boom")); // unverändert
    }

    #[tokio::test]
    async fn manual_edit_sentinel() {
        let Some(pool) = make_pool("t_sm_enrich_edit").await else {
            return;
        };
        // Edit 1: YT + TT-Titel setzen, hashtags_youtube setzen (Zeile via INSERT angelegt).
        update_manual_edit(
            &pool,
            1,
            Some("admin"),
            Some(Some("YT")),
            Some(Some("TT")),
            None,
            None,
            None,
            None,
            Some(&["#a".into(), "#b".into()]),
            None,
            None,
        )
        .await
        .unwrap();
        let r = get_enrichment(&pool, 1).await.unwrap();
        assert_eq!(r.title_youtube.as_deref(), Some("YT"));
        assert_eq!(r.title_tiktok.as_deref(), Some("TT"));
        assert_eq!(r.edited_by.as_deref(), Some("admin"));
        assert_eq!(r.hashtags_youtube, vec!["#a".to_string(), "#b".to_string()]);

        // Edit 2: title_tiktok auf NULL (Some(None)), title_youtube unverändert (None).
        update_manual_edit(
            &pool,
            1,
            Some("admin2"),
            None,
            Some(None),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let r = get_enrichment(&pool, 1).await.unwrap();
        assert_eq!(r.title_youtube.as_deref(), Some("YT")); // unverändert (None=skip)
        assert!(r.title_tiktok.is_none()); // geleert (Some(None))
        assert_eq!(r.edited_by.as_deref(), Some("admin2")); // edited_by immer gesetzt
        assert_eq!(r.hashtags_youtube, vec!["#a".to_string(), "#b".to_string()]);
        // unverändert
    }
}
