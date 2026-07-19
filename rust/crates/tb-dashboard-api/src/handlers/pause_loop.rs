//! Datenbankzugriff fuer die OBS-Pause-Loop-Kohorte.

use sqlx::PgPool;

/// Aktiver Partner-Broadcaster, der fuer Pause-Loop-Clips beruecksichtigt wird.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartnerBroadcaster {
    /// Getrimmter Twitch-Login.
    pub twitch_login: String,
    /// Getrimmte Twitch-User-ID.
    pub twitch_user_id: String,
}

/// Laedt aktive Partner mit gueltiger Twitch-Identitaet ohne aktive Exclusion.
pub async fn load_active_partner_broadcasters(
    pool: &PgPool,
) -> Result<Vec<PartnerBroadcaster>, sqlx::Error> {
    sqlx::query_as!(
        PartnerBroadcaster,
        r#"
        SELECT
            BTRIM(partner.twitch_login, E' \t\n\r\f') AS "twitch_login!",
            BTRIM(partner.twitch_user_id, E' \t\n\r\f') AS "twitch_user_id!"
        FROM twitch_streamers_partner_state partner
        WHERE COALESCE(partner.is_partner_active, 0) <> 0
          AND NULLIF(BTRIM(partner.twitch_login, E' \t\n\r\f'), '') IS NOT NULL
          AND NULLIF(BTRIM(partner.twitch_user_id, E' \t\n\r\f'), '') IS NOT NULL
          AND NOT EXISTS (
              SELECT 1
              FROM twitch_exclusions exclusion
              WHERE exclusion.twitch_user_id = partner.twitch_user_id
                AND exclusion.reactivated_at IS NULL
          )
        ORDER BY
            LOWER(BTRIM(partner.twitch_login, E' \t\n\r\f')),
            BTRIM(partner.twitch_login, E' \t\n\r\f'),
            BTRIM(partner.twitch_user_id, E' \t\n\r\f')
        "#
    )
    .fetch_all(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    fn db_dsn_or_skip() -> Option<String> {
        match std::env::var("TB_TEST_DATABASE_URL") {
            Ok(dsn) => Some(dsn),
            Err(_) => {
                if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                    panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                }
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                None
            }
        }
    }

    struct TestDb {
        schema: String,
        admin: PgPool,
        pool: PgPool,
    }

    impl TestDb {
        async fn new(schema_prefix: &str) -> Option<Self> {
            let dsn = db_dsn_or_skip()?;
            assert!(schema_prefix
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'_'));
            let schema = format!("{}_{}", schema_prefix, uuid::Uuid::new_v4().simple());
            let admin = PgPoolOptions::new()
                .max_connections(1)
                .connect(&dsn)
                .await
                .expect("connect admin test-db");

            sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
                .execute(&admin)
                .await
                .expect("drop stale schema");
            sqlx::query(&format!("CREATE SCHEMA {schema}"))
                .execute(&admin)
                .await
                .expect("create schema");

            let opts = PgConnectOptions::from_str(&dsn)
                .expect("parse test dsn")
                .options([("search_path", schema.as_str())]);
            let pool = PgPoolOptions::new()
                .max_connections(2)
                .connect_with(opts)
                .await
                .expect("connect schema-bound pool");

            sqlx::raw_sql(
                r#"
                CREATE TABLE twitch_streamers_partner_state (
                    twitch_login TEXT,
                    twitch_user_id TEXT,
                    is_partner_active INTEGER
                );

                CREATE TABLE twitch_exclusions (
                    twitch_user_id TEXT PRIMARY KEY,
                    kind TEXT NOT NULL,
                    reactivated_at TIMESTAMPTZ
                );
                "#,
            )
            .execute(&pool)
            .await
            .expect("pause loop fixture ddl");

            Some(Self {
                schema,
                admin,
                pool,
            })
        }

        async fn cleanup(self) {
            self.pool.close().await;
            sqlx::query(&format!("DROP SCHEMA IF EXISTS {} CASCADE", self.schema))
                .execute(&self.admin)
                .await
                .expect("drop schema");
            self.admin.close().await;
        }
    }

    async fn insert_partner(
        pool: &PgPool,
        login: Option<&str>,
        user_id: Option<&str>,
        active: Option<i32>,
    ) {
        sqlx::query(
            r#"
            INSERT INTO twitch_streamers_partner_state
                (twitch_login, twitch_user_id, is_partner_active)
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(login)
        .bind(user_id)
        .bind(active)
        .execute(pool)
        .await
        .expect("insert partner fixture");
    }

    async fn insert_exclusion(pool: &PgPool, user_id: &str, reactivated: bool) {
        sqlx::query(
            r#"
            INSERT INTO twitch_exclusions (twitch_user_id, kind, reactivated_at)
            VALUES ($1, 'opt_out', CASE WHEN $2 THEN NOW() ELSE NULL END)
            "#,
        )
        .bind(user_id)
        .bind(reactivated)
        .execute(pool)
        .await
        .expect("insert exclusion fixture");
    }

    #[tokio::test]
    async fn active_partner_query_filters_activity_and_exclusions() {
        let Some(db) = TestDb::new("pause_loop_active_exclusions").await else {
            return;
        };

        insert_partner(&db.pool, Some("active"), Some("100"), Some(1)).await;
        insert_partner(&db.pool, Some("inactive"), Some("101"), Some(0)).await;
        insert_partner(&db.pool, Some("null_active"), Some("102"), None).await;
        insert_partner(&db.pool, Some("excluded"), Some("103"), Some(1)).await;
        insert_partner(&db.pool, Some("reactivated"), Some("104"), Some(1)).await;
        insert_exclusion(&db.pool, "103", false).await;
        insert_exclusion(&db.pool, "104", true).await;

        let rows = load_active_partner_broadcasters(&db.pool).await.unwrap();
        db.cleanup().await;

        assert_eq!(
            rows,
            vec![
                PartnerBroadcaster {
                    twitch_login: "active".to_owned(),
                    twitch_user_id: "100".to_owned(),
                },
                PartnerBroadcaster {
                    twitch_login: "reactivated".to_owned(),
                    twitch_user_id: "104".to_owned(),
                },
            ]
        );
    }

    #[tokio::test]
    async fn active_partner_query_rejects_blank_identity_and_trims_output() {
        let Some(db) = TestDb::new("pause_loop_identity_hygiene").await else {
            return;
        };

        insert_partner(&db.pool, None, Some("200"), Some(1)).await;
        insert_partner(&db.pool, Some(""), Some("201"), Some(1)).await;
        insert_partner(&db.pool, Some("   "), Some("202"), Some(1)).await;
        insert_partner(&db.pool, Some("missing_id"), None, Some(1)).await;
        insert_partner(&db.pool, Some("empty_id"), Some(""), Some(1)).await;
        insert_partner(&db.pool, Some("blank_id"), Some("  \t "), Some(1)).await;
        insert_partner(&db.pool, Some("  TrimMe  "), Some("  203  "), Some(1)).await;

        let rows = load_active_partner_broadcasters(&db.pool).await.unwrap();
        db.cleanup().await;

        assert_eq!(
            rows,
            vec![PartnerBroadcaster {
                twitch_login: "TrimMe".to_owned(),
                twitch_user_id: "203".to_owned(),
            }]
        );
    }

    #[tokio::test]
    async fn active_partner_query_sorts_by_lower_trimmed_login() {
        let Some(db) = TestDb::new("pause_loop_ordering").await else {
            return;
        };

        insert_partner(&db.pool, Some(" zebra "), Some("300"), Some(1)).await;
        insert_partner(&db.pool, Some("Beta"), Some("301"), Some(1)).await;
        insert_partner(&db.pool, Some(" alpha"), Some("302"), Some(1)).await;

        let rows = load_active_partner_broadcasters(&db.pool).await.unwrap();
        db.cleanup().await;

        assert_eq!(
            rows.into_iter()
                .map(|row| row.twitch_login)
                .collect::<Vec<_>>(),
            vec!["alpha", "Beta", "zebra"]
        );
    }

    #[tokio::test]
    async fn active_partner_query_exclusion_nutzt_rohe_id_und_output_trim() {
        let Some(db) = TestDb::new("pause_loop_raw_exclusion").await else {
            return;
        };

        insert_partner(&db.pool, Some("ExcludedRaw"), Some(" 400 "), Some(1)).await;
        insert_partner(&db.pool, Some(" VisibleRaw "), Some(" 401 "), Some(1)).await;
        insert_exclusion(&db.pool, " 400 ", false).await;

        let rows = load_active_partner_broadcasters(&db.pool).await.unwrap();
        db.cleanup().await;

        assert_eq!(
            rows,
            vec![PartnerBroadcaster {
                twitch_login: "VisibleRaw".to_owned(),
                twitch_user_id: "401".to_owned(),
            }]
        );
    }
}
