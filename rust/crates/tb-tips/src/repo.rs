use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tb_knowledge::TipState;

#[derive(Debug, Clone, Default)]
pub struct TipSettings {
    pub opt_out: bool,
    pub last_tip_sent_at: Option<DateTime<Utc>>,
}

pub async fn tip_settings(pool: &PgPool, twitch_user_id: &str) -> Result<TipSettings, sqlx::Error> {
    let row = sqlx::query_as::<_, (bool, Option<DateTime<Utc>>)>(
        "SELECT opt_out, last_tip_sent_at FROM twitch_tip_settings WHERE twitch_user_id = $1",
    )
    .bind(twitch_user_id)
    .fetch_optional(pool)
    .await?;

    Ok(row
        .map(|(opt_out, last_tip_sent_at)| TipSettings {
            opt_out,
            last_tip_sent_at,
        })
        .unwrap_or_default())
}

pub async fn load_tip_state(
    pool: &PgPool,
    twitch_user_id: &str,
    slugs: &[String],
) -> Result<HashMap<String, TipState>, sqlx::Error> {
    let mut out: HashMap<String, TipState> = HashMap::new();

    let usage = sqlx::query_as::<_, (String, i64)>(
        "SELECT feature, FLOOR(EXTRACT(EPOCH FROM (NOW() - last_used_at)) / 86400)::int8 \
         FROM twitch_feature_usage WHERE twitch_user_id = $1",
    )
    .bind(twitch_user_id)
    .fetch_all(pool)
    .await?;
    for (feature, days) in usage {
        out.entry(feature).or_default().feature_used_days_ago = Some(days);
    }

    let shown = sqlx::query_as::<_, (String, i64)>(
        "SELECT tip_slug, FLOOR(EXTRACT(EPOCH FROM (NOW() - MAX(shown_at))) / 86400)::int8 \
         FROM twitch_tip_history WHERE twitch_user_id = $1 GROUP BY tip_slug",
    )
    .bind(twitch_user_id)
    .fetch_all(pool)
    .await?;
    for (slug, days) in shown {
        out.entry(slug).or_default().tip_shown_days_ago = Some(days);
    }

    for slug in slugs {
        out.entry(slug.clone()).or_default();
    }

    Ok(out)
}

pub async fn record_tip_shown(
    pool: &PgPool,
    twitch_user_id: &str,
    slug: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    sqlx::query("INSERT INTO twitch_tip_history (twitch_user_id, tip_slug) VALUES ($1, $2)")
        .bind(twitch_user_id)
        .bind(slug)
        .execute(&mut *tx)
        .await?;

    sqlx::query(
        "INSERT INTO twitch_tip_settings (twitch_user_id, last_tip_sent_at, updated_at) \
         VALUES ($1, NOW(), NOW()) \
         ON CONFLICT (twitch_user_id) DO UPDATE \
         SET last_tip_sent_at = NOW(), updated_at = NOW()",
    )
    .bind(twitch_user_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await
}

pub async fn record_feature_used(
    pool: &PgPool,
    twitch_user_id: &str,
    feature: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO twitch_feature_usage (twitch_user_id, feature, last_used_at, use_count) \
         VALUES ($1, $2, NOW(), 1) \
         ON CONFLICT (twitch_user_id, feature) DO UPDATE \
         SET last_used_at = NOW(), use_count = twitch_feature_usage.use_count + 1",
    )
    .bind(twitch_user_id)
    .bind(feature)
    .execute(pool)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    async fn test_pool() -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let pool = PgPool::connect(&dsn).await.ok()?;
        sqlx::query(include_str!(
            "../../../migrations/20260621070000_golive_tips.sql"
        ))
        .execute(&pool)
        .await
        .ok()?;
        Some(pool)
    }

    fn test_uid(suffix: &str) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        format!("test_tips_{suffix}_{}_{}", std::process::id(), nanos)
    }

    #[tokio::test]
    async fn opt_out_default_false_und_record_setzt_timestamp() {
        let Some(pool) = test_pool().await else {
            eprintln!("skip: kein TB_TEST_DATABASE_URL");
            return;
        };
        let uid = test_uid("settings");
        let s = tip_settings(&pool, &uid).await.unwrap();
        assert!(!s.opt_out);
        assert!(s.last_tip_sent_at.is_none());

        record_tip_shown(&pool, &uid, "auto-raid").await.unwrap();

        let s2 = tip_settings(&pool, &uid).await.unwrap();
        assert!(
            s2.last_tip_sent_at.is_some(),
            "last_tip_sent_at gesetzt nach record"
        );
    }

    #[tokio::test]
    async fn feature_usage_fliesst_in_tip_state() {
        let Some(pool) = test_pool().await else {
            eprintln!("skip: kein TB_TEST_DATABASE_URL");
            return;
        };
        let uid = test_uid("usage");

        record_feature_used(&pool, &uid, "auto-raid").await.unwrap();

        let st = load_tip_state(&pool, &uid, &["auto-raid".to_string()])
            .await
            .unwrap();
        assert_eq!(
            st.get("auto-raid").and_then(|s| s.feature_used_days_ago),
            Some(0)
        );
    }
}
