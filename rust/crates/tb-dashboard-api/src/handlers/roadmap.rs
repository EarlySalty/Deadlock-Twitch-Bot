//! Öffentlicher read-only Roadmap-Endpunkt + Admin-CRUD (P1.31).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use tb_http_core::AuthLevel;

/// Erlaubte Roadmap-Status (Python: `_VALID_STATUSES`).
const VALID_STATUSES: &[&str] = &["planned", "in_progress", "done"];

/// `GET /twitch/api/v2/roadmap`
pub async fn get_handler(State(pool): State<PgPool>) -> impl IntoResponse {
    let rows = sqlx::query!(
        "SELECT id, title, description, status, priority, created_at, updated_at \
         FROM twitch_roadmap_items ORDER BY priority DESC, id ASC",
    )
    .fetch_all(&pool)
    .await;

    let rows = match rows {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!("roadmap SELECT fehlgeschlagen: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":"DB-Fehler"})),
            )
                .into_response();
        }
    };

    let mut grouped = json!({"planned": [], "in_progress": [], "done": []});
    for row in rows {
        let group = roadmap_group(&row.status);
        let item = roadmap_item_json(
            row.id,
            row.title,
            row.description,
            row.status,
            row.priority,
            row.created_at,
            row.updated_at,
        );
        grouped[group]
            .as_array_mut()
            .expect("Roadmap-Gruppen sind Arrays")
            .push(item);
    }

    Json(grouped).into_response()
}

fn roadmap_group(status: &str) -> &'static str {
    if matches!(status, "planned" | "in_progress" | "done") {
        match status {
            "in_progress" => "in_progress",
            "done" => "done",
            _ => "planned",
        }
    } else {
        "planned"
    }
}

fn roadmap_item_json(
    id: i64,
    title: String,
    description: Option<String>,
    status: String,
    priority: i32,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
) -> Value {
    json!({
        "id": id,
        "title": title,
        "description": description,
        "status": status,
        "priority": priority,
        "created_at": created_at,
        "updated_at": updated_at,
    })
}

// ── Admin-CRUD (P1.31) ────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateBody {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub priority: Option<i32>,
}

/// PATCH-Body: nur explizit gesetzte Felder werden geändert. Wir nutzen
/// `Option<Option<T>>` um "Feld fehlt" von "Feld = null" zu unterscheiden.
#[derive(Deserialize)]
pub struct UpdateBody {
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    pub title: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    pub description: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    pub status: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    pub priority: Option<Option<i32>>,
}

/// Unterscheidet "Schlüssel fehlt" (None) von "Schlüssel = null" (Some(None)).
fn deserialize_optional_field<'de, D, T>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Ok(Some(Option::deserialize(de)?))
}

fn json_err(status: StatusCode, message: &str) -> axum::response::Response {
    (status, Json(json!({ "error": message }))).into_response()
}

async fn fetch_item(pool: &PgPool, id: i64) -> Result<Option<Value>, sqlx::Error> {
    let row = sqlx::query!(
        "SELECT id, title, description, status, priority, created_at, updated_at \
         FROM twitch_roadmap_items WHERE id = $1",
        id
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| {
        roadmap_item_json(
            r.id,
            r.title,
            r.description,
            r.status,
            r.priority,
            r.created_at,
            r.updated_at,
        )
    }))
}

fn normalize_status(raw: Option<&str>, default: &str) -> String {
    let s = raw.unwrap_or(default).trim().to_lowercase();
    if VALID_STATUSES.contains(&s.as_str()) {
        s
    } else {
        default.to_string()
    }
}

/// `POST /twitch/api/v2/roadmap` (admin) — legt einen Eintrag an (201).
pub async fn create_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<CreateBody>,
) -> impl IntoResponse {
    if !auth.is_privileged() {
        return json_err(
            StatusCode::FORBIDDEN,
            "Nur Administratoren dürfen Roadmap-Einträge anlegen.",
        );
    }
    let title = body.title.unwrap_or_default().trim().to_string();
    if title.is_empty() {
        return json_err(StatusCode::BAD_REQUEST, "Bitte einen Titel angeben.");
    }
    let description = body
        .description
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty());
    let status = normalize_status(body.status.as_deref(), "planned");
    let priority = body.priority.unwrap_or(0);

    let inserted = sqlx::query_scalar!(
        "INSERT INTO twitch_roadmap_items (title, description, status, priority) \
         VALUES ($1, $2, $3, $4) RETURNING id",
        title,
        description,
        status,
        priority
    )
    .fetch_one(&pool)
    .await;

    let id: i64 = match inserted {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("roadmap create fehlgeschlagen: {e}");
            return json_err(StatusCode::INTERNAL_SERVER_ERROR, "DB-Fehler");
        }
    };

    match fetch_item(&pool, id).await {
        Ok(Some(item)) => (StatusCode::CREATED, Json(item)).into_response(),
        _ => json_err(StatusCode::INTERNAL_SERVER_ERROR, "DB-Fehler"),
    }
}

/// `PATCH /twitch/api/v2/roadmap/{id}` (admin) — partielles Update (200/404).
pub async fn update_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateBody>,
) -> impl IntoResponse {
    if !auth.is_privileged() {
        return json_err(
            StatusCode::FORBIDDEN,
            "Nur Administratoren dürfen Roadmap-Einträge ändern.",
        );
    }

    // Dynamisches partielles UPDATE über nummerierte Binds.
    let mut sets: Vec<String> = Vec::new();
    enum Bind {
        Str(Option<String>),
        Int(i32),
    }
    let mut binds: Vec<Bind> = Vec::new();
    let mut n = 1;

    if let Some(title) = body.title {
        let t = title.unwrap_or_default().trim().to_string();
        if !t.is_empty() {
            sets.push(format!("title = ${n}"));
            binds.push(Bind::Str(Some(t)));
            n += 1;
        }
    }
    if let Some(desc) = body.description {
        let d = desc.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        sets.push(format!("description = ${n}"));
        binds.push(Bind::Str(d));
        n += 1;
    }
    if let Some(status) = body.status {
        let s = status.unwrap_or_default().trim().to_lowercase();
        if VALID_STATUSES.contains(&s.as_str()) {
            sets.push(format!("status = ${n}"));
            binds.push(Bind::Str(Some(s)));
            n += 1;
        }
    }
    if let Some(priority) = body.priority {
        sets.push(format!("priority = ${n}"));
        binds.push(Bind::Int(priority.unwrap_or(0)));
        n += 1;
    }

    if sets.is_empty() {
        return json_err(
            StatusCode::BAD_REQUEST,
            "Keine änderbaren Felder angegeben.",
        );
    }
    sets.push("updated_at = NOW()".to_string());

    let sql = format!(
        "UPDATE twitch_roadmap_items SET {} WHERE id = ${n}",
        sets.join(", ")
    );
    let mut q = sqlx::query(&sql);
    for b in binds {
        q = match b {
            Bind::Str(s) => q.bind(s),
            Bind::Int(i) => q.bind(i),
        };
    }
    q = q.bind(id);

    let res = q.execute(&pool).await;
    if let Err(e) = res {
        tracing::error!("roadmap update fehlgeschlagen: {e}");
        return json_err(StatusCode::INTERNAL_SERVER_ERROR, "DB-Fehler");
    }

    match fetch_item(&pool, id).await {
        Ok(Some(item)) => (StatusCode::OK, Json(item)).into_response(),
        Ok(None) => json_err(StatusCode::NOT_FOUND, "Roadmap-Eintrag nicht gefunden."),
        Err(e) => {
            tracing::error!("roadmap update fetch fehlgeschlagen: {e}");
            json_err(StatusCode::INTERNAL_SERVER_ERROR, "DB-Fehler")
        }
    }
}

/// `DELETE /twitch/api/v2/roadmap/{id}` (admin) — entfernt einen Eintrag (204).
pub async fn delete_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    if !auth.is_privileged() {
        return json_err(
            StatusCode::FORBIDDEN,
            "Nur Administratoren dürfen Roadmap-Einträge löschen.",
        );
    }
    match sqlx::query!("DELETE FROM twitch_roadmap_items WHERE id = $1", id)
        .execute(&pool)
        .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::error!("roadmap delete fehlgeschlagen: {e}");
            json_err(StatusCode::INTERNAL_SERVER_ERROR, "DB-Fehler")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::to_bytes, response::IntoResponse};
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    const ROADMAP_TEST_DDL: &str = r#"
CREATE TABLE twitch_roadmap_items (
    id          BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
    title       TEXT NOT NULL,
    description TEXT,
    status      TEXT NOT NULL DEFAULT 'planned',
    priority    INTEGER NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
)
"#;

    async fn pool() -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .ok()?;
        sqlx::query("DROP SCHEMA IF EXISTS t_roadmap_native CASCADE")
            .execute(&admin)
            .await
            .ok()?;
        sqlx::query("CREATE SCHEMA t_roadmap_native")
            .execute(&admin)
            .await
            .ok()?;
        admin.close().await;
        let options = PgConnectOptions::from_str(&dsn)
            .ok()?
            .options([("search_path", "t_roadmap_native")]);
        PgPoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .ok()
    }

    #[tokio::test]
    async fn roadmap_gruppiert_status_und_unbekannt_als_geplant() {
        let Some(pool) = pool().await else { return };
        sqlx::query(ROADMAP_TEST_DDL).execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO twitch_roadmap_items (title, status, priority) VALUES \
             ('Aktiv','in_progress',2),('Fertig','done',1),('Alt','unknown',0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let response = get_handler(State(pool)).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["in_progress"][0]["title"], "Aktiv");
        assert_eq!(value["done"][0]["title"], "Fertig");
        assert_eq!(value["planned"][0]["title"], "Alt");
    }

    // ── CRUD (P1.31) ──────────────────────────────────────────────────────────

    use crate::auth::session::{DashboardAuthState, ADMIN_COOKIE_NAME};
    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::Request,
        routing::{patch, post},
        Extension, Router,
    };
    use std::net::SocketAddr;
    use tb_http_core::ExpectedToken;
    use tower::ServiceExt;

    async fn crud_pool(schema: &str) -> Option<PgPool> {
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
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .ok()?;
        sqlx::query(ROADMAP_TEST_DDL).execute(&pool).await.ok()?;
        sqlx::query(
            r#"
            CREATE TABLE dashboard_sessions (
                session_id   TEXT NOT NULL PRIMARY KEY,
                session_type TEXT NOT NULL,
                payload_enc  BYTEA NOT NULL,
                created_at   DOUBLE PRECISION NOT NULL,
                expires_at   DOUBLE PRECISION NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .ok()?;
        Some(pool)
    }

    fn crud_router(pool: PgPool) -> Router {
        Router::new()
            .route("/twitch/api/v2/roadmap", post(create_handler))
            .route(
                "/twitch/api/v2/roadmap/:id",
                patch(update_handler).delete(delete_handler),
            )
            .with_state(pool)
            .layer(Extension(ExpectedToken("tok".to_string())))
    }

    fn admin_request(method: &str, uri: &str, body: &str) -> Request<Body> {
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        Request::builder()
            .method(method)
            .uri(uri)
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", "tok")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn create_legt_item_an_201() {
        let Some(pool) = crud_pool("t_roadmap_create").await else {
            return;
        };
        let res = crud_router(pool)
            .oneshot(admin_request(
                "POST",
                "/twitch/api/v2/roadmap",
                r#"{"title":"Neu","status":"in_progress","priority":5}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        let b = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert!(v["id"].as_i64().unwrap() > 0);
        assert_eq!(v["title"], "Neu");
        assert_eq!(v["status"], "in_progress");
        assert_eq!(v["priority"], 5);
    }

    #[tokio::test]
    async fn create_unbekannter_status_faellt_auf_planned() {
        let Some(pool) = crud_pool("t_roadmap_create_status").await else {
            return;
        };
        let res = crud_router(pool)
            .oneshot(admin_request(
                "POST",
                "/twitch/api/v2/roadmap",
                r#"{"title":"X","status":"garbage"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        let b = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["status"], "planned");
    }

    #[tokio::test]
    async fn create_ohne_titel_400() {
        let Some(pool) = crud_pool("t_roadmap_create_notitle").await else {
            return;
        };
        let res = crud_router(pool)
            .oneshot(admin_request(
                "POST",
                "/twitch/api/v2/roadmap",
                r#"{"title":"  "}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn update_aendert_status_und_404_bei_unbekannt() {
        let Some(pool) = crud_pool("t_roadmap_update").await else {
            return;
        };
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_roadmap_items (title, status, priority) VALUES ('A','planned',0) RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let res = crud_router(pool.clone())
            .oneshot(admin_request(
                "PATCH",
                &format!("/twitch/api/v2/roadmap/{id}"),
                r#"{"status":"done","priority":9}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["status"], "done");
        assert_eq!(v["priority"], 9);

        // Unbekannte ID → 404.
        let res404 = crud_router(pool)
            .oneshot(admin_request(
                "PATCH",
                "/twitch/api/v2/roadmap/999999",
                r#"{"status":"done"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res404.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn update_ungueltiger_status_wird_ignoriert_400_wenn_leer() {
        let Some(pool) = crud_pool("t_roadmap_update_badstatus").await else {
            return;
        };
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_roadmap_items (title, status) VALUES ('A','planned') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        // Nur ungültiger Status → kein änderbares Feld → 400.
        let res = crud_router(pool)
            .oneshot(admin_request(
                "PATCH",
                &format!("/twitch/api/v2/roadmap/{id}"),
                r#"{"status":"nope"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn delete_liefert_204() {
        let Some(pool) = crud_pool("t_roadmap_delete").await else {
            return;
        };
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_roadmap_items (title) VALUES ('A') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let res = crud_router(pool.clone())
            .oneshot(admin_request(
                "DELETE",
                &format!("/twitch/api/v2/roadmap/{id}"),
                "",
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        let remaining: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_roadmap_items WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(remaining, 0);
    }

    #[tokio::test]
    async fn create_ohne_admin_403() {
        let Some(pool) = crud_pool("t_roadmap_noauth").await else {
            return;
        };
        let addr: SocketAddr = "8.8.8.8:1234".parse().unwrap();
        let req = Request::builder()
            .method("POST")
            .uri("/twitch/api/v2/roadmap")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"title":"X"}"#))
            .unwrap();
        let res = crud_router(pool).oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn create_accepts_discord_admin_session_cookie() {
        let Some(pool) = crud_pool("t_roadmap_discord_admin_cookie").await else {
            return;
        };
        let auth_state = DashboardAuthState::new(
            pool.clone(),
            "dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU=".to_string(),
        );
        let session = auth_state
            .create_admin_session("discord-roadmap-admin-1", "Discord Admin")
            .await
            .expect("admin session");
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        let req = Request::builder()
            .method("POST")
            .uri("/twitch/api/v2/roadmap")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .header("x-dashboard-context", "admin")
            .header(
                axum::http::header::COOKIE,
                format!("{ADMIN_COOKIE_NAME}={}", session.session_id),
            )
            .header(crate::auth::csrf::CSRF_HEADER, session.csrf_token)
            .header("content-type", "application/json")
            .body(Body::from(r#"{"title":"Admin Cookie"}"#))
            .unwrap();
        let res = crate::build_roadmap_router(pool, "tok".into())
            .layer(Extension(auth_state))
            .oneshot(req)
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
    }
}
