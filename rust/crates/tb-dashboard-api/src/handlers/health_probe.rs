use axum::{extract::State, Json};
use serde_json::json;
use sqlx::PgPool;

const SERVICE_NAME: &str = "twitch-dashboard-service";

pub async fn healthz_handler() -> Json<serde_json::Value> {
    Json(json!({
        "ok": true,
        "service": SERVICE_NAME,
        "status": "alive",
    }))
}

pub async fn readyz_handler(State(pool): State<PgPool>) -> Json<serde_json::Value> {
    match sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&pool)
        .await
    {
        Ok(_) => Json(json!({
            "ok": true,
            "service": SERVICE_NAME,
            "status": "ready",
            "reasons": [],
            "details": {
                "database": "ok",
            },
        })),
        Err(error) => Json(json!({
            "ok": false,
            "service": SERVICE_NAME,
            "status": "degraded",
            "reasons": ["database_unavailable"],
            "details": {
                "database": "error",
                "databaseError": error.to_string(),
            },
        })),
    }
}
