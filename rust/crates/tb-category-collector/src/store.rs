//! DB-Schicht des Kategoriesammlers: Polls, Snapshots, Kanalstammdaten,
//! Chat-Batches, zusätzliche Rollups und Status. Alle Zeiten UTC (TIMESTAMPTZ).
//!
//! Grundregeln dieses Moduls:
//! - Ein unvollständiger Poll schreibt **keine** Snapshot-/Roster-Zeilen.
//! - Chat-Inserts laufen als Batches mit `ON CONFLICT DO NOTHING`
//!   (Idempotenz über `(room_user_id, message_id)`).
//! - Rollups ergänzen den unverändert erhaltenen Rohchat. Keine Alterslöschung.

use std::collections::HashSet;

#[cfg(test)]
use chrono::Timelike;
use chrono::{DateTime, TimeZone, Utc};
use sqlx::PgPool;

/// Ein beobachteter Live-Stream aus einem Discovery-Durchlauf.
#[derive(Debug, Clone)]
pub struct SnapshotZeile {
    pub stream_id: String,
    pub user_id: String,
    pub user_login: String,
    pub viewer_count: i32,
    pub title: Option<String>,
    pub language: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub tags: Option<Vec<String>>,
    pub is_mature: bool,
}

/// Eine geparste, spracherkannte Chatzeile für den Batch-Insert.
#[derive(Debug, Clone)]
pub struct ChatZeile {
    pub room_user_id: String,
    pub message_id: String,
    pub source_message_id: Option<String>,
    pub sent_at: DateTime<Utc>,
    pub chatter_user_id: String,
    pub chatter_login: String,
    pub message_text: String,
    pub text_len: i32,
    pub detected_lang: Option<String>,
    pub lang_confidence: Option<f32>,
    pub lang_method: String,
    pub emote_count: i32,
}

/// Statuszeile für das Admin-Panel (Gesundheit, Drops, letzte Läufe).
#[derive(Debug, Default, Clone)]
pub struct StatusUpdate {
    pub last_poll_started_at: Option<DateTime<Utc>>,
    pub last_complete_poll_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub last_error_at: Option<DateTime<Utc>>,
    pub roster_size: i32,
    pub chat_privmsgs_dispatched: u64,
    pub chat_privmsgs_dropped: u64,
    pub chat_queue_dropped: u64,
    pub chat_commands_dropped: u64,
    pub chat_invalid_logins_rejected: u64,
    pub chat_reconnects: u64,
    pub chat_duplicates_skipped: u64,
    pub chat_redactions: u64,
    pub retention_deleted_total: u64,
    pub last_retention_run_at: Option<DateTime<Utc>>,
    pub last_rollup_hour: Option<DateTime<Utc>>,
}

/// Öffnet einen Poll-Eintrag (status `running`) und liefert die ID.
pub async fn start_poll(pool: &PgPool, started_at: DateTime<Utc>) -> Result<i64, sqlx::Error> {
    let id: (i64,) =
        sqlx::query_as("INSERT INTO category_polls (started_at) VALUES ($1) RETURNING poll_id")
            .bind(started_at)
            .fetch_one(pool)
            .await?;
    Ok(id.0)
}

/// Schließt einen Poll-Eintrag ab (Status, Zähler, Fehlerdetail).
pub async fn beende_poll(
    pool: &PgPool,
    poll_id: i64,
    status: &str,
    stream_count: i32,
    viewer_total: i64,
    page_count: i32,
    error_detail: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE category_polls \
            SET status = $2, completed_at = NOW(), stream_count = $3, \
                viewer_total = $4, page_count = $5, error_detail = $6 \
          WHERE poll_id = $1",
    )
    .bind(poll_id)
    .bind(status)
    .bind(stream_count)
    .bind(viewer_total)
    .bind(page_count)
    .bind(error_detail)
    .execute(pool)
    .await?;
    Ok(())
}

/// Schreibt die Snapshot-Zeilen eines **vollständigen** Polls (Batch via UNNEST).
pub async fn schreibe_snapshots(
    pool: &PgPool,
    poll_id: i64,
    snapshot_at: DateTime<Utc>,
    zeilen: &[SnapshotZeile],
) -> Result<(), sqlx::Error> {
    if zeilen.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO category_stream_snapshots \
            (poll_id, snapshot_at, stream_id, user_id, user_login, viewer_count, \
             title, language, started_at, tags, is_mature) \
         SELECT $1, $2, u.stream_id, u.user_id, u.user_login, u.viewer_count, \
                u.title, u.language, u.started_at, \
                CASE WHEN u.tags_json IS NULL OR u.tags_json = '' \
                     THEN NULL ELSE u.tags_json::jsonb END, \
                u.is_mature \
           FROM UNNEST($3::text[], $4::text[], $5::text[], $6::int[], $7::text[], \
                       $8::text[], $9::timestamptz[], $10::text[], $11::bool[]) \
             AS u(stream_id, user_id, user_login, viewer_count, title, language, \
                  started_at, tags_json, is_mature)",
    )
    .bind(poll_id)
    .bind(snapshot_at)
    .bind(
        zeilen
            .iter()
            .map(|z| z.stream_id.clone())
            .collect::<Vec<_>>(),
    )
    .bind(zeilen.iter().map(|z| z.user_id.clone()).collect::<Vec<_>>())
    .bind(
        zeilen
            .iter()
            .map(|z| z.user_login.clone())
            .collect::<Vec<_>>(),
    )
    .bind(zeilen.iter().map(|z| z.viewer_count).collect::<Vec<_>>())
    .bind(zeilen.iter().map(|z| z.title.clone()).collect::<Vec<_>>())
    .bind(
        zeilen
            .iter()
            .map(|z| z.language.clone())
            .collect::<Vec<_>>(),
    )
    .bind(zeilen.iter().map(|z| z.started_at).collect::<Vec<_>>())
    .bind(
        zeilen
            .iter()
            .map(|z| {
                z.tags
                    .as_ref()
                    .map(|tags| serde_json::to_string(tags).unwrap_or_default())
            })
            .collect::<Vec<_>>(),
    )
    .bind(zeilen.iter().map(|z| z.is_mature).collect::<Vec<_>>())
    .execute(pool)
    .await?;
    Ok(())
}

/// Aktualisiert Kanalstammdaten + `last_seen_live_at` aus einem Poll-Ergebnis.
pub async fn upsert_kanaele(
    pool: &PgPool,
    zeilen: &[SnapshotZeile],
    gesehen_am: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    if zeilen.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO category_channels (user_id, login, display_name, last_seen_live_at) \
         SELECT * FROM UNNEST($1::text[], $2::text[], $3::text[], $4::timestamptz[]) \
         ON CONFLICT (user_id) DO UPDATE SET \
             login = EXCLUDED.login, \
             display_name = EXCLUDED.display_name, \
             last_seen_live_at = GREATEST(category_channels.last_seen_live_at, \
                                          EXCLUDED.last_seen_live_at)",
    )
    .bind(zeilen.iter().map(|z| z.user_id.clone()).collect::<Vec<_>>())
    .bind(
        zeilen
            .iter()
            .map(|z| z.user_login.clone())
            .collect::<Vec<_>>(),
    )
    .bind(
        zeilen
            .iter()
            .map(|z| z.user_login.clone())
            .collect::<Vec<_>>(),
    )
    .bind(
        std::iter::repeat(gesehen_am)
            .take(zeilen.len())
            .collect::<Vec<_>>(),
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Ergänzt fehlende Profil-Stammdaten (Kontalter, Typ, Beschreibung, Sprache).
pub async fn ergaenze_profile(
    pool: &PgPool,
    user_ids: &[String],
    profile: &std::collections::HashMap<String, tb_transport_twitch::streams::HelixUser>,
    kanalsprache: &std::collections::HashMap<String, String>,
) -> Result<(), sqlx::Error> {
    for user_id in user_ids {
        let Some(user) = profile.get(user_id) else {
            continue;
        };
        let created = parse_rfc3339(&user.created_at);
        let sprache = kanalsprache.get(user_id).map(|s| s.trim().to_string());
        sqlx::query(
            "UPDATE category_channels SET \
                display_name = COALESCE(NULLIF($2, ''), display_name), \
                description = COALESCE(NULLIF($3, ''), description), \
                broadcaster_type = COALESCE(NULLIF($4, ''), broadcaster_type), \
                broadcaster_language = COALESCE(NULLIF($5, ''), broadcaster_language), \
                profile_created_at = COALESCE($6, profile_created_at) \
              WHERE user_id = $1",
        )
        .bind(user_id)
        .bind(user.display_name.trim())
        .bind(user.description.trim())
        .bind(user.broadcaster_type.trim())
        .bind(sprache)
        .bind(created)
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Logins der Kanäle, die aktuell (im Fenster `seit`) als Deadlock live
/// beobachtet wurden — die Chat-Roster-Basis.
pub async fn beobachtete_kanaele(
    pool: &PgPool,
    seit: DateTime<Utc>,
) -> Result<Vec<(String, String)>, sqlx::Error> {
    let zeilen: Vec<(String, String)> = sqlx::query_as(
        "SELECT user_id, login FROM category_channels WHERE last_seen_live_at >= $1",
    )
    .bind(seit)
    .fetch_all(pool)
    .await?;
    Ok(zeilen)
}

/// Batch-Insert für Chatzeilen; Duplikate (nach `(room_user_id, message_id)`)
/// werden still übersprungen und geliefert, damit der Zähler stimmt.
pub async fn schreibe_chat_batch(
    pool: &PgPool,
    zeilen: &[ChatZeile],
) -> Result<(u64, u64), sqlx::Error> {
    if zeilen.is_empty() {
        return Ok((0, 0));
    }
    let ergebnis = sqlx::query(
        "INSERT INTO category_chat_messages \
            (room_user_id, message_id, source_message_id, sent_at, chatter_user_id, \
             chatter_login, message_text, text_len, detected_lang, lang_confidence, \
             lang_method, emote_count) \
         SELECT * FROM UNNEST($1::text[], $2::text[], $3::text[], $4::timestamptz[], \
                $5::text[], $6::text[], $7::text[], $8::int[], $9::text[], $10::real[], \
                $11::text[], $12::int[]) \
         ON CONFLICT (room_user_id, message_id) DO NOTHING",
    )
    .bind(
        zeilen
            .iter()
            .map(|z| z.room_user_id.clone())
            .collect::<Vec<_>>(),
    )
    .bind(
        zeilen
            .iter()
            .map(|z| z.message_id.clone())
            .collect::<Vec<_>>(),
    )
    .bind(
        zeilen
            .iter()
            .map(|z| z.source_message_id.clone())
            .collect::<Vec<_>>(),
    )
    .bind(zeilen.iter().map(|z| z.sent_at).collect::<Vec<_>>())
    .bind(
        zeilen
            .iter()
            .map(|z| z.chatter_user_id.clone())
            .collect::<Vec<_>>(),
    )
    .bind(
        zeilen
            .iter()
            .map(|z| z.chatter_login.clone())
            .collect::<Vec<_>>(),
    )
    .bind(
        zeilen
            .iter()
            .map(|z| z.message_text.clone())
            .collect::<Vec<_>>(),
    )
    .bind(zeilen.iter().map(|z| z.text_len).collect::<Vec<_>>())
    .bind(
        zeilen
            .iter()
            .map(|z| z.detected_lang.clone())
            .collect::<Vec<_>>(),
    )
    .bind(zeilen.iter().map(|z| z.lang_confidence).collect::<Vec<_>>())
    .bind(
        zeilen
            .iter()
            .map(|z| z.lang_method.clone())
            .collect::<Vec<_>>(),
    )
    .bind(zeilen.iter().map(|z| z.emote_count).collect::<Vec<_>>())
    .execute(pool)
    .await?;
    let eingefuegt = ergebnis.rows_affected();
    Ok((eingefuegt, zeilen.len() as u64 - eingefuegt))
}

/// Berechnet eine Stunde idempotent neu (nur originale Aktivität: Shared-Chat
/// via `source_message_id` zählt nicht). distinct_chatters gilt ausschließlich
/// innerhalb genau dieser Stunde + Kanal + Sprache.
pub async fn rollup_stunde(pool: &PgPool, stunde: DateTime<Utc>) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO category_chat_rollup \
            (hour_bucket, room_user_id, lang, messages, distinct_chatters, total_len, \
             avg_len, computed_at) \
         SELECT $1, room_user_id, detected_lang, COUNT(*), \
                COUNT(DISTINCT chatter_user_id), SUM(text_len::BIGINT), \
                (SUM(text_len::BIGINT)::REAL / NULLIF(COUNT(*), 0)), NOW() \
           FROM category_chat_messages \
          WHERE sent_at >= $1 AND sent_at < $1 + INTERVAL '1 hour' \
            AND source_message_id IS NULL \
          GROUP BY room_user_id, detected_lang \
         ON CONFLICT (hour_bucket, room_user_id, lang) DO UPDATE SET \
             messages = EXCLUDED.messages, \
             distinct_chatters = EXCLUDED.distinct_chatters, \
             total_len = EXCLUDED.total_len, \
             avg_len = EXCLUDED.avg_len, \
             computed_at = NOW()",
    )
    .bind(stunde)
    .execute(pool)
    .await?;
    Ok(())
}

/// Letzte gesicherte Rollup-Stunde aus dem Status (None = noch nie gelaufen).
pub async fn letzte_rollup_stunde(pool: &PgPool) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
    let zeile: Option<(Option<DateTime<Utc>>,)> =
        sqlx::query_as("SELECT last_rollup_hour FROM category_collector_status WHERE id = 1")
            .fetch_optional(pool)
            .await?;
    Ok(zeile.and_then(|(wert,)| wert))
}

/// Früheste Chat-Stunde (Start des Catch-ups bei Erstlauf).
pub async fn frueheste_chat_stunde(pool: &PgPool) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
    let zeile: Option<(Option<DateTime<Utc>>,)> =
        sqlx::query_as("SELECT date_trunc('hour', MIN(sent_at)) FROM category_chat_messages")
            .fetch_optional(pool)
            .await?;
    Ok(zeile.and_then(|(wert,)| wert))
}

/// Historische Schnittstelle: absichtlich wirkungslos. Auch ein alter Aufrufer
/// darf nach der Nutzerentscheidung keine archivierten Nachrichten löschen.
pub async fn loesche_chat_batch(
    _pool: &PgPool,
    _retention_grenze: DateTime<Utc>,
    _rollup_grenze: DateTime<Utc>,
    _batch_groesse: i64,
) -> Result<u64, sqlx::Error> {
    Ok(0)
}

/// Historische Schnittstelle: Snapshots bleiben unabhängig von ihrem Alter.
pub async fn loesche_snapshots_batch(
    _pool: &PgPool,
    _grenze: DateTime<Utc>,
    _batch_groesse: i64,
) -> Result<u64, sqlx::Error> {
    Ok(0)
}

/// Persistiert die Statuszeile (Upsert auf id = 1). Alle Felder werden
/// ersetzt; die kumulierten Zähler stammen aus dem shared Status des Dienstes.
pub async fn schreibe_status(pool: &PgPool, update: &StatusUpdate) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO category_collector_status \
            (id, last_poll_started_at, last_complete_poll_at, last_error, last_error_at, \
             roster_size, chat_privmsgs_dispatched, chat_privmsgs_dropped, \
             chat_queue_dropped, chat_commands_dropped, chat_invalid_logins_rejected, \
             chat_reconnects, chat_duplicates_skipped, chat_redactions, \
             retention_deleted_total, last_retention_run_at, last_rollup_hour, updated_at) \
         VALUES (1, $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, NOW()) \
         ON CONFLICT (id) DO UPDATE SET \
             last_poll_started_at = EXCLUDED.last_poll_started_at, \
             last_complete_poll_at = EXCLUDED.last_complete_poll_at, \
             last_error = EXCLUDED.last_error, \
             last_error_at = EXCLUDED.last_error_at, \
             roster_size = EXCLUDED.roster_size, \
             chat_privmsgs_dispatched = EXCLUDED.chat_privmsgs_dispatched, \
             chat_privmsgs_dropped = EXCLUDED.chat_privmsgs_dropped, \
             chat_queue_dropped = EXCLUDED.chat_queue_dropped, \
             chat_commands_dropped = EXCLUDED.chat_commands_dropped, \
             chat_invalid_logins_rejected = EXCLUDED.chat_invalid_logins_rejected, \
             chat_reconnects = EXCLUDED.chat_reconnects, \
             chat_duplicates_skipped = EXCLUDED.chat_duplicates_skipped, \
             chat_redactions = EXCLUDED.chat_redactions, \
             retention_deleted_total = EXCLUDED.retention_deleted_total, \
             last_retention_run_at = EXCLUDED.last_retention_run_at, \
             last_rollup_hour = COALESCE(EXCLUDED.last_rollup_hour, \
                                         category_collector_status.last_rollup_hour), \
             updated_at = NOW()",
    )
    .bind(update.last_poll_started_at)
    .bind(update.last_complete_poll_at)
    .bind(update.last_error.clone())
    .bind(update.last_error_at)
    .bind(update.roster_size)
    .bind(update.chat_privmsgs_dispatched as i64)
    .bind(update.chat_privmsgs_dropped as i64)
    .bind(update.chat_queue_dropped as i64)
    .bind(update.chat_commands_dropped as i64)
    .bind(update.chat_invalid_logins_rejected as i64)
    .bind(update.chat_reconnects as i64)
    .bind(update.chat_duplicates_skipped as i64)
    .bind(update.chat_redactions as i64)
    .bind(update.retention_deleted_total as i64)
    .bind(update.last_retention_run_at)
    .bind(update.last_rollup_hour)
    .execute(pool)
    .await?;
    Ok(())
}

/// Nach Dienstneustart hängengebliebene `running`-Polls als fehlgeschlagen
/// schließen (ehrliche Lücken, keine Phantom-Beobachtung).
pub async fn schliesse_haengende_polls(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE category_polls \
            SET status = 'failed', completed_at = NOW(), \
                error_detail = 'Dienstneustart waehrend des Polls' \
          WHERE status = 'running'",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Sichert VOD-Metadaten eines Kanals (nur Helix-Listenfelder, idempotent).
pub async fn speichere_vods(
    pool: &PgPool,
    user_id: &str,
    vods: &[tb_transport_twitch::streams::ArchiveVideo],
) -> Result<(), sqlx::Error> {
    if vods.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO category_channel_vods (vod_id, user_id, created_at, duration, fetched_at) \
         SELECT * FROM UNNEST($1::text[], $2::text[], $3::timestamptz[], $4::text[], \
                $5::timestamptz[]) \
         ON CONFLICT (vod_id) DO NOTHING",
    )
    .bind(vods.iter().map(|v| v.id.clone()).collect::<Vec<_>>())
    .bind(
        std::iter::repeat(user_id.to_string())
            .take(vods.len())
            .collect::<Vec<_>>(),
    )
    .bind(
        vods.iter()
            .map(|v| parse_rfc3339(&v.created_at))
            .collect::<Vec<_>>(),
    )
    .bind(vods.iter().map(|v| v.duration.clone()).collect::<Vec<_>>())
    .bind(
        std::iter::repeat(Utc::now())
            .take(vods.len())
            .collect::<Vec<_>>(),
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Redigiert eine einzelne Nachricht (CLEARMSG): Rohtext und Sprachableitung
/// entfernen, Zeile bleibt als Zählung erhalten (Aggregat-Ehrlichkeit).
pub async fn redigiere_nachricht(
    pool: &PgPool,
    room_user_id: &str,
    message_id: &str,
) -> Result<u64, sqlx::Error> {
    let ergebnis = sqlx::query(
        "UPDATE category_chat_messages SET \
             message_text = '', text_len = 0, detected_lang = NULL, \
             lang_confidence = NULL, lang_method = 'redigiert' \
           WHERE room_user_id = $1 AND message_id = $2 AND message_text <> ''",
    )
    .bind(room_user_id)
    .bind(message_id)
    .execute(pool)
    .await?;
    Ok(ergebnis.rows_affected())
}

/// Redigiert alle Nachrichten eines Chatters im Raum (CLEARCHAT mit Ziel).
pub async fn redigiere_chatter(
    pool: &PgPool,
    room_user_id: &str,
    chatter_user_id: &str,
) -> Result<u64, sqlx::Error> {
    let ergebnis = sqlx::query(
        "UPDATE category_chat_messages SET \
             message_text = '', text_len = 0, detected_lang = NULL, \
             lang_confidence = NULL, lang_method = 'redigiert' \
           WHERE room_user_id = $1 AND chatter_user_id = $2 AND message_text <> ''",
    )
    .bind(room_user_id)
    .bind(chatter_user_id)
    .execute(pool)
    .await?;
    Ok(ergebnis.rows_affected())
}

/// Ein allgemeines CLEARCHAT ist kein Auftrag, das historische Kanalarchiv
/// zu vernichten. Gezielte Nachricht-/Personen-Redaktion bleibt getrennt.
pub async fn redigiere_raum(_pool: &PgPool, _room_user_id: &str) -> Result<u64, sqlx::Error> {
    Ok(0)
}

/// user_ids ohne bisheriges Profil (für die sparsame Enrichment-Batches).
pub async fn kanaele_ohne_profil(pool: &PgPool, limit: i64) -> Result<Vec<String>, sqlx::Error> {
    let zeilen: Vec<(String,)> = sqlx::query_as(
        "SELECT user_id FROM category_channels \
          WHERE profile_created_at IS NULL AND broadcaster_type IS NULL \
          ORDER BY last_seen_live_at DESC NULLS LAST LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(zeilen.into_iter().map(|(id,)| id).collect())
}

/// Observed-Roster als Set (login), für die Chat-Zuordnung.
pub async fn beobachtete_logins(
    pool: &PgPool,
    seit: DateTime<Utc>,
) -> Result<HashSet<String>, sqlx::Error> {
    Ok(beobachtete_kanaele(pool, seit)
        .await?
        .into_iter()
        .map(|(_, login)| login)
        .collect())
}

fn parse_rfc3339(wert: &str) -> Option<DateTime<Utc>> {
    let wert = wert.trim();
    if wert.is_empty() {
        return None;
    }
    DateTime::parse_from_rfc3339(wert)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn database() -> crate::test_postgres::TestPostgres {
        let database = crate::test_postgres::TestPostgres::start().await;
        sqlx::raw_sql(include_str!(
            "../../../migrations/20260918123000_category_collector.sql"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        sqlx::raw_sql(include_str!(
            "../../../migrations/20260918161000_category_preserve_data.sql"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        database
    }

    fn zeile(stream_id: &str, user_id: &str, login: &str, viewer: i32) -> SnapshotZeile {
        SnapshotZeile {
            stream_id: stream_id.to_string(),
            user_id: user_id.to_string(),
            user_login: login.to_string(),
            viewer_count: viewer,
            title: Some("Ranked Push".to_string()),
            language: Some("de".to_string()),
            started_at: Utc::now().checked_sub_signed(chrono::Duration::hours(1)),
            tags: Some(vec!["Deutsch".to_string()]),
            is_mature: false,
        }
    }

    #[tokio::test]
    async fn vollstaendiger_poll_schreibt_snapshots_und_kanaele() {
        let database = database().await;
        let pool = database.pool.clone();
        let poll_id = start_poll(&pool, Utc::now()).await.unwrap();
        let zeilen = vec![
            zeile("s1", "u1", "kanal_a", 10),
            zeile("s2", "u2", "kanal_b", 20),
        ];
        schreibe_snapshots(&pool, poll_id, Utc::now(), &zeilen)
            .await
            .unwrap();
        upsert_kanaele(&pool, &zeilen, Utc::now()).await.unwrap();
        beende_poll(&pool, poll_id, "complete", 2, 30, 1, None)
            .await
            .unwrap();

        let (count, viewer): (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), COALESCE(SUM(viewer_count), 0) FROM category_stream_snapshots",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!((count, viewer), (2, 30));
        let (logins, status): (i64, String) = sqlx::query_as(
            "SELECT (SELECT COUNT(*) FROM category_channels), \
                    (SELECT status FROM category_polls WHERE poll_id = $1)",
        )
        .bind(poll_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!((logins, status), (2, "complete".to_string()));
    }

    #[tokio::test]
    async fn chat_batch_ist_idempotent_und_zaehlt_duplikate() {
        let database = database().await;
        let pool = database.pool.clone();
        let base = Utc::now();
        let z = |message_id: &str| ChatZeile {
            room_user_id: "99".to_string(),
            message_id: message_id.to_string(),
            source_message_id: None,
            sent_at: base,
            chatter_user_id: "42".to_string(),
            chatter_login: "viewer".to_string(),
            message_text: "hallo welt".to_string(),
            text_len: 11,
            detected_lang: Some("de".to_string()),
            lang_confidence: Some(0.9),
            lang_method: "ok".to_string(),
            emote_count: 0,
        };
        let (eingefuegt, dup1) = schreibe_chat_batch(&pool, &[z("m1"), z("m2")])
            .await
            .unwrap();
        assert_eq!((eingefuegt, dup1), (2, 0));
        let (eingefuegt2, dup2) = schreibe_chat_batch(&pool, &[z("m2"), z("m3")])
            .await
            .unwrap();
        assert_eq!((eingefuegt2, dup2), (1, 1), "Duplikat übersprungen");
    }

    #[tokio::test]
    async fn rollup_ist_idempotent_und_ignoriert_shared_chat() {
        let database = database().await;
        let pool = database.pool.clone();
        let stunde = Utc::now()
            .checked_sub_signed(chrono::Duration::hours(2))
            .unwrap();
        let stunde = Utc.from_utc_datetime(
            &stunde
                .date_naive()
                .and_hms_opt(stunde.time().hour(), 0, 0)
                .unwrap(),
        );
        let mk = |message_id: &str, source: Option<&str>, lang: Option<&str>| ChatZeile {
            room_user_id: "99".to_string(),
            message_id: message_id.to_string(),
            source_message_id: source.map(str::to_string),
            sent_at: stunde + chrono::Duration::minutes(5),
            chatter_user_id: format!("chatter-{message_id}"),
            chatter_login: format!("user-{message_id}"),
            message_text: "ein kurzer text".to_string(),
            text_len: 16,
            detected_lang: lang.map(str::to_string),
            lang_confidence: Some(0.9),
            lang_method: "ok".to_string(),
            emote_count: 0,
        };
        schreibe_chat_batch(
            &pool,
            &[
                mk("a1", None, Some("de")),
                mk("a2", None, Some("de")),
                mk("a3", Some("shared-1"), Some("de")), // Shared-Chat
                mk("a4", None, Some("en")),
            ],
        )
        .await
        .unwrap();

        rollup_stunde(&pool, stunde).await.unwrap();
        // Erneut laufen lassen: identisches Ergebnis, keine Doppelzählung.
        rollup_stunde(&pool, stunde).await.unwrap();

        let zeilen: Vec<(String, Option<String>, i64, i64)> = sqlx::query_as(
            "SELECT room_user_id, lang, messages, distinct_chatters \
               FROM category_chat_rollup WHERE hour_bucket = $1 ORDER BY lang NULLS LAST",
        )
        .bind(stunde)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            zeilen,
            vec![
                ("99".to_string(), Some("de".to_string()), 2, 2),
                ("99".to_string(), Some("en".to_string()), 1, 1),
            ],
            "Shared-Chat zählt nicht als originäre Aktivität; doppeltes Rollup idempotent"
        );
        let null_lang: Vec<(i64,)> = sqlx::query_as(
            "SELECT messages FROM category_chat_rollup WHERE hour_bucket = $1 AND lang IS NULL",
        )
        .bind(stunde)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert!(null_lang.is_empty());
    }

    #[tokio::test]
    async fn alte_rohdaten_bleiben_auch_nach_rollup_erhalten() {
        let database = crate::test_postgres::TestPostgres::start().await;
        let pool = database.pool.clone();
        sqlx::raw_sql(include_str!(
            "../../../migrations/20260918123000_category_collector.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql(include_str!(
            "../../../migrations/20260918161000_category_preserve_data.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        let jetzt = Utc::now();
        let alt = jetzt - chrono::Duration::days(120);
        let jung = jetzt - chrono::Duration::days(1);
        let alt_stunde = Utc.from_utc_datetime(
            &alt.date_naive()
                .and_hms_opt(alt.time().hour(), 0, 0)
                .unwrap(),
        );
        let mk = |message_id: &str, sent_at: DateTime<Utc>| ChatZeile {
            room_user_id: "99".to_string(),
            message_id: message_id.to_string(),
            source_message_id: None,
            sent_at,
            chatter_user_id: "42".to_string(),
            chatter_login: "viewer".to_string(),
            message_text: "hallo welt".to_string(),
            text_len: 11,
            detected_lang: Some("de".to_string()),
            lang_confidence: Some(0.9),
            lang_method: "ok".to_string(),
            emote_count: 0,
        };
        // Alte Nachricht, deren Stunde gesichert ist.
        schreibe_chat_batch(&pool, &[mk("alt-1", alt_stunde)])
            .await
            .unwrap();
        rollup_stunde(&pool, alt_stunde).await.unwrap();
        // Alte Nachricht, deren Stunde NICHT gesichert ist.
        let alt_ungesichert = alt_stunde + chrono::Duration::hours(2);
        schreibe_chat_batch(&pool, &[mk("alt-2", alt_ungesichert)])
            .await
            .unwrap();
        // Junge Nachricht bleibt immer.
        schreibe_chat_batch(&pool, &[mk("jung-1", jung)])
            .await
            .unwrap();

        // Auch ein allgemeines IRC-CLEARCHAT darf das historische Archiv
        // nicht nachträglich leeren; gezielte Redaktionsaufträge sind separat.
        assert_eq!(redigiere_raum(&pool, "99").await.unwrap(), 0);
        let archivierte_texte: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM category_chat_messages WHERE message_text = 'hallo welt'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(archivierte_texte, 3);

        let poll_id = start_poll(&pool, alt_stunde).await.unwrap();
        schreibe_snapshots(
            &pool,
            poll_id,
            alt_stunde,
            &[zeile("s-alt", "99", "channel", 12)],
        )
        .await
        .unwrap();
        assert_eq!(loesche_snapshots_batch(&pool, jetzt, 500).await.unwrap(), 0);
        let snapshots: i64 = sqlx::query_scalar("SELECT count(*) FROM category_stream_snapshots")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(snapshots, 1, "auch alte Minuten-Snapshots bleiben erhalten");

        let retention_grenze = jetzt - chrono::Duration::days(90);
        // Rollup-Grenze: nur Stunden <= alt_stunde gelten als gesichert.
        let rollup_grenze = alt_stunde + chrono::Duration::hours(1);
        let geloescht = loesche_chat_batch(&pool, retention_grenze, rollup_grenze, 500)
            .await
            .unwrap();
        assert_eq!(
            geloescht, 0,
            "auch gesicherte alte Rohdaten bleiben erhalten"
        );

        let rest: Vec<(String,)> =
            sqlx::query_as("SELECT message_id FROM category_chat_messages ORDER BY message_id")
                .fetch_all(&pool)
                .await
                .unwrap();
        let ids: Vec<&str> = rest.iter().map(|(id,)| id.as_str()).collect();
        assert_eq!(ids, vec!["alt-1", "alt-2", "jung-1"]);

        // Zweiter Lauf: nichts zu löschen (idempotenter Catch-up).
        let nochmal = loesche_chat_batch(&pool, retention_grenze, rollup_grenze, 500)
            .await
            .unwrap();
        assert_eq!(nochmal, 0);
    }

    #[tokio::test]
    async fn status_upsert_meldet_keine_altersloeschung() {
        let database = database().await;
        let pool = database.pool.clone();
        let mut update = StatusUpdate {
            last_complete_poll_at: Some(Utc::now()),
            roster_size: 7,
            retention_deleted_total: 0,
            ..Default::default()
        };
        schreibe_status(&pool, &update).await.unwrap();
        update.retention_deleted_total = 0;
        update.last_error = Some("poll incomplete".to_string());
        update.last_error_at = Some(Utc::now());
        schreibe_status(&pool, &update).await.unwrap();

        let (roster, geloescht, fehler): (i32, i64, Option<String>) = sqlx::query_as(
            "SELECT roster_size, retention_deleted_total, last_error \
               FROM category_collector_status WHERE id = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!((roster, geloescht), (7, 0), "keine Alterslöschung");
        assert_eq!(fehler.as_deref(), Some("poll incomplete"));
    }

    #[tokio::test]
    async fn haengende_polls_werden_beim_start_geschlossen() {
        let database = database().await;
        let pool = database.pool.clone();
        let poll_id = start_poll(&pool, Utc::now()).await.unwrap();
        schliesse_haengende_polls(&pool).await.unwrap();
        let status: (String,) =
            sqlx::query_as("SELECT status FROM category_polls WHERE poll_id = $1")
                .bind(poll_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status.0, "failed");
    }
}
