//! Retention/Publication-Logik (vollständiger Port von
//! `bot/social_media/retention.py`).
//!
//! Bestimmt, ob ein Clip auf allen *aktiven* Plattformen des Streamers
//! veröffentlicht ist, und pflegt daraus den `status` der Clip-Zeile
//! (`published_all` ↔ `pending`). Aktive Plattformen = solche mit aktivem
//! Auth-Record. Dazu der Discard-/Cleanup-Teil: Verwerfen einzelner Clips sowie
//! das Auslesen + Löschen abgelaufener Clips (für den Retention-Worker).

use std::collections::HashSet;

use sqlx::PgPool;

/// Plattform → Upload-Flag-Spalte.
const PLATFORM_UPLOAD_COLUMNS: [(&str, &str); 3] = [
    ("tiktok", "uploaded_tiktok"),
    ("youtube", "uploaded_youtube"),
    ("instagram", "uploaded_instagram"),
];

/// Plattformen, für die der Streamer (oder global) einen aktiven Auth-Record hat.
pub async fn get_active_platforms_for_streamer(
    pool: &PgPool,
    streamer_login: Option<&str>,
) -> Result<HashSet<String>, sqlx::Error> {
    let login = streamer_login.unwrap_or("").trim().to_lowercase();
    Ok(sqlx::query_scalar!(
        "SELECT DISTINCT platform FROM social_media_platform_auth \
         WHERE enabled = 1 AND (LOWER(COALESCE(streamer_login, '')) = LOWER($1) OR streamer_login IS NULL)",
        &login
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .collect())
}

/// `true`, wenn der Clip auf allen aktiven Plattformen hochgeladen ist. Ohne
/// aktive Plattform ist nichts veröffentlicht; Prepare-only-Clips bleiben so
/// auch nach Ablauf der Retention-Frist erhalten.
pub async fn is_clip_published_on_all_active_platforms(
    pool: &PgPool,
    clip_db_id: impl Into<i64>,
) -> Result<bool, sqlx::Error> {
    let clip_db_id = clip_db_id.into();
    let row = sqlx::query!(
        "SELECT streamer_login, COALESCE(uploaded_tiktok, false) AS \"uploaded_tiktok!\", \
                COALESCE(uploaded_youtube, false) AS \"uploaded_youtube!\", \
                COALESCE(uploaded_instagram, false) AS \"uploaded_instagram!\" \
         FROM twitch_clips_social_media WHERE id = $1 LIMIT 1",
        clip_db_id
    )
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        return Ok(false);
    };
    let active = get_active_platforms_for_streamer(pool, Some(row.streamer_login.as_str())).await?;
    if active.is_empty() {
        return Ok(false);
    }
    let uploaded = |col: &str| -> bool {
        match col {
            "uploaded_tiktok" => row.uploaded_tiktok,
            "uploaded_youtube" => row.uploaded_youtube,
            _ => row.uploaded_instagram,
        }
    };
    for (platform, col) in PLATFORM_UPLOAD_COLUMNS {
        if active.contains(platform) && !uploaded(col) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Aktualisiert den Clip-`status` aus dem Publication-Stand (Python
/// `refresh_clip_publication_status`). Liefert, ob auf allen aktiven Plattformen
/// veröffentlicht.
pub async fn refresh_clip_publication_status(
    pool: &PgPool,
    clip_db_id: impl Into<i64>,
) -> Result<bool, sqlx::Error> {
    let clip_db_id = clip_db_id.into();
    let published_all = is_clip_published_on_all_active_platforms(pool, clip_db_id).await?;
    if published_all {
        sqlx::query!(
            "UPDATE twitch_clips_social_media SET status = 'published_all' \
             WHERE id = $1 AND discarded_at IS NULL",
            clip_db_id
        )
        .execute(pool)
        .await?;
    } else {
        sqlx::query!(
            "UPDATE twitch_clips_social_media \
             SET status = CASE WHEN discarded_at IS NOT NULL THEN status ELSE 'pending' END \
             WHERE id = $1 AND status = 'published_all'",
            clip_db_id
        )
        .execute(pool)
        .await?;
    }
    Ok(published_all)
}

/// Ein abgelaufener Clip (für den Retention-Cleanup). Timestamps als ISO-Text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpiredClip {
    pub id: i64,
    pub clip_id: Option<String>,
    pub streamer_login: Option<String>,
    pub source_kind: Option<String>,
    pub upload_local_path: Option<String>,
    pub local_file_path: Option<String>,
    pub render_path: Option<String>,
    pub retention_until: Option<String>,
    pub discarded_at: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscardOutcome {
    pub discarded: bool,
    pub pending_stopped: i64,
    /// Jobs in `processing` oder `completed` lassen sich nicht mehr sicher
    /// zurückholen. Der Aufrufer muss das ausdrücklich anzeigen.
    pub already_running: i64,
}

/// Verwirft Clip, Freigabe und wartende Queue-Zeilen in einer Transaktion.
/// Bereits laufende/fertige Uploads werden gezählt statt als sicher gestoppt
/// ausgegeben.
pub async fn discard_clip(
    pool: &PgPool,
    clip_db_id: impl Into<i64>,
) -> Result<Option<DiscardOutcome>, sqlx::Error> {
    let clip_db_id = clip_db_id.into();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "SELECT clip_db_id FROM social_media_clip_preparation \
         WHERE clip_db_id = $1 FOR UPDATE",
    )
    .bind(clip_db_id)
    .fetch_all(&mut *tx)
    .await?;
    let hit: Option<i64> =
        sqlx::query_scalar("SELECT id FROM twitch_clips_social_media WHERE id = $1 FOR UPDATE")
            .bind(clip_db_id)
            .fetch_optional(&mut *tx)
            .await?;
    if hit.is_none() {
        return Ok(None);
    }
    let queue_rows: Vec<(String, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
        "SELECT status, provider_started_at FROM twitch_clips_upload_queue \
         WHERE clip_id = $1 FOR UPDATE",
    )
    .bind(clip_db_id)
    .fetch_all(&mut *tx)
    .await?;
    let already_running = queue_rows
        .iter()
        .filter(|(status, started)| {
            matches!(status.as_str(), "processing" | "reconciliation_required")
                || (started.is_some() && status != "completed" && status != "failed")
        })
        .count() as i64;
    if already_running > 0 {
        tx.rollback().await?;
        return Ok(Some(DiscardOutcome {
            discarded: false,
            pending_stopped: 0,
            already_running,
        }));
    }
    sqlx::query(
        "UPDATE twitch_clips_social_media \
         SET discarded_at = COALESCE(discarded_at, CURRENT_TIMESTAMP), status = 'discarded' \
         WHERE id = $1",
    )
    .bind(clip_db_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO social_media_clip_approval (clip_db_id, state, approved_platforms) \
         VALUES (($1::bigint)::integer, 'skipped', '[]'::jsonb) \
         ON CONFLICT (clip_db_id) DO UPDATE SET state = 'skipped', \
             approved_platforms = '[]'::jsonb, approver_user_id = NULL, \
             approved_render_fingerprint = NULL, decided_at = CURRENT_TIMESTAMP",
    )
    .bind(clip_db_id)
    .execute(&mut *tx)
    .await?;
    let pending_stopped = sqlx::query(
        "UPDATE twitch_clips_upload_queue SET status = 'failed', \
         last_error = 'clip_discarded', last_attempt_at = CURRENT_TIMESTAMP \
         WHERE clip_id = $1 AND status = 'pending' \
           AND provider_started_at IS NULL",
    )
    .bind(clip_db_id)
    .execute(&mut *tx)
    .await?
    .rows_affected() as i64;
    sqlx::query(
        "UPDATE social_media_clip_preparation SET state = 'failed', \
         lease_token = NULL, \
         error_code = 'clip_discarded', error_message = NULL, \
         completed_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP \
         WHERE clip_db_id = $1 AND state NOT IN ('materializing', 'rendering')",
    )
    .bind(clip_db_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Some(DiscardOutcome {
        discarded: true,
        pending_stopped,
        already_running,
    }))
}

/// Kompatibler Bool-Pfad für bestehende interne Aufrufer.
pub async fn mark_clip_discarded(pool: &PgPool, clip_db_id: impl Into<i64>) -> bool {
    match discard_clip(pool, clip_db_id).await {
        Ok(Some(outcome)) => outcome.discarded,
        Ok(None) => false,
        Err(error) => {
            tracing::warn!(%error, "Clip konnte nicht transaktional verworfen werden");
            false
        }
    }
}

/// Clips, deren Retention-Frist (`retention_until`) bis `now` (rfc3339)
/// erreicht ist — älteste zuerst.
pub async fn iter_expired_clips_for_retention(
    pool: &PgPool,
    now: &str,
) -> Result<Vec<ExpiredClip>, sqlx::Error> {
    type ExpiredClipRow = (
        i64,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let rows: Vec<ExpiredClipRow> = sqlx::query_as(
        "SELECT c.id, c.clip_id, c.streamer_login, c.source_kind, c.upload_local_path, \
                    c.local_file_path, p.render_path, c.retention_until::text, \
                    c.discarded_at::text, c.status \
               FROM twitch_clips_social_media c \
               LEFT JOIN social_media_clip_preparation p ON p.clip_db_id = c.id \
              WHERE c.retention_until IS NOT NULL AND c.retention_until <= $1::text::timestamptz \
              ORDER BY c.retention_until ASC, c.id ASC",
    )
    .bind(now)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| ExpiredClip {
            id: row.0,
            clip_id: Some(row.1),
            streamer_login: Some(row.2),
            source_kind: Some(row.3),
            upload_local_path: row.4,
            local_file_path: row.5,
            render_path: row.6,
            retention_until: row.7,
            discarded_at: row.8,
            status: row.9,
        })
        .collect())
}

/// Löscht die Clip-Zeilen mit den gegebenen IDs (leere Liste = No-op).
pub async fn delete_clips_by_ids(pool: &PgPool, clip_ids: &[i64]) {
    if clip_ids.is_empty() {
        return;
    }
    if let Err(error) = sqlx::query!(
        "DELETE FROM twitch_clips_social_media WHERE id = ANY($1::bigint[])",
        clip_ids
    )
    .execute(pool)
    .await
    {
        tracing::warn!(
            %error,
            count = clip_ids.len(),
            "Social-Media-Retention: Clip-Zeilen konnten nicht geloescht werden"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        sqlx::query("CREATE TABLE social_media_platform_auth (id SERIAL PRIMARY KEY, platform TEXT, streamer_login TEXT, enabled INTEGER DEFAULT 1)")
            .execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, clip_id TEXT NOT NULL, clip_url TEXT NOT NULL, streamer_login TEXT NOT NULL, source_kind TEXT NOT NULL DEFAULT 'twitch', upload_local_path TEXT, local_file_path TEXT, status TEXT DEFAULT 'pending', created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), retention_until TIMESTAMPTZ, discarded_at TIMESTAMPTZ, uploaded_tiktok BOOLEAN DEFAULT FALSE, uploaded_youtube BOOLEAN DEFAULT FALSE, uploaded_instagram BOOLEAN DEFAULT FALSE)")
            .execute(&pool).await.unwrap();
        for ddl in [
            "CREATE TABLE social_media_clip_approval (clip_db_id INTEGER PRIMARY KEY, state TEXT NOT NULL, approved_platforms JSONB NOT NULL DEFAULT '[]'::jsonb, approver_user_id TEXT, decided_at TIMESTAMPTZ, approved_render_fingerprint TEXT)",
            "CREATE TABLE twitch_clips_upload_queue (id BIGSERIAL PRIMARY KEY, clip_id BIGINT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', last_error TEXT, last_attempt_at TIMESTAMPTZ, provider_started_at TIMESTAMPTZ, provider_lease_token TEXT, provider_external_id TEXT, provider_accepted_at TIMESTAMPTZ)",
            "CREATE TABLE social_media_clip_preparation (clip_db_id BIGINT PRIMARY KEY, state TEXT NOT NULL DEFAULT 'pending', lease_token TEXT, render_path TEXT, error_code TEXT, error_message TEXT, completed_at TIMESTAMPTZ, updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn publication_status_kette() {
        let Some(pool) = make_pool("t_sm_retention").await else {
            return;
        };
        // Aktive Plattformen für 'nani': tiktok (streamer) + youtube (global).
        sqlx::query("INSERT INTO social_media_platform_auth (platform, streamer_login) VALUES ('tiktok','nani'), ('youtube', NULL)").execute(&pool).await.unwrap();
        let active = get_active_platforms_for_streamer(&pool, Some("nani"))
            .await
            .unwrap();
        assert!(
            active.contains("tiktok")
                && active.contains("youtube")
                && !active.contains("instagram")
        );

        // Clip: nur tiktok hochgeladen → nicht alle aktiven.
        let id: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, uploaded_tiktok) VALUES ('pub-1', 'https://clips.test/pub-1', 'nani', TRUE) RETURNING id").fetch_one(&pool).await.unwrap();
        assert!(!is_clip_published_on_all_active_platforms(&pool, id)
            .await
            .unwrap());
        assert!(!refresh_clip_publication_status(&pool, id).await.unwrap());

        // Jetzt auch youtube → alle aktiven veröffentlicht → published_all.
        sqlx::query("UPDATE twitch_clips_social_media SET uploaded_youtube = TRUE WHERE id = $1")
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        assert!(refresh_clip_publication_status(&pool, id).await.unwrap());
        let status: String =
            sqlx::query_scalar("SELECT status FROM twitch_clips_social_media WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "published_all");
    }

    #[tokio::test]
    async fn keine_aktiven_plattformen_ist_nicht_published() {
        let Some(pool) = make_pool("t_sm_retention_none").await else {
            return;
        };
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login) VALUES ('none-1', 'https://clips.test/none-1', 'x') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        // Keine Auth-Records: Prepare-only-Bestand darf nicht als publiziert
        // gelten und dadurch in Retention fallen.
        assert!(!is_clip_published_on_all_active_platforms(&pool, id)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn discard_und_cleanup() {
        let Some(pool) = make_pool("t_sm_retention_cleanup").await else {
            return;
        };
        // mark_clip_discarded setzt status + discarded_at, liefert true.
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login) VALUES ('discard-1', 'https://clips.test/discard-1', 'nani') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO social_media_clip_approval (clip_db_id, state, approved_platforms) \
             VALUES ($1, 'approved', '[\"tiktok\"]'::jsonb)",
        )
        .bind(i32::try_from(id).unwrap())
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO social_media_clip_preparation (clip_db_id, state) \
             VALUES ($1, 'pending')",
        )
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_clips_upload_queue (clip_id, status) VALUES ($1, 'pending')",
        )
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_clips_upload_queue (clip_id, status) VALUES ($1, 'processing')",
        )
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
        let outcome = discard_clip(&pool, id).await.unwrap().unwrap();
        assert!(!outcome.discarded);
        assert_eq!(outcome.pending_stopped, 0);
        assert_eq!(outcome.already_running, 1);
        let (unchanged_status, unchanged_approval, pending): (String, String, i64) =
            sqlx::query_as(
                "SELECT c.status, a.state, \
                    (SELECT COUNT(*) FROM twitch_clips_upload_queue q \
                     WHERE q.clip_id = c.id AND q.status = 'pending') \
             FROM twitch_clips_social_media c \
             JOIN social_media_clip_approval a ON a.clip_db_id = c.id::integer \
             WHERE c.id = $1",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(unchanged_status, "pending");
        assert_eq!(unchanged_approval, "approved");
        assert_eq!(pending, 1);

        sqlx::query(
            "UPDATE twitch_clips_upload_queue SET status = 'failed' \
             WHERE clip_id = $1 AND status = 'processing'",
        )
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
        let outcome = discard_clip(&pool, id).await.unwrap().unwrap();
        assert!(outcome.discarded);
        assert_eq!(outcome.pending_stopped, 1);
        assert_eq!(outcome.already_running, 0);
        let (status, discarded): (String, Option<String>) = sqlx::query_as(
            "SELECT status, discarded_at::text FROM twitch_clips_social_media WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "discarded");
        assert!(discarded.is_some());
        let (approval_state, platforms): (String, String) = sqlx::query_as(
            "SELECT state, approved_platforms::text FROM social_media_clip_approval WHERE clip_db_id = $1",
        )
        .bind(i32::try_from(id).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(approval_state, "skipped");
        assert_eq!(platforms, "[]");
        let (queue_state, queue_error): (String, Option<String>) = sqlx::query_as(
            "SELECT status, last_error FROM twitch_clips_upload_queue \
             WHERE clip_id = $1 AND last_error = 'clip_discarded'",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(queue_state, "failed");
        assert_eq!(queue_error.as_deref(), Some("clip_discarded"));
        let preparation: (String, Option<String>) = sqlx::query_as(
            "SELECT state, error_code FROM social_media_clip_preparation WHERE clip_db_id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(preparation.0, "failed");
        assert_eq!(preparation.1.as_deref(), Some("clip_discarded"));
        // Nicht existierender Clip → false.
        assert!(!mark_clip_discarded(&pool, 999_999).await);

        // Zwei abgelaufene + ein zukünftiger Clip.
        let past1: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, source_kind, upload_local_path, retention_until) VALUES ('a', 'https://clips.test/a', 'nani', 'twitch', '/a.mp4', NOW() - INTERVAL '2 days') RETURNING id").fetch_one(&pool).await.unwrap();
        let past2: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, retention_until) VALUES ('b', 'https://clips.test/b', 'nani', NOW() - INTERVAL '1 day') RETURNING id").fetch_one(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, retention_until) VALUES ('future', 'https://clips.test/future', 'nani', NOW() + INTERVAL '5 days')").execute(&pool).await.unwrap();

        let now = chrono::Utc::now().to_rfc3339();
        let expired = iter_expired_clips_for_retention(&pool, &now).await.unwrap();
        let ids: Vec<i64> = expired.iter().map(|c| c.id).collect();
        // Älteste zuerst (past1 vor past2), zukünftiger NICHT dabei.
        assert_eq!(ids, vec![past1, past2]);
        assert_eq!(expired[0].clip_id.as_deref(), Some("a"));
        assert_eq!(expired[0].upload_local_path.as_deref(), Some("/a.mp4"));

        // delete_clips_by_ids entfernt sie; leere Liste = No-op.
        delete_clips_by_ids(&pool, &[]).await;
        delete_clips_by_ids(&pool, &ids).await;
        let remaining: Vec<i64> = sqlx::query_scalar(
            "SELECT id FROM twitch_clips_social_media WHERE clip_id IN ('a','b')",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert!(remaining.is_empty());
    }

    #[tokio::test]
    async fn retention_db_fehler_werden_nicht_als_leere_mengen_getarnt() {
        let Some(pool) = make_pool("t_sm_retention_fail_closed_errors").await else {
            return;
        };
        sqlx::query("DROP TABLE twitch_clips_social_media")
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            iter_expired_clips_for_retention(&pool, "2026-09-01T00:00:00Z")
                .await
                .is_err()
        );
        assert!(get_active_platforms_for_streamer(&pool, Some("nani"))
            .await
            .is_ok());
        sqlx::query("DROP TABLE social_media_platform_auth")
            .execute(&pool)
            .await
            .unwrap();
        assert!(get_active_platforms_for_streamer(&pool, Some("nani"))
            .await
            .is_err());
    }
}
