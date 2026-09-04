use sqlx::PgPool;

#[derive(Debug, Clone)]
pub struct Eintrag {
    pub twitch_user_id: String,
    pub page: Option<String>,
    pub language: String,
    pub question: String,
    pub answer: String,
    pub grounded: bool,
    pub flagged_injection: bool,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub latency_ms: Option<i64>,
}

pub async fn insert(pool: &PgPool, eintrag: &Eintrag) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO twitch_dashboard_assistent_log \
            (twitch_user_id, page, language, question, answer, grounded, flagged_injection, provider, model, latency_ms) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(&eintrag.twitch_user_id)
    .bind(&eintrag.page)
    .bind(&eintrag.language)
    .bind(&eintrag.question)
    .bind(&eintrag.answer)
    .bind(eintrag.grounded)
    .bind(eintrag.flagged_injection)
    .bind(&eintrag.provider)
    .bind(&eintrag.model)
    .bind(eintrag.latency_ms)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&pool)
            .await
            .expect("Schema droppen");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .expect("Schema anlegen");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path");
        sqlx::query(
            "CREATE TABLE twitch_dashboard_assistent_log (\
                id BIGSERIAL PRIMARY KEY,\
                twitch_user_id TEXT NOT NULL,\
                page TEXT,\
                language TEXT NOT NULL DEFAULT 'de',\
                question TEXT NOT NULL,\
                answer TEXT NOT NULL,\
                grounded BOOLEAN NOT NULL DEFAULT FALSE,\
                flagged_injection BOOLEAN NOT NULL DEFAULT FALSE,\
                provider TEXT,\
                model TEXT,\
                latency_ms BIGINT,\
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()\
            )",
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_dashboard_assistent_log");
        pool
    }

    #[tokio::test]
    async fn insert_schreibt_zeile() {
        let Some(dsn) = test_dsn() else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let pool = make_pool(&dsn, "test_dash_assistent_log").await;

        insert(
            &pool,
            &Eintrag {
                twitch_user_id: "12345".to_string(),
                page: Some("uplink".to_string()),
                language: "de".to_string(),
                question: "Ist mein Spam-Schutz an?".to_string(),
                answer: "Ja, dein Spam-Schutz ist aktiv.".to_string(),
                grounded: true,
                flagged_injection: false,
                provider: Some("fireworks".to_string()),
                model: Some("deepseek-v4-flash".to_string()),
                latency_ms: Some(842),
            },
        )
        .await
        .expect("insert");

        let row: (String, Option<String>, String, bool, bool, Option<i64>) = sqlx::query_as(
            "SELECT twitch_user_id, page, language, grounded, flagged_injection, latency_ms \
               FROM twitch_dashboard_assistent_log LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .expect("select");
        assert_eq!(row.0, "12345");
        assert_eq!(row.1.as_deref(), Some("uplink"));
        assert_eq!(row.2, "de");
        assert!(row.3);
        assert!(!row.4);
        assert_eq!(row.5, Some(842));
    }

    #[tokio::test]
    async fn insert_erlaubt_null_felder() {
        let Some(dsn) = test_dsn() else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let pool = make_pool(&dsn, "test_dash_assistent_log_null").await;

        insert(
            &pool,
            &Eintrag {
                twitch_user_id: "99".to_string(),
                page: None,
                language: "en".to_string(),
                question: "How does the bot work?".to_string(),
                answer: "The bot raids your viewers.".to_string(),
                grounded: false,
                flagged_injection: true,
                provider: None,
                model: None,
                latency_ms: None,
            },
        )
        .await
        .expect("insert ohne optionale Felder");

        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM twitch_dashboard_assistent_log")
                .fetch_one(&pool)
                .await
                .expect("count");
        assert_eq!(count.0, 1);
    }
}
