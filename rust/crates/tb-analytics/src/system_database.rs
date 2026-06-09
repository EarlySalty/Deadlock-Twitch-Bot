//! Query für `GET /twitch/api/admin/system/database`.
//!
//! Row-Count via `pg_class.reltuples` (Schätzung, wie Python-Seite),
//! Size via `pg_total_relation_size`. Tabellen die nicht existieren
//! werden einfach weggelassen — keine 500-Fehler.

use sqlx::PgPool;

/// Größe einer einzelnen Tabelle.
#[derive(Debug)]
pub struct TableStat {
    pub table: String,
    pub row_count: i64,
    pub size_bytes: i64,
}

/// DB-Gesamtstatistik.
#[derive(Debug)]
pub struct DatabaseStats {
    pub database_size_bytes: i64,
    pub tables: Vec<TableStat>,
}

/// Lädt DB-Größe + Row-Count/Size für die angegebenen Tabellen.
///
/// Tabellen die im aktuellen `search_path`-Schema nicht existieren werden
/// weggelassen. Kein Fehler bei fehlender Tabelle.
pub async fn database_stats(pool: &PgPool, tables: &[&str]) -> Result<DatabaseStats, sqlx::Error> {
    let (database_size_bytes,): (i64,) =
        sqlx::query_as("SELECT pg_database_size(current_database())::BIGINT")
            .fetch_one(pool)
            .await?;

    let mut table_stats: Vec<TableStat> = Vec::new();

    for &table in tables {
        // Prüfen ob Tabelle im aktuellen Schema existiert
        let row_count: Option<(i64,)> = sqlx::query_as(
            r#"
            SELECT reltuples::BIGINT
            FROM pg_class
            WHERE relname = $1
              AND relnamespace = (
                  SELECT oid FROM pg_namespace WHERE nspname = current_schema()
              )
            "#,
        )
        .bind(table)
        .fetch_optional(pool)
        .await?;

        if let Some((count,)) = row_count {
            let size_result: Result<(i64,), sqlx::Error> =
                sqlx::query_as("SELECT pg_total_relation_size($1::regclass)::BIGINT")
                    .bind(table)
                    .fetch_one(pool)
                    .await;
            let size_bytes = size_result.unwrap_or((0,)).0;

            table_stats.push(TableStat {
                table: table.to_string(),
                row_count: count.max(0),
                size_bytes,
            });
        }
        // Tabelle existiert nicht → weglassen (kein Fehler)
    }

    Ok(DatabaseStats {
        database_size_bytes,
        tables: table_stats,
    })
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
        sqlx::query("CREATE TABLE IF NOT EXISTS test_table_a (id BIGSERIAL PRIMARY KEY, val TEXT)")
            .execute(&pool)
            .await
            .expect("DDL test_table_a");
        sqlx::query("TRUNCATE test_table_a")
            .execute(&pool)
            .await
            .expect("TRUNCATE");
        pool
    }

    #[tokio::test]
    async fn existierende_tabelle_wird_zurueckgegeben() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_sysdb_exist").await;
        let stats = database_stats(&pool, &["test_table_a"]).await.unwrap();
        assert!(stats.database_size_bytes > 0);
        assert_eq!(stats.tables.len(), 1);
        assert_eq!(stats.tables[0].table, "test_table_a");
    }

    #[tokio::test]
    async fn nicht_existierende_tabelle_wird_weggelassen() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_sysdb_nonexist").await;
        let stats = database_stats(&pool, &["tabelle_existiert_nicht"])
            .await
            .unwrap();
        assert!(stats.tables.is_empty());
    }

    #[tokio::test]
    async fn mix_existierend_und_fehlend() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_sysdb_mix").await;
        let stats = database_stats(&pool, &["test_table_a", "ghost_table"])
            .await
            .unwrap();
        assert_eq!(stats.tables.len(), 1);
        assert_eq!(stats.tables[0].table, "test_table_a");
    }
}
