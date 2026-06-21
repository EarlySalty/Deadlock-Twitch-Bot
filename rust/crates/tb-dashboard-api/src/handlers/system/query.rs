//! Handler für `GET /twitch/api/admin/system/query` (P2.77 / P2.81).
//!
//! Read-only SQL-Konsole für Admins. Sicherheits-Härtung (HART):
//! - Nur `SELECT`-Statements (nach Whitespace-Normalisierung).
//! - Keyword-Blocklist gegen Schreib-/DDL-/COPY-Befehle.
//! - Ausführung in einer READ-ONLY-Transaktion (Schreibversuche scheitern
//!   serverseitig, selbst falls die Lexer-Prüfung umgangen würde).
//! - Höchstens 200 Zeilen werden zurückgegeben (`LIMIT`-Klammerung).
//!
//! Python-Vorbild: `bot/analytics/api_admin.py::_api_admin_system_query`
//! + `_run_admin_readonly_query`.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{Column, Row, TypeInfo};
use sqlx::PgPool;
use tb_http_core::AuthLevel;

const MAX_ROWS: usize = 200;

/// Verbotene Schlüsselwörter (als ganze Wörter geprüft, case-insensitiv).
const FORBIDDEN_KEYWORDS: &[&str] = &[
    "INSERT", "UPDATE", "DELETE", "DROP", "ALTER", "CREATE", "TRUNCATE", "GRANT",
    "REVOKE", "COPY",
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
    sql.split_whitespace().collect::<Vec<_>>().join(" ").to_uppercase()
}

/// Prüft das SQL gegen die SELECT-only- und Keyword-Blocklist-Regeln.
/// `Ok(())` wenn erlaubt, sonst `Err((status, message))`.
pub fn validate_sql(sql: &str) -> Result<(), (StatusCode, String)> {
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "sql parameter required".to_string()));
    }
    let upper = normalize_upper(trimmed);
    if !upper.starts_with("SELECT") {
        return Err((
            StatusCode::BAD_REQUEST,
            "only SELECT statements allowed".to_string(),
        ));
    }
    for kw in FORBIDDEN_KEYWORDS {
        if contains_word(&upper, kw) {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("forbidden keyword: {kw}"),
            ));
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
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<QueryParams>,
) -> impl IntoResponse {
    if !auth.is_privileged() {
        return err(StatusCode::UNAUTHORIZED, "unauthorized");
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
async fn run_readonly(pool: &PgPool, sql: &str) -> Result<(Vec<String>, Vec<Vec<Option<String>>>), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    // Schreibschutz auf Transaktionsebene — defense in depth.
    sqlx::query("SET TRANSACTION READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    let rows = sqlx::query(sql)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    let _ = tx.rollback().await;

    let columns: Vec<String> = if let Some(first) = rows.first() {
        first.columns().iter().map(|c| c.name().to_string()).collect()
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
    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::Request,
        routing::get,
        Extension, Router,
    };
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

    async fn pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.ok()?;
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.ok()?;
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.ok()?;
        admin.close().await;
        let options = PgConnectOptions::from_str(&dsn).ok()?.options([("search_path", schema)]);
        PgPoolOptions::new().max_connections(2).connect_with(options).await.ok()
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
        let Some(pool) = pool("t_query_select1").await else { return };
        let res = router(pool).oneshot(admin_req("sql=SELECT%201%20AS%20n")).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 65536).await.unwrap();
        let v: Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["rowCount"], 1);
        assert_eq!(v["columns"][0], "n");
        assert_eq!(v["rows"][0][0], "1");
    }

    #[tokio::test]
    async fn nicht_select_abgelehnt() {
        let Some(pool) = pool("t_query_reject").await else { return };
        let res = router(pool).oneshot(admin_req("sql=DELETE%20FROM%20x")).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let b = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let v: Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["error"], "only SELECT statements allowed");
    }

    #[tokio::test]
    async fn keyword_blocklist_abgelehnt() {
        let Some(pool) = pool("t_query_kw").await else { return };
        let res = router(pool).oneshot(admin_req("sql=SELECT%201%3B%20DROP%20TABLE%20t")).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let b = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let v: Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["error"], "forbidden keyword: DROP");
    }

    #[tokio::test]
    async fn ohne_auth_401() {
        let Some(pool) = pool("t_query_unauth").await else { return };
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
