//! Query für `GET /twitch/api/admin/system/errors` (paginiert).
//!
//! Falls `twitch_admin_error_log` nicht existiert → leere Antwort statt 500.
//! Fehlererkennung: sqlx `Database`-Error dessen Meldung "does not exist" enthält.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// Eine einzelne Error-Log-Zeile.
#[derive(Debug)]
pub struct ErrorLogEntry {
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub level: Option<String>,
    pub message: Option<String>,
    pub context: Option<String>,
}

/// Paginiertes Ergebnis.
#[derive(Debug)]
pub struct ErrorLogPage {
    pub total: i64,
    pub entries: Vec<ErrorLogEntry>,
}

/// Lädt Error-Log-Einträge paginiert aus `twitch_admin_error_log`.
///
/// `page`: 1-basiert, wird auf `>= 1` geclampt.
/// `page_size`: wird auf `[1, 100]` geclampt.
/// Sortierung: `id DESC` (neueste zuerst).
pub async fn error_log_entries(
    pool: &PgPool,
    page: i64,
    page_size: i64,
) -> Result<ErrorLogPage, sqlx::Error> {
    let page_size = page_size.clamp(1, 100);
    let page = page.max(1);
    let offset = (page - 1) * page_size;

    let total_result: Result<(i64,), sqlx::Error> =
        sqlx::query_as("SELECT COUNT(*) FROM twitch_admin_error_log")
            .fetch_one(pool)
            .await;

    let total = match total_result {
        Ok((n,)) => n,
        Err(sqlx::Error::Database(ref e)) if e.message().contains("does not exist") => {
            return Ok(ErrorLogPage {
                total: 0,
                entries: vec![],
            });
        }
        Err(e) => return Err(e),
    };

    if total == 0 {
        return Ok(ErrorLogPage {
            total: 0,
            entries: vec![],
        });
    }

    type ErrorRow = (
        i64,
        DateTime<Utc>,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let rows: Vec<ErrorRow> = sqlx::query_as(
        r#"
            SELECT id, created_at, level, message, context
            FROM twitch_admin_error_log
            ORDER BY id DESC
            LIMIT $1 OFFSET $2
            "#,
    )
    .bind(page_size)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    let entries = rows
        .into_iter()
        .map(|(id, created_at, level, message, context)| ErrorLogEntry {
            id,
            created_at,
            level,
            message,
            context,
        })
        .collect();

    Ok(ErrorLogPage { total, entries })
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
        sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
            .execute(&pool)
            .await
            .expect("Schema");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path");
        pool
    }

    async fn make_pool_with_table(dsn: &str, schema: &str) -> PgPool {
        let pool = make_pool(dsn, schema).await;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_admin_error_log (
                id         BIGSERIAL PRIMARY KEY,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                level      TEXT,
                message    TEXT,
                context    TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL error_log");
        sqlx::query("TRUNCATE twitch_admin_error_log")
            .execute(&pool)
            .await
            .expect("TRUNCATE");
        pool
    }

    #[tokio::test]
    async fn tabelle_nicht_vorhanden_gibt_leere_antwort() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        // Schema ohne die Tabelle — simuliert fehlende Tabelle
        let pool = make_pool(&dsn, "test_syserr_notable").await;
        let page = error_log_entries(&pool, 1, 25).await.unwrap();
        assert_eq!(page.total, 0);
        assert!(page.entries.is_empty());
    }

    #[tokio::test]
    async fn leere_tabelle_gibt_leere_antwort() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool_with_table(&dsn, "test_syserr_leer").await;
        let page = error_log_entries(&pool, 1, 25).await.unwrap();
        assert_eq!(page.total, 0);
        assert!(page.entries.is_empty());
    }

    #[tokio::test]
    async fn paginierung_funktioniert() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool_with_table(&dsn, "test_syserr_pages").await;
        for i in 0..5i64 {
            sqlx::query("INSERT INTO twitch_admin_error_log (level, message) VALUES ('ERROR', $1)")
                .bind(format!("Fehler {i}"))
                .execute(&pool)
                .await
                .unwrap();
        }
        let page1 = error_log_entries(&pool, 1, 3).await.unwrap();
        assert_eq!(page1.total, 5);
        assert_eq!(page1.entries.len(), 3);

        let page2 = error_log_entries(&pool, 2, 3).await.unwrap();
        assert_eq!(page2.total, 5);
        assert_eq!(page2.entries.len(), 2);
    }
}
