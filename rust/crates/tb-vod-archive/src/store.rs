//! Zustand des VOD-Archivs in Postgres.
//!
//! Der ganze Fortschritt liegt in der Datenbank, nicht neben den Dateien: nur
//! so ueberlebt ein Abbruch mitten im Upload einen Neustart, und nur so laesst
//! sich der Stand von aussen ansehen.
//!
//! Die Abfragen laufen bewusst ueber das Laufzeit-API von sqlx statt ueber die
//! Makros. Das ist im Repo die haeufigere Form und haelt neue Tabellen vom
//! Offline-Cache unabhaengig.

use chrono::NaiveDate;
use sqlx::{PgPool, Row};

use crate::error::VodArchiveError;

/// Status eines VOD. Als Konstanten statt Enum, weil der Wert so in der
/// Datenbank steht und dort auch von Hand lesbar sein soll.
pub const STATUS_NEU: &str = "new";
pub const STATUS_LAEDT: &str = "downloading";
pub const STATUS_GELADEN: &str = "downloaded";
pub const STATUS_HOCHGELADEN: &str = "uploaded";
pub const STATUS_DOWNLOAD_FEHLER: &str = "download_failed";
pub const STATUS_UPLOAD_FEHLER: &str = "upload_failed";
pub const STATUS_ARCHIVIERT: &str = "archived";

pub const TEIL_OFFEN: &str = "pending";
pub const TEIL_FERTIG: &str = "done";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vod {
    pub id: i64,
    pub twitch_id: String,
    pub title: String,
    pub duration_sec: i64,
    pub recorded_at: Option<NaiveDate>,
    pub status: String,
    pub local_path: Option<String>,
}

impl Vod {
    /// Braucht dieses VOD noch einen Download? Fehlgeschlagene Downloads
    /// zaehlen dazu, damit sie beim naechsten Lauf erneut versucht werden.
    pub fn braucht_download(&self) -> bool {
        matches!(
            self.status.as_str(),
            STATUS_NEU | STATUS_LAEDT | STATUS_DOWNLOAD_FEHLER
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Teil {
    pub id: i64,
    pub part_index: i32,
    pub file_path: String,
    pub status: String,
    pub upload_session_uri: Option<String>,
    pub upload_offset: i64,
    pub youtube_video_id: Option<String>,
}

/// Traegt ein neu entdecktes VOD ein. Bekannte VODs bleiben unberuehrt, damit
/// ein erneuter Lauf keinen Fortschritt ueberschreibt.
pub async fn merke_vod(
    pool: &PgPool,
    twitch_id: &str,
    channel: &str,
    title: &str,
    duration_sec: i64,
) -> Result<bool, VodArchiveError> {
    let eingefuegt = sqlx::query(
        "INSERT INTO twitch_vod_archive_vods (twitch_id, channel_login, title, duration_sec) \
         VALUES ($1, $2, $3, $4) ON CONFLICT (twitch_id) DO NOTHING",
    )
    .bind(twitch_id)
    .bind(channel)
    .bind(title)
    .bind(duration_sec)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(eingefuegt > 0)
}

/// Offene VODs, aeltestes zuerst. Fertige und aufgeraeumte bleiben draussen.
pub async fn offene_vods(
    pool: &PgPool,
    channel: &str,
    limit: i64,
) -> Result<Vec<Vod>, VodArchiveError> {
    let rows = sqlx::query(
        "SELECT id, twitch_id, title, duration_sec, recorded_at, status, local_path \
         FROM twitch_vod_archive_vods \
         WHERE channel_login = $1 AND status NOT IN ('uploaded', 'archived') \
         ORDER BY discovered_at ASC, id ASC LIMIT $2",
    )
    .bind(channel)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| Vod {
            id: row.get("id"),
            twitch_id: row.get("twitch_id"),
            title: row.get("title"),
            duration_sec: row.get("duration_sec"),
            recorded_at: row.get("recorded_at"),
            status: row.get("status"),
            local_path: row.get("local_path"),
        })
        .collect())
}

pub async fn setze_status(pool: &PgPool, id: i64, status: &str) -> Result<(), VodArchiveError> {
    sqlx::query(
        "UPDATE twitch_vod_archive_vods \
         SET status = $2, updated_at = CURRENT_TIMESTAMP WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .execute(pool)
    .await?;
    Ok(())
}

/// Haelt einen Fehler fest, ohne den Zustand zu verlieren. Der Text wird
/// gekuerzt, weil eine yt-dlp-Ausgabe sonst die Zeile sprengt.
pub async fn setze_fehler(
    pool: &PgPool,
    id: i64,
    status: &str,
    fehler: &str,
) -> Result<(), VodArchiveError> {
    let kurz: String = fehler.chars().take(1000).collect();
    sqlx::query(
        "UPDATE twitch_vod_archive_vods \
         SET status = $2, last_error = $3, updated_at = CURRENT_TIMESTAMP WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(kurz)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn setze_geladen(
    pool: &PgPool,
    id: i64,
    local_path: &str,
    recorded_at: Option<NaiveDate>,
    duration_sec: i64,
) -> Result<(), VodArchiveError> {
    sqlx::query(
        "UPDATE twitch_vod_archive_vods \
         SET status = 'downloaded', local_path = $2, recorded_at = COALESCE($3, recorded_at), \
             duration_sec = $4, downloaded_at = CURRENT_TIMESTAMP, last_error = NULL, \
             updated_at = CURRENT_TIMESTAMP \
         WHERE id = $1",
    )
    .bind(id)
    .bind(local_path)
    .bind(recorded_at)
    .bind(duration_sec)
    .execute(pool)
    .await?;
    Ok(())
}

/// Legt die Teile eines VOD an. Bereits hochgeladene Teile bleiben stehen,
/// damit ein wiederholter Download keinen fertigen Upload vergisst.
pub async fn setze_teile(
    pool: &PgPool,
    vod_id: i64,
    dateien: &[String],
) -> Result<(), VodArchiveError> {
    for (index, datei) in dateien.iter().enumerate() {
        let groesse = tokio::fs::metadata(datei)
            .await
            .map(|meta| meta.len() as i64)
            .unwrap_or(0);
        sqlx::query(
            "INSERT INTO twitch_vod_archive_parts (vod_id, part_index, file_path, size_bytes) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (vod_id, part_index) DO UPDATE \
             SET file_path = EXCLUDED.file_path, size_bytes = EXCLUDED.size_bytes, \
                 updated_at = CURRENT_TIMESTAMP \
             WHERE twitch_vod_archive_parts.status <> 'done'",
        )
        .bind(vod_id)
        .bind(index as i32)
        .bind(datei)
        .bind(groesse)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn teile(pool: &PgPool, vod_id: i64) -> Result<Vec<Teil>, VodArchiveError> {
    let rows = sqlx::query(
        "SELECT id, part_index, file_path, status, upload_session_uri, upload_offset, \
                youtube_video_id \
         FROM twitch_vod_archive_parts WHERE vod_id = $1 ORDER BY part_index ASC",
    )
    .bind(vod_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| Teil {
            id: row.get("id"),
            part_index: row.get("part_index"),
            file_path: row.get("file_path"),
            status: row.get("status"),
            upload_session_uri: row.get("upload_session_uri"),
            upload_offset: row.get("upload_offset"),
            youtube_video_id: row.get("youtube_video_id"),
        })
        .collect())
}

/// Haelt die frisch begonnene Upload-Sitzung fest. Ohne diesen Schritt faengt
/// ein Abbruch beim naechsten Lauf wieder bei null an.
pub async fn setze_teil_sitzung(
    pool: &PgPool,
    teil_id: i64,
    session_uri: &str,
    offset: i64,
) -> Result<(), VodArchiveError> {
    sqlx::query(
        "UPDATE twitch_vod_archive_parts \
         SET upload_session_uri = $2, upload_offset = $3, status = 'uploading', \
             updated_at = CURRENT_TIMESTAMP \
         WHERE id = $1",
    )
    .bind(teil_id)
    .bind(session_uri)
    .bind(offset)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn setze_teil_offset(
    pool: &PgPool,
    teil_id: i64,
    offset: i64,
) -> Result<(), VodArchiveError> {
    sqlx::query(
        "UPDATE twitch_vod_archive_parts \
         SET upload_offset = $2, updated_at = CURRENT_TIMESTAMP WHERE id = $1",
    )
    .bind(teil_id)
    .bind(offset)
    .execute(pool)
    .await?;
    Ok(())
}

/// Wirft eine verfallene Sitzung weg, damit der naechste Versuch eine neue
/// beginnt statt gegen eine tote URL zu laufen.
pub async fn loesche_teil_sitzung(pool: &PgPool, teil_id: i64) -> Result<(), VodArchiveError> {
    sqlx::query(
        "UPDATE twitch_vod_archive_parts \
         SET upload_session_uri = NULL, upload_offset = 0, updated_at = CURRENT_TIMESTAMP \
         WHERE id = $1",
    )
    .bind(teil_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn setze_teil_fertig(
    pool: &PgPool,
    teil_id: i64,
    youtube_video_id: &str,
) -> Result<(), VodArchiveError> {
    sqlx::query(
        "UPDATE twitch_vod_archive_parts \
         SET status = 'done', youtube_video_id = $2, last_error = NULL, \
             updated_at = CURRENT_TIMESTAMP \
         WHERE id = $1",
    )
    .bind(teil_id)
    .bind(youtube_video_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn setze_teil_fehler(
    pool: &PgPool,
    teil_id: i64,
    fehler: &str,
) -> Result<(), VodArchiveError> {
    let kurz: String = fehler.chars().take(1000).collect();
    sqlx::query(
        "UPDATE twitch_vod_archive_parts \
         SET status = 'failed', last_error = $2, updated_at = CURRENT_TIMESTAMP WHERE id = $1",
    )
    .bind(teil_id)
    .bind(kurz)
    .execute(pool)
    .await?;
    Ok(())
}

/// Markiert das VOD als vollstaendig hochgeladen.
pub async fn setze_hochgeladen(pool: &PgPool, id: i64) -> Result<(), VodArchiveError> {
    sqlx::query(
        "UPDATE twitch_vod_archive_vods \
         SET status = 'uploaded', uploaded_at = CURRENT_TIMESTAMP, last_error = NULL, \
             updated_at = CURRENT_TIMESTAMP \
         WHERE id = $1",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// VODs, deren lokale Dateien nach der eingestellten Frist wegduerfen. Nur
/// vollstaendig hochgeladene kommen infrage, sonst waere das Archiv weg,
/// bevor die Kopie steht.
pub async fn abgelaufen_lokal(
    pool: &PgPool,
    tage: i64,
) -> Result<Vec<(i64, String)>, VodArchiveError> {
    let rows = sqlx::query(
        "SELECT id, twitch_id FROM twitch_vod_archive_vods \
         WHERE status = 'uploaded' \
           AND uploaded_at IS NOT NULL \
           AND uploaded_at < CURRENT_TIMESTAMP - make_interval(days => $1::int)",
    )
    .bind(tage as i32)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.get("id"), row.get("twitch_id")))
        .collect())
}

pub async fn markiere_archiviert(pool: &PgPool, id: i64) -> Result<(), VodArchiveError> {
    sqlx::query(
        "UPDATE twitch_vod_archive_vods \
         SET status = 'archived', local_path = NULL, updated_at = CURRENT_TIMESTAMP WHERE id = $1",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    /// Wegwerf-Schema. Ohne TB_TEST_DATABASE_URL ueberspringen die Tests still,
    /// wie im uebrigen Workspace.
    async fn pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .ok()?;
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
            "CREATE TABLE twitch_vod_archive_vods (id BIGSERIAL PRIMARY KEY, twitch_id TEXT NOT NULL UNIQUE, \
             channel_login TEXT NOT NULL, title TEXT NOT NULL, duration_sec BIGINT NOT NULL DEFAULT 0, \
             recorded_at DATE, status TEXT NOT NULL DEFAULT 'new', local_path TEXT, last_error TEXT, \
             discovered_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP, downloaded_at TIMESTAMPTZ, \
             uploaded_at TIMESTAMPTZ, updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP)",
            "CREATE TABLE twitch_vod_archive_parts (id BIGSERIAL PRIMARY KEY, vod_id BIGINT NOT NULL \
             REFERENCES twitch_vod_archive_vods (id) ON DELETE CASCADE, part_index INTEGER NOT NULL, \
             file_path TEXT NOT NULL, size_bytes BIGINT NOT NULL DEFAULT 0, status TEXT NOT NULL DEFAULT 'pending', \
             upload_session_uri TEXT, upload_offset BIGINT NOT NULL DEFAULT 0, youtube_video_id TEXT, \
             last_error TEXT, updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP, \
             UNIQUE (vod_id, part_index))",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn entdecken_ist_wiederholbar() {
        let Some(pool) = pool("t_vod_entdecken").await else {
            return;
        };
        assert!(merke_vod(&pool, "v1", "earlysalty", "Erster", 100)
            .await
            .unwrap());
        // Zweiter Lauf sieht dasselbe VOD und darf nichts anfassen.
        assert!(!merke_vod(&pool, "v1", "earlysalty", "Anderer Titel", 999)
            .await
            .unwrap());
        let offen = offene_vods(&pool, "earlysalty", 10).await.unwrap();
        assert_eq!(offen.len(), 1);
        assert_eq!(offen[0].title, "Erster");
        assert!(offen[0].braucht_download());
    }

    #[tokio::test]
    async fn upload_stand_ueberlebt_und_fertige_teile_bleiben() {
        let Some(pool) = pool("t_vod_upload").await else {
            return;
        };
        merke_vod(&pool, "v2", "earlysalty", "Langer Stream", 50_000)
            .await
            .unwrap();
        let vod = offene_vods(&pool, "earlysalty", 10).await.unwrap()[0].clone();
        setze_geladen(
            &pool,
            vod.id,
            "/archiv/v2.mp4",
            NaiveDate::from_ymd_opt(2026, 8, 13),
            50_000,
        )
        .await
        .unwrap();
        setze_teile(
            &pool,
            vod.id,
            &[
                "/archiv/v2.part000.mp4".into(),
                "/archiv/v2.part001.mp4".into(),
            ],
        )
        .await
        .unwrap();

        let teile_liste = teile(&pool, vod.id).await.unwrap();
        assert_eq!(teile_liste.len(), 2);

        // Erster Teil fertig, zweiter mitten im Upload.
        setze_teil_fertig(&pool, teile_liste[0].id, "yt-a")
            .await
            .unwrap();
        setze_teil_sitzung(&pool, teile_liste[1].id, "https://sitzung/2", 0)
            .await
            .unwrap();
        setze_teil_offset(&pool, teile_liste[1].id, 33_554_432)
            .await
            .unwrap();

        // Ein erneuter Download darf den fertigen Teil nicht zuruecksetzen.
        setze_teile(
            &pool,
            vod.id,
            &["/neu/v2.part000.mp4".into(), "/neu/v2.part001.mp4".into()],
        )
        .await
        .unwrap();

        let nachher = teile(&pool, vod.id).await.unwrap();
        assert_eq!(nachher[0].status, TEIL_FERTIG);
        assert_eq!(nachher[0].file_path, "/archiv/v2.part000.mp4");
        assert_eq!(nachher[0].youtube_video_id.as_deref(), Some("yt-a"));
        assert_eq!(nachher[1].file_path, "/neu/v2.part001.mp4");
        assert_eq!(nachher[1].upload_offset, 33_554_432);
        assert_eq!(
            nachher[1].upload_session_uri.as_deref(),
            Some("https://sitzung/2")
        );

        // Verfallene Sitzung wegwerfen setzt den Stand zurueck.
        loesche_teil_sitzung(&pool, nachher[1].id).await.unwrap();
        let danach = teile(&pool, vod.id).await.unwrap();
        assert!(danach[1].upload_session_uri.is_none());
        assert_eq!(danach[1].upload_offset, 0);

        setze_hochgeladen(&pool, vod.id).await.unwrap();
        assert!(offene_vods(&pool, "earlysalty", 10)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn fehler_behaelt_das_vod_in_der_warteschlange() {
        let Some(pool) = pool("t_vod_fehler").await else {
            return;
        };
        merke_vod(&pool, "v3", "earlysalty", "Kaputt", 10)
            .await
            .unwrap();
        let vod = offene_vods(&pool, "earlysalty", 10).await.unwrap()[0].clone();
        setze_fehler(&pool, vod.id, STATUS_DOWNLOAD_FEHLER, &"y".repeat(5000))
            .await
            .unwrap();
        let offen = offene_vods(&pool, "earlysalty", 10).await.unwrap();
        assert_eq!(offen.len(), 1);
        assert!(offen[0].braucht_download());
    }

    #[tokio::test]
    async fn nur_hochgeladene_vods_werden_aufgeraeumt() {
        let Some(pool) = pool("t_vod_aufraeumen").await else {
            return;
        };
        merke_vod(&pool, "v4", "earlysalty", "Alt", 10)
            .await
            .unwrap();
        merke_vod(&pool, "v5", "earlysalty", "Neu", 10)
            .await
            .unwrap();
        let alle = offene_vods(&pool, "earlysalty", 10).await.unwrap();
        setze_hochgeladen(&pool, alle[0].id).await.unwrap();
        sqlx::query(
            "UPDATE twitch_vod_archive_vods SET uploaded_at = CURRENT_TIMESTAMP - INTERVAL '40 days' WHERE id = $1",
        )
        .bind(alle[0].id)
        .execute(&pool)
        .await
        .unwrap();

        // Das noch nicht hochgeladene VOD bleibt unangetastet.
        let faellig = abgelaufen_lokal(&pool, 30).await.unwrap();
        assert_eq!(faellig.len(), 1);
        assert_eq!(faellig[0].1, "v4");

        markiere_archiviert(&pool, faellig[0].0).await.unwrap();
        assert!(abgelaufen_lokal(&pool, 30).await.unwrap().is_empty());
    }
}
