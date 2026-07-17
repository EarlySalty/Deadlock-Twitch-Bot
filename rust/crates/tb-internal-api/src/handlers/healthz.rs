//! Handler für `GET /internal/twitch/v1/healthz`.
//!
//! Antwort-Vertrag — Parität zu `bot/internal_api/routes/telemetry.py::healthz`
//! und `bot/storage/pg.py::analytics_db_fingerprint_details`:
//! `{ok, service, analyticsDbFingerprint, analyticsDb: {fingerprint,
//! hostHash, databaseHash, portHash, engine}}`.
//!
//! Die DB-Identität erscheint ausschließlich als PBKDF2-Hash, nie im
//! Klartext (log-/health-sicher).

use axum::{response::IntoResponse, Json};
use serde::Serialize;
use tb_http_core::{ApiError, AuthLevel};

/// Gehashte DB-Identität, Feld für Feld wie Pythons
/// `analytics_db_fingerprint_details()` (`pg.py:278-288`).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbIdentity {
    pub fingerprint: Option<String>,
    pub host_hash: String,
    pub database_hash: String,
    pub port_hash: String,
    pub engine: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthzResponse {
    pub ok: bool,
    pub service: &'static str,
    pub analytics_db_fingerprint: Option<String>,
    pub analytics_db: DbIdentity,
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

/// Baut den `analyticsDb`-Block wie Pythons
/// `analytics_db_fingerprint_details()`: jedes Identitätsfeld einzeln
/// gehasht, leere Felder als `"-"` (`pg.py:284-287`).
fn analytics_db_identity(dsn: &str) -> DbIdentity {
    let (host, port, dbname) = analytics_identity_fields(dsn);
    let or_dash = |v: &str| {
        if v.is_empty() {
            "-".to_string()
        } else {
            v.to_string()
        }
    };
    DbIdentity {
        fingerprint: (!dsn.trim().is_empty()).then(|| analytics_identity_fingerprint(dsn)),
        host_hash: fingerprint_hex(&or_dash(&host)),
        database_hash: fingerprint_hex(&or_dash(&dbname)),
        port_hash: fingerprint_hex(&or_dash(&port)),
        engine: "postgres",
    }
}

/// `GET /internal/twitch/v1/healthz`
pub async fn healthz_handler(auth: AuthLevel) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let dsn = std::env::var("TWITCH_ANALYTICS_DSN").unwrap_or_default();
    let identity = analytics_db_identity(&dsn);

    Ok(Json(HealthzResponse {
        ok: true,
        service: "twitch-internal-api",
        analytics_db_fingerprint: identity.fingerprint.clone(),
        analytics_db: identity,
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

    // Referenzwerte mit CPython erzeugt (hashlib.pbkdf2_hmac, dklen=6) —
    // sichert die Feld-Hashes des analyticsDb-Blocks gegen pg.py ab.
    #[test]
    fn analytics_db_identity_hashes_entsprechen_python_referenz() {
        let identity = analytics_db_identity("postgres://u:p@localhost:5432/deadlock");
        assert_eq!(identity.fingerprint.as_deref(), Some("pg:1686bea09e14"));
        assert_eq!(identity.host_hash, "c131b8a26878");
        assert_eq!(identity.database_hash, "8e3806c50c33");
        assert_eq!(identity.port_hash, "1f9e8d5c1d8a");
        assert_eq!(identity.engine, "postgres");
    }

    // Leere Felder hasht Python als "-" (pg.py:284-287); leerer DSN → kein
    // Fingerprint, aber stabile Dash-Hashes.
    #[test]
    fn analytics_db_identity_leerer_dsn_hasht_dash() {
        let identity = analytics_db_identity("");
        assert_eq!(identity.fingerprint, None);
        assert_eq!(identity.host_hash, "ac177b3a66d6");
        assert_eq!(identity.database_hash, "ac177b3a66d6");
        assert_eq!(identity.port_hash, "ac177b3a66d6");
    }

    // Die HTTP-Shape: camelCase-Schlüssel, keine Klartext-Identität.
    #[test]
    fn healthz_response_shape_enthaelt_keine_klartext_identitaet() {
        let identity = analytics_db_identity("postgres://u:p@localhost:5432/deadlock");
        let json = serde_json::to_value(HealthzResponse {
            ok: true,
            service: "twitch-internal-api",
            analytics_db_fingerprint: identity.fingerprint.clone(),
            analytics_db: identity,
        })
        .unwrap();
        let db = &json["analyticsDb"];
        assert!(db.get("hostHash").is_some());
        assert!(db.get("databaseHash").is_some());
        assert!(db.get("portHash").is_some());
        assert_eq!(db["engine"], "postgres");
        for klartext in ["host", "port", "database", "user"] {
            assert!(
                db.get(klartext).is_none(),
                "{klartext} darf nicht erscheinen"
            );
        }
        assert_eq!(json["analyticsDbFingerprint"], db["fingerprint"]);
    }
}
