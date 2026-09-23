//! Handler für `GET /twitch/api/admin/system/query` (P2.77 / P2.81).
//!
//! Read-only SQL-Konsole für Admins. Sicherheits-Härtung (HART):
//! - Nur `SELECT`-Statements (nach Whitespace-Normalisierung).
//! - Keyword-Blocklist gegen Schreib-/DDL-/COPY-Befehle.
//! - Ausführung in einer READ-ONLY-Transaktion (Schreibversuche scheitern
//!   serverseitig, selbst falls die Lexer-Prüfung umgangen würde).
//! - Ein NO-SCROLL-Cursor ruft höchstens 200 Zeilen aus PostgreSQL ab.
//! - SQL-Eingaben sind auf 16 KiB begrenzt; Statements auf 5 s, Locks auf 1 s.
//! Die Konsole führt absichtlich SQL von authentifizierten Admins aus. Die
//! Keyword-Prüfung ist keine SQL-Sandbox; PostgreSQL erzwingt READ ONLY.
//!
//! Python-Vorbild: `bot/analytics/api_admin.py::_api_admin_system_query`
//! + `_run_admin_readonly_query`.

use crate::auth::level::DashboardAuthLevel;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use sqlx::{Column, Row, TypeInfo};

const MAX_ROWS: usize = 200;

/// Verbotene Schlüsselwörter (als ganze Wörter geprüft, case-insensitiv).
const FORBIDDEN_KEYWORDS: &[&str] = &[
    "INSERT", "UPDATE", "DELETE", "DROP", "ALTER", "CREATE", "TRUNCATE", "GRANT", "REVOKE", "COPY",
];

#[derive(Deserialize)]
pub struct QueryParams {
    #[serde(default)]
    pub sql: Option<String>,
}

fn err(status: StatusCode, message: impl Into<String>) -> axum::response::Response {
    (status, Json(json!({ "error": message.into() }))).into_response()
}

/// Normalisiert Whitespace zu einzelnen Leerzeichen und uppercased.
fn normalize_upper(sql: &str) -> String {
    sql.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
}

/// Prüft das SQL gegen die SELECT-only- und Keyword-Blocklist-Regeln.
/// `Ok(())` wenn erlaubt, sonst `Err((status, message))`.
pub fn validate_sql(sql: &str) -> Result<(), (StatusCode, String)> {
    if sql.len() > 16 * 1024 {
        return Err((
            StatusCode::BAD_REQUEST,
            "sql parameter exceeds 16 KiB".into(),
        ));
    }
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "sql parameter required".to_string(),
        ));
    }
    let upper = normalize_upper(trimmed);
    if !upper.starts_with("SELECT")
        || upper
            .as_bytes()
            .get(6)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "only SELECT statements allowed".to_string(),
        ));
    }
    for kw in FORBIDDEN_KEYWORDS {
        if contains_word(&upper, kw) {
            return Err((StatusCode::BAD_REQUEST, format!("forbidden keyword: {kw}")));
        }
    }
    Ok(())
}

/// `true` wenn `word` als eigenständiges Wort in `haystack` vorkommt
/// (Wortgrenzen über Nicht-Alphanumerik). Beide Seiten sind bereits uppercased.
fn contains_word(haystack: &str, word: &str) -> bool {
    let bytes = haystack.as_bytes();
    let wbytes = word.as_bytes();
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut i = 0;
    while let Some(pos) = haystack[i..].find(word) {
        let start = i + pos;
        let end = start + wbytes.len();
        let before_ok = start == 0 || !is_word(bytes[start - 1]);
        let after_ok = end >= bytes.len() || !is_word(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        i = start + 1;
        if i >= haystack.len() {
            break;
        }
    }
    false
}

/// `GET /twitch/api/admin/system/query`
pub async fn query_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<QueryParams>,
) -> impl IntoResponse {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return err.into_response();
    }

    let sql = params.sql.unwrap_or_default();
    if let Err((status, message)) = validate_sql(&sql) {
        return err(status, message);
    }

    match run_readonly(&pool, sql.trim()).await {
        Ok((columns, rows)) => {
            let row_count = rows.len();
            (
                StatusCode::OK,
                Json(json!({
                    "columns": columns,
                    "rows": rows,
                    "rowCount": row_count,
                })),
            )
                .into_response()
        }
        // Python gibt DB-Fehler als 400 mit der Fehlermeldung zurück.
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

/// Führt das SELECT in einer READ-ONLY-Transaktion aus und stringifiziert alle
/// Werte (max. 200 Zeilen). Gibt `Err(message)` bei DB-Fehlern.
async fn run_readonly(
    pool: &PgPool,
    sql: &str,
) -> Result<(Vec<String>, Vec<Vec<Option<String>>>), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    // Schreibschutz auf Transaktionsebene — defense in depth.
    sqlx::query!("SET TRANSACTION READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("SET LOCAL lock_timeout = '1s'")
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    // Prepared extended-protocol statements reject stacked SQL. A non-held,
    // forward-only cursor bounds the fetch itself, instead of collecting every
    // row and truncating afterward. No untrusted SQL enters the FETCH command.
    sqlx::query(&format!(
        "DECLARE tb_admin_readonly NO SCROLL CURSOR FOR {sql}"
    ))
    .persistent(false)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    // A cursor can expose different column shapes on each request. Do not cache
    // the prepared FETCH statement across unrelated admin queries.
    let rows = sqlx::query(&format!("FETCH FORWARD {MAX_ROWS} FROM tb_admin_readonly"))
        .persistent(false)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    if let Err(error) = tx.rollback().await {
        tracing::warn!(%error, "system-query Readonly-Transaktion Rollback fehlgeschlagen");
    }

    let columns: Vec<String> = if let Some(first) = rows.first() {
        first
            .columns()
            .iter()
            .map(|c| c.name().to_string())
            .collect()
    } else {
        Vec::new()
    };

    let mut out_rows: Vec<Vec<Option<String>>> = Vec::new();
    for row in rows.iter().take(MAX_ROWS) {
        let mut cells: Vec<Option<String>> = Vec::with_capacity(row.columns().len());
        for (idx, col) in row.columns().iter().enumerate() {
            cells.push(stringify_cell(row, idx, col.type_info().name()));
        }
        out_rows.push(cells);
    }
    Ok((columns, out_rows))
}

/// Liest eine Zelle typ-tolerant als `Option<String>`. Alle Skalartypen werden
/// nach Python-Vorbild zu Strings serialisiert (`None if v is None else str(v)`).
fn stringify_cell(row: &sqlx::postgres::PgRow, idx: usize, type_name: &str) -> Option<String> {
    // Reihenfolge: häufigste Typen zuerst.
    if let Ok(v) = row.try_get::<Option<String>, _>(idx) {
        return v;
    }
    if let Ok(v) = row.try_get::<Option<i64>, _>(idx) {
        return v.map(|x| x.to_string());
    }
    if let Ok(v) = row.try_get::<Option<i32>, _>(idx) {
        return v.map(|x| x.to_string());
    }
    if let Ok(v) = row.try_get::<Option<i16>, _>(idx) {
        return v.map(|x| x.to_string());
    }
    if let Ok(v) = row.try_get::<Option<bool>, _>(idx) {
        return v.map(|x| x.to_string());
    }
    if let Ok(v) = row.try_get::<Option<f64>, _>(idx) {
        return v.map(|x| x.to_string());
    }
    if let Ok(v) = row.try_get::<Option<f32>, _>(idx) {
        return v.map(|x| x.to_string());
    }
    if let Ok(v) = row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>(idx) {
        return v.map(|x| x.to_rfc3339());
    }
    if let Ok(v) = row.try_get::<Option<Value>, _>(idx) {
        return v.map(|x| x.to_string());
    }
    // Unbekannter Typ → ehrlicher Platzhalter mit Typname statt Panic.
    Some(format!("<{type_name}>"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, extract::ConnectInfo, http::Request, routing::get, Extension, Router};
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::net::SocketAddr;
    use std::str::FromStr;
    use tb_http_core::ExpectedToken;
    use tower::ServiceExt;

    #[test]
    fn validate_blockt_non_select_und_keywords() {
        assert!(validate_sql("SELECT 1").is_ok());
        assert!(validate_sql("  select * from t  ").is_ok());
        assert_eq!(validate_sql("").unwrap_err().0, StatusCode::BAD_REQUEST);
        assert!(validate_sql("DELETE FROM t").is_err());
        assert!(validate_sql("SELECT 1; DROP TABLE t").is_err());
        assert!(validate_sql("WITH x AS (SELECT 1) SELECT * FROM x").is_err());
        // Wortgrenzen: "created_at" enthält nicht das Keyword "CREATE".
        assert!(validate_sql("SELECT created_at FROM t").is_ok());
        // "updated" enthält "UPDATE" als Teilwort → erlaubt (kein ganzes Wort).
        assert!(validate_sql("SELECT updated_flag FROM t").is_ok());
    }

    #[test]
    fn security_query_rejects_oversized_sql_and_select_prefixes() {
        assert!(validate_sql(&format!("SELECT '{}'", "x".repeat(16 * 1024))).is_err());
        assert!(validate_sql("SELECTED value").is_err());
        assert!(validate_sql("SELECT_value").is_err());
        assert!(validate_sql("SELECT(1)").is_ok());
    }

    #[tokio::test]
    async fn security_query_fetches_only_the_first_200_rows() {
        let db = crate::test_postgres::TestPostgres::start().await;
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            run_readonly(&db.pool, "SELECT i, pg_sleep(CASE WHEN i > 200 THEN 10 ELSE 0 END) FROM generate_series(1, 201) AS i"),
        ).await.expect("The 201st row must never be evaluated").unwrap();
        assert_eq!(result.1.len(), 200);
        assert_eq!(result.1[199][0].as_deref(), Some("200"));
    }

    #[tokio::test]
    async fn security_query_times_out_and_keeps_the_pool_usable() {
        let db = crate::test_postgres::TestPostgres::start().await;
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(8),
            run_readonly(&db.pool, "SELECT pg_sleep(10)"),
        )
        .await
        .expect("PostgreSQL must enforce the query deadline");
        assert!(result.is_err());
        let (_, rows) = run_readonly(&db.pool, "SELECT 1").await.unwrap();
        assert_eq!(rows[0][0].as_deref(), Some("1"));
    }

    #[tokio::test]
    async fn security_query_cannot_write_or_stack_statements() {
        let db = crate::test_postgres::TestPostgres::start().await;
        assert!(run_readonly(&db.pool, "SELECT 1 INTO forbidden")
            .await
            .is_err());
        assert!(
            run_readonly(&db.pool, "SELECT 1; COMMIT; CREATE TABLE forbidden(n INT)")
                .await
                .is_err()
        );
        let exists: Option<String> = sqlx::query_scalar("SELECT to_regclass('forbidden')::text")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert!(exists.is_none());
    }

    async fn pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .ok()?;
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .ok()?;
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .ok()?;
        admin.close().await;
        let options = PgConnectOptions::from_str(&dsn)
            .ok()?
            .options([("search_path", schema)]);
        PgPoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .ok()
    }

    fn router(pool: PgPool) -> Router {
        Router::new()
            .route("/twitch/api/admin/system/query", get(query_handler))
            .with_state(pool)
            .layer(Extension(ExpectedToken("tok".to_string())))
    }

    fn admin_req(qs: &str) -> Request<Body> {
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        Request::builder()
            .uri(format!("/twitch/api/admin/system/query?{qs}"))
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", "tok")
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test]
    async fn select_eins_liefert_columns_rows() {
        let Some(pool) = pool("t_query_select1").await else {
            return;
        };
        let res = router(pool)
            .oneshot(admin_req("sql=SELECT%201%20AS%20n"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 65536).await.unwrap();
        let v: Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["rowCount"], 1);
        assert_eq!(v["columns"][0], "n");
        assert_eq!(v["rows"][0][0], "1");
    }

    #[tokio::test]
    async fn nicht_select_abgelehnt() {
        let Some(pool) = pool("t_query_reject").await else {
            return;
        };
        let res = router(pool)
            .oneshot(admin_req("sql=DELETE%20FROM%20x"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let b = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let v: Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["error"], "only SELECT statements allowed");
    }

    #[tokio::test]
    async fn keyword_blocklist_abgelehnt() {
        let Some(pool) = pool("t_query_kw").await else {
            return;
        };
        let res = router(pool)
            .oneshot(admin_req("sql=SELECT%201%3B%20DROP%20TABLE%20t"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let b = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let v: Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["error"], "forbidden keyword: DROP");
    }

    #[tokio::test]
    async fn ohne_auth_401() {
        let Some(pool) = pool("t_query_unauth").await else {
            return;
        };
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        let req = Request::builder()
            .uri("/twitch/api/admin/system/query?sql=SELECT%201")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .body(Body::empty())
            .unwrap();
        let res = router(pool).oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }
}
