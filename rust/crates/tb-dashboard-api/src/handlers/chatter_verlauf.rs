//! Die interne Verlaufs-Route für rs-relay:
//! `GET /twitch/api/v2/internal/chatter-verlauf?streamer=<id>&logins=a,b,c`.
//!
//! Das Chat-Dock hebt Erstchatter hervor, so wie Twitch die allererste
//! Nachricht eines Zuschauers lila färbt. Ob jemand neu ist, sieht das Relay
//! nicht: es kennt nur den laufenden Stream. Diese Route beantwortet die
//! Frage für bis zu fünfzig Logins auf einmal, damit ein Raid mit vielen
//! neuen Namen nicht viele Anfragen kostet.
//!
//! Schutz wie bei den anderen internen Routen: Loopback plus
//! `X-Internal-Token`. Herausgegeben werden nur Logins, Zahlen und ein
//! Zeitpunkt, nie eine Chatter-ID.

// Axum-Responses direkt im Result, wie in platform_token.rs.
#![allow(clippy::result_large_err)]

use std::net::SocketAddr;

use axum::{
    extract::{ConnectInfo, Extension, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use sqlx::{PgPool, Row};
use tb_analytics::chatter_verlauf::LOGINS_MAX;
use tb_http_core::{ExpectedToken, INTERNAL_TOKEN_HEADER};

use crate::auth::security::require_internal;

#[derive(Deserialize)]
pub struct ChatterVerlaufQuery {
    /// Twitch-Nutzernummer, dieselbe Nummer wie bei der Token-Route.
    pub streamer: Option<i64>,
    /// Logins, mit Komma getrennt.
    pub logins: Option<String>,
}

/// Loopback plus `X-Internal-Token`, konstante Laufzeit, fail-closed.
fn intern_erlaubt(
    connect: Option<&ConnectInfo<SocketAddr>>,
    headers: &HeaderMap,
    expected: Option<&ExpectedToken>,
) -> bool {
    let loopback = connect.map(|c| c.0.ip().is_loopback()).unwrap_or(false);
    let presented = headers
        .get(INTERNAL_TOKEN_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim();
    let expected = expected.map(|e| e.0.trim()).unwrap_or("");
    require_internal(loopback, presented, expected)
}

/// Trennt und säubert die Login-Liste. `Err` heißt: zu viele auf einmal.
fn logins_lesen(roh: &str) -> Result<Vec<String>, usize> {
    let liste: Vec<String> = roh
        .split(',')
        .map(|l| l.trim().trim_start_matches('@').to_lowercase())
        .filter(|l| !l.is_empty())
        .collect();
    if liste.len() > LOGINS_MAX {
        return Err(liste.len());
    }
    Ok(liste)
}

/// `GET /twitch/api/v2/internal/chatter-verlauf?streamer=&logins=`.
///
/// 200 mit `{"eintraege":[...]}`, 400 ohne `streamer`/`logins` oder über der
/// Grenze, 401 ohne gültigen internen Zugang, 404 `nicht_live` ohne
/// laufenden Stream, 503 wenn die Auswertung nicht antwortet.
pub async fn internal_chatter_verlauf_handler(
    State(pool): State<PgPool>,
    connect: Option<ConnectInfo<SocketAddr>>,
    expected: Option<Extension<ExpectedToken>>,
    headers: HeaderMap,
    Query(query): Query<ChatterVerlaufQuery>,
) -> Response {
    if !intern_erlaubt(connect.as_ref(), &headers, expected.as_ref().map(|e| &e.0)) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    let (Some(streamer_id), Some(roh)) = (query.streamer, query.logins.as_deref()) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "streamer und logins fehlen" })),
        )
            .into_response();
    };
    let logins =
        match logins_lesen(roh) {
            Ok(l) => l,
            Err(anzahl) => return (
                StatusCode::BAD_REQUEST,
                Json(
                    json!({ "error": "zu_viele_logins", "grenze": LOGINS_MAX, "gefragt": anzahl }),
                ),
            )
                .into_response(),
        };
    if logins.is_empty() {
        return Json(json!({ "eintraege": [] })).into_response();
    }

    let session = match laufende_session(&pool, streamer_id).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "nicht_live" })),
            )
                .into_response()
        }
        Err(e) => {
            tracing::warn!(streamer_id, error = %e, "chatter-verlauf: Kanal nicht lesbar");
            return nicht_verfuegbar();
        }
    };

    match tb_analytics::chatter_verlauf::laden(
        &pool,
        &session.streamer_login,
        &logins,
        Some(session.session_id),
        session.started_at,
    )
    .await
    {
        Ok(eintraege) => Json(json!({ "eintraege": eintraege })).into_response(),
        Err(e) => {
            tracing::warn!(streamer_id, error = %e, "chatter-verlauf: Auswertung fehlgeschlagen");
            nicht_verfuegbar()
        }
    }
}

struct LaufendeSession {
    streamer_login: String,
    session_id: i64,
    started_at: Option<DateTime<Utc>>,
}

/// Kanal und laufende Session zur Nutzernummer. `None` heißt: nicht live.
async fn laufende_session(
    pool: &PgPool,
    streamer_id: i64,
) -> Result<Option<LaufendeSession>, sqlx::Error> {
    let zeile = sqlx::query(
        "SELECT LOWER(l.streamer_login) AS streamer_login,
                l.active_session_id     AS session_id,
                s.started_at            AS started_at
         FROM twitch_live_state l
         LEFT JOIN twitch_stream_sessions s ON s.id = l.active_session_id
         WHERE l.twitch_user_id = $1
           AND COALESCE(l.is_live, 0) = 1
           AND l.active_session_id IS NOT NULL
         LIMIT 1",
    )
    .bind(streamer_id.to_string())
    .fetch_optional(pool)
    .await?;
    let Some(zeile) = zeile else {
        return Ok(None);
    };
    Ok(Some(LaufendeSession {
        streamer_login: zeile.try_get("streamer_login")?,
        session_id: zeile.try_get("session_id")?,
        started_at: zeile.try_get("started_at")?,
    }))
}

fn nicht_verfuegbar() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "error": "nicht_verfuegbar" })),
    )
        .into_response()
}

// ───────────────────────────────────────────────────────────────────────────
// Tests
// ───────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ohne_loopback_und_token_kein_zugang() {
        let headers = HeaderMap::new();
        assert!(!intern_erlaubt(None, &headers, None));
        let erwartet = ExpectedToken("geheim".into());
        let connect = ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 40000)));
        assert!(!intern_erlaubt(Some(&connect), &headers, Some(&erwartet)));

        let mut richtig = HeaderMap::new();
        richtig.insert(INTERNAL_TOKEN_HEADER, "geheim".parse().unwrap());
        assert!(intern_erlaubt(Some(&connect), &richtig, Some(&erwartet)));

        // Richtiger Token vom fremden Rechner reicht nicht.
        let fremd = ConnectInfo(SocketAddr::from(([10, 0, 0, 7], 40000)));
        assert!(!intern_erlaubt(Some(&fremd), &richtig, Some(&erwartet)));
    }

    #[test]
    fn logins_werden_gesaeubert_und_begrenzt() {
        assert_eq!(
            logins_lesen(" Anna , @Bert ,, cara ").unwrap(),
            vec!["anna", "bert", "cara"]
        );
        assert_eq!(logins_lesen("").unwrap(), Vec::<String>::new());
        let viele = (0..LOGINS_MAX)
            .map(|i| format!("n{i}"))
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(logins_lesen(&viele).unwrap().len(), LOGINS_MAX);
        let zu_viele = (0..LOGINS_MAX + 1)
            .map(|i| format!("n{i}"))
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(logins_lesen(&zu_viele), Err(LOGINS_MAX + 1));
    }

    // ── mit DB ─────────────────────────────────────────────────────────────

    async fn maybe_pool() -> Option<PgPool> {
        if std::env::var("TB_TEST_REQUIRE_DB").as_deref() != Ok("1") {
            return None;
        }
        let url = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let schema = crate::auth::session::test_schema_name("chatter_verlauf");
        let admin = PgPool::connect(&url).await.ok()?;
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .ok()?;
        admin.close().await;
        let opts: sqlx::postgres::PgConnectOptions = url.parse().ok()?;
        let opts = opts.options([("search_path", schema.as_str())]);
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .ok()?;
        for ddl in [
            "CREATE TABLE twitch_live_state (
                 twitch_user_id     TEXT NOT NULL PRIMARY KEY,
                 streamer_login     TEXT NOT NULL,
                 is_live            INTEGER DEFAULT 0,
                 active_session_id  BIGINT,
                 last_viewer_count  INTEGER DEFAULT 0
             )",
            "CREATE TABLE twitch_stream_sessions (
                 id             BIGINT NOT NULL PRIMARY KEY,
                 streamer_login TEXT NOT NULL,
                 started_at     TIMESTAMPTZ NOT NULL,
                 peak_viewers   INTEGER DEFAULT 0
             )",
            "CREATE TABLE twitch_session_chatters (
                 session_id             BIGINT NOT NULL,
                 streamer_login         TEXT NOT NULL,
                 chatter_login          TEXT NOT NULL,
                 chatter_id             TEXT,
                 messages               INTEGER DEFAULT 0,
                 seen_via_chatters_api  BOOLEAN DEFAULT FALSE,
                 confirmed_first_ever   BOOLEAN DEFAULT FALSE,
                 is_first_time_streamer BOOLEAN DEFAULT FALSE,
                 first_message_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                 last_seen_at           TIMESTAMPTZ
             )",
            "CREATE TABLE twitch_chatter_rollup (
                 streamer_login TEXT NOT NULL,
                 chatter_login  TEXT NOT NULL,
                 chatter_id     TEXT,
                 total_messages INTEGER DEFAULT 0,
                 total_sessions INTEGER DEFAULT 0,
                 first_seen_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                 last_seen_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                 PRIMARY KEY (streamer_login, chatter_login)
             )",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    macro_rules! pool_oder_ende {
        () => {
            match maybe_pool().await {
                Some(p) => p,
                None => {
                    assert!(
                        std::env::var("TB_TEST_REQUIRE_DB").as_deref() != Ok("1"),
                        "TB_TEST_REQUIRE_DB=1, aber keine Test-DB erreichbar"
                    );
                    return;
                }
            }
        };
    }

    async fn aufrufen(
        pool: &PgPool,
        von: Option<[u8; 4]>,
        token: Option<&str>,
        streamer: Option<i64>,
        logins: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        let mut headers = HeaderMap::new();
        if let Some(t) = token {
            headers.insert(INTERNAL_TOKEN_HEADER, t.parse().unwrap());
        }
        let antwort = internal_chatter_verlauf_handler(
            State(pool.clone()),
            von.map(|ip| ConnectInfo(SocketAddr::from((ip, 40000)))),
            Some(Extension(ExpectedToken("geheim".into()))),
            headers,
            Query(ChatterVerlaufQuery {
                streamer,
                logins: logins.map(str::to_string),
            }),
        )
        .await;
        let status = antwort.status();
        let body = axum::body::to_bytes(antwort.into_body(), 256 * 1024)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
        )
    }

    #[tokio::test]
    async fn schutz_und_fehlerfaelle() {
        let pool = pool_oder_ende!();

        let (status, body) = aufrufen(&pool, None, None, Some(42), Some("anna")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "unauthorized");

        let (status, _) = aufrufen(
            &pool,
            Some([10, 0, 0, 7]),
            Some("geheim"),
            Some(42),
            Some("anna"),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, _) = aufrufen(&pool, Some([127, 0, 0, 1]), Some("geheim"), None, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let zu_viele = (0..LOGINS_MAX + 1)
            .map(|i| format!("n{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let (status, body) = aufrufen(
            &pool,
            Some([127, 0, 0, 1]),
            Some("geheim"),
            Some(42),
            Some(&zu_viele),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "zu_viele_logins");

        // Nicht live: 404, kein Fehler.
        let (status, body) = aufrufen(
            &pool,
            Some([127, 0, 0, 1]),
            Some("geheim"),
            Some(42),
            Some("anna"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], "nicht_live");
    }

    #[tokio::test]
    async fn live_beantwortet_jeden_login_und_zeigt_keine_ids() {
        let pool = pool_oder_ende!();
        sqlx::query(
            "INSERT INTO twitch_live_state
             (twitch_user_id, streamer_login, is_live, active_session_id)
             VALUES ('4343', 'verlaufkanal', 1, 55)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (id, streamer_login, started_at)
             VALUES (55, 'verlaufkanal', NOW() - INTERVAL '60 minutes')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            // Ein Stammgast hat geschrieben. Ohne `total_messages` waere das
            // eine reine Anwesenheitszeile des Chatters-Pollers und damit
            // kein Verlauf.
            "INSERT INTO twitch_chatter_rollup
             (streamer_login, chatter_login, chatter_id, total_messages,
              first_seen_at, last_seen_at)
             VALUES ('verlaufkanal', 'stammgast', 'id-geheim', 40,
                     NOW() - INTERVAL '9 days', NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Der Verlauf steckt in den Session-Zeilen: einmal vor Wochen, einmal
        // im laufenden Stream. Ohne die alte Zeile waere der Stammgast heute
        // ein Erstchatter, und genau so soll es auch sein.
        sqlx::query(
            "INSERT INTO twitch_session_chatters
             (session_id, streamer_login, chatter_login, chatter_id, messages,
              first_message_at)
             VALUES (54, 'verlaufkanal', 'stammgast', 'id-geheim', 30,
                     NOW() - INTERVAL '9 days'),
                    (55, 'verlaufkanal', 'stammgast', 'id-geheim', 4, NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();

        let (status, body) = aufrufen(
            &pool,
            Some([127, 0, 0, 1]),
            Some("geheim"),
            Some(4343),
            Some("Stammgast,neuling"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let eintraege = body["eintraege"].as_array().expect("Liste");
        assert_eq!(eintraege.len(), 2);
        let stammgast = eintraege
            .iter()
            .find(|e| e["login"] == "stammgast")
            .expect("stammgast");
        assert_eq!(stammgast["erster_chat_ueberhaupt"], false);
        assert_eq!(stammgast["sessions"], 2);
        let neuling = eintraege
            .iter()
            .find(|e| e["login"] == "neuling")
            .expect("neuling");
        assert_eq!(neuling["erster_chat_ueberhaupt"], true);
        assert_eq!(neuling["erster_chat_am"], serde_json::Value::Null);

        assert!(!body.to_string().contains("id-geheim"), "{body}");
    }
}
