//! Bounded metadata sweeps using the shared Helix read transport. Pagination
//! cursors persist across restarts; completion flags describe the returned
//! API list, not a guarantee that Twitch exposes all historical content.
use crate::{config, store};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::time::Duration;
use tb_transport_twitch::HelixClient;

async fn get(helix: &HelixClient, path: &str, params: &[(&str, String)]) -> Result<Value, String> {
    let value = helix
        .category_read_json(path, params)
        .await
        .map_err(|_| "metadata HTTP response failed")?;
    if !value.get("data").is_some_and(Value::is_array) {
        return Err("metadata data array missing".into());
    }
    Ok(value)
}
fn timestamp(value: &Value) -> Option<DateTime<Utc>> {
    value
        .as_str()
        .and_then(|v| DateTime::parse_from_rfc3339(v).ok())
        .map(|v| v.with_timezone(&Utc))
}

async fn channel(pool: &PgPool, helix: &HelixClient, id: &str, reset: bool) -> Result<(), String> {
    let profile = get(helix, "/users", &[("id", id.into())]).await?;
    let info = get(helix, "/channels", &[("broadcaster_id", id.into())]).await?;
    let user = profile["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["id"].as_str() == Some(id))
        .ok_or("metadata user missing")?;
    let language = info["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["broadcaster_id"].as_str() == Some(id))
        .and_then(|u| u["broadcaster_language"].as_str());
    sqlx::query("UPDATE category_channels SET display_name=$2,description=$3,broadcaster_type=$4,
        profile_created_at=$5,profile_image_url=$6,broadcaster_language=COALESCE($7,broadcaster_language) WHERE user_id=$1")
        .bind(id).bind(user["display_name"].as_str()).bind(user["description"].as_str()).bind(user["broadcaster_type"].as_str())
        .bind(timestamp(&user["created_at"])).bind(user["profile_image_url"].as_str()).bind(language)
        .execute(pool).await.map_err(|_|"metadata profile storage failed")?;
    if reset {
        sqlx::query("UPDATE category_channels SET videos_cursor=NULL,clips_cursor=NULL,videos_complete=false,clips_complete=false,
            videos_pages=0,clips_pages=0,metadata_error=NULL WHERE user_id=$1")
            .bind(id).execute(pool).await.map_err(|_|"metadata reset failed")?;
    }
    for kind in ["video", "clip"] {
        let (cursor,complete,pages):(Option<String>,bool,i32)=if kind=="video" {
            sqlx::query_as("SELECT videos_cursor,videos_complete,videos_pages FROM category_channels WHERE user_id=$1").bind(id).fetch_one(pool).await
        } else {
            sqlx::query_as("SELECT clips_cursor,clips_complete,clips_pages FROM category_channels WHERE user_id=$1").bind(id).fetch_one(pool).await
        }.map_err(|_|"metadata cursor read failed")?;
        if complete {
            continue;
        }
        if pages >= 100 {
            return Err("metadata pagination budget reached; list incomplete".into());
        }
        let mut params = vec![
            (
                if kind == "video" {
                    "user_id"
                } else {
                    "broadcaster_id"
                },
                id.to_owned(),
            ),
            ("first", "100".into()),
        ];
        if let Some(after) = &cursor {
            params.push(("after", after.clone()));
        }
        let body = get(
            helix,
            if kind == "video" { "/videos" } else { "/clips" },
            &params,
        )
        .await?;
        let next = body["pagination"]["cursor"]
            .as_str()
            .filter(|s| !s.is_empty());
        if next.is_some() && next == cursor.as_deref() {
            return Err("metadata cursor repeated; list incomplete".into());
        }
        let values = body["data"].as_array().unwrap();
        if values
            .iter()
            .any(|v| v["id"].as_str().is_none_or(str::is_empty))
        {
            return Err("metadata media id missing".into());
        }
        if !body["pagination"].is_object() || (values.is_empty() && next.is_some()) {
            return Err("invalid metadata pagination".into());
        }
        let rows: Vec<_> = values
            .iter()
            .map(|v| json!({"id":v["id"],"created_at":timestamp(&v["created_at"]),"metadata":v}))
            .collect();
        let mut tx = pool
            .begin()
            .await
            .map_err(|_| "metadata transaction failed")?;
        sqlx::query("INSERT INTO category_media_items(kind,media_id,user_id,created_at,metadata)
            SELECT $1,id,$2,created_at,metadata FROM jsonb_to_recordset($3) AS r(id text,created_at timestamptz,metadata jsonb)
            ON CONFLICT(kind,media_id) DO UPDATE SET metadata=EXCLUDED.metadata,fetched_at=now()")
            .bind(kind).bind(id).bind(json!(rows)).execute(&mut *tx).await.map_err(|_|"media metadata storage failed")?;
        let update = if kind == "video" {
            "UPDATE category_channels SET videos_cursor=$2,videos_complete=$3,videos_pages=videos_pages+1 WHERE user_id=$1"
        } else {
            "UPDATE category_channels SET clips_cursor=$2,clips_complete=$3,clips_pages=clips_pages+1 WHERE user_id=$1"
        };
        sqlx::query(update)
            .bind(id)
            .bind(next)
            .bind(next.is_none())
            .execute(&mut *tx)
            .await
            .map_err(|_| "metadata cursor storage failed")?;
        tx.commit().await.map_err(|_| "metadata commit failed")?;
    }
    sqlx::query("UPDATE category_channels SET metadata_fetched_at=now(),metadata_error=NULL WHERE user_id=$1")
        .bind(id).execute(pool).await.map_err(|_|"metadata completion storage failed")?;
    Ok(())
}

pub async fn run(pool: PgPool, helix: HelixClient) -> Result<(), sqlx::Error> {
    loop {
        let cfg = config::lade(&pool).await?;
        if cfg.enabled && cfg.vod_metadata_enabled {
            let channels: Vec<(String, bool)> = sqlx::query_as(
                "SELECT user_id,
                COALESCE(metadata_fetched_at<now()-interval '6 hours',false) AS reset
                FROM category_channels WHERE metadata_fetched_at IS NULL
                  OR metadata_fetched_at<now()-interval '6 hours'
                  OR (metadata_error IS NULL AND (NOT videos_complete OR NOT clips_complete))
                ORDER BY metadata_fetched_at NULLS FIRST,last_seen_live_at DESC NULLS LAST LIMIT 5",
            )
            .fetch_all(&pool)
            .await?;
            for (id, reset) in channels {
                if let Err(detail) = channel(&pool, &helix, &id, reset).await {
                    sqlx::query("UPDATE category_channels SET metadata_fetched_at=now(),metadata_error=$2 WHERE user_id=$1")
                        .bind(&id).bind(&detail).execute(&pool).await?;
                    store::error(&pool, &detail).await?;
                    tracing::warn!(
                        channel_id = id,
                        error = detail,
                        "category metadata sweep incomplete"
                    );
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}
