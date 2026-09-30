use std::str::FromStr;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

#[path = "../../../test-support/database.rs"]
mod test_database;

#[tokio::test]
async fn unlearnable_new_corpus_preserves_existing_template_and_timestamp() {
    let Some(dsn) = test_database::database_url() else {
        assert!(
            !test_database::required(),
            "PostgreSQL test config is required"
        );
        return;
    };
    let admin_options = PgConnectOptions::from_str(&dsn).expect("parse configured test DSN");
    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(admin_options.clone())
        .await
        .expect("connect configured test database");
    let db_name = format!(
        "clip_learning_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {db_name}")))
        .execute(&admin)
        .await
        .expect("create isolated learning database");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(admin_options.database(&db_name))
        .await
        .expect("connect isolated learning database");
    sqlx::raw_sql(
        "CREATE TABLE twitch_clip_context_runs (
             clip_id text PRIMARY KEY, moment_offset_s integer NOT NULL, visual_status text NOT NULL,
             peak_s integer, recommended_start_s integer, recommended_end_s integer
         );
         CREATE TABLE twitch_clip_context_seconds (
             clip_id text NOT NULL, vod_second integer NOT NULL, lufs double precision,
             peak_dbfs double precision, speech text, laughter boolean, exclamation boolean,
             kill_feed text, souls integer, soul_jump integer, objective boolean,
             death_screen boolean, scene_change boolean, chat_messages integer NOT NULL DEFAULT 0,
             ocr_sampled boolean NOT NULL DEFAULT false, PRIMARY KEY (clip_id, vod_second)
         );
         CREATE TABLE twitch_clip_cut_templates (
             name text PRIMARY KEY, weights jsonb NOT NULL, lead_seconds integer NOT NULL,
             trail_seconds integer NOT NULL, sample_count integer NOT NULL,
             updated_at timestamptz NOT NULL DEFAULT now()
         );",
    )
    .execute(&pool)
    .await
    .expect("create minimum learning schema");
    let original_weights = serde_json::json!({
        "audio": 1.0, "speech": 0.0, "laughter": 0.0, "kill": 0.0,
        "souls": 0.0, "objective": 0.0, "death": 0.0, "scene": 0.0, "chat": 0.0
    });
    sqlx::query("INSERT INTO twitch_clip_cut_templates (name,weights,lead_seconds,trail_seconds,sample_count,updated_at) VALUES ('chat_clip_v1',$1,30,20,8,'2026-01-02T03:04:05Z')")
        .bind(&original_weights)
        .execute(&pool)
        .await
        .expect("seed existing valid template");
    sqlx::query("INSERT INTO twitch_clip_context_runs (clip_id,moment_offset_s,visual_status) VALUES ('silent-corpus',90,'sampled')")
        .execute(&pool)
        .await
        .expect("seed non-empty corpus");
    for second in 0..=180 {
        sqlx::query("INSERT INTO twitch_clip_context_seconds (clip_id,vod_second) VALUES ('silent-corpus',$1)")
            .bind(second)
            .execute(&pool)
            .await
            .expect("seed signal-free timeline");
    }

    let learned = tb_social_media::clip_context_harvest::learn_and_store(&pool)
        .await
        .expect("learning attempt completes");
    assert!(
        learned.is_none(),
        "no positive evidence is not a learned model"
    );
    let stored = tb_social_media::clip_context_harvest::read_template(&pool, "chat_clip_v1")
        .await
        .expect("read existing template")
        .expect("existing template remains");
    assert_eq!(stored.weights.audio, 1.0);
    let updated_at: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "SELECT updated_at FROM twitch_clip_cut_templates WHERE name='chat_clip_v1'",
    )
    .fetch_one(&pool)
    .await
    .expect("read template timestamp");
    assert_eq!(updated_at.to_rfc3339(), "2026-01-02T03:04:05+00:00");

    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP DATABASE {db_name}")))
        .execute(&admin)
        .await
        .expect("drop isolated learning database");
    admin.close().await;
}
