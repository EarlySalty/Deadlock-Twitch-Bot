//! Optional public follower-total enrichment using the bot's existing token
//! owner. The collector never receives a bot token or a second refresh loop.
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tb_chat::token::BotTokenManager;
use tb_transport_twitch::HelixClient;

pub async fn run(pool: PgPool, helix: HelixClient, token_manager: Arc<BotTokenManager>) {
    let mut timer = tokio::time::interval(Duration::from_secs(30));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        timer.tick().await;
        if let Err(error) = refresh(&pool, &helix, &token_manager).await {
            tracing::warn!(%error,"category public follower totals unavailable; counts remain unknown/stale");
        }
    }
}
async fn refresh(
    pool: &PgPool,
    helix: &HelixClient,
    token_manager: &BotTokenManager,
) -> Result<(), String> {
    let exists: bool =
        sqlx::query_scalar("SELECT to_regclass('public.category_channels') IS NOT NULL")
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;
    if !exists {
        return Ok(());
    }
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT user_id FROM category_channels WHERE last_seen>now()-interval '1 day'
        AND (followers_checked_at IS NULL OR followers_checked_at<now()-interval '6 hours')
        ORDER BY followers_checked_at NULLS FIRST,user_id LIMIT 25",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    if ids.is_empty() {
        return Ok(());
    }
    let token = token_manager
        .access_token()
        .await
        .map_err(|e| e.to_string())?;
    for id in ids {
        // This is GET /channels/followers total-only, never Chatters or a chat
        // subscription, and it creates no presence in another channel.
        let result = helix
            .get_followers_total(&id, Some(&token))
            .await
            .map_err(|e| e.to_string())?;
        sqlx::query("UPDATE category_channels SET followers_total=$2,followers_checked_at=now(),followers_http_status=$3 WHERE user_id=$1")
            .bind(id).bind(result.total).bind(result.http_status.map(i32::from)).execute(pool).await.map_err(|e|e.to_string())?;
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Ok(())
}
