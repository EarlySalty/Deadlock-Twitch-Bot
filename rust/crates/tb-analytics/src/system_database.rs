//! Query für `GET /twitch/api/admin/system/database`.
//!
//! Row-Count via exaktem `COUNT(*)` (Parität zu Python
//! `admin_streamer_queries.py:827`), Size via `pg_total_relation_size`.
//! Tabellen die nicht existieren werden einfach weggelassen — keine 500-Fehler.

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
    let database_size_bytes = sqlx::query_scalar!(
        "SELECT pg_database_size(current_database())::BIGINT AS \"database_size_bytes!\"",
    )
    .fetch_one(pool)
    .await?;

    let mut table_stats: Vec<TableStat> = Vec::new();

    for &table in tables {
        // Prüfen ob Tabelle im aktuellen Schema existiert. Der relname aus
        // pg_class ist die kanonische, im Schema existierende Relation — er
        // dient als Allowlist-Quelle für den nachfolgenden COUNT(*)-Identifier.
        let existing = sqlx::query_scalar!(
            r#"
            SELECT relname AS "relname!"
            FROM pg_class
            WHERE relname = $1
              AND relnamespace = (
                  SELECT oid FROM pg_namespace WHERE nspname = current_schema()
              )
            "#,
            table
        )
        .fetch_optional(pool)
        .await?;

        if let Some(relname) = existing {
            // Exakter Row-Count (Python-Parität): reltuples ist nur eine
            // ANALYZE-/autovacuum-gepflegte Schätzung und meldet 0 für frisch
            // befüllte, nie analysierte Tabellen. Identifier wird quote-escaped;
            // die Quelle ist der verifizierte pg_class.relname (keine Injection).
            let quoted = format!("\"{}\"", relname.replace('"', "\"\""));
            let (count,): (i64,) =
                sqlx::query_as(&format!("SELECT COUNT(*)::BIGINT FROM {quoted}"))
                    .fetch_one(pool)
                    .await?;

            let size_result = sqlx::query_scalar!(
                "SELECT pg_total_relation_size($1::text::regclass)::BIGINT AS \"size_bytes!\"",
                &relname
            )
            .fetch_one(pool)
            .await;
            let size_bytes = size_result.unwrap_or(0);

            table_stats.push(TableStat {
                table: relname,
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
    async fn row_count_ist_exakt_ohne_analyze() {
        // P2.87: Frisch befüllte, nie analysierte Tabelle → reltuples wäre 0,
        // COUNT(*) muss die echte Zeilenzahl liefern.
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_sysdb_exact").await;
        for i in 0..7 {
            sqlx::query("INSERT INTO test_table_a (val) VALUES ($1)")
                .bind(format!("row_{i}"))
                .execute(&pool)
                .await
                .unwrap();
        }
        // KEIN ANALYZE — reltuples bliebe stale/0.
        let stats = database_stats(&pool, &["test_table_a"]).await.unwrap();
        assert_eq!(stats.tables.len(), 1);
        assert_eq!(stats.tables[0].row_count, 7, "exakter COUNT(*) erwartet");
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
