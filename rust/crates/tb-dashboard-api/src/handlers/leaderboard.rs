//! Web-Leaderboard (B13-2) — Ersatz für den gedroppten Discord-`!twl`.
//!
//! `GET /twitch/api/v2/leaderboard` liefert die rollierende 30-Tage-Rangliste der
//! Streamer nach durchschnittlichen Zuschauern, getrennt in zwei Kategorien:
//! - `tracked`  — aktive Partner (`twitch_streamers_partner_state.is_partner_active = 1`)
//! - `category` — alle übrigen Streamer
//!
//! Port der Kern-Rangliste aus `bot/community/leaderboard.py` (`_compute_stats`,
//! `top_sql`, Zeilen 641-905). Quelle sind die Live-Snapshot-Tabellen
//! `twitch_stats_tracked` ∪ `twitch_stats_category` (ein Sample pro Cron-Tick).
//! Pro Eintrag: avg/max-Viewers, Sample-Zahl, Partner-/Discord-Flag (aus der
//! Partner-State-View angereichert).
//!
//! **Scope (Grillme: BUILD, Low-Prio):** der ranglistenbildende Kern. Die
//! analytischen Zusatzblöcke des Discord-Embeds (Retention/Chat/Discovery/
//! Content-Performance/Hourly/Weekday) sind im Dashboard bereits durch dedizierte
//! Endpoints abgedeckt (`/retention-curve`, `/chat-analytics`, …) und hier nicht
//! dupliziert.
//!
//! Auth: eingeloggt (Partner/Admin/Localhost), wie die übrigen `/api/v2`-Reads.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::auth::level::DashboardAuthLevel;

/// Default-Limit (Python `limit=5`), geklemmt auf 1..=20.
const DEFAULT_LIMIT: i64 = 5;
const MAX_LIMIT: i64 = 20;

#[derive(Debug, Deserialize, Default)]
pub struct LeaderboardQuery {
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub order: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub min_samples: Option<i64>,
    #[serde(default)]
    pub min_avg: Option<f64>,
}

/// Eine Rangliste-Zeile aus der Snapshot-Aggregation.
#[derive(Debug, sqlx::FromRow)]
struct TopRow {
    streamer: String,
    avg_viewers: Option<f64>,
    max_viewers: Option<i64>,
    samples: Option<i64>,
    is_partner: Option<i64>,
    is_on_discord: Option<i64>,
    discord_user_id: Option<String>,
    discord_display_name: Option<String>,
}

/// `GET /twitch/api/v2/leaderboard`.
pub async fn leaderboard_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<LeaderboardQuery>,
) -> Response {
    if !auth.is_authenticated() {
        return crate::auth::unauthorized_v2_response();
    }

    let sort_key = normalize_sort(params.sort.as_deref());
    let descending = !matches!(params.order.as_deref().map(str::to_lowercase).as_deref(), Some("asc"));
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let min_samples = params.min_samples.filter(|&v| v > 0);
    let min_avg = params.min_avg.filter(|&v| v > 0.0);

    let tracked = match load_category(&pool, true).await {
        Ok(rows) => rows,
        Err(error) => {
            tracing::error!(%error, "leaderboard tracked query failed");
            return analytics_error();
        }
    };
    let category = match load_category(&pool, false).await {
        Ok(rows) => rows,
        Err(error) => {
            tracing::error!(%error, "leaderboard category query failed");
            return analytics_error();
        }
    };

    let tracked_entries = shape_entries(tracked, sort_key, descending, min_samples, min_avg, limit);
    let category_entries = shape_entries(category, sort_key, descending, min_samples, min_avg, limit);

    Json(json!({
        "window": { "days": 30 },
        "options": {
            "sort_key": sort_key.as_str(),
            "sort_order": if descending { "desc" } else { "asc" },
            "limit": limit,
            "min_samples": min_samples,
            "min_avg": min_avg,
        },
        "categories": [
            {
                "key": "tracked",
                "title": "Top Tracked",
                "count": tracked_entries.len(),
                "entries": tracked_entries,
            },
            {
                "key": "category",
                "title": "Top Kategorie",
                "count": category_entries.len(),
                "entries": category_entries,
            },
        ],
    }))
    .into_response()
}

/// Sortierschlüssel (Python `sort` ∈ {avg, samples, peak, name}).
#[derive(Clone, Copy, PartialEq, Eq)]
enum SortKey {
    Avg,
    Samples,
    Peak,
    Name,
}

impl SortKey {
    fn as_str(self) -> &'static str {
        match self {
            Self::Avg => "avg",
            Self::Samples => "samples",
            Self::Peak => "peak",
            Self::Name => "name",
        }
    }
}

fn normalize_sort(raw: Option<&str>) -> SortKey {
    match raw.map(str::trim).map(str::to_lowercase).as_deref() {
        Some("samples") => SortKey::Samples,
        Some("peak") => SortKey::Peak,
        Some("name") => SortKey::Name,
        _ => SortKey::Avg,
    }
}

/// Lädt die aggregierte 30-Tage-Rangliste für `tracked` (Partner) oder `category`.
///
/// `ts_utc` ist Text → explizit auf `timestamptz` gecastet (Python verlässt sich
/// auf den impliziten Cast; explizit ist robuster gegen abweichende Spaltentypen).
/// Discord-/Partner-Felder kommen per LEFT JOIN aus der Partner-State-View.
async fn load_category(pool: &PgPool, tracked: bool) -> Result<Vec<TopRow>, sqlx::Error> {
    // Partner-Zugehörigkeit (IN bei tracked, NOT IN bei category).
    let membership = if tracked {
        "LOWER(s.streamer) IN (SELECT LOWER(twitch_login) FROM twitch_streamers_partner_state WHERE is_partner_active = 1)"
    } else {
        "LOWER(s.streamer) NOT IN (SELECT LOWER(twitch_login) FROM twitch_streamers_partner_state WHERE is_partner_active = 1)"
    };

    let sql = format!(
        r#"
        WITH source_rows AS (
            SELECT streamer, viewer_count, is_partner, ts_utc FROM twitch_stats_tracked
            UNION ALL
            SELECT streamer, viewer_count, is_partner, ts_utc FROM twitch_stats_category
        ),
        agg AS (
            SELECT s.streamer AS streamer,
                   AVG(s.viewer_count)::float8 AS avg_viewers,
                   MAX(s.viewer_count)::int8   AS max_viewers,
                   COUNT(*)::int8              AS samples,
                   MAX(CASE WHEN s.is_partner <> 0 THEN 1 ELSE 0 END)::int8 AS is_partner
              FROM source_rows s
             WHERE s.ts_utc::timestamptz >= NOW() - INTERVAL '30 days'
               AND {membership}
             GROUP BY s.streamer
        )
        SELECT a.streamer,
               a.avg_viewers,
               a.max_viewers,
               a.samples,
               GREATEST(a.is_partner, CASE WHEN ps.is_partner_active = 1 THEN 1 ELSE 0 END)::int8 AS is_partner,
               CASE WHEN COALESCE(ps.is_on_discord, 0) <> 0 OR NULLIF(TRIM(COALESCE(ps.discord_user_id, '')), '') IS NOT NULL
                    THEN 1 ELSE 0 END::int8 AS is_on_discord,
               NULLIF(TRIM(COALESCE(ps.discord_user_id, '')), '')      AS discord_user_id,
               NULLIF(TRIM(COALESCE(ps.discord_display_name, '')), '') AS discord_display_name
          FROM agg a
          LEFT JOIN twitch_streamers_partner_state ps
                 ON LOWER(ps.twitch_login) = LOWER(a.streamer)
        "#,
    );

    sqlx::query_as::<_, TopRow>(&sql).fetch_all(pool).await
}

/// Filtert (min_samples/min_avg), sortiert und kürzt auf `limit` Einträge,
/// vergibt Ränge und serialisiert (Python `_finalize_top` analog).
fn shape_entries(
    mut rows: Vec<TopRow>,
    sort_key: SortKey,
    descending: bool,
    min_samples: Option<i64>,
    min_avg: Option<f64>,
    limit: i64,
) -> Vec<Value> {
    rows.retain(|r| {
        let samples_ok = min_samples.is_none_or(|m| r.samples.unwrap_or(0) >= m);
        let avg_ok = min_avg.is_none_or(|m| r.avg_viewers.unwrap_or(0.0) >= m);
        samples_ok && avg_ok
    });

    // Einheitlicher Vergleich in natürlicher (aufsteigender) Richtung; `descending`
    // dreht danach um — wie Pythons `sorted(items, key=_key_func, reverse=descending)`.
    rows.sort_by(|a, b| {
        let ord = match sort_key {
            SortKey::Avg => a
                .avg_viewers
                .unwrap_or(0.0)
                .partial_cmp(&b.avg_viewers.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal),
            SortKey::Samples => a.samples.unwrap_or(0).cmp(&b.samples.unwrap_or(0)),
            SortKey::Peak => a.max_viewers.unwrap_or(0).cmp(&b.max_viewers.unwrap_or(0)),
            SortKey::Name => a.streamer.to_lowercase().cmp(&b.streamer.to_lowercase()),
        };
        if descending { ord.reverse() } else { ord }
    });

    rows.into_iter()
        .take(limit as usize)
        .enumerate()
        .map(|(idx, r)| {
            json!({
                "rank": idx + 1,
                "streamer": r.streamer,
                "avg_viewers": r.avg_viewers.unwrap_or(0.0),
                "max_viewers": r.max_viewers.unwrap_or(0),
                "samples": r.samples.unwrap_or(0),
                "is_partner": r.is_partner.unwrap_or(0),
                "is_on_discord": r.is_on_discord.unwrap_or(0),
                "has_discord_profile": i64::from(r.discord_user_id.is_some()),
                "discord_user_id": r.discord_user_id,
                "discord_display_name": r.discord_display_name,
            })
        })
        .collect()
}

fn analytics_error() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": "analytics_request_failed" })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(streamer: &str, avg: f64, peak: i64, samples: i64) -> TopRow {
        TopRow {
            streamer: streamer.into(),
            avg_viewers: Some(avg),
            max_viewers: Some(peak),
            samples: Some(samples),
            is_partner: Some(0),
            is_on_discord: Some(0),
            discord_user_id: None,
            discord_display_name: None,
        }
    }

    #[test]
    fn normalize_sort_default_avg() {
        assert_eq!(normalize_sort(None).as_str(), "avg");
        assert_eq!(normalize_sort(Some("PEAK")).as_str(), "peak");
        assert_eq!(normalize_sort(Some("samples")).as_str(), "samples");
        assert_eq!(normalize_sort(Some("name")).as_str(), "name");
        assert_eq!(normalize_sort(Some("müll")).as_str(), "avg");
    }

    #[test]
    fn shape_sortiert_filtert_und_raengt() {
        let rows = vec![
            row("a", 100.0, 200, 50),
            row("b", 300.0, 400, 10),
            row("c", 50.0, 90, 5),
        ];
        // Sort avg desc → b, a, c. Rang 1..3.
        let out = shape_entries(rows, SortKey::Avg, true, None, None, 5);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0]["streamer"], "b");
        assert_eq!(out[0]["rank"], 1);
        assert_eq!(out[2]["streamer"], "c");
    }

    #[test]
    fn shape_min_samples_und_limit() {
        let rows = vec![
            row("a", 100.0, 200, 50),
            row("b", 300.0, 400, 4),  // unter min_samples
            row("c", 80.0, 90, 20),
        ];
        let out = shape_entries(rows, SortKey::Avg, true, Some(10), None, 1);
        // b rausgefiltert (4 < 10), limit 1 → nur Top-Eintrag (a, avg 100 > c 80).
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["streamer"], "a");
    }

    #[test]
    fn shape_sort_name_und_peak() {
        let rows = vec![row("Zeta", 10.0, 5, 1), row("alpha", 10.0, 99, 1)];
        // name desc → alpha (lowercase 'a') vs 'z' → desc bedeutet z zuerst.
        let by_name = shape_entries(
            vec![row("Zeta", 10.0, 5, 1), row("alpha", 10.0, 99, 1)],
            SortKey::Name,
            true,
            None,
            None,
            5,
        );
        assert_eq!(by_name[0]["streamer"], "Zeta");
        // peak desc → alpha(99) vor Zeta(5).
        let by_peak = shape_entries(rows, SortKey::Peak, true, None, None, 5);
        assert_eq!(by_peak[0]["streamer"], "alpha");
    }

    // ── DB-Logik (env-gated über TB_TEST_DATABASE_URL) ──────────────────────
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        for ddl in [
            "CREATE TABLE twitch_stats_tracked (ts_utc TEXT, streamer TEXT, viewer_count INTEGER, is_partner INTEGER DEFAULT 0)",
            "CREATE TABLE twitch_stats_category (ts_utc TEXT, streamer TEXT, viewer_count INTEGER, is_partner INTEGER DEFAULT 0)",
            // Vereinfachte Partner-State-Tabelle (Ersatz der View für den Test).
            "CREATE TABLE twitch_streamers_partner_state (twitch_login TEXT, is_partner_active INTEGER, \
                 is_on_discord INTEGER, discord_user_id TEXT, discord_display_name TEXT)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn load_category_trennt_partner_und_rest() {
        let Some(pool) = make_pool("t_lb").await else { return };
        let now = chrono::Utc::now().to_rfc3339();
        // Partner "nani" (tracked), Nicht-Partner "rando" (category).
        sqlx::query("INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active, is_on_discord, discord_user_id, discord_display_name) VALUES ('nani', 1, 1, '123', 'NaniDC')")
            .execute(&pool).await.unwrap();
        for (tbl, streamer, vc) in [
            ("twitch_stats_tracked", "nani", 100),
            ("twitch_stats_tracked", "nani", 200),
            ("twitch_stats_category", "rando", 40),
        ] {
            sqlx::query(&format!("INSERT INTO {tbl} (ts_utc, streamer, viewer_count, is_partner) VALUES ($1, $2, $3, 0)"))
                .bind(&now).bind(streamer).bind(vc).execute(&pool).await.unwrap();
        }
        // Alte Zeile (>30 Tage) wird ausgefenstert.
        let old = (chrono::Utc::now() - chrono::Duration::days(40)).to_rfc3339();
        sqlx::query("INSERT INTO twitch_stats_tracked (ts_utc, streamer, viewer_count, is_partner) VALUES ($1, 'nani', 9999, 0)")
            .bind(&old).execute(&pool).await.unwrap();

        let tracked = load_category(&pool, true).await.unwrap();
        assert_eq!(tracked.len(), 1);
        assert_eq!(tracked[0].streamer, "nani");
        // avg = (100+200)/2 = 150 (die 9999-Zeile ist außerhalb des Fensters).
        assert_eq!(tracked[0].avg_viewers.unwrap().round() as i64, 150);
        assert_eq!(tracked[0].max_viewers, Some(200));
        assert_eq!(tracked[0].is_partner, Some(1));
        assert_eq!(tracked[0].is_on_discord, Some(1));
        assert_eq!(tracked[0].discord_user_id.as_deref(), Some("123"));

        let category = load_category(&pool, false).await.unwrap();
        assert_eq!(category.len(), 1);
        assert_eq!(category[0].streamer, "rando");
        assert_eq!(category[0].is_partner, Some(0));
    }
}
