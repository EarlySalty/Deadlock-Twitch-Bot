//! DB-Schicht für `twitch_self_explainer_log`.
//!
//! Die Tabelle wird vom Dashboard beim Beantworten von Streamer-Fragen
//! beschrieben (`bot/dashboard/routes_self_explainer.py`).  Die interne
//! API-Route `POST /discord/self-explainer-log` ist ein reiner HTTP-Relay zum
//! Master-Broker und schreibt selbst NICHT in diese Tabelle — diese Schicht ist
//! dennoch bereitgestellt, damit der künftige Dashboard-Port darauf zugreifen
//! kann ohne ein neues Crate-Modul anzulegen.
//!
//! Prod-DDL (verifiziert aus `routes_self_explainer.py`):
//! ```sql
//! CREATE TABLE IF NOT EXISTS twitch_self_explainer_log (
//!     id                BIGSERIAL PRIMARY KEY,
//!     question          TEXT NOT NULL,
//!     answer            TEXT NOT NULL,
//!     grounded          BOOLEAN NOT NULL DEFAULT FALSE,
//!     flagged_injection BOOLEAN NOT NULL DEFAULT FALSE,
//!     peer              TEXT,
//!     created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
//! )
//! ```

use sqlx::PgPool;

/// Ein Log-Eintrag aus `twitch_self_explainer_log`.
#[derive(Debug, sqlx::FromRow)]
pub struct SelfExplainerLogEntry {
    pub id: i64,
    pub question: String,
    pub answer: String,
    pub grounded: bool,
    pub flagged_injection: bool,
    pub peer: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Schreibt eine Frage+Antwort in `twitch_self_explainer_log`.
///
/// Parität zur Python-INSERT in `_log_to_db_sync`:
/// `(question, answer, grounded, flagged_injection, peer)`.
pub async fn insert(
    pool: &PgPool,
    question: &str,
    answer: &str,
    grounded: bool,
    flagged_injection: bool,
    peer: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO twitch_self_explainer_log
            (question, answer, grounded, flagged_injection, peer)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(question)
    .bind(answer)
    .bind(grounded)
    .bind(flagged_injection)
    .bind(peer)
    .execute(pool)
    .await?;
    Ok(())
}

/// Liest die neuesten `limit` Einträge (neueste zuerst).
pub async fn list_recent(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<SelfExplainerLogEntry>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT id, question, answer, grounded, flagged_injection, peer, created_at
        FROM twitch_self_explainer_log
        ORDER BY created_at DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    macro_rules! db_dsn_or_skip {
        () => {
            match test_dsn() {
                Some(d) => d,
                None => {
                    if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                        panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                    }
                    eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                    return;
                }
            }
        };
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
            r#"
            CREATE TABLE twitch_self_explainer_log (
                id                BIGSERIAL PRIMARY KEY,
                question          TEXT NOT NULL,
                answer            TEXT NOT NULL,
                grounded          BOOLEAN NOT NULL DEFAULT FALSE,
                flagged_injection BOOLEAN NOT NULL DEFAULT FALSE,
                peer              TEXT,
                created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_self_explainer_log");

        pool
    }

    #[tokio::test]
    async fn insert_und_list_recent() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sel_log_insert").await;

        insert(&pool, "Wie funktioniert der Bot?", "Der Bot macht X.", true, false, Some("1.2.3.4"))
            .await
            .expect("insert");

        let entries = list_recent(&pool, 10).await.expect("list");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].question, "Wie funktioniert der Bot?");
        assert_eq!(entries[0].answer, "Der Bot macht X.");
        assert!(entries[0].grounded);
        assert!(!entries[0].flagged_injection);
        assert_eq!(entries[0].peer.as_deref(), Some("1.2.3.4"));
    }

    #[tokio::test]
    async fn peer_null_erlaubt() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sel_log_null_peer").await;

        insert(&pool, "Frage", "Antwort", false, false, None)
            .await
            .expect("insert ohne peer");

        let entries = list_recent(&pool, 1).await.expect("list");
        assert_eq!(entries.len(), 1);
        assert!(entries[0].peer.is_none());
    }

    #[tokio::test]
    async fn list_recent_neueste_zuerst() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sel_log_order").await;

        // Explizite created_at-Werte für deterministischen Reihenfolge-Test.
        sqlx::query(
            r#"
            INSERT INTO twitch_self_explainer_log
                (question, answer, grounded, flagged_injection, peer, created_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind("Alte Frage")
        .bind("Alte Antwort")
        .bind(false)
        .bind(false)
        .bind(Option::<String>::None)
        .bind(chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z").unwrap().with_timezone(&chrono::Utc))
        .execute(&pool)
        .await
        .expect("insert alt");

        sqlx::query(
            r#"
            INSERT INTO twitch_self_explainer_log
                (question, answer, grounded, flagged_injection, peer, created_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind("Neue Frage")
        .bind("Neue Antwort")
        .bind(true)
        .bind(false)
        .bind(Option::<String>::None)
        .bind(chrono::DateTime::parse_from_rfc3339("2026-06-01T00:00:00Z").unwrap().with_timezone(&chrono::Utc))
        .execute(&pool)
        .await
        .expect("insert neu");

        let entries = list_recent(&pool, 10).await.expect("list");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].question, "Neue Frage", "neueste zuerst");
        assert_eq!(entries[1].question, "Alte Frage");
    }

    #[tokio::test]
    async fn list_recent_limit_greift() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sel_log_limit").await;

        for i in 0..5 {
            insert(&pool, &format!("Frage {i}"), "Antwort", false, false, None)
                .await
                .expect("insert");
        }

        let entries = list_recent(&pool, 3).await.expect("list mit limit=3");
        assert_eq!(entries.len(), 3);
    }
}
