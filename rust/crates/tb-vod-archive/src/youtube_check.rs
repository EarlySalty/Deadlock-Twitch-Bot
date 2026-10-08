use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use tb_config::vod_archive::VodArchiveOptions;
use tb_social_media::credentials::CredentialManager;
use tb_social_media::upload_worker::youtube_uploader;
use tb_social_media::uploaders::youtube::ArchivVideo;
use tb_social_media::uploaders::UploadError;

use crate::error::VodArchiveError;

#[derive(Clone, sqlx::FromRow)]
struct Ziel {
    id: i32,
    revision: String,
    channel_id: Option<String>,
}

#[derive(Clone, sqlx::FromRow)]
struct PruefVod {
    id: i64,
    twitch_id: String,
    duration_sec: i64,
    snapshot: Value,
    parts: Value,
    check: Option<Value>,
}

#[derive(Default)]
struct Fortsetzung {
    started_at: Option<DateTime<Utc>>,
    refreshed: BTreeSet<String>,
    observations: Vec<Beobachtung>,
}

fn suchnachweis_noetig(vod: &PruefVod, observations: &[Beobachtung]) -> bool {
    let known = vod.parts.as_array().is_some_and(|parts| {
        !parts.is_empty()
            && parts
                .iter()
                .all(|part| part["video_id"].as_str().is_some_and(|id| !id.is_empty()))
    });
    let processed: Vec<_> = observations
        .iter()
        .cloned()
        .map(|mut observation| {
            observation.state = "processed".into();
            observation
        })
        .collect();
    !known && !decision(vod, &processed, true, false).1
}

async fn fortsetzung(
    pool: &PgPool,
    target: &Ziel,
    channel: &str,
    vod: &PruefVod,
    sources: &Value,
    hours: u64,
) -> Result<Fortsetzung, sqlx::Error> {
    let row: Option<(Value, Value, DateTime<Utc>)> = sqlx::query_as("SELECT refreshed,observations,started_at FROM twitch_vod_youtube_continuations WHERE vod_id=$1 AND auth_id=$2 AND auth_revision=$3 AND channel_id=$4 AND vod_snapshot=$5 AND upload_snapshot=$6 AND source_snapshot=$7 AND started_at>NOW()-make_interval(hours=>$8)")
        .bind(vod.id).bind(target.id).bind(&target.revision).bind(channel).bind(&vod.snapshot).bind(&vod.parts).bind(sources).bind(hours as i32).fetch_optional(pool).await?;
    match row {
        Some((refreshed, observations, started_at)) => Ok(Fortsetzung {
            started_at: Some(started_at),
            refreshed: serde_json::from_value(refreshed)
                .map_err(|error| sqlx::Error::Decode(Box::new(error)))?,
            observations: serde_json::from_value(observations)
                .map_err(|error| sqlx::Error::Decode(Box::new(error)))?,
        }),
        None => Ok(Fortsetzung::default()),
    }
}

async fn save_fortsetzung(
    pool: &PgPool,
    user: &str,
    target: &Ziel,
    channel: &str,
    vod: &PruefVod,
    sources: &Value,
    progress: &Fortsetzung,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    if !ziel_guard(&mut tx, user, target).await? {
        return Ok(());
    }
    let current: Option<Value> = sqlx::query_scalar("SELECT jsonb_build_array(status,updated_at,twitch_id,duration_sec) FROM twitch_vod_archive_vods WHERE id=$1 AND twitch_user_id=$2 FOR UPDATE")
        .bind(vod.id).bind(user).fetch_optional(&mut *tx).await?;
    sqlx::query("SELECT id FROM twitch_vod_archive_parts WHERE vod_id=$1 FOR UPDATE")
        .bind(vod.id)
        .fetch_all(&mut *tx)
        .await?;
    let parts: Value = sqlx::query_scalar("SELECT COALESCE(jsonb_agg(jsonb_build_object('id',id,'index',part_index,'status',status,'video_id',youtube_video_id,'updated_at',updated_at) ORDER BY part_index),'[]'::jsonb) FROM twitch_vod_archive_parts WHERE vod_id=$1")
        .bind(vod.id).fetch_one(&mut *tx).await?;
    if current.as_ref() != Some(&vod.snapshot) || parts != vod.parts {
        return Ok(());
    }
    sqlx::query("INSERT INTO twitch_vod_youtube_continuations (vod_id,auth_id,auth_revision,channel_id,vod_snapshot,upload_snapshot,source_snapshot,refreshed,observations,started_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) ON CONFLICT(vod_id) DO UPDATE SET auth_id=$2,auth_revision=$3,channel_id=$4,vod_snapshot=$5,upload_snapshot=$6,source_snapshot=$7,refreshed=$8,observations=$9,started_at=$10")
        .bind(vod.id).bind(target.id).bind(&target.revision).bind(channel).bind(&vod.snapshot).bind(&vod.parts).bind(sources).bind(json!(progress.refreshed)).bind(json!(progress.observations)).bind(progress.started_at.unwrap_or_else(Utc::now)).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct Kandidat {
    twitch_id: String,
    video_id: String,
    part_index: Option<i32>,
    part_total: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Beobachtung {
    pub video_id: String,
    pub part_index: Option<i32>,
    pub part_total: Option<i32>,
    pub duration_sec: Option<i64>,
    pub state: String,
    pub privacy: Option<String>,
    pub observed_at: DateTime<Utc>,
}

#[cfg(test)]
#[path = "youtube_check_tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) async fn test_schema(pool: &PgPool) {
    sqlx::raw_sql(include_str!(
        "../../../migrations/20261008003000_vod_youtube_checks.sql"
    ))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("CREATE TABLE social_media_platform_auth (id SERIAL PRIMARY KEY, platform TEXT, twitch_user_id TEXT, platform_user_id TEXT, authorized_at TEXT DEFAULT CURRENT_TIMESTAMP, refresh_token_enc BYTEA, enabled INTEGER DEFAULT 1, streamer_login TEXT, access_token_enc BYTEA, client_id TEXT, client_secret_enc BYTEA, token_expires_at TEXT, scopes TEXT, platform_username TEXT, enc_version INTEGER DEFAULT 1)").execute(pool).await.unwrap();
}

#[cfg(test)]
pub(crate) async fn test_confirm(pool: &PgPool, user: &str, id: i64) {
    sqlx::query("INSERT INTO social_media_platform_auth(platform,twitch_user_id,platform_user_id) VALUES ('youtube',$1,'synthetic-channel')").bind(user).execute(pool).await.unwrap();
    let target = ziel(pool, user).await.unwrap().unwrap();
    let vod = vods(pool, user, &target)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.id == id)
        .unwrap();
    let observations = vod
        .parts
        .as_array()
        .unwrap()
        .iter()
        .map(|part| Beobachtung {
            video_id: part["video_id"].as_str().unwrap().into(),
            part_index: part["index"].as_i64().map(|i| i as i32),
            part_total: Some(vod.parts.as_array().unwrap().len() as i32),
            duration_sec: None,
            state: "processed".into(),
            privacy: Some("private".into()),
            observed_at: Utc::now(),
        })
        .collect::<Vec<_>>();
    assert!(save(
        pool,
        user,
        &target,
        Some("synthetic-channel"),
        &vod,
        &observations,
        "confirmed",
        true,
        None,
        86400
    )
    .await
    .unwrap());
}

const REVISION: &str = "md5(COALESCE(a.refresh_token_enc::text,'') || COALESCE(a.platform_user_id,'') || COALESCE(a.authorized_at::text,''))";

async fn ziel(pool: &PgPool, user: &str) -> Result<Option<Ziel>, sqlx::Error> {
    let query = format!("SELECT a.id, {REVISION} AS revision, a.platform_user_id AS channel_id FROM social_media_platform_auth a WHERE a.platform='youtube' AND a.twitch_user_id=$1 AND a.enabled=1 ORDER BY a.authorized_at DESC, a.id DESC LIMIT 1");
    sqlx::query_as(sqlx::AssertSqlSafe(query))
        .bind(user)
        .fetch_optional(pool)
        .await
}

async fn ziel_guard(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user: &str,
    expected: &Ziel,
) -> Result<bool, sqlx::Error> {
    let query = format!("SELECT a.id, {REVISION} AS revision, a.platform_user_id AS channel_id FROM social_media_platform_auth a WHERE a.platform='youtube' AND a.twitch_user_id=$1 AND a.enabled=1 ORDER BY a.authorized_at DESC, a.id DESC LIMIT 1 FOR UPDATE");
    let current: Option<Ziel> = sqlx::query_as(sqlx::AssertSqlSafe(query))
        .bind(user)
        .fetch_optional(&mut **tx)
        .await?;
    Ok(current.is_some_and(|current| {
        current.id == expected.id
            && current.revision == expected.revision
            && current.channel_id == expected.channel_id
    }))
}

async fn vods(pool: &PgPool, user: &str, target: &Ziel) -> Result<Vec<PruefVod>, sqlx::Error> {
    sqlx::query_as("SELECT v.id, v.twitch_id, v.duration_sec, jsonb_build_array(v.status,v.updated_at,v.twitch_id,v.duration_sec) AS snapshot, COALESCE((SELECT jsonb_agg(jsonb_build_object('id',p.id,'index',p.part_index,'status',p.status,'video_id',p.youtube_video_id,'updated_at',p.updated_at) ORDER BY p.part_index) FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id),'[]'::jsonb) AS parts, CASE WHEN c.auth_id=$2 AND c.auth_revision=$3 THEN jsonb_build_object('observations',c.observations,'requested_at',c.requested_at,'last_attempt_at',c.last_attempt_at) END AS check FROM twitch_vod_archive_vods v LEFT JOIN twitch_vod_youtube_checks c ON c.vod_id=v.id WHERE v.twitch_user_id=$1 AND (v.status IN ('uploaded','archived') OR EXISTS (SELECT 1 FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id AND p.youtube_video_id IS NOT NULL)) AND (c.vod_id IS NULL OR c.auth_id<>$2 OR c.auth_revision<>$3 OR c.next_check_at<=NOW() OR c.requested_at IS NOT NULL OR c.upload_snapshot<>COALESCE((SELECT jsonb_agg(jsonb_build_object('id',p.id,'index',p.part_index,'status',p.status,'video_id',p.youtube_video_id,'updated_at',p.updated_at) ORDER BY p.part_index) FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id),'[]'::jsonb)) ORDER BY c.requested_at ASC NULLS LAST, CASE WHEN c.state='processing' THEN 0 WHEN c.vod_id IS NULL THEN 1 ELSE 2 END, c.next_check_at ASC NULLS FIRST, v.id LIMIT 500")
        .bind(user).bind(target.id).bind(&target.revision).fetch_all(pool).await
}

async fn kandidaten(
    pool: &PgPool,
    user: &str,
    target: &Ziel,
    channel: &str,
    pending: &[PruefVod],
) -> Result<Vec<Kandidat>, sqlx::Error> {
    let ids: Vec<_> = pending
        .iter()
        .filter(|vod| {
            vod.parts.as_array().is_none_or(|parts| {
                parts.is_empty()
                    || parts
                        .iter()
                        .any(|part| part["video_id"].as_str().is_none_or(str::is_empty))
            })
        })
        .map(|vod| vod.twitch_id.trim_start_matches('v').to_owned())
        .collect();
    sqlx::query_as("SELECT i.twitch_id,i.video_id,i.part_index,i.part_total FROM twitch_vod_youtube_inventory i JOIN twitch_vod_youtube_scans s USING(twitch_user_id) WHERE i.twitch_user_id=$1 AND i.twitch_id=ANY($2) AND i.generation=s.generation AND s.auth_id=$3 AND s.auth_revision=$4 AND s.channel_id=$5 ORDER BY i.part_index,i.video_id")
        .bind(user).bind(ids).bind(target.id).bind(&target.revision).bind(channel).fetch_all(pool).await
}

fn source_snapshot(vod: &PruefVod, candidates: &[Kandidat], generation: Option<i64>) -> Value {
    json!([
        generation,
        candidates
            .iter()
            .filter(|candidate| { candidate.twitch_id == vod.twitch_id.trim_start_matches('v') })
            .map(|candidate| (
                &candidate.video_id,
                candidate.part_index,
                candidate.part_total
            ))
            .collect::<Vec<_>>()
    ])
}

fn required_ids(vod: &PruefVod, candidates: &[Kandidat]) -> BTreeSet<String> {
    vod.parts
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| {
            part["video_id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
        })
        .chain(
            candidates
                .iter()
                .filter(|candidate| candidate.twitch_id == vod.twitch_id.trim_start_matches('v'))
                .map(|candidate| candidate.video_id.clone()),
        )
        .collect()
}

fn source(video: &ArchivVideo) -> Option<(String, Option<(i32, i32)>)> {
    let mut ids = BTreeSet::new();
    for tail in video
        .description
        .split("https://www.twitch.tv/videos/")
        .skip(1)
    {
        let number: String = tail.chars().take_while(char::is_ascii_digit).collect();
        let suffix = &tail[number.len()..];
        if number.is_empty()
            || suffix
                .chars()
                .next()
                .is_some_and(|c| !c.is_whitespace() && !['?', '#', '/', ')', '.'].contains(&c))
        {
            return None;
        }
        ids.insert(number);
    }
    if ids.len() != 1 {
        return None;
    }
    let id = ids.into_iter().next()?;
    let mut marker = None;
    for line in video
        .description
        .lines()
        .filter(|line| line.starts_with("Archivquelle: Twitch-VOD "))
    {
        let rest = line.strip_prefix(&format!("Archivquelle: Twitch-VOD {id}; Teil "))?;
        let current = part_marker(rest)?;
        if marker.is_some_and(|previous| previous != current) {
            return None;
        }
        marker = Some(current);
    }
    if let Some((_, part)) = video
        .title
        .strip_suffix(')')
        .and_then(|title| title.rsplit_once("(Teil "))
    {
        let current = part_marker(part)?;
        if marker.is_some_and(|previous| previous != current) {
            return None;
        }
        marker = Some(current);
    }
    Some((id, marker))
}

fn part_marker(text: &str) -> Option<(i32, i32)> {
    let (index, total) = text.split_once('/')?;
    let index: i32 = index.parse().ok()?;
    let total: i32 = total.parse().ok()?;
    (index > 0 && index <= total && total <= 1000).then_some((index - 1, total))
}

fn read_error(error: &UploadError) -> &'static str {
    match error {
        UploadError::QuotaExceeded(_) => "quota",
        UploadError::NotAuthenticated => "connection",
        UploadError::Api(message)
            if message.contains("HTTP 401")
                || message.contains("HTTP 403")
                || message.contains("Token-Refresh abgelehnt") =>
        {
            "connection"
        }
        _ => "request",
    }
}

fn decision(
    vod: &PruefVod,
    observations: &[Beobachtung],
    scan_complete: bool,
    known: bool,
) -> (&'static str, bool) {
    if observations.is_empty() {
        return (
            if scan_complete {
                "unresolved"
            } else {
                "searching"
            },
            false,
        );
    }
    let mut indexes = BTreeSet::new();
    let mut videos = BTreeSet::new();
    let parts = vod.parts.as_array().map(Vec::as_slice).unwrap_or_default();
    let expected = if parts.is_empty() {
        observations.first().and_then(|o| o.part_total).unwrap_or(1)
    } else {
        parts.len() as i32
    };
    let consistent = expected > 0
        && observations.len() == expected as usize
        && observations.iter().all(|o| {
            let index = o.part_index.unwrap_or(0);
            index >= 0
                && index < expected
                && indexes.insert(index)
                && videos.insert(&o.video_id)
                && (known || o.part_total.unwrap_or(1) == expected)
        })
        && parts.iter().all(|part| {
            part["video_id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .is_none_or(|id| {
                    observations.iter().any(|o| {
                        o.video_id == id && o.part_index.map(i64::from) == part["index"].as_i64()
                    })
                })
        });
    let duration: Option<i64> = observations
        .iter()
        .try_fold(0i64, |sum, o| sum.checked_add(o.duration_sec?));
    let covered = known
        || (scan_complete
            && vod.duration_sec > 0
            && duration.is_some_and(|duration| {
                duration >= vod.duration_sec
                    && duration - vod.duration_sec <= 5 * i64::from(expected)
            }));
    let all_processed = observations.iter().all(|o| o.state == "processed");
    if consistent && covered && all_processed {
        return ("confirmed", true);
    }
    if observations.iter().any(|o| o.state == "rejected") {
        return ("rejected", false);
    }
    if observations.iter().any(|o| o.state == "unavailable") {
        return ("unavailable", false);
    }
    if observations.iter().any(|o| o.state == "processing") {
        return ("processing", false);
    }
    ("partial", false)
}

#[allow(clippy::too_many_arguments)]
async fn save(
    pool: &PgPool,
    user: &str,
    target: &Ziel,
    channel: Option<&str>,
    vod: &PruefVod,
    observations: &[Beobachtung],
    state: &str,
    complete: bool,
    error: Option<&str>,
    seconds: i64,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    if !ziel_guard(&mut tx, user, target).await? {
        return Ok(false);
    }
    let current: Option<Value> = sqlx::query_scalar("SELECT jsonb_build_array(status,updated_at,twitch_id,duration_sec) FROM twitch_vod_archive_vods WHERE id=$1 AND twitch_user_id=$2 FOR UPDATE")
        .bind(vod.id).bind(user).fetch_optional(&mut *tx).await?;
    let rows = sqlx::query(
        "SELECT id FROM twitch_vod_archive_parts WHERE vod_id=$1 ORDER BY part_index FOR UPDATE",
    )
    .bind(vod.id)
    .fetch_all(&mut *tx)
    .await?;
    let _ = rows;
    let parts: Value = sqlx::query_scalar("SELECT COALESCE(jsonb_agg(jsonb_build_object('id',id,'index',part_index,'status',status,'video_id',youtube_video_id,'updated_at',updated_at) ORDER BY part_index),'[]'::jsonb) FROM twitch_vod_archive_parts WHERE vod_id=$1")
        .bind(vod.id).fetch_one(&mut *tx).await?;
    if current.as_ref() != Some(&vod.snapshot) || parts != vod.parts {
        return Ok(false);
    }
    let mut success_at = observations
        .iter()
        .map(|observation| observation.observed_at)
        .min();
    if error.is_none() && suchnachweis_noetig(vod, observations) {
        let searched_at: Option<DateTime<Utc>> = sqlx::query_scalar("SELECT last_success_at FROM twitch_vod_youtube_scans WHERE twitch_user_id=$1 AND auth_id=$2 AND auth_revision=$3 AND channel_id=$4 AND complete")
            .bind(user).bind(target.id).bind(&target.revision).bind(channel).fetch_optional(&mut *tx).await?.flatten();
        success_at = searched_at
            .map(|searched| success_at.map_or(searched, |observed| observed.min(searched)));
    }
    sqlx::query("INSERT INTO twitch_vod_youtube_checks (vod_id,auth_id,auth_revision,channel_id,state,complete,observations,last_attempt_at,last_success_at,last_error,next_check_at,upload_snapshot) VALUES ($1,$2,$3,$4,$5,$6,$7,NOW(),CASE WHEN $8::text IS NULL THEN $11 END,$8,NOW()+make_interval(secs=>$9::double precision),$10) ON CONFLICT (vod_id) DO UPDATE SET auth_id=EXCLUDED.auth_id,auth_revision=EXCLUDED.auth_revision,channel_id=EXCLUDED.channel_id,state=EXCLUDED.state,complete=EXCLUDED.complete,observations=CASE WHEN $8::text IS NOT NULL AND twitch_vod_youtube_checks.auth_id=$2 AND twitch_vod_youtube_checks.auth_revision=$3 THEN twitch_vod_youtube_checks.observations ELSE EXCLUDED.observations END,last_attempt_at=NOW(),last_success_at=CASE WHEN $8::text IS NULL THEN $11 WHEN twitch_vod_youtube_checks.auth_id=$2 AND twitch_vod_youtube_checks.auth_revision=$3 THEN twitch_vod_youtube_checks.last_success_at END,last_error=$8,requested_at=NULL,next_check_at=EXCLUDED.next_check_at,upload_snapshot=CASE WHEN $8::text IS NOT NULL AND twitch_vod_youtube_checks.auth_id=$2 AND twitch_vod_youtube_checks.auth_revision=$3 THEN twitch_vod_youtube_checks.upload_snapshot ELSE EXCLUDED.upload_snapshot END")
        .bind(vod.id).bind(target.id).bind(&target.revision).bind(channel).bind(state).bind(complete).bind(json!(observations)).bind(error).bind(seconds as f64).bind(&vod.parts).bind(success_at).execute(&mut *tx).await?;
    if error.is_none() {
        sqlx::query("DELETE FROM twitch_vod_youtube_continuations WHERE vod_id=$1")
            .bind(vod.id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(true)
}

pub async fn cleanup_proof<'a>(
    pool: &'a PgPool,
    user: &str,
    vod_id: i64,
    hours: u64,
) -> Result<Option<sqlx::Transaction<'a, sqlx::Postgres>>, sqlx::Error> {
    let Some(target) = ziel(pool, user).await? else {
        return Ok(None);
    };
    let mut tx = pool.begin().await?;
    if !ziel_guard(&mut tx, user, &target).await? {
        return Ok(None);
    }
    let Some(lock_id) = i32::try_from(vod_id).ok() else {
        return Ok(None);
    };
    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(186976768,$1)")
        .bind(lock_id)
        .fetch_one(&mut *tx)
        .await?;
    if !locked {
        return Ok(None);
    }
    sqlx::query("SELECT id FROM twitch_vod_archive_vods WHERE id=$1 FOR UPDATE")
        .bind(vod_id)
        .fetch_optional(&mut *tx)
        .await?;
    sqlx::query("SELECT id FROM twitch_vod_archive_parts WHERE vod_id=$1 FOR UPDATE")
        .bind(vod_id)
        .fetch_all(&mut *tx)
        .await?;
    let valid: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM twitch_vod_youtube_checks c JOIN twitch_vod_archive_vods v ON v.id=c.vod_id WHERE c.vod_id=$1 AND v.twitch_user_id=$2 AND v.status='uploaded' AND c.auth_id=$3 AND c.auth_revision=$4 AND c.channel_id IS NOT NULL AND c.state='confirmed' AND c.complete AND c.last_error IS NULL AND c.last_success_at>NOW()-make_interval(hours=>$5) AND jsonb_array_length(c.observations)>0 AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(c.observations) o WHERE o->>'state'<>'processed' OR (o->>'observed_at')::timestamptz<=NOW()-make_interval(hours=>$5)) AND c.upload_snapshot=(SELECT COALESCE(jsonb_agg(jsonb_build_object('id',p.id,'index',p.part_index,'status',p.status,'video_id',p.youtube_video_id,'updated_at',p.updated_at) ORDER BY p.part_index),'[]'::jsonb) FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id))")
        .bind(vod_id).bind(user).bind(target.id).bind(&target.revision).bind(hours as i32).fetch_one(&mut *tx).await?;
    Ok(valid.then_some(tx))
}

#[allow(clippy::too_many_arguments)]
pub async fn run_once(
    pool: &PgPool,
    credentials: &CredentialManager,
    config: &VodArchiveOptions,
) -> Result<(), VodArchiveError> {
    run_with(pool, credentials, config, youtube_uploader).await
}

async fn run_with(
    pool: &PgPool,
    credentials: &CredentialManager,
    config: &VodArchiveOptions,
    client_factory: impl Fn(
        &tb_social_media::credentials::SocialMediaCredentials,
    ) -> tb_social_media::uploaders::youtube::YouTubeUploader,
) -> Result<(), VodArchiveError> {
    let users: Vec<String> = sqlx::query_scalar("SELECT DISTINCT v.twitch_user_id FROM twitch_vod_archive_vods v WHERE v.twitch_user_id IS NOT NULL AND (v.status IN ('uploaded','archived') OR EXISTS (SELECT 1 FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id AND p.youtube_video_id IS NOT NULL)) ORDER BY v.twitch_user_id")
        .fetch_all(pool).await?;
    let mut budget = config.youtube_requests_per_run;
    for user in users {
        if budget < 3 {
            break;
        }
        let Some(target) = ziel(pool, &user).await? else {
            continue;
        };
        let mut guard = pool.begin().await?;
        let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(186976769,$1)")
            .bind(target.id)
            .fetch_one(&mut *guard)
            .await?;
        if !locked {
            continue;
        }
        let pending = vods(pool, &user, &target).await?;
        if pending.is_empty() {
            continue;
        }
        let Some(creds) = credentials
            .get_channel_credentials_for_id("youtube", &user)
            .await
        else {
            for vod in &pending {
                save(
                    pool,
                    &user,
                    &target,
                    None,
                    vod,
                    &[],
                    "error",
                    false,
                    Some("connection"),
                    config.youtube_error_minutes as i64 * 60,
                )
                .await?;
            }
            tracing::warn!(user, "YouTube-Abgleich: eigener Zugang fehlt");
            continue;
        };
        if creds.id != target.id {
            continue;
        }
        let client = client_factory(&creds).with_retry(1, std::time::Duration::ZERO);
        budget -= 1;
        let channel = match client.upload_kanal().await {
            Ok(channel)
                if target
                    .channel_id
                    .as_deref()
                    .is_none_or(|expected| expected == channel.id) =>
            {
                channel
            }
            result => {
                let error = match result {
                    Err(error) => read_error(&error),
                    Ok(_) => "channel_changed",
                };
                let seconds = if error == "quota" {
                    config.youtube_quota_hours * 3600
                } else {
                    config.youtube_error_minutes * 60
                };
                for vod in &pending {
                    save(
                        pool,
                        &user,
                        &target,
                        None,
                        vod,
                        &[],
                        "error",
                        false,
                        Some(error),
                        seconds as i64,
                    )
                    .await?;
                }
                tracing::warn!(user, error, "YouTube-Abgleich: Kanal nicht lesbar");
                if error == "quota" {
                    break;
                }
                continue;
            }
        };
        let historical = pending.iter().any(|v| {
            v.parts.as_array().is_none_or(|p| {
                p.is_empty()
                    || p.iter()
                        .any(|p| p["video_id"].as_str().is_none_or(str::is_empty))
            })
        });
        let mut scan_complete = false;
        if historical {
            let mut tx = pool.begin().await?;
            if !ziel_guard(&mut tx, &user, &target).await? {
                continue;
            }
            sqlx::query("INSERT INTO twitch_vod_youtube_scans (twitch_user_id,auth_id,auth_revision,channel_id,playlist_id) VALUES ($1,$2,$3,$4,$5) ON CONFLICT (twitch_user_id) DO UPDATE SET auth_id=$2,auth_revision=$3,channel_id=$4,playlist_id=$5,cursor=NULL,generation=twitch_vod_youtube_scans.generation+1,complete=FALSE,next_scan_at=NOW(),last_error=NULL WHERE twitch_vod_youtube_scans.auth_id<>$2 OR twitch_vod_youtube_scans.auth_revision<>$3 OR twitch_vod_youtube_scans.channel_id<>$4 OR twitch_vod_youtube_scans.playlist_id<>$5")
                .bind(&user).bind(target.id).bind(&target.revision).bind(&channel.id).bind(&channel.playlist_id).execute(&mut *tx).await?;
            let refresh_search = pending
                .iter()
                .filter_map(|vod| {
                    let check = vod.check.as_ref()?;
                    let observations: Vec<Beobachtung> =
                        serde_json::from_value(check["observations"].clone()).ok()?;
                    if !suchnachweis_noetig(vod, &observations) {
                        return None;
                    }
                    ["requested_at", "last_attempt_at"]
                        .into_iter()
                        .filter_map(|field| check[field].as_str()?.parse::<DateTime<Utc>>().ok())
                        .max()
                })
                .max();
            sqlx::query("UPDATE twitch_vod_youtube_scans s SET cursor=NULL,generation=generation+1,complete=FALSE WHERE twitch_user_id=$1 AND complete AND (next_scan_at<=NOW() OR $2::timestamptz>COALESCE(last_success_at,'-infinity'::timestamptz)) AND NOT EXISTS (SELECT 1 FROM twitch_vod_youtube_continuations p JOIN twitch_vod_archive_vods v ON v.id=p.vod_id WHERE v.twitch_user_id=$1 AND p.auth_id=s.auth_id AND p.auth_revision=s.auth_revision AND p.channel_id=s.channel_id AND p.started_at>NOW()-make_interval(hours=>$3))")
                .bind(&user).bind(refresh_search).bind(config.youtube_check_hours as i32).execute(&mut *tx).await?;
            scan_complete = sqlx::query_scalar(
                "SELECT complete FROM twitch_vod_youtube_scans WHERE twitch_user_id=$1",
            )
            .bind(&user)
            .fetch_one(&mut *tx)
            .await?;
            tx.commit().await?;
        }
        let candidates = kandidaten(pool, &user, &target, &channel.id, &pending).await?;
        let generation: Option<i64> = sqlx::query_scalar("SELECT generation FROM twitch_vod_youtube_scans WHERE twitch_user_id=$1 AND auth_id=$2 AND auth_revision=$3 AND channel_id=$4")
            .bind(&user).bind(target.id).bind(&target.revision).bind(&channel.id).fetch_optional(pool).await?;
        let mut progress = HashMap::new();
        let mut known = BTreeSet::new();
        for vod in &pending {
            let sources = source_snapshot(vod, &candidates, generation);
            let continuation = fortsetzung(
                pool,
                &target,
                &channel.id,
                vod,
                &sources,
                config.youtube_check_hours,
            )
            .await?;
            known.extend(
                required_ids(vod, &candidates)
                    .difference(&continuation.refreshed)
                    .cloned(),
            );
            progress.insert(vod.id, (sources, continuation));
        }
        let known: Vec<_> = known.into_iter().collect();
        let reserved = if historical && !scan_complete { 2 } else { 0 };
        let mut found = HashMap::new();
        let mut failed = None;
        let mut queried = BTreeSet::new();
        for batch in known.chunks(50) {
            if budget <= reserved {
                break;
            }
            budget -= 1;
            match client.archiv_videos(batch).await {
                Ok(videos) => {
                    if videos.iter().any(|video| video.channel_id != channel.id) {
                        failed = Some("channel_changed");
                        break;
                    }
                    queried.extend(batch.iter().cloned());
                    for video in videos {
                        found.insert(video.id.clone(), video);
                    }
                }
                Err(error) => {
                    failed = Some(read_error(&error));
                    break;
                }
            }
        }
        if failed.is_none() && historical && !scan_complete && budget >= 2 {
            loop {
                let (cursor,generation,complete): (Option<String>,i64,bool) = sqlx::query_as("SELECT cursor,generation,complete FROM twitch_vod_youtube_scans WHERE twitch_user_id=$1").bind(&user).fetch_one(pool).await?;
                scan_complete = complete;
                if complete || budget < 2 {
                    break;
                }
                budget -= 1;
                let page = match client
                    .upload_seite(&channel.playlist_id, cursor.as_deref())
                    .await
                {
                    Ok(page) => page,
                    Err(error) => {
                        failed = Some(read_error(&error));
                        break;
                    }
                };
                budget -= 1;
                let videos = match client.archiv_videos(&page.ids).await {
                    Ok(videos) => videos,
                    Err(error) => {
                        failed = Some(read_error(&error));
                        break;
                    }
                };
                if videos.iter().any(|video| video.channel_id != channel.id) {
                    failed = Some("channel_changed");
                    break;
                }
                let mut tx = pool.begin().await?;
                if !ziel_guard(&mut tx, &user, &target).await? {
                    break;
                }
                queried.extend(page.ids.iter().cloned());
                for video in videos {
                    found.insert(video.id.clone(), video.clone());
                    let Some((twitch_id, marker)) = source(&video) else {
                        continue;
                    };
                    sqlx::query("INSERT INTO twitch_vod_youtube_inventory (twitch_user_id,video_id,generation,twitch_id,part_index,part_total,duration_sec,state,privacy) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT (twitch_user_id,video_id) DO UPDATE SET generation=$3,twitch_id=$4,part_index=$5,part_total=$6,duration_sec=$7,state=$8,privacy=$9,observed_at=NOW()")
                        .bind(&user).bind(&video.id).bind(generation).bind(twitch_id).bind(marker.map(|p|p.0)).bind(marker.map(|p|p.1)).bind(video.duration_sec).bind(&video.state).bind(&video.privacy).execute(&mut *tx).await?;
                }
                sqlx::query("UPDATE twitch_vod_youtube_scans SET cursor=$2,complete=$2::text IS NULL,last_attempt_at=NOW(),last_success_at=CASE WHEN $2::text IS NULL THEN NOW() ELSE last_success_at END,last_error=NULL,next_scan_at=NOW()+make_interval(secs=>$3::double precision) WHERE twitch_user_id=$1 AND generation=$4")
                    .bind(&user).bind(&page.next).bind((config.youtube_check_hours*3600) as f64).bind(generation).execute(&mut *tx).await?;
                tx.commit().await?;
            }
        }
        let candidates = kandidaten(pool, &user, &target, &channel.id, &pending).await?;
        let generation: Option<i64> = sqlx::query_scalar("SELECT generation FROM twitch_vod_youtube_scans WHERE twitch_user_id=$1 AND auth_id=$2 AND auth_revision=$3 AND channel_id=$4")
            .bind(&user).bind(target.id).bind(&target.revision).bind(&channel.id).fetch_optional(pool).await?;
        for vod in &pending {
            let parts = vod.parts.as_array().unwrap();
            let known_complete = !parts.is_empty()
                && parts
                    .iter()
                    .all(|p| p["video_id"].as_str().is_some_and(|id| !id.is_empty()));
            let recovered: Vec<_> = candidates
                .iter()
                .filter(|candidate| candidate.twitch_id == vod.twitch_id.trim_start_matches('v'))
                .collect();
            let sources = source_snapshot(vod, &candidates, generation);
            let (old_sources, mut continuation) = progress.remove(&vod.id).unwrap();
            if old_sources != sources {
                continuation = Fortsetzung::default();
            }
            let mut observations: Vec<Beobachtung> = parts
                .iter()
                .filter_map(|part| {
                    let id = part["video_id"]
                        .as_str()
                        .filter(|id| queried.contains(*id))?;
                    let video = found.get(id).filter(|v| v.channel_id == channel.id);
                    Some(Beobachtung {
                        video_id: id.into(),
                        part_index: part["index"].as_i64().map(|i| i as i32),
                        part_total: Some(parts.len() as i32),
                        duration_sec: video.and_then(|v| v.duration_sec),
                        state: video.map_or("unavailable", |v| v.state.as_str()).into(),
                        privacy: video.and_then(|v| v.privacy.clone()),
                        observed_at: Utc::now(),
                    })
                })
                .collect();
            if !known_complete {
                for candidate in recovered {
                    let video_id = &candidate.video_id;
                    if !queried.contains(video_id)
                        || observations.iter().any(|o| o.video_id == *video_id)
                    {
                        continue;
                    }
                    let video = found.get(video_id);
                    let marker = match video {
                        Some(video) => {
                            let Some((twitch_id, marker)) = source(video) else {
                                continue;
                            };
                            if twitch_id != vod.twitch_id.trim_start_matches('v') {
                                continue;
                            }
                            marker
                        }
                        None => candidate.part_index.zip(candidate.part_total),
                    };
                    let part = parts.iter().find(|part| part["video_id"] == *video_id);
                    observations.push(Beobachtung {
                        video_id: video_id.clone(),
                        part_index: part.map_or_else(
                            || marker.map(|m| m.0),
                            |p| p["index"].as_i64().map(|i| i as i32),
                        ),
                        part_total: part
                            .map_or_else(|| marker.map(|m| m.1), |_| Some(parts.len() as i32)),
                        duration_sec: video.and_then(|v| v.duration_sec),
                        state: video.map_or("unavailable", |v| v.state.as_str()).into(),
                        privacy: video.and_then(|v| v.privacy.clone()),
                        observed_at: Utc::now(),
                    });
                }
            }
            observations.extend(
                continuation
                    .observations
                    .into_iter()
                    .filter(|observation| !queried.contains(&observation.video_id)),
            );
            let required = required_ids(vod, &candidates);
            continuation
                .refreshed
                .extend(required.intersection(&queried).cloned());
            let awaiting = !required.is_subset(&continuation.refreshed);
            if awaiting && failed.is_none() {
                continuation.observations = observations;
                save_fortsetzung(
                    pool,
                    &user,
                    &target,
                    &channel.id,
                    vod,
                    &sources,
                    &continuation,
                )
                .await?;
                tracing::info!(
                    vod_id = vod.id,
                    refreshed = continuation.refreshed.len(),
                    total = required.len(),
                    "youtube_archive_continuation"
                );
                continue;
            }
            let awaiting_known = parts.iter().any(|part| {
                part["video_id"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .is_some_and(|id| !observations.iter().any(|o| o.video_id == id))
            });
            let (state, complete) = decision(vod, &observations, scan_complete, known_complete);
            let seconds = match failed {
                Some("quota") => config.youtube_quota_hours * 3600,
                Some(_) => config.youtube_error_minutes * 60,
                None if state == "processing"
                    || state == "searching"
                    || awaiting_known
                    || (!known_complete && !scan_complete) =>
                {
                    config.youtube_processing_minutes * 60
                }
                None => config.youtube_check_hours * 3600,
            };
            let saved = save(
                pool,
                &user,
                &target,
                Some(&channel.id),
                vod,
                &observations,
                if failed.is_some() { "error" } else { state },
                complete && failed.is_none(),
                failed,
                seconds as i64,
            )
            .await?;
            tracing::info!(
                vod_id = vod.id,
                state = if failed.is_some() { "error" } else { state },
                complete = complete && failed.is_none(),
                saved,
                scan_complete,
                "youtube_archive_reconciled"
            );
        }
        if failed == Some("quota") {
            break;
        }
    }
    Ok(())
}
