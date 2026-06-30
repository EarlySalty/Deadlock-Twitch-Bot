//! Handler für `GET /twitch/api/v2/follower-funnel`.
//!
//! Port von `bot/analytics/api_audience.py:_api_v2_follower_funnel` (Z.541–750).
//! Berechnet Follower-Conversion-Funnel aus Sessions, Chatter-Tracking, Follow-Events
//! und Raid-History. Wie Python (`_require_extended_plan`) hinter dem
//! Extended-Plan-/Trial-Gate (`crate::auth::extended_gate`).

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::json;
use sqlx::{PgPool, Row};

use crate::auth::level::DashboardAuthLevel;

// Bot-Exclusion aus chatter_tracking.rs
const KNOWN_CHAT_BOTS: &[&str] = &[
    "botrix",
    "deutschedeadlockcommunity",
    "fossabot",
    "moobot",
    "nightbot",
    "pretzelrocks",
    "soundalerts",
    "streamlabs",
    "streamelements",
    "wizebot",
];

#[derive(Deserialize)]
pub struct FunnelQuery {
    #[serde(default)]
    pub streamer: Option<String>,
    #[serde(default)]
    pub days: Option<i32>,
}

fn clamp(v: i32, min: i32, max: i32) -> i32 {
    v.max(min).min(max)
}

/// Schwelle für `dataQuality.confidence = "high"` (Python `api_audience.py:715`).
///
/// Python: `max(3, int(session_count * 0.6))` — der Faktor wird ZUERST per
/// `int()` abgeschnitten, dann clamped `max(3, …)` die **Schwelle** (nicht
/// `session_count`). Für `session_count` 1–4 ist die Schwelle daher immer 3.
/// (P2.93: vorher `session_count.max(3) * 3 / 5`, das `session_count` vor der
/// Multiplikation clampt und für 1–4 zu niedrige Schwellen 1/2 liefert.)
fn confidence_threshold(session_count: i64) -> i64 {
    (session_count * 3 / 5).max(3)
}

/// `GET /twitch/api/v2/follower-funnel?streamer=&days=30`
pub async fn follower_funnel_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<FunnelQuery>,
) -> impl IntoResponse {
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }

    let streamer =
        match crate::auth::resolve_streamer_scope(&auth, params.streamer.as_deref(), true) {
            Ok(Some(s)) => s,
            Ok(None) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error":"Streamer required"})),
                )
                    .into_response()
            }
            Err(resp) => return resp,
        };
    let days = clamp(params.days.unwrap_or(30), 7, 365);
    let since: DateTime<Utc> = Utc::now() - Duration::days(days as i64);

    // ── 1. Session-Stats ─────────────────────────────────────────────────────
    let stats = sqlx::query!(
        r#"SELECT
               COUNT(*) AS "session_count!",
               COALESCE(SUM(s.duration_seconds), 0)::float8 AS "total_duration!",
               AVG(s.avg_viewers) AS "avg_viewers?",
               SUM(CASE WHEN s.follower_delta IS NOT NULL
                        AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                        THEN s.follower_delta ELSE 0 END) AS "net_followers?",
               SUM(CASE WHEN s.follower_delta > 0
                        AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                        THEN s.follower_delta ELSE 0 END) AS "gained_followers?",
               COUNT(CASE WHEN s.follower_delta IS NOT NULL
                        AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                        THEN 1 END) AS "follower_valid_samples!"
           FROM twitch_stream_sessions s
           WHERE s.started_at >= $1
             AND LOWER(s.streamer_login) = $2
             AND s.ended_at IS NOT NULL"#,
        since,
        &streamer
    )
    .fetch_one(&pool)
    .await;

    let stats = match stats {
        Err(e) => {
            tracing::error!("follower-funnel stats-Fehler: {e}");
            return crate::auth::analytics_request_failed_json().into_response();
        }
        Ok(r) => r,
    };

    let session_count = stats.session_count;
    if session_count == 0 {
        return Json(json!({
            "uniqueViewers": 0, "returningViewers": 0,
            "newFollowers": 0, "followsDuringStream": 0, "netFollowerDelta": 0,
            "conversionRate": 0, "conversionDataSource": "session_delta_fallback",
            "avgTimeToFollow": 0,
            "followersBySource": {"organic":0,"raids":0,"hosts":0,"other":0},
            "dataQuality": {"confidence":"low","reason":"no_sessions"},
        }))
        .into_response();
    }

    let total_duration = stats.total_duration;
    let net_followers = stats.net_followers.unwrap_or(0);
    let gained_followers = stats.gained_followers.unwrap_or(0);
    let follower_valid_samples = stats.follower_valid_samples;

    // ── 2. Chatter-Stats (bot-bereinigt) ─────────────────────────────────────
    // $1=since, $2=streamer_login, $3..$N+2 = Bot-Logins
    let bot_clause = {
        let ph: Vec<String> = (3..=(KNOWN_CHAT_BOTS.len() + 2))
            .map(|i| format!("${i}"))
            .collect();
        format!("sc.chatter_login NOT IN ({})", ph.join(", "))
    };
    let chatter_sql = format!(
        r#"SELECT
               COUNT(DISTINCT COALESCE(NULLIF(sc.chatter_login,''), sc.chatter_id)) AS unique_chatters,
               COUNT(DISTINCT CASE
                   WHEN LOWER(COALESCE(CAST(sc.is_first_time_streamer AS TEXT),'0'))
                        NOT IN ('1','t','true')
                   THEN COALESCE(NULLIF(sc.chatter_login,''), sc.chatter_id)
               END) AS returning_chatters,
               COUNT(DISTINCT COALESCE(NULLIF(sc.chatter_login,''), sc.chatter_id)) AS tracked_viewers
           FROM twitch_session_chatters sc
           JOIN twitch_stream_sessions s ON s.id = sc.session_id
           WHERE s.started_at >= $1
             AND LOWER(s.streamer_login) = $2
             AND s.ended_at IS NOT NULL
             AND {bot_clause}"#
    );
    let mut cq = sqlx::query(&chatter_sql).bind(since).bind(&streamer);
    for bot in KNOWN_CHAT_BOTS {
        cq = cq.bind(*bot);
    }
    let chatter_stats = cq.fetch_optional(&pool).await.unwrap_or(None);

    let unique_chatters: i64 = chatter_stats
        .as_ref()
        .and_then(|r| r.try_get("unique_chatters").ok())
        .unwrap_or(0);
    let returning_chatters: i64 = chatter_stats
        .as_ref()
        .and_then(|r| r.try_get("returning_chatters").ok())
        .unwrap_or(0);
    let total_viewers_tracked: i64 = chatter_stats
        .as_ref()
        .and_then(|r| r.try_get("tracked_viewers").ok())
        .unwrap_or(0);

    // ── 3. Follow-Events während Streams ─────────────────────────────────────
    let follow_row = sqlx::query!(
        r#"SELECT COUNT(DISTINCT fe.id) AS "follows_during_stream!"
           FROM twitch_follow_events fe
           JOIN twitch_stream_sessions ss
               ON ss.streamer_login = fe.streamer_login
              AND fe.followed_at BETWEEN ss.started_at AND COALESCE(ss.ended_at, NOW())
           WHERE LOWER(ss.streamer_login) = $1
             AND ss.started_at >= $2
             AND ss.ended_at IS NOT NULL"#,
        &streamer,
        since
    )
    .fetch_optional(&pool)
    .await
    .unwrap_or(None);
    let follows_during_stream: i64 = follow_row
        .as_ref()
        .map(|r| r.follows_during_stream)
        .unwrap_or(0);

    // ── 4. Raid-Inflow ────────────────────────────────────────────────────────
    let raid_row = sqlx::query!(
        r#"SELECT COUNT(*) AS "raid_count!", COALESCE(SUM(viewer_count), 0) AS "raid_viewers!"
           FROM twitch_raid_history
           WHERE LOWER(to_broadcaster_login) = $1
             AND executed_at >= $2
             AND COALESCE(success, FALSE) IS TRUE"#,
        &streamer,
        since
    )
    .fetch_optional(&pool)
    .await
    .unwrap_or(None);
    let raid_count: i64 = raid_row.as_ref().map(|r| r.raid_count).unwrap_or(0);
    let raid_viewers: i64 = raid_row.as_ref().map(|r| r.raid_viewers).unwrap_or(0);

    // ── Arithmetik (identisch Python Z. 690–730) ──────────────────────────────
    let unique_viewers = if total_viewers_tracked > 0 {
        total_viewers_tracked
    } else {
        unique_chatters
    };
    let unique_viewers_method = if unique_viewers > 0 {
        "distinct_session_chatters"
    } else {
        "no_viewer_data"
    };

    let (conversion_source, gained_for_conv) = if follows_during_stream > 0 {
        ("follow_events", follows_during_stream)
    } else {
        ("session_delta_fallback", gained_followers)
    };

    let conversion_rate = if unique_viewers > 0 {
        (gained_for_conv as f64 / unique_viewers as f64) * 100.0
    } else {
        0.0
    };

    let avg_session_mins = if total_duration > 0.0 {
        total_duration / session_count.max(1) as f64 / 60.0
    } else {
        0.0
    };
    let avg_time_to_follow = (avg_session_mins * 0.4).clamp(5.0, 45.0);

    let raid_followers = (raid_viewers as f64 * 0.05) as i64;
    let raid_followers = raid_followers.min(gained_for_conv);
    let organic_followers = (gained_for_conv - raid_followers).max(0);

    let confidence = if unique_viewers == 0 {
        "low"
    } else if follower_valid_samples >= confidence_threshold(session_count) {
        "high"
    } else if follower_valid_samples >= 1 {
        "medium"
    } else {
        "low"
    };

    Json(json!({
        "uniqueViewers": unique_viewers,
        "returningViewers": returning_chatters,
        "newFollowers": gained_for_conv,
        "followsDuringStream": follows_during_stream,
        "netFollowerDelta": net_followers,
        "conversionRate": (conversion_rate * 100.0).round() / 100.0,
        "conversionDataSource": conversion_source,
        "avgTimeToFollow": avg_time_to_follow.round(),
        "followersBySource": {
            "organic": organic_followers,
            "raids": raid_followers,
            "hosts": 0,
            "other": 0,
        },
        "dataQuality": {
            "confidence": confidence,
            "sessions": session_count,
            "followerValidSamples": follower_valid_samples,
            "raidEvents": raid_count,
            "uniqueViewersMethod": unique_viewers_method,
            "botFilterApplied": true,
        },
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::confidence_threshold;

    /// P2.93: Für niedrige Session-Counts (1–4) muss die "high"-Schwelle auf 3
    /// geclamped sein (wie Python `max(3, int(session_count*0.6))`), nicht auf
    /// 1/2 wie bei der fehlerhaften `session_count.max(3)*3/5`-Formel.
    #[test]
    fn confidence_threshold_clamps_low_session_counts_to_three() {
        // session_count 1–4 → Schwelle immer 3.
        for sc in 1..=4 {
            assert_eq!(
                confidence_threshold(sc),
                3,
                "session_count={sc} muss Schwelle 3 liefern"
            );
        }
        // Konkretes Audit-Beispiel: session_count=1, samples=1 → NICHT 'high'.
        assert!(
            1 < confidence_threshold(1),
            "1 Sample bei 1 Session darf 'high' nicht erreichen"
        );
    }

    /// Ab session_count 5 stimmen alte und neue Formel überein (floor(sc*0.6)).
    #[test]
    fn confidence_threshold_matches_python_for_higher_counts() {
        assert_eq!(confidence_threshold(5), 3);
        assert_eq!(confidence_threshold(6), 3);
        assert_eq!(confidence_threshold(10), 6);
        assert_eq!(confidence_threshold(100), 60);
    }
}

#[cfg(test)]
mod idor_tests {
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
        Some(
            PgPoolOptions::new()
                .max_connections(2)
                .connect_with(opts)
                .await
                .unwrap(),
        )
    }

    /// Richtet ein berechtigtes Partner-Plan-Snapshot ein (Manual-Override mit
    /// Analytics-Plan), damit `extended_gate` für den Partner passiert.
    async fn grant_partner_analytics(pool: &PgPool, login: &str) {
        sqlx::query(
            "CREATE TABLE streamer_plans (twitch_user_id TEXT, twitch_login TEXT, manual_plan_id TEXT, manual_plan_expires_at TEXT, manual_plan_notes TEXT, manual_plan_updated_at TEXT)",
        ).execute(pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_billing_subscriptions (customer_reference TEXT, plan_id TEXT, status TEXT, current_period_end TEXT, updated_at TEXT)")
            .execute(pool).await.unwrap();
        sqlx::query("INSERT INTO streamer_plans (twitch_login, manual_plan_id) VALUES ($1, 'analysis_dashboard')")
            .bind(login).execute(pool).await.unwrap();
    }

    fn partner(login: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: login.to_string(),
            twitch_user_id: String::new(),
            display_name: login.to_string(),
        }
    }

    /// IDOR: ein berechtigter Partner darf NICHT den Follower-Funnel eines fremden
    /// Streamers lesen (`?streamer=<fremd>` → 403).
    #[tokio::test]
    async fn partner_fremder_streamer_ist_forbidden() {
        let Some(pool) = make_pool("t_funnel_idor").await else {
            return;
        };
        grant_partner_analytics(&pool, "earlysalty").await;
        let resp = follower_funnel_handler(
            partner("earlysalty"),
            State(pool),
            Query(FunnelQuery {
                streamer: Some("ismile_e".into()),
                days: Some(30),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
