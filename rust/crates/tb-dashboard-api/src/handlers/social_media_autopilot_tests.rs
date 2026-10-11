use std::str::FromStr;

use super::*;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use tb_social_media::autopilot_settings::{ClipDateFilter, ClipSort};

fn query(owner: Option<&str>) -> Query<OwnerQuery> {
    Query(OwnerQuery {
        twitch_user_id: owner.map(str::to_owned),
        limit: None,
        offset: None,
    })
}

fn partner() -> DashboardAuthLevel {
    DashboardAuthLevel::Partner {
        twitch_login: "outdated_login".into(),
        twitch_user_id: "42".into(),
        display_name: String::new(),
    }
}

#[tokio::test]
async fn pool_api_uses_session_id_and_existing_access_policy_without_enabling_autopilot() {
    let dsn = std::env::var("TB_TEST_DATABASE_URL").expect("owned local test database required");
    let schema = "t_m1_wf5f4_api_scope";
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP SCHEMA IF EXISTS {schema} CASCADE"
    )))
    .execute(&admin)
    .await
    .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    let options = PgConnectOptions::from_str(&dsn)
        .unwrap()
        .options([("search_path", schema)]);
    let pool = PgPoolOptions::new()
        .max_connections(3)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::raw_sql("CREATE TABLE twitch_streamers (twitch_login TEXT PRIMARY KEY, twitch_user_id TEXT UNIQUE);
        INSERT INTO twitch_streamers VALUES ('renamed_channel', '42'), ('other', '99');
        CREATE TABLE social_media_partner_access (twitch_user_id TEXT PRIMARY KEY, granted BOOLEAN);
        INSERT INTO social_media_partner_access VALUES ('42', TRUE);
        CREATE TABLE social_media_autopilot_settings (twitch_user_id TEXT PRIMARY KEY, enabled BOOLEAN DEFAULT FALSE,
            ai_editing_enabled BOOLEAN DEFAULT TRUE, clip_date_filter JSONB DEFAULT '{\"kind\":\"all\"}',
            clip_sort TEXT DEFAULT 'oldest_first', timezone TEXT DEFAULT 'Europe/Berlin', updated_at TIMESTAMPTZ)")
        .execute(&pool).await.unwrap();
    assert_eq!(
        pool_handler(DashboardAuthLevel::None, State(pool.clone()), query(None))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        preferences_get_handler(partner(), State(pool.clone()), query(Some("99")))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        import_start_handler(partner(), State(pool.clone()), query(Some("99")))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        selection_handler(
            partner(),
            State(pool.clone()),
            query(Some("99")),
            Path("foreign".into()),
            Json(SelectionInput { selected: true })
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        preferences_get_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            query(None)
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let response = preferences_put_handler(
        partner(),
        State(pool.clone()),
        query(None),
        Json(PoolPreferences {
            clip_date_filter: ClipDateFilter::Year { year: 2025 },
            clip_sort: ClipSort::NewestFirst,
            timezone: "America/New_York".into(),
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let stored: (String, bool, bool) = sqlx::query_as("SELECT timezone, enabled, ai_editing_enabled FROM social_media_autopilot_settings WHERE twitch_user_id = '42'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(stored, ("America/New_York".into(), false, true));
    assert_eq!(
        target(&partner(), &pool, None).await.unwrap(),
        ("renamed_channel".into(), "42".into())
    );
    sqlx::query(
        "UPDATE social_media_partner_access SET granted = FALSE WHERE twitch_user_id = '42'",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        preferences_get_handler(partner(), State(pool), query(None))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
}

#[test]
fn pool_api_rejects_unowned_settings_fields_and_maps_storage_failures_to_server_errors() {
    assert!(serde_json::from_value::<PoolPreferences>(
        json!({"clip_date_filter":{"kind":"all"},"clip_sort":"oldest_first",
        "timezone":"Europe/Berlin","enabled":true})
    )
    .is_err());
    assert!(serde_json::from_value::<SelectionInput>(
        json!({"selected":true,"twitch_user_id":"99"})
    )
    .is_err());
    assert_eq!(
        error_response(PoolError::Settings(
            tb_social_media::autopilot_settings::SettingsError::Database(sqlx::Error::PoolClosed)
        ))
        .status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
}
