//! Query für `GET /twitch/api/v2/overview` (Admin).

use chrono::{NaiveDate, NaiveTime};
use serde::Serialize;
use sqlx::PgPool;

/// Aggregierte Metriken für einen Zeitraum.
///
/// Felder spiegeln Pythons `_calculate_overview_metrics` (session-abgeleitete
/// Teilmenge — die chatter-basierten Felder uniqueChatters/engagement folgen
/// separat, da sie Joins auf twitch_session_chatters + Bot-Filter brauchen).
/// Retention-Werte sind hier als Rohbruch (LEAST(1.0,..)) aggregiert; das *100
/// macht der Aufrufer (wie Python).
#[derive(Debug, sqlx::FromRow)]
pub struct OverviewMetricsRow {
    pub avg_avg_viewers: Option<f64>,
    pub max_peak_viewers: Option<i64>,
    pub total_hours_watched: Option<f64>,
    pub total_airtime_hours: Option<f64>,
    pub total_followers: Option<i64>,
    pub gained_followers: Option<i64>,
    pub avg_retention_10m: Option<f64>,
    pub retention_sample_count: Option<i64>,
    pub chat_sample_count: Option<i64>,
    pub follower_valid_count: Option<i64>,
    pub session_count: Option<i64>,
}

/// Holt aggregierte Metriken für einen Streamer im angegebenen Zeitraum.
///
/// `streamer_login`: `None` → alle Streamer aggregiert.
/// `since`: ISO-8601-String (>= since_date).
pub async fn overview_metrics(
    pool: &PgPool,
    since: &str,
    streamer_login: Option<&str>,
    until: Option<&str>,
) -> Result<Option<OverviewMetricsRow>, sqlx::Error> {
    sqlx::query_as::<_, OverviewMetricsRow>(
        r#"
        WITH sessions AS (
            SELECT
                s.*,
                CASE
                    WHEN s.started_at IS NOT NULL AND s.ended_at IS NOT NULL
                    THEN GREATEST(
                        0.0::FLOAT8,
                        EXTRACT(EPOCH FROM (
                            s.ended_at::text::TIMESTAMPTZ
                            - s.started_at::text::TIMESTAMPTZ
                        ))::FLOAT8
                    )
                    ELSE GREATEST(COALESCE(s.duration_seconds, 0)::FLOAT8, 0.0::FLOAT8)
                END AS effective_duration_seconds
            FROM twitch_stream_sessions s
            WHERE s.started_at::text::TIMESTAMPTZ >= $1::text::TIMESTAMPTZ
              AND s.ended_at IS NOT NULL
              AND ($2::TEXT IS NULL OR LOWER(s.streamer_login) = LOWER($2))
              AND ($3::TEXT IS NULL OR s.started_at::text::TIMESTAMPTZ < $3::text::TIMESTAMPTZ)
        )
        SELECT
            AVG(s.avg_viewers)::FLOAT8                                AS avg_avg_viewers,
            MAX(s.peak_viewers)::BIGINT                               AS max_peak_viewers,
            SUM(s.avg_viewers * s.effective_duration_seconds / 3600.0)::FLOAT8
                                                                        AS total_hours_watched,
            SUM(s.effective_duration_seconds / 3600.0)::FLOAT8        AS total_airtime_hours,
            SUM(CASE
                    WHEN s.follower_delta IS NOT NULL
                     AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                    THEN s.follower_delta
                    ELSE 0
                END)::BIGINT                                          AS total_followers,
            COALESCE(SUM(CASE
                    WHEN s.follower_delta > 0
                     AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                    THEN s.follower_delta
                    ELSE 0
                END), 0)::BIGINT                                      AS gained_followers,
            AVG(CASE
                    WHEN s.avg_viewers >= 3 AND s.peak_viewers > 0
                    THEN LEAST(1.0, s.retention_10m)
                    ELSE NULL
                END)::FLOAT8                                          AS avg_retention_10m,
            COUNT(CASE
                    WHEN s.avg_viewers >= 3 AND s.peak_viewers > 0 AND s.retention_10m IS NOT NULL
                    THEN 1
                END)::BIGINT                                          AS retention_sample_count,
            COUNT(CASE
                    WHEN s.avg_viewers >= 3 AND s.peak_viewers > 0 AND s.unique_chatters IS NOT NULL
                    THEN 1
                END)::BIGINT                                          AS chat_sample_count,
            COUNT(CASE
                    WHEN s.follower_delta IS NOT NULL
                     AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                    THEN 1
                END)::BIGINT                                          AS follower_valid_count,
            COUNT(*)::BIGINT                                          AS session_count
        FROM sessions s
        "#,
    )
    .bind(since)
    .bind(streamer_login)
    .bind(until)
    .fetch_optional(pool)
    .await
}

/// Existenz-Check: gibt 0 zurück wenn keine Sessions vorhanden.
pub async fn overview_session_count(
    pool: &PgPool,
    since: &str,
    streamer_login: Option<&str>,
) -> Result<i64, sqlx::Error> {
    let count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)::BIGINT
        FROM twitch_stream_sessions s
        WHERE s.started_at::text::TIMESTAMPTZ >= $1::text::TIMESTAMPTZ
          AND s.ended_at IS NOT NULL
          AND ($2::TEXT IS NULL OR LOWER(s.streamer_login) = LOWER($2))
        "#,
    )
    .bind(since)
    .bind(streamer_login)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

/// Bekannte Chat-Bot-Logins (Python `KNOWN_CHAT_BOTS`, core/chat_bots.py) —
/// werden aus Chatter-Zählungen gefiltert. Kleingeschrieben.
pub const KNOWN_CHAT_BOTS: &[&str] = &[
    "botrix",
    "deutschedeadlockcommunity",
    "fossabot",
    "moobot",
    "nightbot",
    "pretzelrocks",
    "soundalerts",
    "streamelements",
    "streamlabs",
    "wizebot",
];

/// Chatter-abgeleitete Overview-Metriken (Bot-gefiltert).
#[derive(Debug, Default, Clone, Copy)]
pub struct OverviewChatterMetrics {
    /// Fenster-distinkte Chatter mit ≥1 Nachricht + Legacy-Aggregat für
    /// Sessions ohne Per-Chatter-Zeilen (Python total_unique_chatters).
    pub unique_chatters: i64,
    /// Distinkte aktive Chatter (≥1 Nachricht), Tracked-Teil (Python active_chatters).
    pub active_chatters: i64,
    /// Distinkte Zuschauer (Nachricht ODER via Chatters-API gesehen).
    pub unique_viewers: i64,
    /// active_chatters / unique_viewers * 100, 2 Nachkommastellen (Python engagement_rate).
    pub engagement_rate: f64,
}

/// Berechnet die chatter-basierten Overview-Metriken über
/// `twitch_session_chatters` (Bot-gefiltert, Python `_calculate_overview_metrics`-
/// Teilmenge: distinct_tracked + legacy_unique + active_chatters + distinct_viewers).
pub async fn overview_chatter_metrics(
    pool: &PgPool,
    since: &str,
    streamer_login: Option<&str>,
) -> Result<OverviewChatterMetrics, sqlx::Error> {
    let bots: Vec<String> = KNOWN_CHAT_BOTS.iter().map(|b| b.to_string()).collect();

    // distinct_tracked == active_chatters: distinkte Chatter mit ≥1 Nachricht.
    let active = sqlx::query_scalar!(
        r#"
        SELECT COUNT(DISTINCT COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id))::BIGINT AS "count!"
        FROM twitch_session_chatters sc
        JOIN twitch_stream_sessions s ON s.id = sc.session_id
        WHERE s.started_at >= $1::text::TIMESTAMPTZ
          AND s.ended_at IS NOT NULL
          AND ($2::TEXT IS NULL OR LOWER(s.streamer_login) = LOWER($2))
          AND sc.messages > 0
          AND (sc.chatter_login IS NULL OR sc.chatter_login = ''
               OR LOWER(sc.chatter_login) <> ALL($3::text[]))
        "#,
        since,
        streamer_login,
        &bots
    )
    .fetch_one(pool)
    .await?;

    // legacy_unique: Alt-Sessions ohne Per-Chatter-Zeilen.
    let legacy = sqlx::query_scalar!(
        r#"
        SELECT COALESCE(SUM(s.unique_chatters), 0)::BIGINT AS "count!"
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1::text::TIMESTAMPTZ
          AND s.ended_at IS NOT NULL
          AND ($2::TEXT IS NULL OR LOWER(s.streamer_login) = LOWER($2))
          AND NOT EXISTS (
              SELECT 1 FROM twitch_session_chatters sc WHERE sc.session_id = s.id
          )
        "#,
        since,
        streamer_login
    )
    .fetch_one(pool)
    .await?;

    // distinct_viewers: Nachricht ODER via Chatters-API gesehen.
    let viewers = sqlx::query_scalar!(
        r#"
        SELECT COUNT(DISTINCT COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id))::BIGINT AS "count!"
        FROM twitch_session_chatters sc
        JOIN twitch_stream_sessions s ON s.id = sc.session_id
        WHERE s.started_at >= $1::text::TIMESTAMPTZ
          AND s.ended_at IS NOT NULL
          AND ($2::TEXT IS NULL OR LOWER(s.streamer_login) = LOWER($2))
          AND (sc.messages > 0 OR COALESCE(sc.seen_via_chatters_api, FALSE) IS TRUE)
          AND (sc.chatter_login IS NULL OR sc.chatter_login = ''
               OR LOWER(sc.chatter_login) <> ALL($3::text[]))
        "#,
        since,
        streamer_login,
        &bots
    )
    .fetch_one(pool)
    .await?;

    let engagement_rate = if viewers > 0 {
        ((active as f64 / viewers as f64) * 100.0 * 100.0).round() / 100.0
    } else {
        0.0
    };

    Ok(OverviewChatterMetrics {
        unique_chatters: active + legacy,
        active_chatters: active,
        unique_viewers: viewers,
        engagement_rate,
    })
}

/// Raid-Netzwerk-Kachel des Overviews (Python `_get_network_stats`).
#[derive(Debug, Default, Clone, Copy)]
pub struct OverviewNetworkStats {
    /// Erfolgreiche ausgehende Raids im Zeitraum.
    pub sent: i64,
    /// Summe der mitgenommenen Zuschauer (viewer_count) der ausgehenden Raids.
    pub sent_viewers: i64,
    /// Erfolgreiche eingehende Raids im Zeitraum.
    pub received: i64,
}

/// Zählt erfolgreiche ein-/ausgehende Raids für die Netzwerk-Kachel.
/// Ohne `streamer_login` (Aggregat über alle) → alles 0 (wie Python).
pub async fn overview_network_stats(
    pool: &PgPool,
    since: &str,
    streamer_login: Option<&str>,
) -> Result<OverviewNetworkStats, sqlx::Error> {
    let Some(login) = streamer_login else {
        return Ok(OverviewNetworkStats::default());
    };

    let sent_row = sqlx::query!(
        r#"
        SELECT COUNT(*)::BIGINT AS "sent!", COALESCE(SUM(viewer_count), 0)::BIGINT AS "sent_viewers!"
        FROM twitch_raid_history
        WHERE LOWER(from_broadcaster_login) = LOWER($1)
          AND executed_at >= $2::text::TIMESTAMPTZ
          AND COALESCE(success, FALSE) IS TRUE
        "#,
        login,
        since
    )
    .fetch_one(pool)
    .await?;

    let received = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*)::BIGINT AS "received!"
        FROM twitch_raid_history
        WHERE LOWER(to_broadcaster_login) = LOWER($1)
          AND executed_at >= $2::text::TIMESTAMPTZ
          AND COALESCE(success, FALSE) IS TRUE
        "#,
        login,
        since
    )
    .fetch_one(pool)
    .await?;

    Ok(OverviewNetworkStats {
        sent: sent_row.sent,
        sent_viewers: sent_row.sent_viewers,
        received,
    })
}

/// Monetarisierungs-Event-Zähler für den Monetization-Health-Score
/// (Python `_get_monetization_event_counts`). Die Event-Tabellen existieren
/// nicht überall — fehlende Tabelle/Spalte je Query → 0 (Python try/except).
#[derive(Debug, Default, Clone, Copy)]
pub struct OverviewMonetization {
    pub sub_events: i64,
    pub bits_events: i64,
    pub hype_trains: i64,
}

/// Entscheidet, ob ein Datenbankfehler die "Tabelle gibt es hier nicht"-Lage
/// beschreibt. Nur die zaehlt als 0; alles andere (Zeitueberschreitung beim
/// Holen einer Verbindung, Verbindungsabbruch, Syntaxfehler) muss sichtbar
/// bleiben, sonst zeigt der Monetarisierungs-Score still eine 0 an.
fn ist_fehlende_tabelle(err: &sqlx::Error) -> bool {
    match err {
        sqlx::Error::Database(db) => matches!(
            db.code().as_deref(),
            // undefined_table, undefined_column, undefined_object
            Some("42P01") | Some("42703") | Some("42704")
        ),
        // Spaltenname passt nicht zum erwarteten Schema.
        sqlx::Error::ColumnNotFound(_) => true,
        _ => false,
    }
}

/// Wandelt das Ergebnis einer Zaehl-Abfrage in eine Zahl: fehlende Tabelle → 0,
/// jeder andere Fehler bleibt ein Fehler.
fn zaehler_oder_null(res: Result<i64, sqlx::Error>) -> Result<i64, sqlx::Error> {
    match res {
        Ok(v) => Ok(v),
        Err(e) if ist_fehlende_tabelle(&e) => Ok(0),
        Err(e) => Err(e),
    }
}

/// Zählt Sub-/Bits-/Hype-Train-Events im Zeitraum. Ohne Streamer → 0.
/// Fehlende Event-Tabellen ergeben 0, andere Datenbankfehler werden gemeldet.
pub async fn overview_monetization_counts(
    pool: &PgPool,
    since: &str,
    streamer_login: Option<&str>,
) -> Result<OverviewMonetization, sqlx::Error> {
    let Some(login) = streamer_login else {
        return Ok(OverviewMonetization::default());
    };

    let sub_events = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*)::BIGINT AS "count!"
        FROM twitch_subscription_events e
        LEFT JOIN twitch_stream_sessions s ON s.id = e.session_id
        LEFT JOIN twitch_live_state l ON l.twitch_user_id = e.twitch_user_id
        WHERE e.received_at >= $1::text::TIMESTAMPTZ
          AND LOWER(COALESCE(s.streamer_login, l.streamer_login, '')) = LOWER($2)
        "#,
        since,
        login
    )
    .fetch_one(pool)
    .await;
    let sub_events = zaehler_oder_null(sub_events)?;

    let bits_events = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*)::BIGINT AS "count!"
        FROM twitch_bits_events e
        LEFT JOIN twitch_stream_sessions s ON s.id = e.session_id
        LEFT JOIN twitch_live_state l ON l.twitch_user_id = e.twitch_user_id
        WHERE e.received_at >= $1::text::TIMESTAMPTZ
          AND LOWER(COALESCE(s.streamer_login, l.streamer_login, '')) = LOWER($2)
        "#,
        since,
        login
    )
    .fetch_one(pool)
    .await;
    let bits_events = zaehler_oder_null(bits_events)?;

    let hype_trains = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*)::BIGINT AS "count!"
        FROM twitch_hype_train_events h
        LEFT JOIN twitch_stream_sessions s ON s.id = h.session_id
        WHERE h.started_at >= $1::text::TIMESTAMPTZ AND h.ended_at IS NOT NULL
          AND LOWER(COALESCE(s.streamer_login, '')) = LOWER($2)
        "#,
        since,
        login
    )
    .fetch_one(pool)
    .await;
    let hype_trains = zaehler_oder_null(hype_trains)?;

    Ok(OverviewMonetization {
        sub_events,
        bits_events,
        hype_trains,
    })
}

/// Chatter pro 100 Peak-Viewer, über Sessions gemittelt (Python `chat_per_100`
/// aus `_calculate_overview_metrics`). Pro Session: distinkte Chatter (Bot-
/// gefiltert) ODER Legacy-`unique_chatters`, gedeckelt bei 100, gegated avg>=3
/// & peak>0. Kein Sample → 0.
pub async fn overview_chat_per_100(
    pool: &PgPool,
    since: &str,
    streamer_login: Option<&str>,
) -> Result<f64, sqlx::Error> {
    let bots: Vec<String> = KNOWN_CHAT_BOTS.iter().map(|b| b.to_string()).collect();
    let avg = sqlx::query_scalar!(
        r#"
        WITH fsc AS (
            SELECT sc.session_id,
                   COUNT(DISTINCT CASE WHEN sc.messages > 0
                        THEN COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id) END) AS unique_chatters
            FROM twitch_session_chatters sc
            JOIN twitch_stream_sessions s ON s.id = sc.session_id
            WHERE s.started_at >= $1::text::TIMESTAMPTZ AND s.ended_at IS NOT NULL
              AND ($2::TEXT IS NULL OR LOWER(s.streamer_login) = LOWER($2))
              AND (sc.chatter_login IS NULL OR sc.chatter_login = ''
                   OR LOWER(sc.chatter_login) <> ALL($3::text[]))
            GROUP BY sc.session_id
        ),
        scp AS (
            SELECT sc.session_id, 1 AS has_any_chatters
            FROM twitch_session_chatters sc
            JOIN twitch_stream_sessions s ON s.id = sc.session_id
            WHERE s.started_at >= $1::text::TIMESTAMPTZ AND s.ended_at IS NOT NULL
              AND ($2::TEXT IS NULL OR LOWER(s.streamer_login) = LOWER($2))
            GROUP BY sc.session_id
        )
        SELECT AVG(CASE
                WHEN s.avg_viewers >= 3 AND s.peak_viewers > 0
                THEN LEAST(
                    100.0,
                    (CASE WHEN scp.has_any_chatters = 1 THEN COALESCE(fsc.unique_chatters, 0)
                          ELSE COALESCE(s.unique_chatters, 0) END) * 100.0 / NULLIF(s.peak_viewers, 0)
                )
                ELSE NULL
            END)::FLOAT8
        FROM twitch_stream_sessions s
        LEFT JOIN fsc ON fsc.session_id = s.id
        LEFT JOIN scp ON scp.session_id = s.id
        WHERE s.started_at >= $1::text::TIMESTAMPTZ AND s.ended_at IS NOT NULL
          AND ($2::TEXT IS NULL OR LOWER(s.streamer_login) = LOWER($2))
        "#,
        since,
        streamer_login,
        &bots
    )
    .fetch_one(pool)
    .await?;
    Ok(avg.unwrap_or(0.0))
}

/// Eine Session-Zeile der Overview-Sessions-Liste (Python `_get_sessions`).
#[derive(Debug, Clone, Serialize)]
pub struct OverviewSession {
    pub id: i64,
    pub date: String,
    #[serde(rename = "startTime")]
    pub start_time: String,
    pub duration: i64,
    #[serde(rename = "startViewers")]
    pub start_viewers: i64,
    #[serde(rename = "peakViewers")]
    pub peak_viewers: i64,
    #[serde(rename = "endViewers")]
    pub end_viewers: i64,
    #[serde(rename = "avgViewers")]
    pub avg_viewers: f64,
    #[serde(rename = "retention5m")]
    pub retention_5m: f64,
    #[serde(rename = "retention10m")]
    pub retention_10m: f64,
    #[serde(rename = "retention20m")]
    pub retention_20m: f64,
    #[serde(rename = "dropoffPct")]
    pub dropoff_pct: f64,
    #[serde(rename = "uniqueChatters")]
    pub unique_chatters: i64,
    #[serde(rename = "totalChatterSessions")]
    pub total_chatter_sessions: i64,
    #[serde(rename = "firstTimeChatters")]
    pub first_time_chatters: i64,
    #[serde(rename = "returningChatters")]
    pub returning_chatters: i64,
    #[serde(rename = "followersStart")]
    pub followers_start: i64,
    #[serde(rename = "followersEnd")]
    pub followers_end: i64,
    pub title: String,
}

#[derive(sqlx::FromRow)]
struct SessionRaw {
    id: i64,
    start_date: NaiveDate,
    start_time: NaiveTime,
    duration_seconds: Option<i64>,
    start_viewers: Option<i64>,
    peak_viewers: Option<i64>,
    end_viewers: Option<i64>,
    avg_viewers: Option<f64>,
    retention_5m: Option<f64>,
    retention_10m: Option<f64>,
    retention_20m: Option<f64>,
    dropoff_pct: Option<f64>,
    unique_chatters: Option<i64>,
    first_time_chatters: Option<i64>,
    returning_chatters: Option<i64>,
    followers_start: Option<i64>,
    followers_end: Option<i64>,
    title: Option<String>,
}

/// Liste der jüngsten Sessions mit Metriken (Python `_get_sessions`, LIMIT 50).
/// Chatter-Zähler Bot-gefiltert mit Presence-Fallback auf die Legacy-Spalten;
/// Retention hart auf [0,1] geklemmt und *100; dropoff *100.
pub async fn overview_sessions(
    pool: &PgPool,
    since: &str,
    streamer_login: Option<&str>,
    limit: i64,
) -> Result<Vec<OverviewSession>, sqlx::Error> {
    let bots: Vec<String> = KNOWN_CHAT_BOTS.iter().map(|b| b.to_string()).collect();
    let raws: Vec<SessionRaw> = sqlx::query_as(
        r#"
        WITH base_sessions AS (
            SELECT s.id, s.started_at,
                   CASE
                       WHEN s.started_at IS NOT NULL AND s.ended_at IS NOT NULL
                       THEN GREATEST(
                           0::BIGINT,
                           FLOOR(EXTRACT(EPOCH FROM (
                               s.ended_at::text::TIMESTAMPTZ
                               - s.started_at::text::TIMESTAMPTZ
                           )))::BIGINT
                       )
                       ELSE GREATEST(COALESCE(s.duration_seconds, 0)::BIGINT, 0::BIGINT)
                   END AS duration_seconds,
                   s.start_viewers, s.peak_viewers,
                   s.end_viewers, s.avg_viewers, s.retention_5m, s.retention_10m, s.retention_20m,
                   s.dropoff_pct, s.unique_chatters, s.first_time_chatters, s.returning_chatters,
                   s.followers_start, s.followers_end, s.stream_title
            FROM twitch_stream_sessions s
            WHERE s.started_at::text::TIMESTAMPTZ >= $1::text::TIMESTAMPTZ AND s.ended_at IS NOT NULL
              AND ($2::TEXT IS NULL OR LOWER(s.streamer_login) = LOWER($2))
            ORDER BY s.started_at::text::TIMESTAMPTZ DESC
            LIMIT $3
        ),
        filtered_chatters AS (
            SELECT sc.session_id,
                COUNT(DISTINCT CASE WHEN sc.messages > 0
                    THEN COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id) END) AS unique_chatters,
                COUNT(DISTINCT CASE WHEN sc.messages > 0
                    AND COALESCE(sc.is_first_time_streamer, FALSE) IS TRUE
                    THEN COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id) END) AS first_time_chatters,
                COUNT(DISTINCT CASE WHEN sc.messages > 0
                    AND COALESCE(sc.is_first_time_streamer, FALSE) IS FALSE
                    THEN COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id) END) AS returning_chatters
            FROM twitch_session_chatters sc
            JOIN base_sessions bs ON bs.id = sc.session_id
            WHERE (sc.chatter_login IS NULL OR sc.chatter_login = ''
                   OR LOWER(sc.chatter_login) <> ALL($4::text[]))
            GROUP BY sc.session_id
        ),
        session_chatter_presence AS (
            SELECT sc.session_id, 1 AS has_any_chatters
            FROM twitch_session_chatters sc
            JOIN base_sessions bs ON bs.id = sc.session_id
            GROUP BY sc.session_id
        )
        SELECT
            bs.id AS id,
            CAST(bs.started_at AS DATE) AS start_date,
            CAST(bs.started_at AS TIME) AS start_time,
            bs.duration_seconds::BIGINT AS duration_seconds,
            bs.start_viewers::BIGINT AS start_viewers,
            bs.peak_viewers::BIGINT AS peak_viewers,
            bs.end_viewers::BIGINT AS end_viewers,
            bs.avg_viewers::FLOAT8 AS avg_viewers,
            COALESCE(bs.retention_5m, 0)::FLOAT8 AS retention_5m,
            COALESCE(bs.retention_10m, 0)::FLOAT8 AS retention_10m,
            COALESCE(bs.retention_20m, 0)::FLOAT8 AS retention_20m,
            COALESCE(bs.dropoff_pct, 0)::FLOAT8 AS dropoff_pct,
            (CASE WHEN scp.has_any_chatters = 1 THEN COALESCE(fc.unique_chatters, 0)
                  ELSE COALESCE(bs.unique_chatters, 0) END)::BIGINT AS unique_chatters,
            (CASE WHEN scp.has_any_chatters = 1 THEN COALESCE(fc.first_time_chatters, 0)
                  ELSE COALESCE(bs.first_time_chatters, 0) END)::BIGINT AS first_time_chatters,
            (CASE WHEN scp.has_any_chatters = 1 THEN COALESCE(fc.returning_chatters, 0)
                  ELSE COALESCE(bs.returning_chatters, 0) END)::BIGINT AS returning_chatters,
            COALESCE(bs.followers_start, 0)::BIGINT AS followers_start,
            COALESCE(bs.followers_end, 0)::BIGINT AS followers_end,
            COALESCE(bs.stream_title, '') AS title
        FROM base_sessions bs
        LEFT JOIN filtered_chatters fc ON fc.session_id = bs.id
        LEFT JOIN session_chatter_presence scp ON scp.session_id = bs.id
        ORDER BY bs.started_at::text::TIMESTAMPTZ DESC
        "#,
    )
    .bind(since)
    .bind(streamer_login)
    .bind(limit)
    .bind(&bots)
    .fetch_all(pool)
    .await?;

    let clamp_pct = |v: f64| v.clamp(0.0, 1.0) * 100.0;
    Ok(raws
        .into_iter()
        .map(|r| OverviewSession {
            id: r.id,
            date: r.start_date.to_string(),
            start_time: r.start_time.to_string(),
            duration: r.duration_seconds.unwrap_or(0),
            start_viewers: r.start_viewers.unwrap_or(0),
            peak_viewers: r.peak_viewers.unwrap_or(0),
            end_viewers: r.end_viewers.unwrap_or(0),
            avg_viewers: r.avg_viewers.unwrap_or(0.0),
            retention_5m: clamp_pct(r.retention_5m.unwrap_or(0.0)),
            retention_10m: clamp_pct(r.retention_10m.unwrap_or(0.0)),
            retention_20m: clamp_pct(r.retention_20m.unwrap_or(0.0)),
            dropoff_pct: r.dropoff_pct.unwrap_or(0.0) * 100.0,
            unique_chatters: r.unique_chatters.unwrap_or(0),
            total_chatter_sessions: r.unique_chatters.unwrap_or(0),
            first_time_chatters: r.first_time_chatters.unwrap_or(0),
            returning_chatters: r.returning_chatters.unwrap_or(0),
            followers_start: r.followers_start.unwrap_or(0),
            followers_end: r.followers_end.unwrap_or(0),
            title: r.title.unwrap_or_default(),
        })
        .collect())
}

/// Kategorie-Perzentil/Rang eines Streamers (Python `_get_category_percentiles`,
/// `_percentile_of` und die Rank-Berechnung). `percentile` speist den Reach-Score;
/// die Felder `rank` und `total` fuellen categoryRank/categoryTotal.
#[derive(Debug, Clone, Copy)]
pub struct CategoryRank {
    pub percentile: f64,
    pub rank: i64,
    pub total: i64,
}

/// Liefert Perzentil/Rang aus `twitch_stats_category` (per-Streamer AVG der
/// Viewer, `ts_utc` als `TIMESTAMPTZ`). Ohne Streamer, leere Daten oder Streamer nicht
/// in den Kategorie-Daten → `None`; Query-Fehler werden propagiert.
pub async fn overview_category_rank(
    pool: &PgPool,
    since: &str,
    streamer_login: Option<&str>,
) -> Result<Option<CategoryRank>, sqlx::Error> {
    let Some(login) = streamer_login else {
        return Ok(None);
    };
    let login = login.to_lowercase();
    // `since` bleibt API-kompatibel als ISO-String; Postgres castet ihn explizit
    // gegen die Prod-`TIMESTAMPTZ`-Spalte.
    let rows = sqlx::query!(
        r#"
        SELECT streamer AS "streamer!", AVG(viewer_count)::FLOAT8 AS "avg_vc!"
        FROM twitch_stats_category
        WHERE ts_utc >= $1::text::TIMESTAMPTZ
        GROUP BY streamer
        ORDER BY AVG(viewer_count)::FLOAT8
        "#,
        since
    )
    .fetch_all(pool)
    .await?;
    if rows.is_empty() {
        return Ok(None);
    }
    let total = rows.len() as i64;
    // streamer_map: bei Lowercase-Kollision gewinnt die letzte Zeile (Python-Dict).
    let Some(value) = rows
        .iter()
        .rev()
        .find(|row| row.streamer.to_lowercase() == login)
        .map(|row| row.avg_vc)
    else {
        return Ok(None);
    };
    // _percentile_of: (below + 0.5*equal) / n.
    let below = rows.iter().filter(|row| row.avg_vc < value).count() as f64;
    let equal = rows.iter().filter(|row| row.avg_vc == value).count() as f64;
    let percentile = (below + 0.5 * equal) / rows.len() as f64;
    // Rank = total - int(percentile * total) (1 = best).
    let rank = total - (percentile * total as f64) as i64;
    Ok(Some(CategoryRank {
        percentile,
        rank,
        total,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect test-db");
        sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
            .execute(&pool)
            .await
            .expect("Schema anlegen fehlgeschlagen");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path setzen fehlgeschlagen");
        // Tabelle frisch anlegen, damit Schema-Änderungen (neue Spalten) auch im
        // persistenten Test-Container greifen (IF NOT EXISTS würde sie überspringen).
        sqlx::query("DROP TABLE IF EXISTS twitch_stream_sessions")
            .execute(&pool)
            .await
            .expect("DROP fehlgeschlagen");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_stream_sessions (
                id               BIGSERIAL PRIMARY KEY,
                streamer_login   TEXT NOT NULL,
                started_at       TIMESTAMPTZ NOT NULL,
                ended_at         TIMESTAMPTZ,
                avg_viewers      DOUBLE PRECISION,
                peak_viewers     INTEGER,
                duration_seconds INTEGER,
                follower_delta   INTEGER,
                followers_start  INTEGER,
                followers_end    INTEGER,
                retention_5m     DOUBLE PRECISION,
                retention_10m    DOUBLE PRECISION,
                retention_20m    DOUBLE PRECISION,
                dropoff_pct      DOUBLE PRECISION,
                start_viewers    INTEGER,
                end_viewers      INTEGER,
                unique_chatters  INTEGER,
                first_time_chatters INTEGER,
                returning_chatters  INTEGER,
                stream_title     TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL fehlgeschlagen");
        sqlx::query("DROP TABLE IF EXISTS twitch_session_chatters")
            .execute(&pool)
            .await
            .expect("DROP chatters fehlgeschlagen");
        sqlx::query(
            r#"
            CREATE TABLE twitch_session_chatters (
                session_id            BIGINT NOT NULL,
                chatter_login         TEXT,
                chatter_id            TEXT,
                messages              INTEGER DEFAULT 0,
                seen_via_chatters_api BOOLEAN DEFAULT FALSE,
                is_first_time_streamer BOOLEAN DEFAULT FALSE
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL chatters fehlgeschlagen");
        sqlx::query("DROP TABLE IF EXISTS twitch_raid_history")
            .execute(&pool)
            .await
            .expect("DROP raid_history fehlgeschlagen");
        sqlx::query(
            r#"
            CREATE TABLE twitch_raid_history (
                from_broadcaster_login TEXT,
                to_broadcaster_login   TEXT,
                viewer_count           BIGINT,
                success                BOOLEAN,
                executed_at            TIMESTAMPTZ
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL raid_history fehlgeschlagen");
        sqlx::query("DROP TABLE IF EXISTS twitch_stats_category")
            .execute(&pool)
            .await
            .expect("DROP stats_category fehlgeschlagen");
        sqlx::query(
            r#"
            CREATE TABLE twitch_stats_category (
                ts_utc       TIMESTAMPTZ,
                streamer     TEXT,
                viewer_count INTEGER
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL stats_category fehlgeschlagen");
        // Tabellen leeren damit Wiederholungsläufe nicht alte Daten sehen
        sqlx::query("TRUNCATE twitch_stream_sessions")
            .execute(&pool)
            .await
            .expect("TRUNCATE fehlgeschlagen");
        pool
    }

    #[tokio::test]
    async fn leere_tabelle_gibt_null_count() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_overview_leer").await;
        let since = "2000-01-01T00:00:00+00:00";
        let count = overview_session_count(&pool, since, None).await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn session_count_und_metrics_fuer_bekannten_streamer() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_overview_mit_daten").await;
        sqlx::query(
            r#"
            INSERT INTO twitch_stream_sessions
                (streamer_login, started_at, ended_at, avg_viewers, peak_viewers,
                 duration_seconds, follower_delta, followers_start, followers_end, retention_10m)
            VALUES
                ('streamer_x', NOW() - INTERVAL '1 day', NOW() - INTERVAL '23 hours',
                 100.0, 200, 3600, 5, 1000, 1005, 0.5)
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let since = "2000-01-01T00:00:00+00:00";
        let count = overview_session_count(&pool, since, Some("streamer_x"))
            .await
            .unwrap();
        assert_eq!(count, 1);

        let metrics = overview_metrics(&pool, since, Some("streamer_x"), None)
            .await
            .unwrap()
            .expect("Sollte Metriken liefern");
        assert_eq!(metrics.session_count, Some(1));
        assert!((metrics.avg_avg_viewers.unwrap() - 100.0).abs() < 0.001);
        // Neue session-abgeleitete Felder.
        assert_eq!(metrics.gained_followers, Some(5));
        assert_eq!(metrics.follower_valid_count, Some(1));
        assert_eq!(metrics.retention_sample_count, Some(1));
        assert!((metrics.avg_retention_10m.unwrap() - 0.5).abs() < 0.001);
    }

    #[tokio::test]
    async fn metrics_nutzen_zeitspanne_statt_korrupter_duration_seconds() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_overview_duration_corrupt").await;
        sqlx::query(
            r#"
            INSERT INTO twitch_stream_sessions
                (streamer_login, started_at, ended_at, avg_viewers, peak_viewers, duration_seconds)
            VALUES
                ('streamer_x', NOW() - INTERVAL '1 day', NOW() - INTERVAL '23 hours',
                 100.0, 200, 1782860400)
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let since = "2000-01-01T00:00:00+00:00";
        let metrics = overview_metrics(&pool, since, Some("streamer_x"), None)
            .await
            .unwrap()
            .expect("Sollte Metriken liefern");
        assert!(
            (metrics.total_airtime_hours.unwrap() - 1.0).abs() < 0.01,
            "1h aus ended_at-started_at statt 495238h aus duration_seconds"
        );
        assert!((metrics.total_hours_watched.unwrap() - 100.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn chatter_metrics_bot_gefiltert_und_engagement() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_overview_chatter").await;
        sqlx::query(
            r#"
            INSERT INTO twitch_stream_sessions
                (id, streamer_login, started_at, ended_at, avg_viewers, peak_viewers, duration_seconds)
            VALUES (1, 'streamer_x', NOW() - INTERVAL '1 day', NOW() - INTERVAL '23 hours', 100.0, 200, 3600)
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO twitch_session_chatters (session_id, chatter_login, chatter_id, messages, seen_via_chatters_api)
            VALUES
                (1, 'alice', 'a1', 3, FALSE),       -- aktiver Chatter + Viewer
                (1, 'streamlabs', 'sl', 9, FALSE),  -- Bot → gefiltert
                (1, 'bob', 'b2', 0, TRUE)           -- nur via API gesehen → Viewer, nicht aktiv
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let since = "2000-01-01T00:00:00+00:00";
        let m = overview_chatter_metrics(&pool, since, Some("streamer_x"))
            .await
            .unwrap();
        assert_eq!(m.active_chatters, 1, "nur alice aktiv (streamlabs=Bot)");
        assert_eq!(
            m.unique_viewers, 2,
            "alice + bob (bob via API), streamlabs=Bot raus"
        );
        assert_eq!(
            m.unique_chatters, 1,
            "1 tracked + 0 legacy (Session hat Chatter-Zeilen)"
        );
        assert!((m.engagement_rate - 50.0).abs() < 0.001, "1/2*100");

        // chat_per_100: 1 distinkter Chatter / 200 Peak * 100 = 0.5.
        let cp100 = overview_chat_per_100(&pool, since, Some("streamer_x"))
            .await
            .unwrap();
        assert!((cp100 - 0.5).abs() < 0.001, "1 Chatter / 200 Peak");
    }

    #[tokio::test]
    async fn network_stats_zaehlt_nur_erfolgreiche_raids() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_overview_network").await;
        sqlx::query(
            r#"
            INSERT INTO twitch_raid_history (from_broadcaster_login, to_broadcaster_login, viewer_count, success, executed_at)
            VALUES
                ('streamer_x', 'partner_a', 40, TRUE,  NOW() - INTERVAL '1 hour'),  -- sent ok
                ('streamer_x', 'partner_b', 10, FALSE, NOW() - INTERVAL '2 hours'), -- sent gescheitert -> ignoriert
                ('partner_c',  'streamer_x', 5, TRUE,  NOW() - INTERVAL '3 hours')  -- received ok
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let since = "2000-01-01T00:00:00+00:00";
        let n = overview_network_stats(&pool, since, Some("streamer_x"))
            .await
            .unwrap();
        assert_eq!(n.sent, 1, "nur der erfolgreiche ausgehende Raid");
        assert_eq!(n.sent_viewers, 40);
        assert_eq!(n.received, 1);

        // Ohne Streamer -> alles 0.
        let agg = overview_network_stats(&pool, since, None).await.unwrap();
        assert_eq!((agg.sent, agg.sent_viewers, agg.received), (0, 0, 0));
    }

    #[tokio::test]
    async fn sessions_liste_felder_und_chatter_split() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_overview_sessions").await;
        sqlx::query(
            r#"
            INSERT INTO twitch_stream_sessions
                (id, streamer_login, started_at, ended_at, avg_viewers, peak_viewers,
                 duration_seconds, retention_5m, retention_10m, retention_20m, dropoff_pct,
                 start_viewers, end_viewers, followers_start, followers_end, stream_title)
            VALUES (2, 'streamer_x', NOW() - INTERVAL '1 day', NOW() - INTERVAL '22 hours',
                    50.0, 100, 1782860400, 0.7, 0.6, 0.5, 0.1, 10, 40, 1000, 1010, 'Test Titel')
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO twitch_session_chatters (session_id, chatter_login, chatter_id, messages, is_first_time_streamer)
            VALUES (2, 'alice', 'a1', 3, TRUE),
                   (2, 'carol', 'c1', 2, FALSE),
                   (2, 'streamlabs', 'sl', 9, FALSE)
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let since = "2000-01-01T00:00:00+00:00";
        let list = overview_sessions(&pool, since, Some("streamer_x"), 50)
            .await
            .unwrap();
        assert_eq!(list.len(), 1);
        let s = &list[0];
        assert_eq!(s.id, 2);
        assert_eq!(s.peak_viewers, 100);
        assert_eq!(s.start_viewers, 10);
        assert_eq!(s.end_viewers, 40);
        assert_eq!(s.duration, 7200);
        assert!((s.retention_10m - 60.0).abs() < 0.01);
        assert!((s.retention_5m - 70.0).abs() < 0.01);
        assert!((s.retention_20m - 50.0).abs() < 0.01);
        assert!((s.dropoff_pct - 10.0).abs() < 0.01);
        assert_eq!(s.unique_chatters, 2, "alice+carol (streamlabs=Bot raus)");
        assert_eq!(s.first_time_chatters, 1, "alice");
        assert_eq!(s.returning_chatters, 1, "carol");
        assert_eq!(s.title, "Test Titel");
        assert_eq!(s.followers_start, 1000);
        assert_eq!(s.followers_end, 1010);
    }

    #[tokio::test]
    async fn category_rank_perzentil_und_rang() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_overview_category").await;
        sqlx::query(
            r#"
            INSERT INTO twitch_stats_category (ts_utc, streamer, viewer_count) VALUES
                ('2026-06-14T08:00:00+00:00', 'streamer_x', 50),
                ('2026-06-14T08:00:00+00:00', 'a', 10),
                ('2026-06-14T08:00:00+00:00', 'b', 30),
                ('2026-06-14T08:00:00+00:00', 'c', 90)
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let since = "2000-01-01T00:00:00+00:00";
        // sorted_avgs=[10,30,50,90]; streamer_x=50: below=2, equal=1 → (2+0.5)/4=0.625.
        // rank = 4 - int(0.625*4) = 4 - 2 = 2.
        let c = overview_category_rank(&pool, since, Some("streamer_x"))
            .await
            .unwrap()
            .unwrap();
        assert!((c.percentile - 0.625).abs() < 1e-9);
        assert_eq!(c.rank, 2);
        assert_eq!(c.total, 4);
        // Unbekannter Streamer / kein Streamer → None.
        assert!(overview_category_rank(&pool, since, Some("nobody"))
            .await
            .unwrap()
            .is_none());
        assert!(overview_category_rank(&pool, since, None)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn category_rank_propagiert_query_fehler() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let schema = "test_overview_category_query_fail";
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .expect("connect test-db");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&pool)
            .await
            .expect("Schema droppen fehlgeschlagen");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .expect("Schema anlegen fehlgeschlagen");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path setzen fehlgeschlagen");

        let err = overview_category_rank(&pool, "2000-01-01T00:00:00+00:00", Some("streamer_x"))
            .await
            .expect_err("fehlende twitch_stats_category muss sichtbar fehlschlagen");
        assert!(matches!(err, sqlx::Error::Database(_)));
    }
}
