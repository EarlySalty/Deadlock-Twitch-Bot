//! Public profile and media metadata enrichment. No downloads or Twitch writes.
use crate::{config::Config, Error};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use std::{sync::Arc, time::Duration};
use tb_transport_twitch::HelixClient;

pub async fn run(
    pool: PgPool,
    helix: Arc<HelixClient>,
    config: Arc<Config>,
    game_id: String,
) -> Result<(), Error> {
    let mut timer = tokio::time::interval(Duration::from_secs(10));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        timer.tick().await;
        // Metadata failures must not erase snapshots or reconnect anonymous chat.
        if let Err(error) = profiles(&pool, &helix).await {
            tracing::warn!(%error,"public category profile enrichment failed");
        }
        if config.media_enabled {
            if let Err(error) = media(&pool, &helix, &game_id).await {
                tracing::warn!(%error,"category media metadata enrichment failed");
            }
        }
    }
}
async fn profiles(pool: &PgPool, helix: &HelixClient) -> Result<(), Error> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT user_id FROM category_channels WHERE last_seen>now()-interval '1 day'
        AND (profile_checked_at IS NULL OR profile_checked_at<now()-interval '1 day')
        ORDER BY profile_checked_at NULLS FIRST,user_id LIMIT 20",
    )
    .fetch_all(pool)
    .await?;
    if ids.is_empty() {
        return Ok(());
    }
    let profiles = helix.get_public_profiles(&ids).await?;
    for profile in profiles {
        let created = DateTime::parse_from_rfc3339(&profile.created_at)
            .ok()
            .map(|d| d.with_timezone(&Utc));
        sqlx::query("UPDATE category_channels SET login=$2,display_name=$3,created_at=$4,broadcaster_type=$5,
            description=$6,profile_image_url=$7,profile_checked_at=now() WHERE user_id=$1")
            .bind(&profile.id).bind(&profile.login).bind(&profile.display_name).bind(created).bind(&profile.broadcaster_type)
            .bind(&profile.description).bind(&profile.profile_image_url).execute(pool).await?;
    }
    Ok(())
}

async fn media(pool: &PgPool, helix: &HelixClient, game_id: &str) -> Result<(), Error> {
    sqlx::query("INSERT INTO category_media_jobs(user_id,kind) SELECT user_id,kind FROM category_channels
        CROSS JOIN (VALUES('video'),('clip')) AS kinds(kind) WHERE last_seen>now()-interval '1 day' ON CONFLICT DO NOTHING")
        .execute(pool).await?;
    let job = sqlx::query(
        "SELECT user_id,kind,cursor,window_start,window_end,pages FROM category_media_jobs
        WHERE next_at<=now() ORDER BY next_at,user_id LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    let Some(job) = job else {
        return Ok(());
    };
    let user: String = job.try_get("user_id")?;
    let kind: String = job.try_get("kind")?;
    let cursor: Option<String> = job.try_get("cursor")?;
    let since: DateTime<Utc> = job.try_get("window_start")?;
    let until: DateTime<Utc> = job.try_get("window_end")?;
    let pages: i32 = job.try_get("pages")?;
    let result = helix
        .public_media_page(
            kind == "clip",
            &user,
            cursor.as_deref(),
            &since.to_rfc3339(),
            &until.to_rfc3339(),
        )
        .await;
    let (items, next) = match result {
        Ok(page) => page,
        Err(error) => {
            sqlx::query("UPDATE category_media_jobs SET next_at=now()+interval '15 minutes',last_error=$3 WHERE user_id=$1 AND kind=$2")
                .bind(user).bind(kind).bind(error.to_string()).execute(pool).await?;
            return Err(error.into());
        }
    };
    let mut tx = pool.begin().await?;
    for item in items {
        let Some(id) = item
            .get("id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        // Videos do not expose game_id. Their channel's category is not evidence
        // that the entire video is Deadlock; preserve this uncertainty.
        let verified =
            kind == "clip" && item.get("game_id").and_then(|v| v.as_str()) == Some(game_id);
        sqlx::query("INSERT INTO category_media(kind,media_id,user_id,metadata,category_verified)
            VALUES($1,$2,$3,$4::jsonb,$5) ON CONFLICT(kind,media_id) DO UPDATE SET metadata=excluded.metadata,
            last_seen=now(),category_verified=excluded.category_verified")
            .bind(&kind).bind(id).bind(&user).bind(item.to_string()).bind(verified).execute(&mut *tx).await?;
    }
    let repeated = next.is_some() && next == cursor;
    let bounded = pages >= 999 || (kind == "clip" && pages >= 9);
    if repeated || bounded {
        sqlx::query("UPDATE category_media_jobs SET next_at=now()+interval '6 hours',cursor=NULL,pages=0,
            last_error=$3,window_start=now()-interval '7 days',window_end=now() WHERE user_id=$1 AND kind=$2")
            .bind(&user).bind(&kind).bind(if repeated {"repeated_cursor_incomplete"}else{"api_window_limit_incomplete"}).execute(&mut *tx).await?;
    } else if let Some(next) = next {
        sqlx::query("UPDATE category_media_jobs SET cursor=$3,pages=pages+1,next_at=now()+interval '10 seconds',last_error=NULL
            WHERE user_id=$1 AND kind=$2").bind(&user).bind(&kind).bind(next).execute(&mut *tx).await?;
    } else {
        sqlx::query("UPDATE category_media_jobs SET cursor=NULL,pages=0,next_at=now()+interval '6 hours',last_error=NULL,
            window_start=now()-interval '7 days',window_end=now() WHERE user_id=$1 AND kind=$2")
            .bind(&user).bind(&kind).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}
