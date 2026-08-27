//! Kennzahlen des laufenden Streams für das Chat-Dock des Relays.
//!
//! Jede Kennzahl kommt in zwei Sichten: "dieser Stream" und "gesamt"
//! (GRILLME C4-A1). Das Karussell im Dock zeigt beide nebeneinander, damit
//! ein Name im laufenden Stream nicht so aussieht, als wäre er die
//! Bestenliste des Kanals.
//!
//! Nur Logins, nie eine Chatter-ID: das Dock läuft im Browser des Streamers
//! und bekommt darum nichts, was über den sichtbaren Chat hinausgeht.
//!
//! Wichtig für die Zahlen:
//! - Anwesenheit steckt in `twitch_viewer_presence_ticks`, ein Tick je
//!   Zuschauer je 30 Sekunden. Minuten sind also `Anzahl Ticks / 2`.
//! - Häufigkeit zählt `COUNT(DISTINCT session_id)` aus
//!   `twitch_session_chatters`. `twitch_chatter_rollup.total_sessions` zählt
//!   nur Sessions mit Nachricht und taugt dafür nicht.
//! - Bots und der Streamer selbst kommen nie in eine Liste; die Ausschlussliste
//!   reicht der Aufrufer durch, damit hier keine zweite Wahrheit entsteht.
//!
//! Die Gesamt-Sichten gehen über alle Sessions eines Kanals, die Anwesenheit
//! sogar über eine Hypertable. Das Dock fragt alle 30 Sekunden, also liegen
//! die Gesamt-Werte fünf Minuten in einem Cache je Kanal; die Werte des
//! laufenden Streams werden jedes Mal frisch gerechnet.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgPool, Row};

/// Wie viele Namen je Liste. Das Karussell zeigt Gold, Silber, Bronze.
pub const TOP_N: i64 = 3;

/// Ein Tick je Zuschauer je 30 Sekunden, also eine halbe Minute je Tick.
const MINUTEN_JE_TICK: f64 = 0.5;

/// So lange gilt ein Gesamt-Wert als frisch genug.
pub const GESAMT_CACHE_FRIST: Duration = Duration::from_secs(5 * 60);

/// Wer am längsten zugesehen hat, in Minuten.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ZuschauerMinuten {
    pub login: String,
    pub minuten: f64,
}

/// Wer bei den meisten Streams dabei war.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ZuschauerSessions {
    pub login: String,
    pub sessions: i64,
}

/// Wer am meisten geschrieben hat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatterNachrichten {
    pub login: String,
    pub nachrichten: i64,
}

/// Eine Kennzahl in beiden Sichten.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Sichten<T> {
    /// Nur der laufende Stream.
    pub session: Vec<T>,
    /// Über alle Streams dieses Kanals.
    pub gesamt: Vec<T>,
}

/// Eine Kennzahl, die es nur über alle Streams hinweg gibt.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NurGesamt<T> {
    pub gesamt: Vec<T>,
}

/// Wie viele mitlesen, ohne zu schreiben.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LurkerAnteil {
    /// Alle, die in dieser Session gesehen wurden (schreibend oder still).
    pub anwesend: i64,
    /// Davon still: keine Nachricht, aber über die Zuschauerliste gesehen.
    pub still: i64,
    /// `still / anwesend`, zwischen 0 und 1. Ohne Anwesende 0.
    pub anteil: f64,
}

/// Der stille Anteil über alle Streams, als Mittel der Session-Anteile.
///
/// Bewusst nicht die Summe geteilt durch die Summe: ein einziger langer
/// Stream mit vielen Zuschauern würde die Zahl sonst allein bestimmen.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LurkerGesamt {
    /// Mittelwert der Session-Anteile, zwischen 0 und 1.
    pub anteil_durchschnitt: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LurkerSichten {
    pub session: LurkerAnteil,
    pub gesamt: LurkerGesamt,
}

/// Zuschauerzahlen: jetzt, Spitze dieses Streams, Spitze aller Streams.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Zuschauer {
    pub jetzt: i64,
    pub spitze_session: i64,
    pub spitze_gesamt: i64,
}

/// Alles, was das Dock für einen laufenden Stream zeigt.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StreamKennzahlen {
    pub streamer_login: String,
    pub session_id: i64,
    pub session_started_at: DateTime<Utc>,
    /// Zeitpunkt der Abfrage, damit das Dock alte Stände erkennt.
    pub stand: DateTime<Utc>,
    pub zuschauer: Zuschauer,
    pub top_chatter: Sichten<ChatterNachrichten>,
    pub laengster_zuschauer: Sichten<ZuschauerMinuten>,
    pub haeufigster_zuschauer: NurGesamt<ZuschauerSessions>,
    pub lurker: LurkerSichten,
}

/// Die teuren Werte, die über alle Streams eines Kanals gehen.
#[derive(Debug, Clone, PartialEq)]
struct GesamtWerte {
    top_chatter: Vec<ChatterNachrichten>,
    laengster_zuschauer: Vec<ZuschauerMinuten>,
    haeufigster_zuschauer: Vec<ZuschauerSessions>,
    lurker_anteil_durchschnitt: f64,
    spitze_zuschauer: i64,
}

fn gesamt_cache() -> &'static Mutex<HashMap<String, (Instant, GesamtWerte)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (Instant, GesamtWerte)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Schlüssel des Gesamt-Caches. Die Ausschlussliste gehört dazu: sie kommt aus
/// der Datenbank und kann sich ändern, und mit ihr die Bestenlisten. Ohne sie
/// im Schlüssel stünde ein frisch eingetragener Bot noch fünf Minuten in der
/// Liste.
fn gesamt_schluessel(streamer_login: &str, ausgeschlossen: &[String]) -> String {
    let mut sortiert: Vec<&str> = ausgeschlossen.iter().map(|s| s.as_str()).collect();
    sortiert.sort_unstable();
    format!("{streamer_login}\u{1f}{}", sortiert.join("\u{1e}"))
}

/// Baut `LOWER(<spalte>) NOT IN ($n, $n+1, ...)` ab Platzhalter `start_idx`.
/// Ohne Ausschlüsse eine Bedingung, die immer wahr ist, damit der Aufrufer
/// den Rest der Abfrage nicht doppelt schreiben muss.
fn nicht_in(start_idx: usize, spalte: &str, ausgeschlossen: &[String]) -> String {
    if ausgeschlossen.is_empty() {
        return "TRUE".to_string();
    }
    let platzhalter: Vec<String> = (start_idx..start_idx + ausgeschlossen.len())
        .map(|i| format!("${i}"))
        .collect();
    format!("LOWER({spalte}) NOT IN ({})", platzhalter.join(", "))
}

/// Der laufende Stream eines Streamers, oder `None`, wenn keiner läuft.
///
/// `twitch_user_id` ist die Twitch-Nutzernummer, dieselbe Nummer, mit der das
/// Relay seine Streamer führt.
pub async fn laden(
    pool: &PgPool,
    twitch_user_id: &str,
    ausgeschlossen: &[String],
) -> Result<Option<StreamKennzahlen>, sqlx::Error> {
    laden_mit_frist(pool, twitch_user_id, ausgeschlossen, GESAMT_CACHE_FRIST).await
}

/// Wie [`laden`], aber mit eigener Cache-Frist. `Duration::ZERO` rechnet die
/// Gesamt-Werte jedes Mal neu; genau das brauchen die Tests, sonst sähe der
/// zweite Test den Cache des ersten.
pub async fn laden_mit_frist(
    pool: &PgPool,
    twitch_user_id: &str,
    ausgeschlossen: &[String],
    cache_frist: Duration,
) -> Result<Option<StreamKennzahlen>, sqlx::Error> {
    let live = sqlx::query(
        r#"SELECT LOWER(streamer_login) AS streamer_login,
                  COALESCE(is_live, 0)          AS is_live,
                  active_session_id,
                  COALESCE(last_viewer_count, 0) AS last_viewer_count
           FROM twitch_live_state
           WHERE twitch_user_id = $1
           LIMIT 1"#,
    )
    .bind(twitch_user_id)
    .fetch_optional(pool)
    .await?;

    let Some(live) = live else {
        return Ok(None);
    };
    if live.try_get::<i32, _>("is_live")? != 1 {
        return Ok(None);
    }
    let Some(session_id) = live.try_get::<Option<i64>, _>("active_session_id")? else {
        return Ok(None);
    };
    let streamer_login: String = live.try_get("streamer_login")?;
    let zuschauer_jetzt = i64::from(live.try_get::<i32, _>("last_viewer_count")?);

    let session_zeile = sqlx::query(
        r#"SELECT started_at, COALESCE(peak_viewers, 0) AS peak_viewers
           FROM twitch_stream_sessions WHERE id = $1 LIMIT 1"#,
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;
    let Some(session_zeile) = session_zeile else {
        // Live-Zeile ohne Session-Zeile: der Stream ist für uns nicht
        // auswertbar, also lieber nichts als eine erfundene Startzeit.
        return Ok(None);
    };
    let session_started_at: DateTime<Utc> = session_zeile.try_get("started_at")?;
    let spitze_session = i64::from(session_zeile.try_get::<i32, _>("peak_viewers")?);

    let laengster_session = laengster_zuschauer_session(pool, session_id, ausgeschlossen).await?;
    let top_chatter_session = top_chatter_session(pool, session_id, ausgeschlossen).await?;
    let lurker_session = lurker_session(pool, session_id, ausgeschlossen).await?;
    let gesamt = gesamt_werte(pool, &streamer_login, ausgeschlossen, cache_frist).await?;

    Ok(Some(StreamKennzahlen {
        streamer_login,
        session_id,
        session_started_at,
        stand: Utc::now(),
        zuschauer: Zuschauer {
            jetzt: zuschauer_jetzt,
            spitze_session,
            spitze_gesamt: gesamt.spitze_zuschauer.max(spitze_session),
        },
        top_chatter: Sichten {
            session: top_chatter_session,
            gesamt: gesamt.top_chatter,
        },
        laengster_zuschauer: Sichten {
            session: laengster_session,
            gesamt: gesamt.laengster_zuschauer,
        },
        haeufigster_zuschauer: NurGesamt {
            gesamt: gesamt.haeufigster_zuschauer,
        },
        lurker: LurkerSichten {
            session: lurker_session,
            gesamt: LurkerGesamt {
                anteil_durchschnitt: gesamt.lurker_anteil_durchschnitt,
            },
        },
    }))
}

/// Die Gesamt-Werte aus dem Cache oder frisch.
///
/// Ein Fehler beim Rechnen wird durchgereicht; der Handler antwortet dann für
/// die ganze Abfrage mit 503, auch für die frischen Session-Zahlen. Das Relay
/// behält in dem Fall seinen letzten Stand und zeigt weiter Karten, das
/// Auffangen passiert also dort und nicht hier.
async fn gesamt_werte(
    pool: &PgPool,
    streamer_login: &str,
    ausgeschlossen: &[String],
    cache_frist: Duration,
) -> Result<GesamtWerte, sqlx::Error> {
    let schluessel = gesamt_schluessel(streamer_login, ausgeschlossen);
    if !cache_frist.is_zero() {
        let cache = gesamt_cache().lock().expect("Kennzahlen-Cache");
        if let Some((seit, werte)) = cache.get(&schluessel) {
            if seit.elapsed() < cache_frist {
                return Ok(werte.clone());
            }
        }
    }
    let werte = GesamtWerte {
        top_chatter: top_chatter_gesamt(pool, streamer_login, ausgeschlossen).await?,
        laengster_zuschauer: laengster_zuschauer_gesamt(pool, streamer_login, ausgeschlossen)
            .await?,
        haeufigster_zuschauer: haeufigster_zuschauer_gesamt(pool, streamer_login, ausgeschlossen)
            .await?,
        lurker_anteil_durchschnitt: lurker_gesamt(pool, streamer_login, ausgeschlossen).await?,
        spitze_zuschauer: spitze_zuschauer_gesamt(pool, streamer_login).await?,
    };
    if !cache_frist.is_zero() {
        let mut cache = gesamt_cache().lock().expect("Kennzahlen-Cache");
        // Abgelaufene Einträge raus, bevor ein neuer dazukommt: sonst wächst
        // die Karte mit jeder Änderung der Ausschlussliste weiter.
        cache.retain(|_, (seit, _)| seit.elapsed() < cache_frist);
        cache.insert(schluessel, (Instant::now(), werte.clone()));
    }
    Ok(werte)
}

/// Anwesenheit dieser Session, längster zuerst. Gleichstand nach Login
/// sortiert, damit die Reihenfolge zwischen zwei Abrufen nicht springt.
async fn laengster_zuschauer_session(
    pool: &PgPool,
    session_id: i64,
    ausgeschlossen: &[String],
) -> Result<Vec<ZuschauerMinuten>, sqlx::Error> {
    let bedingung = nicht_in(3, "viewer_login", ausgeschlossen);
    let sql = format!(
        r#"SELECT LOWER(viewer_login) AS login, COUNT(*) AS ticks
           FROM twitch_viewer_presence_ticks
           WHERE session_id = $1 AND {bedingung}
           GROUP BY 1
           ORDER BY ticks DESC, login ASC
           LIMIT $2"#
    );
    let mut q = sqlx::query(&sql).bind(session_id).bind(TOP_N);
    for login in ausgeschlossen {
        q = q.bind(login);
    }
    minuten_zeilen(q.fetch_all(pool).await?)
}

/// Anwesenheit über alle Streams dieses Kanals. Läuft über die Hypertable
/// und ist der Grund für den Cache.
///
/// Der Filter geht über `session_id`, nicht über `streamer_login`. Die
/// Hypertable ist nach `session_id` segmentiert und indiziert
/// (`20260728120100_presence_ticks_hypertable.sql`); ein Filter auf
/// `streamer_login` könnte weder Index noch Chunk noch Segment ausschließen
/// und würde die ganze Tabelle auspacken.
async fn laengster_zuschauer_gesamt(
    pool: &PgPool,
    streamer_login: &str,
    ausgeschlossen: &[String],
) -> Result<Vec<ZuschauerMinuten>, sqlx::Error> {
    let bedingung = nicht_in(3, "viewer_login", ausgeschlossen);
    let sql = format!(
        r#"SELECT LOWER(viewer_login) AS login, COUNT(*) AS ticks
           FROM twitch_viewer_presence_ticks
           WHERE session_id IN (
                   SELECT id FROM twitch_stream_sessions
                   WHERE LOWER(streamer_login) = $1
                 )
             AND {bedingung}
           GROUP BY 1
           ORDER BY ticks DESC, login ASC
           LIMIT $2"#
    );
    let mut q = sqlx::query(&sql).bind(streamer_login).bind(TOP_N);
    for login in ausgeschlossen {
        q = q.bind(login);
    }
    minuten_zeilen(q.fetch_all(pool).await?)
}

fn minuten_zeilen(
    zeilen: Vec<sqlx::postgres::PgRow>,
) -> Result<Vec<ZuschauerMinuten>, sqlx::Error> {
    zeilen
        .into_iter()
        .map(|z| {
            Ok(ZuschauerMinuten {
                login: z.try_get("login")?,
                minuten: z.try_get::<i64, _>("ticks")? as f64 * MINUTEN_JE_TICK,
            })
        })
        .collect()
}

/// Wer in diesem Stream am meisten geschrieben hat. Das Relay rechnet
/// dasselbe plattformübergreifend aus seinem Chat; diese Zahl hier ist die
/// Twitch-Wahrheit und trägt auch, was vor dem Verbinden des Docks kam.
async fn top_chatter_session(
    pool: &PgPool,
    session_id: i64,
    ausgeschlossen: &[String],
) -> Result<Vec<ChatterNachrichten>, sqlx::Error> {
    let bedingung = nicht_in(3, "chatter_login", ausgeschlossen);
    let sql = format!(
        r#"SELECT LOWER(chatter_login) AS login,
                  COALESCE(messages, 0)::bigint AS nachrichten
           FROM twitch_session_chatters
           WHERE session_id = $1
             AND {bedingung}
             AND COALESCE(messages, 0) > 0
           ORDER BY nachrichten DESC, login ASC
           LIMIT $2"#
    );
    let mut q = sqlx::query(&sql).bind(session_id).bind(TOP_N);
    for login in ausgeschlossen {
        q = q.bind(login);
    }
    nachrichten_zeilen(q.fetch_all(pool).await?)
}

/// Nachrichten über alle Streams hinweg, aus dem Rollup.
async fn top_chatter_gesamt(
    pool: &PgPool,
    streamer_login: &str,
    ausgeschlossen: &[String],
) -> Result<Vec<ChatterNachrichten>, sqlx::Error> {
    let bedingung = nicht_in(3, "chatter_login", ausgeschlossen);
    let sql = format!(
        r#"SELECT LOWER(chatter_login) AS login,
                  COALESCE(total_messages, 0)::bigint AS nachrichten
           FROM twitch_chatter_rollup
           WHERE LOWER(streamer_login) = $1
             AND {bedingung}
             AND COALESCE(total_messages, 0) > 0
           ORDER BY nachrichten DESC, login ASC
           LIMIT $2"#
    );
    let mut q = sqlx::query(&sql).bind(streamer_login).bind(TOP_N);
    for login in ausgeschlossen {
        q = q.bind(login);
    }
    nachrichten_zeilen(q.fetch_all(pool).await?)
}

fn nachrichten_zeilen(
    zeilen: Vec<sqlx::postgres::PgRow>,
) -> Result<Vec<ChatterNachrichten>, sqlx::Error> {
    zeilen
        .into_iter()
        .map(|z| {
            Ok(ChatterNachrichten {
                login: z.try_get("login")?,
                nachrichten: z.try_get("nachrichten")?,
            })
        })
        .collect()
}

/// Bei wie vielen Streams dieses Kanals jemand schon dabei war. Es gibt
/// diese Kennzahl nur gesamt: innerhalb eines Streams wäre sie immer 1.
async fn haeufigster_zuschauer_gesamt(
    pool: &PgPool,
    streamer_login: &str,
    ausgeschlossen: &[String],
) -> Result<Vec<ZuschauerSessions>, sqlx::Error> {
    let bedingung = nicht_in(3, "chatter_login", ausgeschlossen);
    let sql = format!(
        r#"SELECT LOWER(chatter_login) AS login, COUNT(DISTINCT session_id) AS sessions
           FROM twitch_session_chatters
           WHERE LOWER(streamer_login) = $1 AND {bedingung}
           GROUP BY 1
           ORDER BY sessions DESC, login ASC
           LIMIT $2"#
    );
    let mut q = sqlx::query(&sql).bind(streamer_login).bind(TOP_N);
    for login in ausgeschlossen {
        q = q.bind(login);
    }
    let zeilen = q.fetch_all(pool).await?;
    zeilen
        .into_iter()
        .map(|z| {
            Ok(ZuschauerSessions {
                login: z.try_get("login")?,
                sessions: z.try_get("sessions")?,
            })
        })
        .collect()
}

/// Still mitlesend heißt: über die Zuschauerliste gesehen, aber ohne
/// Nachricht in dieser Session.
async fn lurker_session(
    pool: &PgPool,
    session_id: i64,
    ausgeschlossen: &[String],
) -> Result<LurkerAnteil, sqlx::Error> {
    let bedingung = nicht_in(2, "chatter_login", ausgeschlossen);
    let sql = format!(
        r#"SELECT COUNT(*) AS anwesend,
                  COUNT(*) FILTER (
                      WHERE COALESCE(messages, 0) = 0
                        AND COALESCE(seen_via_chatters_api, FALSE)
                  ) AS still
           FROM twitch_session_chatters
           WHERE session_id = $1 AND {bedingung}"#
    );
    let mut q = sqlx::query(&sql).bind(session_id);
    for login in ausgeschlossen {
        q = q.bind(login);
    }
    let zeile = q.fetch_one(pool).await?;
    let anwesend: i64 = zeile.try_get("anwesend")?;
    let still: i64 = zeile.try_get("still")?;
    Ok(LurkerAnteil {
        anwesend,
        still,
        anteil: anteil(still, anwesend),
    })
}

/// Mittel der Session-Anteile über alle Streams dieses Kanals.
async fn lurker_gesamt(
    pool: &PgPool,
    streamer_login: &str,
    ausgeschlossen: &[String],
) -> Result<f64, sqlx::Error> {
    let bedingung = nicht_in(2, "chatter_login", ausgeschlossen);
    let sql = format!(
        r#"WITH je_session AS (
               SELECT session_id,
                      COUNT(*)::float8 AS anwesend,
                      COUNT(*) FILTER (
                          WHERE COALESCE(messages, 0) = 0
                            AND COALESCE(seen_via_chatters_api, FALSE)
                      )::float8 AS still
               FROM twitch_session_chatters
               WHERE LOWER(streamer_login) = $1 AND {bedingung}
               GROUP BY session_id
           )
           SELECT COALESCE(AVG(still / anwesend), 0)::float8 AS anteil
           FROM je_session
           WHERE anwesend > 0"#
    );
    let mut q = sqlx::query(&sql).bind(streamer_login);
    for login in ausgeschlossen {
        q = q.bind(login);
    }
    let roh: f64 = q.fetch_one(pool).await?.try_get("anteil")?;
    Ok(gerundet(roh))
}

/// Höchste Zuschauerzahl, die dieser Kanal je erreicht hat.
async fn spitze_zuschauer_gesamt(pool: &PgPool, streamer_login: &str) -> Result<i64, sqlx::Error> {
    let spitze: Option<i32> = sqlx::query_scalar(
        "SELECT MAX(COALESCE(peak_viewers, 0)) FROM twitch_stream_sessions
         WHERE LOWER(streamer_login) = $1",
    )
    .bind(streamer_login)
    .fetch_one(pool)
    .await?;
    Ok(i64::from(spitze.unwrap_or(0)))
}

fn anteil(teil: i64, ganzes: i64) -> f64 {
    if ganzes <= 0 {
        return 0.0;
    }
    gerundet(teil as f64 / ganzes as f64)
}

/// Auf vier Stellen gerundet: das Dock zeigt Prozent, und ein wandernder
/// Bruch in der letzten Stelle liesse die Karte flackern.
fn gerundet(wert: f64) -> f64 {
    (wert * 10_000.0).round() / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    /// Tests rechnen die Gesamt-Werte immer frisch; sonst sähe der zweite
    /// Test den Cache des ersten.
    const OHNE_CACHE: Duration = Duration::ZERO;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    macro_rules! db_dsn_or_skip {
        () => {
            match test_dsn() {
                Some(d) => d,
                None => {
                    if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                        panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                    }
                    eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                    return;
                }
            }
        };
    }

    /// Schema-isolierter Pool mit den fünf Tabellen dieses Wegs. Spaltentypen
    /// wie in `fresh_schema_snapshot.txt`, damit kein Test grün wird, den die
    /// Produktionstabelle ablehnen würde.
    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect test-db");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .expect("Schema droppen");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .expect("Schema anlegen");
        admin.close().await;

        let opts: sqlx::postgres::PgConnectOptions = dsn.parse().expect("DSN");
        let opts = opts.options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .expect("connect schema-pool");

        for ddl in [
            r#"CREATE TABLE twitch_live_state (
                   twitch_user_id     TEXT NOT NULL PRIMARY KEY,
                   streamer_login     TEXT NOT NULL,
                   is_live            INTEGER DEFAULT 0,
                   active_session_id  BIGINT,
                   last_viewer_count  INTEGER DEFAULT 0
               )"#,
            r#"CREATE TABLE twitch_stream_sessions (
                   id               BIGINT NOT NULL PRIMARY KEY,
                   streamer_login   TEXT NOT NULL,
                   started_at       TIMESTAMPTZ NOT NULL,
                   ended_at         TIMESTAMPTZ,
                   duration_seconds INTEGER,
                   peak_viewers     INTEGER DEFAULT 0
               )"#,
            r#"CREATE TABLE twitch_viewer_presence_ticks (
                   session_id     BIGINT NOT NULL,
                   streamer_login TEXT NOT NULL,
                   viewer_login   TEXT NOT NULL,
                   twitch_user_id TEXT,
                   tick_at        TIMESTAMPTZ NOT NULL
               )"#,
            r#"CREATE TABLE twitch_session_chatters (
                   session_id            BIGINT NOT NULL,
                   streamer_login        TEXT NOT NULL,
                   chatter_login         TEXT NOT NULL,
                   chatter_id            TEXT,
                   messages              INTEGER DEFAULT 0,
                   seen_via_chatters_api BOOLEAN DEFAULT FALSE,
                   first_message_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                   last_seen_at          TIMESTAMPTZ
               )"#,
            r#"CREATE TABLE twitch_chatter_rollup (
                   streamer_login TEXT NOT NULL,
                   chatter_login  TEXT NOT NULL,
                   chatter_id     TEXT,
                   total_messages INTEGER DEFAULT 0,
                   total_sessions INTEGER DEFAULT 0,
                   first_seen_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                   last_seen_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                   PRIMARY KEY (streamer_login, chatter_login)
               )"#,
        ] {
            sqlx::query(ddl).execute(&pool).await.expect("DDL");
        }
        pool
    }

    /// Bots und der Streamer selbst, wie der Handler sie durchreicht.
    fn ausschluss() -> Vec<String> {
        vec!["nightbot".to_string(), "earlysalty".to_string()]
    }

    async fn session_anlegen(pool: &PgPool, id: i64, login: &str, peak: i32) {
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, peak_viewers)
             VALUES ($1, $2, NOW() - INTERVAL '90 minutes', $3)",
        )
        .bind(id)
        .bind(login)
        .bind(peak)
        .execute(pool)
        .await
        .expect("session");
    }

    async fn live_anlegen(pool: &PgPool, uid: &str, login: &str, session_id: i64, zuschauer: i32) {
        sqlx::query(
            "INSERT INTO twitch_live_state
             (twitch_user_id, streamer_login, is_live, active_session_id, last_viewer_count)
             VALUES ($1, $2, 1, $3, $4)",
        )
        .bind(uid)
        .bind(login)
        .bind(session_id)
        .bind(zuschauer)
        .execute(pool)
        .await
        .expect("live_state");
        session_anlegen(pool, session_id, login, zuschauer).await;
    }

    async fn ticks(pool: &PgPool, session_id: i64, login: &str, viewer: &str, anzahl: i32) {
        for i in 0..anzahl {
            sqlx::query(
                "INSERT INTO twitch_viewer_presence_ticks
                 (session_id, streamer_login, viewer_login, tick_at)
                 VALUES ($1, $2, $3, NOW() - ($4 || ' seconds')::interval)",
            )
            .bind(session_id)
            .bind(login)
            .bind(viewer)
            .bind((i * 30).to_string())
            .execute(pool)
            .await
            .expect("tick");
        }
    }

    async fn chatter(
        pool: &PgPool,
        session_id: i64,
        streamer: &str,
        login: &str,
        messages: i32,
        via_api: bool,
    ) {
        sqlx::query(
            "INSERT INTO twitch_session_chatters
             (session_id, streamer_login, chatter_login, messages, seen_via_chatters_api)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(session_id)
        .bind(streamer)
        .bind(login)
        .bind(messages)
        .bind(via_api)
        .execute(pool)
        .await
        .expect("chatter");
    }

    async fn rollup(pool: &PgPool, streamer: &str, login: &str, nachrichten: i32) {
        sqlx::query(
            "INSERT INTO twitch_chatter_rollup
             (streamer_login, chatter_login, total_messages, total_sessions)
             VALUES ($1, $2, $3, 1)",
        )
        .bind(streamer)
        .bind(login)
        .bind(nachrichten)
        .execute(pool)
        .await
        .expect("rollup");
    }

    async fn kennzahlen(pool: &PgPool, uid: &str) -> Option<StreamKennzahlen> {
        laden_mit_frist(pool, uid, &ausschluss(), OHNE_CACHE)
            .await
            .expect("Abfrage")
    }

    #[tokio::test]
    async fn ohne_live_zeile_keine_kennzahlen() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_kennzahlen_offline").await;
        assert_eq!(kennzahlen(&pool, "999").await, None);

        // Zeile da, aber offline: auch nichts.
        sqlx::query(
            "INSERT INTO twitch_live_state
             (twitch_user_id, streamer_login, is_live, active_session_id)
             VALUES ('42', 'earlysalty', 0, 7)",
        )
        .execute(&pool)
        .await
        .expect("offline-Zeile");
        assert_eq!(kennzahlen(&pool, "42").await, None);
    }

    #[tokio::test]
    async fn top_drei_in_reihenfolge_ohne_bots() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_kennzahlen_top3").await;
        live_anlegen(&pool, "42", "earlysalty", 7, 123).await;

        // Anwesenheit: nightbot hat die meisten Ticks und darf trotzdem
        // nirgends auftauchen, der Streamer selbst genauso wenig.
        ticks(&pool, 7, "earlysalty", "nightbot", 40).await;
        ticks(&pool, 7, "earlysalty", "earlysalty", 40).await;
        ticks(&pool, 7, "earlysalty", "anna", 20).await;
        ticks(&pool, 7, "earlysalty", "bert", 10).await;
        ticks(&pool, 7, "earlysalty", "cara", 4).await;
        ticks(&pool, 7, "earlysalty", "dora", 2).await;

        for (login, sessions) in [("bert", 5), ("anna", 3), ("cara", 2), ("dora", 1)] {
            for s in 0..sessions {
                chatter(&pool, 100 + s, "earlysalty", login, 1, false).await;
            }
        }
        chatter(&pool, 100, "earlysalty", "nightbot", 500, false).await;

        rollup(&pool, "earlysalty", "cara", 900).await;
        rollup(&pool, "earlysalty", "anna", 500).await;
        rollup(&pool, "earlysalty", "bert", 100).await;
        rollup(&pool, "earlysalty", "dora", 0).await;
        rollup(&pool, "earlysalty", "nightbot", 9999).await;
        // Fremder Kanal: darf nicht durchschlagen.
        rollup(&pool, "fremdkanal", "zoe", 100000).await;

        let k = kennzahlen(&pool, "42").await.expect("live");
        assert_eq!(k.streamer_login, "earlysalty");
        assert_eq!(k.session_id, 7);
        assert_eq!(k.zuschauer.jetzt, 123);

        assert_eq!(
            k.laengster_zuschauer.session,
            vec![
                ZuschauerMinuten {
                    login: "anna".into(),
                    minuten: 10.0
                },
                ZuschauerMinuten {
                    login: "bert".into(),
                    minuten: 5.0
                },
                ZuschauerMinuten {
                    login: "cara".into(),
                    minuten: 2.0
                },
            ]
        );
        assert_eq!(
            k.haeufigster_zuschauer.gesamt,
            vec![
                ZuschauerSessions {
                    login: "bert".into(),
                    sessions: 5
                },
                ZuschauerSessions {
                    login: "anna".into(),
                    sessions: 3
                },
                ZuschauerSessions {
                    login: "cara".into(),
                    sessions: 2
                },
            ]
        );
        assert_eq!(
            k.top_chatter.gesamt,
            vec![
                ChatterNachrichten {
                    login: "cara".into(),
                    nachrichten: 900
                },
                ChatterNachrichten {
                    login: "anna".into(),
                    nachrichten: 500
                },
                ChatterNachrichten {
                    login: "bert".into(),
                    nachrichten: 100
                },
            ]
        );
    }

    /// Die zweite Sicht: dieselbe Kennzahl über alle Streams. Der laufende
    /// Stream darf die Gesamt-Liste nicht sein, sonst wäre die Karte doppelt.
    #[tokio::test]
    async fn session_und_gesamt_sind_zwei_verschiedene_listen() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_kennzahlen_sichten").await;
        live_anlegen(&pool, "42", "earlysalty", 7, 50).await;
        session_anlegen(&pool, 6, "earlysalty", 300).await;

        // Anwesenheit: anna ist heute vorn, bert über alle Streams.
        ticks(&pool, 7, "earlysalty", "anna", 20).await;
        ticks(&pool, 7, "earlysalty", "bert", 4).await;
        ticks(&pool, 6, "earlysalty", "bert", 100).await;

        // Nachrichten: heute schreibt anna am meisten, insgesamt cara.
        chatter(&pool, 7, "earlysalty", "anna", 12, true).await;
        chatter(&pool, 7, "earlysalty", "bert", 3, true).await;
        chatter(&pool, 7, "earlysalty", "nightbot", 999, true).await;
        rollup(&pool, "earlysalty", "cara", 900).await;
        rollup(&pool, "earlysalty", "anna", 40).await;

        let k = kennzahlen(&pool, "42").await.expect("live");

        assert_eq!(
            k.laengster_zuschauer.session,
            vec![
                ZuschauerMinuten {
                    login: "anna".into(),
                    minuten: 10.0
                },
                ZuschauerMinuten {
                    login: "bert".into(),
                    minuten: 2.0
                },
            ]
        );
        assert_eq!(
            k.laengster_zuschauer.gesamt,
            vec![
                ZuschauerMinuten {
                    login: "bert".into(),
                    minuten: 52.0
                },
                ZuschauerMinuten {
                    login: "anna".into(),
                    minuten: 10.0
                },
            ]
        );
        assert_eq!(
            k.top_chatter.session,
            vec![
                ChatterNachrichten {
                    login: "anna".into(),
                    nachrichten: 12
                },
                ChatterNachrichten {
                    login: "bert".into(),
                    nachrichten: 3
                },
            ]
        );
        assert_eq!(
            k.top_chatter.gesamt,
            vec![
                ChatterNachrichten {
                    login: "cara".into(),
                    nachrichten: 900
                },
                ChatterNachrichten {
                    login: "anna".into(),
                    nachrichten: 40
                },
            ]
        );
        // Spitze: dieser Stream 50, bester Stream aller Zeiten 300.
        assert_eq!(k.zuschauer.spitze_session, 50);
        assert_eq!(k.zuschauer.spitze_gesamt, 300);
    }

    #[tokio::test]
    async fn gleichstand_bleibt_stabil_nach_login() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_kennzahlen_gleichstand").await;
        live_anlegen(&pool, "42", "earlysalty", 7, 5).await;
        // Vier mit derselben Anwesenheit: die Reihenfolge darf nicht von der
        // Einfügereihenfolge abhängen, sonst springt die Karte im Dock.
        for viewer in ["zoe", "mia", "bert", "anna"] {
            ticks(&pool, 7, "earlysalty", viewer, 6).await;
        }
        let k = kennzahlen(&pool, "42").await.expect("live");
        let logins: Vec<&str> = k
            .laengster_zuschauer
            .session
            .iter()
            .map(|z| z.login.as_str())
            .collect();
        assert_eq!(logins, vec!["anna", "bert", "mia"]);
    }

    #[tokio::test]
    async fn lurker_anteil_zaehlt_nur_stille_ohne_nachricht() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_kennzahlen_lurker").await;
        live_anlegen(&pool, "42", "earlysalty", 7, 10).await;

        // Drei still gesehen, einer davon hat geschrieben; einer hat
        // geschrieben, ohne je in der Zuschauerliste zu stehen.
        chatter(&pool, 7, "earlysalty", "anna", 0, true).await;
        chatter(&pool, 7, "earlysalty", "bert", 0, true).await;
        chatter(&pool, 7, "earlysalty", "cara", 4, true).await;
        chatter(&pool, 7, "earlysalty", "dora", 2, false).await;
        // Bot zählt nirgends mit.
        chatter(&pool, 7, "earlysalty", "nightbot", 0, true).await;

        let k = kennzahlen(&pool, "42").await.expect("live");
        assert_eq!(k.lurker.session.anwesend, 4);
        assert_eq!(k.lurker.session.still, 2);
        assert_eq!(k.lurker.session.anteil, 0.5);
    }

    /// Der Gesamt-Anteil ist das Mittel der Session-Anteile, nicht die Summe
    /// geteilt durch die Summe. Sonst bestimmte ein einziger grosser Stream
    /// die Zahl allein.
    #[tokio::test]
    async fn lurker_gesamt_mittelt_ueber_die_sessions() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_kennzahlen_lurker_gesamt").await;
        live_anlegen(&pool, "42", "earlysalty", 7, 10).await;

        // Session 7: 1 von 2 still, also 0,5.
        chatter(&pool, 7, "earlysalty", "anna", 0, true).await;
        chatter(&pool, 7, "earlysalty", "bert", 5, true).await;
        // Session 6: alle vier still, also 1,0.
        for login in ["cara", "dora", "emil", "finn"] {
            chatter(&pool, 6, "earlysalty", login, 0, true).await;
        }

        let k = kennzahlen(&pool, "42").await.expect("live");
        // Mittel aus 0,5 und 1,0. Summe durch Summe waere 5/6 = 0,8333.
        assert_eq!(k.lurker.gesamt.anteil_durchschnitt, 0.75);
    }

    #[tokio::test]
    async fn ohne_anwesende_kein_geteilt_durch_null() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_kennzahlen_leer").await;
        live_anlegen(&pool, "42", "earlysalty", 7, 0).await;
        let k = kennzahlen(&pool, "42").await.expect("live");
        assert_eq!(k.lurker.session.anwesend, 0);
        assert_eq!(k.lurker.session.still, 0);
        assert_eq!(k.lurker.session.anteil, 0.0);
        assert_eq!(k.lurker.gesamt.anteil_durchschnitt, 0.0);
        assert!(k.laengster_zuschauer.session.is_empty());
        assert!(k.laengster_zuschauer.gesamt.is_empty());
        assert!(k.top_chatter.session.is_empty());
        assert!(k.top_chatter.gesamt.is_empty());
    }

    /// Der Cache soll die teuren Gesamt-Abfragen sparen, aber nie die Werte
    /// des laufenden Streams einfrieren.
    #[tokio::test]
    async fn cache_haelt_gesamt_fest_und_laesst_die_session_frei() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_kennzahlen_cache").await;
        live_anlegen(&pool, "77", "cachekanal", 7, 5).await;
        ticks(&pool, 7, "cachekanal", "anna", 2).await;
        rollup(&pool, "cachekanal", "cara", 100).await;

        let frist = Duration::from_secs(300);
        let erst = laden_mit_frist(&pool, "77", &[], frist)
            .await
            .expect("Abfrage")
            .expect("live");
        assert_eq!(erst.top_chatter.gesamt.len(), 1);

        // Neue Daten in beiden Sichten.
        ticks(&pool, 7, "cachekanal", "bert", 8).await;
        rollup(&pool, "cachekanal", "dora", 5000).await;

        let zweit = laden_mit_frist(&pool, "77", &[], frist)
            .await
            .expect("Abfrage")
            .expect("live");
        // Gesamt kommt aus dem Cache, ist also unverändert.
        assert_eq!(zweit.top_chatter.gesamt, erst.top_chatter.gesamt);
        // Die Session-Sicht rechnet jedes Mal neu.
        assert_eq!(
            zweit
                .laengster_zuschauer
                .session
                .first()
                .map(|z| z.login.as_str()),
            Some("bert")
        );

        // Ohne Frist ist der neue Stand sofort da.
        let frisch = laden_mit_frist(&pool, "77", &[], OHNE_CACHE)
            .await
            .expect("Abfrage")
            .expect("live");
        assert_eq!(
            frisch.top_chatter.gesamt.first().map(|c| c.login.as_str()),
            Some("dora")
        );
    }
}
