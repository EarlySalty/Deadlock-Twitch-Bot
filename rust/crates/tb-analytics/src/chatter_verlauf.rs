//! Wer schreibt hier zum ersten Mal? Für die Hervorhebung im Chat-Dock.
//!
//! Twitch färbt die allererste Nachricht eines Zuschauers in einem Kanal
//! lila. Das Relay kann das nicht wissen: es sieht nur den laufenden Stream.
//! Der Bot führt den Verlauf, also beantwortet er die Frage, und zwar im
//! Bund für alle Autoren auf einmal.
//!
//! "Erster Chat überhaupt" heißt hier eines von dreien:
//! - Der Bot hat die Zeile dieser Session selbst als erste markiert
//!   (`confirmed_first_ever` oder `is_first_time_streamer`).
//! - Zu diesem Login gibt es überhaupt keinen Verlauf in diesem Kanal.
//! - Der Verlauf beginnt erst in dieser Session; die Person hat also gerade
//!   eben zum ersten Mal geschrieben und deshalb schon eine Zeile im Rollup.
//!
//! Der Verlauf kommt aus `twitch_session_chatters` und zählt nur Zeilen mit
//! Nachrichten. `twitch_chatter_rollup.first_seen_at` taugt dafür nicht: der
//! Chatters-Poller legt für jeden Anwesenden eine Rollup-Zeile an, und der
//! Nachrichten-Pfad erhöht später nur `total_messages`, ohne `first_seen_at`
//! anzufassen. Wer monatelang zugeschaut hat und heute die erste Nachricht
//! schreibt, hätte dort ein Datum von damals und sähe aus wie ein alter
//! Bekannter. `first_message_at` je Session sagt dagegen wirklich, wann jemand
//! zum ersten Mal geschrieben hat.
//!
//! Der dritte Punkt ist der Grund, warum die Session-Startzeit mit in die
//! Abfrage geht: ohne sie wäre jeder Erstchatter eine Sekunde nach seiner
//! ersten Nachricht kein Erstchatter mehr.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgPool, Row};

/// So viele Logins beantwortet eine Abfrage. Mehr weist der Aufrufer ab.
pub const LOGINS_MAX: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerlaufEintrag {
    pub login: String,
    pub erster_chat_ueberhaupt: bool,
    /// Wann diese Person in diesem Kanal zum ersten Mal geschrieben hat.
    pub erster_chat_am: Option<DateTime<Utc>>,
    /// Bei wie vielen Streams dieses Kanals sie schon dabei war.
    pub sessions: i64,
}

/// Der Verlauf zu einer Liste von Logins, in derselben Reihenfolge.
///
/// Unbekannte Logins kommen als "erster Chat überhaupt" zurück, nicht als
/// Lücke: das Dock soll für jeden Namen eine Antwort bekommen.
pub async fn laden(
    pool: &PgPool,
    streamer_login: &str,
    logins: &[String],
    session_id: Option<i64>,
    session_started_at: Option<DateTime<Utc>>,
) -> Result<Vec<VerlaufEintrag>, sqlx::Error> {
    let gefragt: Vec<String> = logins
        .iter()
        .take(LOGINS_MAX)
        .map(|l| l.trim().to_lowercase())
        .filter(|l| !l.is_empty())
        .collect();
    if gefragt.is_empty() {
        return Ok(Vec::new());
    }

    let zeilen = sqlx::query(
        r#"WITH gefragt AS (
               SELECT UNNEST($2::text[]) AS login
           ),
           verlauf AS (
               SELECT LOWER(chatter_login) AS login, MIN(first_message_at) AS erster
               FROM twitch_session_chatters
               WHERE LOWER(streamer_login) = $1
                 AND COALESCE(messages, 0) > 0
               GROUP BY 1
           ),
           dabei AS (
               SELECT LOWER(chatter_login) AS login,
                      COUNT(DISTINCT session_id) AS sessions
               FROM twitch_session_chatters
               WHERE LOWER(streamer_login) = $1
               GROUP BY 1
           ),
           jetzt AS (
               SELECT LOWER(chatter_login) AS login,
                      BOOL_OR(
                          COALESCE(confirmed_first_ever, FALSE)
                          OR COALESCE(is_first_time_streamer, FALSE)
                      ) AS erstmals
               FROM twitch_session_chatters
               WHERE session_id = $3
               GROUP BY 1
           )
           SELECT g.login                          AS login,
                  v.erster                         AS erster_chat_am,
                  COALESCE(d.sessions, 0)          AS sessions,
                  COALESCE(j.erstmals, FALSE)      AS erstmals
           FROM gefragt g
           LEFT JOIN verlauf v ON v.login = g.login
           LEFT JOIN dabei   d ON d.login = g.login
           LEFT JOIN jetzt   j ON j.login = g.login"#,
    )
    .bind(streamer_login.trim().to_lowercase())
    .bind(&gefragt)
    // Ohne laufende Session gibt es keine Zeile dazu; `-1` trifft nichts.
    .bind(session_id.unwrap_or(-1))
    .fetch_all(pool)
    .await?;

    zeilen
        .into_iter()
        .map(|z| {
            let erster_chat_am: Option<DateTime<Utc>> = z.try_get("erster_chat_am")?;
            let erstmals: bool = z.try_get("erstmals")?;
            let beginnt_jetzt = match (erster_chat_am, session_started_at) {
                (Some(erster), Some(start)) => erster >= start,
                _ => false,
            };
            Ok(VerlaufEintrag {
                login: z.try_get("login")?,
                erster_chat_ueberhaupt: erstmals || erster_chat_am.is_none() || beginnt_jetzt,
                erster_chat_am,
                sessions: z.try_get("sessions")?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

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
            r#"CREATE TABLE twitch_session_chatters (
                   session_id            BIGINT NOT NULL,
                   streamer_login        TEXT NOT NULL,
                   chatter_login         TEXT NOT NULL,
                   chatter_id            TEXT,
                   messages              INTEGER DEFAULT 0,
                   seen_via_chatters_api BOOLEAN DEFAULT FALSE,
                   confirmed_first_ever  BOOLEAN DEFAULT FALSE,
                   is_first_time_streamer BOOLEAN DEFAULT FALSE,
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

    fn zeit(stunden: i64) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-08-27T18:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
            + chrono::Duration::hours(stunden)
    }

    async fn rollup(pool: &PgPool, login: &str, erster: DateTime<Utc>) {
        sqlx::query(
            "INSERT INTO twitch_chatter_rollup
             (streamer_login, chatter_login, total_messages, first_seen_at, last_seen_at)
             VALUES ('earlysalty', $1, 5, $2, $2)",
        )
        .bind(login)
        .bind(erster)
        .execute(pool)
        .await
        .expect("rollup");
    }

    /// Eine Zeile, wie sie der Nachrichten-Pfad schreibt: mit Nachrichten und
    /// mit dem Zeitpunkt der ersten.
    async fn chatter(
        pool: &PgPool,
        session_id: i64,
        login: &str,
        erstmals: bool,
        erste: DateTime<Utc>,
    ) {
        sqlx::query(
            "INSERT INTO twitch_session_chatters
             (session_id, streamer_login, chatter_login, messages,
              confirmed_first_ever, first_message_at)
             VALUES ($1, 'earlysalty', $2, 3, $3, $4)",
        )
        .bind(session_id)
        .bind(login)
        .bind(erstmals)
        .bind(erste)
        .execute(pool)
        .await
        .expect("chatter");
    }

    /// Eine Zeile, wie sie der Chatters-Poller schreibt: anwesend, aber ohne
    /// eine einzige Nachricht.
    async fn zuschauer(pool: &PgPool, session_id: i64, login: &str, gesehen: DateTime<Utc>) {
        sqlx::query(
            "INSERT INTO twitch_session_chatters
             (session_id, streamer_login, chatter_login, messages,
              seen_via_chatters_api, confirmed_first_ever, first_message_at)
             VALUES ($1, 'earlysalty', $2, 0, TRUE, FALSE, $3)",
        )
        .bind(session_id)
        .bind(login)
        .bind(gesehen)
        .execute(pool)
        .await
        .expect("zuschauer");
    }

    fn logins(namen: &[&str]) -> Vec<String> {
        namen.iter().map(|s| s.to_string()).collect()
    }

    #[tokio::test]
    async fn unbekannter_login_ist_erster_chat_ueberhaupt() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_verlauf_unbekannt").await;
        let eintraege = laden(
            &pool,
            "earlysalty",
            &logins(&["neuling"]),
            Some(7),
            Some(zeit(0)),
        )
        .await
        .expect("Abfrage");
        assert_eq!(
            eintraege,
            vec![VerlaufEintrag {
                login: "neuling".into(),
                erster_chat_ueberhaupt: true,
                erster_chat_am: None,
                sessions: 0,
            }]
        );
    }

    #[tokio::test]
    async fn stammgast_ist_kein_erstchatter() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_verlauf_stammgast").await;
        // Schreibt seit gestern, war bei zwei Streams dabei.
        rollup(&pool, "stammgast", zeit(-24)).await;
        chatter(&pool, 6, "stammgast", false, zeit(-24)).await;
        chatter(&pool, 7, "stammgast", false, zeit(1)).await;

        let eintraege = laden(
            &pool,
            "earlysalty",
            &logins(&["stammgast"]),
            Some(7),
            Some(zeit(0)),
        )
        .await
        .expect("Abfrage");
        assert!(!eintraege[0].erster_chat_ueberhaupt);
        assert_eq!(eintraege[0].erster_chat_am, Some(zeit(-24)));
        assert_eq!(eintraege[0].sessions, 2);
    }

    /// Der Fall, der ohne Session-Startzeit falsch wäre: wer gerade eben zum
    /// ersten Mal geschrieben hat, hat schon eine Zeile im Verlauf.
    #[tokio::test]
    async fn verlauf_beginnt_in_dieser_session_zaehlt_als_erster_chat() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_verlauf_gerade_eben").await;
        rollup(&pool, "neuling", zeit(1)).await;
        chatter(&pool, 7, "neuling", false, zeit(1)).await;

        let eintraege = laden(
            &pool,
            "earlysalty",
            &logins(&["neuling"]),
            Some(7),
            Some(zeit(0)),
        )
        .await
        .expect("Abfrage");
        assert!(eintraege[0].erster_chat_ueberhaupt);
        assert_eq!(eintraege[0].erster_chat_am, Some(zeit(1)));
    }

    /// Das eigene Kennzeichen des Bots gewinnt, auch wenn der Verlauf älter
    /// aussieht.
    #[tokio::test]
    async fn kennzeichen_des_bots_gewinnt() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_verlauf_kennzeichen").await;
        rollup(&pool, "markiert", zeit(-48)).await;
        // Der Verlauf sieht alt aus, das Kennzeichen des Bots sagt trotzdem
        // "zum ersten Mal".
        chatter(&pool, 5, "markiert", false, zeit(-48)).await;
        chatter(&pool, 7, "markiert", true, zeit(1)).await;

        let eintraege = laden(
            &pool,
            "earlysalty",
            &logins(&["markiert"]),
            Some(7),
            Some(zeit(0)),
        )
        .await
        .expect("Abfrage");
        assert!(eintraege[0].erster_chat_ueberhaupt);
    }

    #[tokio::test]
    async fn mehrere_logins_kommen_alle_zurueck_und_fremder_kanal_zaehlt_nicht() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_verlauf_bund").await;
        rollup(&pool, "stammgast", zeit(-24)).await;
        chatter(&pool, 6, "stammgast", false, zeit(-24)).await;
        chatter(&pool, 7, "stammgast", false, zeit(1)).await;
        // Derselbe Name in einem anderen Kanal darf nichts ändern.
        sqlx::query(
            "INSERT INTO twitch_chatter_rollup
             (streamer_login, chatter_login, first_seen_at, last_seen_at)
             VALUES ('fremdkanal', 'neuling', $1, $1)",
        )
        .bind(zeit(-72))
        .execute(&pool)
        .await
        .expect("fremder Kanal");

        let eintraege = laden(
            &pool,
            "earlysalty",
            &logins(&["stammgast", "neuling", "NOCHEINER"]),
            Some(7),
            Some(zeit(0)),
        )
        .await
        .expect("Abfrage");
        assert_eq!(eintraege.len(), 3);
        let neu: Vec<&str> = eintraege
            .iter()
            .filter(|e| e.erster_chat_ueberhaupt)
            .map(|e| e.login.as_str())
            .collect();
        assert_eq!(neu, vec!["neuling", "nocheiner"]);
    }

    /// Der Fall aus dem Betrieb: jemand schaut seit Wochen zu, ohne je zu
    /// schreiben, und schreibt heute zum ersten Mal.
    ///
    /// So sieht die Datenbank in dem Moment aus: der Chatters-Poller hat je
    /// Stream eine stille Zeile angelegt und eine Rollup-Zeile von damals;
    /// der Nachrichten-Pfad hat gerade `total_messages` erhöht, `first_seen_at`
    /// aber stehen lassen, und `confirmed_first_ever` ist nicht gesetzt, weil
    /// Twitchs eigenes Kennzeichen nicht immer kommt. Wer den Verlauf aus dem
    /// Rollup liest, hält die Person deshalb für einen alten Bekannten.
    #[tokio::test]
    async fn wer_bisher_nur_zugeschaut_hat_ist_erstchatter() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_verlauf_lurker").await;
        // Rollup-Zeile von damals, durch die erste Nachricht gerade auf eins
        // erhöht.
        sqlx::query(
            "INSERT INTO twitch_chatter_rollup
             (streamer_login, chatter_login, total_messages, first_seen_at, last_seen_at)
             VALUES ('earlysalty', 'stiller', 1, $1, NOW())",
        )
        .bind(zeit(-240))
        .execute(&pool)
        .await
        .expect("rollup");
        // Zwei Streams nur zugeschaut.
        zuschauer(&pool, 5, "stiller", zeit(-240)).await;
        zuschauer(&pool, 6, "stiller", zeit(-48)).await;
        // Und heute die erste Nachricht.
        chatter(&pool, 7, "stiller", false, zeit(1)).await;

        let eintraege = laden(
            &pool,
            "earlysalty",
            &logins(&["stiller"]),
            Some(7),
            Some(zeit(0)),
        )
        .await
        .expect("Abfrage");
        assert!(
            eintraege[0].erster_chat_ueberhaupt,
            "wer noch nie geschrieben hat, ist beim ersten Mal ein Erstchatter"
        );
        assert_eq!(
            eintraege[0].erster_chat_am,
            Some(zeit(1)),
            "erster_chat_am ist der erste Chat, nicht die erste Anwesenheit"
        );
        assert_eq!(eintraege[0].sessions, 3, "dabei war er dreimal");
    }

    #[tokio::test]
    async fn mehr_als_die_grenze_wird_abgeschnitten() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_verlauf_grenze").await;
        let viele: Vec<String> = (0..80).map(|i| format!("nutzer{i}")).collect();
        let eintraege = laden(&pool, "earlysalty", &viele, Some(7), Some(zeit(0)))
            .await
            .expect("Abfrage");
        assert_eq!(eintraege.len(), LOGINS_MAX);
    }
}
