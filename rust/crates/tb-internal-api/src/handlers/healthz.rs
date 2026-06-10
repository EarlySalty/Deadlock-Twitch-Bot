//! Handler für `GET /internal/twitch/v1/healthz`.

use axum::{extract::State, response::IntoResponse, Json};
use serde::Serialize;
use sqlx::PgPool;
use tb_analytics::global_ban::db_schema_fingerprint;
use tb_http_core::{ApiError, AuthLevel};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbInfo {
    pub fingerprint: Option<String>,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthzResponse {
    pub ok: bool,
    pub service: &'static str,
    pub analytics_db_fingerprint: Option<String>,
    pub analytics_db: DbInfo,
}

/// Parst Host, Port, DB-Name und User aus einem PostgreSQL-DSN.
///
/// Unterstützte Formate:
/// - `postgres://user:pass@host:port/dbname`
/// - `postgresql://user@host/dbname`
/// - `host=… port=… dbname=… user=…` (Key-Value)
fn parse_dsn(dsn: &str) -> DbInfo {
    // URL-Format
    if dsn.starts_with("postgres://") || dsn.starts_with("postgresql://") {
        if let Ok(url) = url::Url::parse(dsn) {
            return DbInfo {
                fingerprint: None,
                host: url.host_str().unwrap_or("unknown").to_string(),
                port: url.port().unwrap_or(5432),
                database: url.path().trim_start_matches('/').to_string(),
                user: url.username().to_string(),
            };
        }
    }
    // Key-Value-Format: host=... port=... dbname=... user=...
    let get = |key: &str| -> Option<String> {
        dsn.split_whitespace()
            .find(|s| s.starts_with(&format!("{key}=")))
            .and_then(|s| s.split_once('='))
            .map(|(_, v)| v.trim_matches('\'').to_string())
    };
    DbInfo {
        fingerprint: None,
        host: get("host").unwrap_or_else(|| "unknown".to_string()),
        port: get("port").and_then(|v| v.parse().ok()).unwrap_or(5432),
        database: get("dbname").unwrap_or_else(|| "unknown".to_string()),
        user: get("user").unwrap_or_else(|| "unknown".to_string()),
    }
}

/// Identitäts-Fingerprint kompatibel zu `bot/storage/pg.py`
/// (`analytics_db_fingerprint`): PBKDF2-HMAC-SHA256 über
/// `host|port|dbname` (lowercase/getrimmt, fehlende Felder = leer — KEIN
/// 5432-Default, Python kennt keinen), gleicher Salt, gleiche Iterationen,
/// 6 Bytes → `pg:<12 hex>`. Der Dashboard-Service vergleicht seinen lokal
/// berechneten Wert mit `analyticsDbFingerprint` aus dieser Antwort — jede
/// Abweichung im Algorithmus löst dort einen falschen
/// "different analytics databases"-Alarm aus.
const FINGERPRINT_SALT: &[u8] = b"deadlock.analytics-db-fingerprint.v1";
const FINGERPRINT_ITERATIONS: u32 = 100_000;

fn fingerprint_hex(value: &str) -> String {
    let mut out = [0u8; 6];
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(
        value.as_bytes(),
        FINGERPRINT_SALT,
        FINGERPRINT_ITERATIONS,
        &mut out,
    );
    hex::encode(out)
}

fn analytics_identity_fingerprint(dsn: &str) -> String {
    let (host, port, dbname) = analytics_identity_fields(dsn);
    format!("pg:{}", fingerprint_hex(&format!("{host}|{port}|{dbname}")))
}

fn analytics_identity_fields(dsn: &str) -> (String, String, String) {
    let norm = |v: Option<String>| v.unwrap_or_default().trim().to_lowercase();
    if dsn.starts_with("postgres://") || dsn.starts_with("postgresql://") {
        if let Ok(url) = url::Url::parse(dsn) {
            return (
                norm(url.host_str().map(str::to_string)),
                norm(url.port().map(|p| p.to_string())),
                norm(Some(url.path().trim_start_matches('/').to_string())),
            );
        }
    }
    let get = |key: &str| -> Option<String> {
        dsn.split_whitespace()
            .find(|s| s.starts_with(&format!("{key}=")))
            .and_then(|s| s.split_once('='))
            .map(|(_, v)| v.trim_matches('\'').to_string())
    };
    (
        norm(get("host")),
        norm(get("port")),
        norm(get("dbname").or_else(|| get("database"))),
    )
}

/// `GET /internal/twitch/v1/healthz`
pub async fn healthz_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let dsn = std::env::var("TWITCH_ANALYTICS_DSN").unwrap_or_default();

    // Top-Level: Identitäts-Fingerprint im Python-Format (Vertrag mit dem
    // Dashboard-Mismatch-Check); der Schema-Fingerprint bleibt als
    // Detail-Info in analytics_db.fingerprint erhalten.
    let identity_fingerprint = (!dsn.trim().is_empty()).then(|| analytics_identity_fingerprint(&dsn));

    let mut db_info = parse_dsn(&dsn);
    db_info.fingerprint = db_schema_fingerprint(&pool).await.ok();

    Ok(Json(HealthzResponse {
        ok: true,
        service: "twitch-internal-api",
        analytics_db_fingerprint: identity_fingerprint,
        analytics_db: db_info,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Erwartungswerte mit CPython erzeugt (hashlib.pbkdf2_hmac, gleicher
    // Salt/Iterationen wie bot/storage/pg.py) — sichert die Interop ab.
    #[test]
    fn fingerprint_entspricht_python_referenz() {
        assert_eq!(
            analytics_identity_fingerprint("postgres://u:p@localhost:5432/deadlock"),
            "pg:1686bea09e14"
        );
        // Ohne expliziten Port bleibt das Port-Feld leer — wie in Python.
        assert_eq!(
            analytics_identity_fingerprint("postgres://u:p@localhost/deadlock"),
            "pg:55d1dbe19794"
        );
        assert_eq!(
            analytics_identity_fingerprint("host=LOCALHOST port=5432 dbname=Deadlock"),
            "pg:1686bea09e14"
        );
    }
}
