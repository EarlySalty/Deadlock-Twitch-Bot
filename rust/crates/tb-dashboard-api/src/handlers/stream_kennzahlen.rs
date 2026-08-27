//! Die interne Kennzahlen-Route für rs-relay:
//! `GET /twitch/api/v2/internal/stream-kennzahlen?streamer=<twitch_user_id>`.
//!
//! Das Chat-Dock zeigt neben dem Chat ein Karussell mit Zahlen zum laufenden
//! Stream. Die Session-Zahlen rechnet das Relay selbst aus dem Chat-Bus, den
//! Verlauf (Anwesenheit, Häufigkeit, Nachrichten insgesamt) kennt nur der Bot.
//! Diese Route liefert genau diesen Verlauf, gebündelt in einer Antwort.
//!
//! Schutz wie bei der Token-Route: Loopback plus `X-Internal-Token`, kein
//! Cookie, kein CSRF. Herausgegeben werden nur Logins und Zahlen, nie eine
//! Chatter-ID; das Dock läuft im Browser und soll nichts sehen, was über den
//! sichtbaren Chat hinausgeht.

// Axum-Responses direkt im Result, wie in platform_token.rs.
#![allow(clippy::result_large_err)]

use std::net::SocketAddr;

use axum::{
    extract::{ConnectInfo, Extension, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use tb_http_core::{ExpectedToken, INTERNAL_TOKEN_HEADER};

use crate::auth::security::require_internal;
use crate::handlers::viewer_exclusion::viewer_exclusion_logins;

#[derive(Deserialize)]
pub struct StreamKennzahlenQuery {
    /// Twitch-Nutzernummer, dieselbe Nummer wie bei der Token-Route.
    pub streamer: Option<i64>,
}

/// Loopback plus `X-Internal-Token`, konstante Laufzeit, fail-closed.
///
/// Eigene Kopie statt Aufruf in `platform_token`: die beiden Routen teilen
/// den Token, aber nicht die Zuständigkeit. `require_internal` ist die
/// gemeinsame Prüfung, dort liegt die Wahrheit.
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

/// `GET /twitch/api/v2/internal/stream-kennzahlen?streamer=`.
///
/// 200 mit den Kennzahlen, 404 `nicht_live` ohne laufenden Stream,
/// 400 ohne `streamer`, 401 ohne gültigen internen Zugang,
/// 503 wenn die Auswertung gerade nicht antwortet.
pub async fn internal_stream_kennzahlen_handler(
    State(pool): State<PgPool>,
    connect: Option<ConnectInfo<SocketAddr>>,
    expected: Option<Extension<ExpectedToken>>,
    headers: HeaderMap,
    Query(query): Query<StreamKennzahlenQuery>,
) -> Response {
    if !intern_erlaubt(connect.as_ref(), &headers, expected.as_ref().map(|e| &e.0)) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    let Some(streamer_id) = query.streamer else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "streamer fehlt" })),
        )
            .into_response();
    };

    // Erst den Kanal auflösen, dann die Ausschlussliste dazu: Bots und der
    // Streamer selbst gehören in keine Bestenliste. Ohne laufenden Stream
    // fällt das gleich auf 404, die Ausschlussliste kostet dann nichts mehr.
    let streamer_login = match streamer_login_lesen(&pool, streamer_id).await {
        Ok(Some(login)) => login,
        Ok(None) => return nicht_live(),
        Err(e) => {
            tracing::warn!(streamer_id, error = %e, "stream-kennzahlen: Kanal nicht lesbar");
            return nicht_verfuegbar();
        }
    };
    let ausgeschlossen = viewer_exclusion_logins(&pool, &streamer_login).await;

    match tb_analytics::stream_kennzahlen::laden(&pool, &streamer_id.to_string(), &ausgeschlossen)
        .await
    {
        Ok(Some(kennzahlen)) => Json(kennzahlen).into_response(),
        Ok(None) => nicht_live(),
        Err(e) => {
            tracing::warn!(streamer_id, error = %e, "stream-kennzahlen: Auswertung fehlgeschlagen");
            nicht_verfuegbar()
        }
    }
}

/// Der Kanalname zur Nutzernummer, aber nur solange der Stream läuft.
/// `None` heißt: nicht live, und damit gibt es hier nichts zu zeigen.
async fn streamer_login_lesen(
    pool: &PgPool,
    streamer_id: i64,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        "SELECT LOWER(streamer_login)
         FROM twitch_live_state
         WHERE twitch_user_id = $1
           AND COALESCE(is_live, 0) = 1
           AND active_session_id IS NOT NULL
         LIMIT 1",
    )
    .bind(streamer_id.to_string())
    .fetch_optional(pool)
    .await
}

fn nicht_live() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "nicht_live" })),
    )
        .into_response()
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
        assert!(!intern_erlaubt(None, &headers, Some(&erwartet)));

        // Loopback allein reicht nicht.
        let connect = ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 40000)));
        assert!(!intern_erlaubt(Some(&connect), &headers, Some(&erwartet)));

        // Falscher Token reicht auch nicht.
        let mut falsch = HeaderMap::new();
        falsch.insert(INTERNAL_TOKEN_HEADER, "daneben".parse().unwrap());
        assert!(!intern_erlaubt(Some(&connect), &falsch, Some(&erwartet)));

        // Richtig plus Loopback: durch.
        let mut richtig = HeaderMap::new();
        richtig.insert(INTERNAL_TOKEN_HEADER, "geheim".parse().unwrap());
        assert!(intern_erlaubt(Some(&connect), &richtig, Some(&erwartet)));
    }

    #[test]
    fn fremder_peer_mit_richtigem_token_bleibt_draussen() {
        // Sonst reichte ein geleakter Header aus dem Netz.
        let erwartet = ExpectedToken("geheim".into());
        let mut headers = HeaderMap::new();
        headers.insert(INTERNAL_TOKEN_HEADER, "geheim".parse().unwrap());
        let fremd = ConnectInfo(SocketAddr::from(([10, 0, 0, 7], 40000)));
        assert!(!intern_erlaubt(Some(&fremd), &headers, Some(&erwartet)));
    }

    /// Die Antwort trägt Logins, aber keine Chatter-IDs, und jede Kennzahl
    /// kommt in beiden Sichten. Der Test hängt am serialisierten JSON, nicht
    /// an der Struktur, weil genau das über die Leitung geht.
    #[test]
    fn antwort_traegt_beide_sichten_und_keine_ids() {
        use tb_analytics::stream_kennzahlen::{
            ChatterNachrichten, LurkerAnteil, LurkerGesamt, LurkerSichten, NurGesamt, Sichten,
            StreamKennzahlen, Zuschauer, ZuschauerMinuten, ZuschauerSessions,
        };
        let k = StreamKennzahlen {
            streamer_login: "earlysalty".into(),
            session_id: 7,
            session_started_at: chrono::Utc::now(),
            stand: chrono::Utc::now(),
            zuschauer: Zuschauer {
                jetzt: 42,
                spitze_session: 51,
                spitze_gesamt: 300,
            },
            top_chatter: Sichten {
                session: vec![ChatterNachrichten {
                    login: "anna".into(),
                    nachrichten: 12,
                }],
                gesamt: vec![ChatterNachrichten {
                    login: "cara".into(),
                    nachrichten: 900,
                }],
            },
            laengster_zuschauer: Sichten {
                session: vec![ZuschauerMinuten {
                    login: "anna".into(),
                    minuten: 12.5,
                }],
                gesamt: vec![ZuschauerMinuten {
                    login: "bert".into(),
                    minuten: 900.0,
                }],
            },
            haeufigster_zuschauer: NurGesamt {
                gesamt: vec![ZuschauerSessions {
                    login: "bert".into(),
                    sessions: 9,
                }],
            },
            lurker: LurkerSichten {
                session: LurkerAnteil {
                    anwesend: 10,
                    still: 4,
                    anteil: 0.4,
                },
                gesamt: LurkerGesamt {
                    anteil_durchschnitt: 0.55,
                },
            },
        };
        let json: serde_json::Value = serde_json::to_value(&k).expect("serialisierbar");

        assert_eq!(json["streamer_login"], "earlysalty");
        assert_eq!(json["session_id"], 7);
        assert!(json["session_started_at"].is_string());
        assert!(json["stand"].is_string());
        assert_eq!(json["zuschauer"]["jetzt"], 42);
        assert_eq!(json["zuschauer"]["spitze_session"], 51);
        assert_eq!(json["zuschauer"]["spitze_gesamt"], 300);
        assert_eq!(json["top_chatter"]["session"][0]["login"], "anna");
        assert_eq!(json["top_chatter"]["session"][0]["nachrichten"], 12);
        assert_eq!(json["top_chatter"]["gesamt"][0]["login"], "cara");
        assert_eq!(json["laengster_zuschauer"]["session"][0]["minuten"], 12.5);
        assert_eq!(json["laengster_zuschauer"]["gesamt"][0]["login"], "bert");
        assert_eq!(json["haeufigster_zuschauer"]["gesamt"][0]["sessions"], 9);
        assert_eq!(json["lurker"]["session"]["anwesend"], 10);
        assert_eq!(json["lurker"]["session"]["still"], 4);
        assert_eq!(json["lurker"]["session"]["anteil"], 0.4);
        assert_eq!(json["lurker"]["gesamt"]["anteil_durchschnitt"], 0.55);

        let text = json.to_string();
        assert!(!text.contains("chatter_id"), "{text}");
        assert!(!text.contains("sender_id"), "{text}");
        assert!(!text.contains("user_id"), "{text}");
    }

    // ── mit DB ─────────────────────────────────────────────────────────────

    /// Eigenes Testschema mit den Tabellen, die dieser Weg anfasst.
    async fn maybe_pool() -> Option<PgPool> {
        if std::env::var("TB_TEST_REQUIRE_DB").as_deref() != Ok("1") {
            return None;
        }
        let url = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let schema = crate::auth::session::test_schema_name("stream_kennzahlen");
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
                 id               BIGINT NOT NULL PRIMARY KEY,
                 streamer_login   TEXT NOT NULL,
                 started_at       TIMESTAMPTZ NOT NULL,
                 ended_at         TIMESTAMPTZ,
                 duration_seconds INTEGER,
                 peak_viewers     INTEGER DEFAULT 0
             )",
            "CREATE TABLE twitch_viewer_presence_ticks (
                 session_id     BIGINT NOT NULL,
                 streamer_login TEXT NOT NULL,
                 viewer_login   TEXT NOT NULL,
                 twitch_user_id TEXT,
                 tick_at        TIMESTAMPTZ NOT NULL
             )",
            "CREATE TABLE twitch_session_chatters (
                 session_id            BIGINT NOT NULL,
                 streamer_login        TEXT NOT NULL,
                 chatter_login         TEXT NOT NULL,
                 chatter_id            TEXT,
                 messages              INTEGER DEFAULT 0,
                 seen_via_chatters_api BOOLEAN DEFAULT FALSE,
                 first_message_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                 last_seen_at          TIMESTAMPTZ
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

    /// Ruft den Handler so auf, wie ihn der Router aufruft.
    async fn aufrufen(
        pool: &PgPool,
        von: Option<[u8; 4]>,
        token: Option<&str>,
        streamer: Option<i64>,
    ) -> (StatusCode, serde_json::Value) {
        let mut headers = HeaderMap::new();
        if let Some(t) = token {
            headers.insert(INTERNAL_TOKEN_HEADER, t.parse().unwrap());
        }
        let connect = von.map(|ip| ConnectInfo(SocketAddr::from((ip, 40000))));
        let antwort = internal_stream_kennzahlen_handler(
            State(pool.clone()),
            connect,
            Some(Extension(ExpectedToken("geheim".into()))),
            headers,
            Query(StreamKennzahlenQuery { streamer }),
        )
        .await;
        let status = antwort.status();
        let body = axum::body::to_bytes(antwort.into_body(), 256 * 1024)
            .await
            .unwrap();
        let wert = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
        (status, wert)
    }

    #[tokio::test]
    async fn ohne_token_401_ohne_streamer_400_ohne_live_404() {
        let pool = pool_oder_ende!();

        // Ohne Header und ohne Loopback: 401, und die Auswertung laeuft gar
        // nicht erst an.
        let (status, body) = aufrufen(&pool, None, None, Some(42)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "unauthorized");

        // Richtiger Token, aber fremder Peer: auch 401.
        let (status, _) = aufrufen(&pool, Some([10, 0, 0, 7]), Some("geheim"), Some(42)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // Drin, aber ohne `streamer`: 400.
        let (status, body) = aufrufen(&pool, Some([127, 0, 0, 1]), Some("geheim"), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "streamer fehlt");

        // Kein laufender Stream: 404 `nicht_live`, kein Fehler.
        let (status, body) = aufrufen(&pool, Some([127, 0, 0, 1]), Some("geheim"), Some(42)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], "nicht_live");

        // Offline-Zeile in der DB zaehlt genauso wenig.
        sqlx::query(
            "INSERT INTO twitch_live_state
             (twitch_user_id, streamer_login, is_live, active_session_id)
             VALUES ('42', 'earlysalty', 0, 7)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let (status, body) = aufrufen(&pool, Some([127, 0, 0, 1]), Some("geheim"), Some(42)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], "nicht_live");
    }

    #[tokio::test]
    async fn live_liefert_beide_sichten_ohne_bots() {
        let pool = pool_oder_ende!();
        sqlx::query(
            "INSERT INTO twitch_live_state
             (twitch_user_id, streamer_login, is_live, active_session_id, last_viewer_count)
             VALUES ('4242', 'kennzahlenkanal', 1, 91, 37)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, peak_viewers)
             VALUES (91, 'kennzahlenkanal', NOW() - INTERVAL '30 minutes', 44)",
        )
        .execute(&pool)
        .await
        .unwrap();
        for (login, nachrichten, still) in [
            ("anna", 9, false),
            ("bert", 0, true),
            ("nightbot", 500, false),
        ] {
            sqlx::query(
                "INSERT INTO twitch_session_chatters
                 (session_id, streamer_login, chatter_login, chatter_id, messages,
                  seen_via_chatters_api)
                 VALUES (91, 'kennzahlenkanal', $1, 'id-geheim', $2, $3)",
            )
            .bind(login)
            .bind(nachrichten)
            .bind(still)
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::query(
            "INSERT INTO twitch_chatter_rollup (streamer_login, chatter_login, total_messages)
             VALUES ('kennzahlenkanal', 'anna', 700), ('kennzahlenkanal', 'nightbot', 90000)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let (status, body) =
            aufrufen(&pool, Some([127, 0, 0, 1]), Some("geheim"), Some(4242)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["streamer_login"], "kennzahlenkanal");
        assert_eq!(body["session_id"], 91);
        assert_eq!(body["zuschauer"]["jetzt"], 37);
        assert_eq!(body["zuschauer"]["spitze_session"], 44);
        assert_eq!(body["top_chatter"]["session"][0]["login"], "anna");
        assert_eq!(body["top_chatter"]["gesamt"][0]["login"], "anna");
        assert_eq!(body["lurker"]["session"]["anwesend"], 2);
        assert_eq!(body["lurker"]["session"]["still"], 1);

        // Der Bot steht in der DB, aber in keiner Liste, und die Chatter-ID
        // verlaesst den Bot nie.
        let text = body.to_string();
        assert!(!text.contains("nightbot"), "{text}");
        assert!(!text.contains("id-geheim"), "{text}");
    }
}
