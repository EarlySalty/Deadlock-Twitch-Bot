//! Queries fuer `GET /twitch/api/v2/public/network-stats`.

use sqlx::{PgPool, Row};

#[derive(Debug)]
pub struct LivePartnerRow {
    pub login: String,
    pub display_name: String,
    pub started_at: Option<String>,
}

#[derive(Debug)]
pub struct NetworkStatsResult {
    pub active_partners: u64,
    pub raids_total: u64,
    pub raids_7d: u64,
    pub viewers_forwarded_total: Option<u64>,
    pub live: Vec<LivePartnerRow>,
}

fn non_negative_u64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

fn optional_non_negative_u64(value: Option<i64>) -> Option<u64> {
    value.and_then(|v| u64::try_from(v).ok())
}

/// Laedt aggregierte Netzwerk-Metriken und aktuell live streamende aktive Partner.
pub async fn network_stats(pool: &PgPool) -> Result<NetworkStatsResult, sqlx::Error> {
    let partner_row = sqlx::query(
        r#"
        SELECT COUNT(DISTINCT LOWER(TRIM(twitch_login))) AS active_partners
          FROM twitch_streamers_partner_state
         WHERE COALESCE(is_partner_active, 0) = 1
           AND NULLIF(TRIM(twitch_login), '') IS NOT NULL
        "#,
    )
    .fetch_one(pool)
    .await?;
    let active_partners = non_negative_u64(partner_row.try_get::<i64, _>("active_partners")?);

    let raid_row = sqlx::query(
        r#"
        SELECT
            COUNT(*) FILTER (WHERE success IS TRUE) AS raids_total,
            COUNT(*) FILTER (
                WHERE success IS TRUE
                  AND executed_at >= NOW() - INTERVAL '7 days'
            ) AS raids_7d,
            SUM(viewer_count) FILTER (
                WHERE success IS TRUE
                  AND viewer_count IS NOT NULL
            ) AS viewers_forwarded_total
          FROM twitch_raid_history
        "#,
    )
    .fetch_one(pool)
    .await?;
    let raids_total = non_negative_u64(raid_row.try_get::<i64, _>("raids_total")?);
    let raids_7d = non_negative_u64(raid_row.try_get::<i64, _>("raids_7d")?);
    let viewers_forwarded_total =
        optional_non_negative_u64(raid_row.try_get::<Option<i64>, _>("viewers_forwarded_total")?);

    let rows = sqlx::query(
        r#"
        SELECT DISTINCT ON (LOWER(COALESCE(NULLIF(TRIM(ls.streamer_login), ''), NULLIF(TRIM(sp.twitch_login), ''))))
               LOWER(COALESCE(NULLIF(TRIM(ls.streamer_login), ''), NULLIF(TRIM(sp.twitch_login), ''))) AS login,
               COALESCE(NULLIF(TRIM(sp.twitch_login), ''), NULLIF(TRIM(ls.streamer_login), ''), '') AS display_name,
               NULLIF(TRIM(ls.last_started_at), '') AS started_at
          FROM twitch_live_state ls
          JOIN twitch_streamers_partner_state sp
            ON (
                NULLIF(TRIM(ls.twitch_user_id), '') IS NOT NULL
                AND NULLIF(TRIM(sp.twitch_user_id), '') IS NOT NULL
                AND ls.twitch_user_id = sp.twitch_user_id
               )
            OR LOWER(ls.streamer_login) = LOWER(sp.twitch_login)
         WHERE COALESCE(ls.is_live, 0) = 1
           AND COALESCE(ls.active_session_id, 0) <> 0
           AND COALESCE(sp.is_partner_active, 0) = 1
           AND COALESCE(NULLIF(TRIM(ls.streamer_login), ''), NULLIF(TRIM(sp.twitch_login), '')) IS NOT NULL
         ORDER BY LOWER(COALESCE(NULLIF(TRIM(ls.streamer_login), ''), NULLIF(TRIM(sp.twitch_login), ''))) ASC,
                  NULLIF(TRIM(ls.last_started_at), '') DESC NULLS LAST
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut live = Vec::with_capacity(rows.len());
    for row in rows {
        live.push(LivePartnerRow {
            login: row.try_get("login")?,
            display_name: row.try_get("display_name")?,
            started_at: row.try_get("started_at")?,
        });
    }

    Ok(NetworkStatsResult {
        active_partners,
        raids_total,
        raids_7d,
        viewers_forwarded_total,
        live,
    })
}
