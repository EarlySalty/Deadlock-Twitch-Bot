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
    extract::{Extension, Query, State},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};

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
    twitch_user_id: Option<String>,
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
    let descending = !matches!(
        params.order.as_deref().map(str::to_lowercase).as_deref(),
        Some("asc")
    );
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

    // P2.124: Discord-IDs/-Namen nur für privilegierte Aufrufer (Localhost/Admin)
    // serialisieren — nicht für eingeloggte Partner (Python: localhost-only gate).
    let show_discord = auth.is_privileged();
    let own_login = match &auth {
        DashboardAuthLevel::Partner { twitch_user_id, .. } => Some(twitch_user_id.as_str()),
        DashboardAuthLevel::Admin {
            actor: Some(actor), ..
        } => Some(actor.twitch_user_id.as_str()),
        DashboardAuthLevel::Admin { actor: None } | DashboardAuthLevel::None => None,
    };
    let shape_options = ShapeOptions {
        sort_key,
        descending,
        min_samples,
        min_avg,
        limit,
        show_discord,
    };
    let (tracked_entries, tracked_own_position) =
        shape_entries_with_own(tracked, shape_options, own_login);
    let category_entries = shape_entries(
        category,
        sort_key,
        descending,
        min_samples,
        min_avg,
        limit,
        show_discord,
    );

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
                "own_position": tracked_own_position,
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

/// `GET /twitch/api/v2/leaderboard/effort`.
///
/// Monatswertung aus der append-only Effort-Engine. Nicht eingeloggte Requests
/// erhalten bewusst 401. Passive Partner werden weiterhin vom zentralen
/// Partner-Status-Gate vor dem Handler abgewiesen.
pub async fn effort_leaderboard_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Extension(challenge_engine): Extension<super::challenges::ChallengeEngine>,
) -> Response {
    if !auth.is_authenticated() {
        return crate::auth::unauthorized_v2_response();
    }

    let own_id = match &auth {
        DashboardAuthLevel::Partner { twitch_user_id, .. } => twitch_user_id,
        DashboardAuthLevel::Admin {
            actor: Some(actor), ..
        } => &actor.twitch_user_id,
        _ => return crate::auth::unauthorized_v2_response(),
    };
    let Some(engine) = challenge_engine.0 else {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"not_current"})),
        )
            .into_response();
    };
    if engine
        .ensure_display_ready(chrono::Utc::now())
        .await
        .is_err()
    {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"not_current"})),
        )
            .into_response();
    }

    let category_data_complete = match engine.category_data_complete(chrono::Utc::now()).await {
        Ok(complete) => complete,
        Err(error) => {
            tracing::warn!(%error, "effort leaderboard coverage query failed");
            return analytics_error();
        }
    };

    let sql = r#"
        WITH bounds AS (
            SELECT
                date_trunc('month', timezone('Europe/Berlin', now()))
                    AT TIME ZONE 'Europe/Berlin' AS start_at,
                (date_trunc('month', timezone('Europe/Berlin', now())) + interval '1 month')
                    AT TIME ZONE 'Europe/Berlin' AS end_at,
                to_char(timezone('Europe/Berlin', now()), 'YYYY-MM') AS month_key
        ),
        active AS (
            SELECT twitch_user_id, lower(twitch_login) AS twitch_login
            FROM twitch_partners
            WHERE status='active'
              AND departnered_at IS NULL
              AND admin_archived_at IS NULL
              AND COALESCE(manual_partner_opt_out,0)=0
              AND COALESCE(trim(technical_pause_reason),'')=''
        ),
        event_running AS (
            SELECT e.partner_twitch_user_id,e.event_type,e.credited_at,e.id,
                SUM(e.points) OVER(PARTITION BY e.partner_twitch_user_id ORDER BY e.credited_at,e.id ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)::bigint AS running_points,
                SUM(e.points) OVER(PARTITION BY e.partner_twitch_user_id)::bigint AS final_points
            FROM partner_effort_events e CROSS JOIN bounds b
            WHERE e.credited_at >= b.start_at AND e.credited_at < b.end_at
        ),
        monthly AS (
            SELECT partner_twitch_user_id,MAX(final_points)::bigint AS points,
                COUNT(*) FILTER(WHERE event_type='qualified_invite')::bigint AS qualified_invites,
                MIN(credited_at) FILTER(WHERE running_points=final_points) AS score_reached_at
            FROM event_running GROUP BY partner_twitch_user_id
        ),
        scores AS (
            SELECT a.twitch_user_id,a.twitch_login,COALESCE(m.points,0)::bigint AS points,
                COALESCE(m.qualified_invites,0)::bigint AS qualified_invites,
                CASE WHEN COALESCE(m.points,0)=0 THEN b.start_at ELSE m.score_reached_at END AS score_reached_at
            FROM active a CROSS JOIN bounds b LEFT JOIN monthly m ON m.partner_twitch_user_id=a.twitch_user_id
        ),
        ranked AS (
            SELECT
                twitch_user_id,
                twitch_login,
                points,
                ROW_NUMBER() OVER (
                    ORDER BY points DESC,
                             qualified_invites DESC,
                             score_reached_at ASC NULLS LAST,
                             twitch_user_id
                )::bigint AS rank
            FROM scores
        )
        SELECT r.twitch_user_id, r.twitch_login, r.points, r.rank, b.month_key,
               EXISTS(SELECT 1 FROM twitch_partner_raid_boost_grants g WHERE g.twitch_user_id=r.twitch_user_id AND g.streams_remaining>0 AND g.granted_at<=now() AND g.expires_at>now()) OR EXISTS(SELECT 1 FROM twitch_partner_raid_boost_streams u JOIN twitch_stream_sessions s ON s.id=u.session_id AND s.twitch_user_id=u.twitch_user_id JOIN twitch_live_state l ON l.twitch_user_id=u.twitch_user_id WHERE u.twitch_user_id=r.twitch_user_id AND u.stream_ended_at IS NULL AND s.ended_at IS NULL AND l.is_live=1 AND l.active_session_id=u.session_id) AS raid_boost
        FROM ranked r
        CROSS JOIN bounds b
        ORDER BY r.rank
    "#;

    let rows = match sqlx::query(sql).fetch_all(&pool).await {
        Ok(rows) => rows,
        Err(error) => {
            tracing::error!(%error, "effort leaderboard query failed");
            return analytics_error();
        }
    };

    let mut entries = Vec::new();
    let mut own_position = None;
    let mut month = String::new();

    for row in rows {
        let twitch_login: String = match row.try_get("twitch_login") {
            Ok(value) => value,
            Err(error) => {
                tracing::error!(%error, "effort leaderboard login decode failed");
                return analytics_error();
            }
        };
        let points: i64 = match row.try_get("points") {
            Ok(value) => value,
            Err(error) => {
                tracing::error!(%error, "effort leaderboard points decode failed");
                return analytics_error();
            }
        };
        let rank: i64 = match row.try_get("rank") {
            Ok(value) => value,
            Err(error) => {
                tracing::error!(%error, "effort leaderboard rank decode failed");
                return analytics_error();
            }
        };
        if month.is_empty() {
            month = row.try_get("month_key").unwrap_or_default();
        }

        let is_self = row
            .try_get::<String, _>("twitch_user_id")
            .is_ok_and(|id| id == *own_id);
        let value = json!({
            "rank": rank,
            "twitch_login": twitch_login,
            "points": points,
            "raid_boost": row.try_get::<bool, _>("raid_boost").unwrap_or(false),
            "is_self": is_self,
        });

        if is_self {
            own_position = Some(value.clone());
        }
        if rank <= 10 {
            entries.push(value);
        }
    }

    Json(json!({
        "month": month,
        "category_data_complete": category_data_complete,
        "entries": entries,
        "own_position": own_position,
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

#[derive(Clone, Copy)]
struct ShapeOptions {
    sort_key: SortKey,
    descending: bool,
    min_samples: Option<i64>,
    min_avg: Option<f64>,
    limit: i64,
    show_discord: bool,
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
                   MAX(CASE WHEN COALESCE(s.is_partner, FALSE) THEN 1 ELSE 0 END)::int8 AS is_partner
              FROM source_rows s
             WHERE s.ts_utc::timestamptz >= NOW() - INTERVAL '30 days'
               AND {membership}
             GROUP BY s.streamer
        )
        SELECT a.streamer, ps.twitch_user_id,
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

    sqlx::query_as::<_, TopRow>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
}

/// Filtert, sortiert und kürzt auf das angeforderte Limit.
fn shape_entries(
    rows: Vec<TopRow>,
    sort_key: SortKey,
    descending: bool,
    min_samples: Option<i64>,
    min_avg: Option<f64>,
    limit: i64,
    show_discord: bool,
) -> Vec<Value> {
    shape_entries_with_own(
        rows,
        ShapeOptions {
            sort_key,
            descending,
            min_samples,
            min_avg,
            limit,
            show_discord,
        },
        None,
    )
    .0
}

fn shape_entries_with_own(
    mut rows: Vec<TopRow>,
    options: ShapeOptions,
    own_login: Option<&str>,
) -> (Vec<Value>, Option<Value>) {
    let ShapeOptions {
        sort_key,
        descending,
        min_samples,
        min_avg,
        limit,
        show_discord,
    } = options;
    rows.retain(|row| {
        let samples_ok = min_samples.is_none_or(|minimum| row.samples.unwrap_or(0) >= minimum);
        let avg_ok = min_avg.is_none_or(|minimum| row.avg_viewers.unwrap_or(0.0) >= minimum);
        samples_ok && avg_ok
    });

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
        if descending {
            ord.reverse()
        } else {
            ord
        }
    });

    let own_position = own_login.and_then(|login| {
        rows.iter()
            .position(|row| row.twitch_user_id.as_deref() == Some(login))
            .map(|index| row_json(&rows[index], index + 1, show_discord))
    });

    let entries = rows
        .iter()
        .take(limit as usize)
        .enumerate()
        .map(|(index, row)| row_json(row, index + 1, show_discord))
        .collect();

    (entries, own_position)
}

fn row_json(row: &TopRow, rank: usize, show_discord: bool) -> Value {
    let has_discord_profile = row.discord_user_id.is_some();
    let (discord_user_id, discord_display_name) = if show_discord {
        (
            row.discord_user_id.as_deref(),
            row.discord_display_name.as_deref(),
        )
    } else {
        (None, None)
    };

    json!({
        "rank": rank,
        "streamer": row.streamer,
        "avg_viewers": row.avg_viewers.unwrap_or(0.0),
        "max_viewers": row.max_viewers.unwrap_or(0),
        "samples": row.samples.unwrap_or(0),
        "is_partner": row.is_partner.unwrap_or(0),
        "is_on_discord": row.is_on_discord.unwrap_or(0),
        "has_discord_profile": i64::from(has_discord_profile),
        "discord_user_id": discord_user_id,
        "discord_display_name": discord_display_name,
    })
}

fn analytics_error() -> Response {
    crate::auth::analytics_request_failed_json().into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(streamer: &str, avg: f64, peak: i64, samples: i64) -> TopRow {
        TopRow {
            twitch_user_id: Some(streamer.into()),
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
        let out = shape_entries(rows, SortKey::Avg, true, None, None, 5, true);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0]["streamer"], "b");
        assert_eq!(out[0]["rank"], 1);
        assert_eq!(out[2]["streamer"], "c");
    }

    #[test]
    fn shape_min_samples_und_limit() {
        let rows = vec![
            row("a", 100.0, 200, 50),
            row("b", 300.0, 400, 4), // unter min_samples
            row("c", 80.0, 90, 20),
        ];
        let out = shape_entries(rows, SortKey::Avg, true, Some(10), None, 1, true);
        // b rausgefiltert (4 < 10), limit 1 → nur Top-Eintrag (a, avg 100 > c 80).
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["streamer"], "a");
    }

    #[test]
    fn eigene_position_bleibt_ausserhalb_des_limits_verfuegbar() {
        let rows = (1..=12)
            .map(|rank| {
                let login = if rank == 12 {
                    "mein_kanal".to_string()
                } else {
                    format!("kanal_{rank}")
                };
                let mut entry = row(&login, f64::from(13 - rank) * 10.0, 100, 20);
                entry.twitch_user_id = Some(rank.to_string());
                entry
            })
            .collect();

        let (entries, own_position) = shape_entries_with_own(
            rows,
            ShapeOptions {
                sort_key: SortKey::Avg,
                descending: true,
                min_samples: None,
                min_avg: None,
                limit: 10,
                show_discord: false,
            },
            Some("12"),
        );

        assert_eq!(entries.len(), 10);
        let own_position = own_position.expect("eigene Position fehlt");
        assert_eq!(own_position["rank"], 12);
        assert_eq!(own_position["streamer"], "mein_kanal");
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
            true,
        );
        assert_eq!(by_name[0]["streamer"], "Zeta");
        // peak desc → alpha(99) vor Zeta(5).
        let by_peak = shape_entries(rows, SortKey::Peak, true, None, None, 5, true);
        assert_eq!(by_peak[0]["streamer"], "alpha");
    }

    /// P2.124: Discord-Felder nur für privilegierte Aufrufer, `has_discord_profile`
    /// bleibt für alle sichtbar.
    #[test]
    fn shape_gatet_discord_felder() {
        let with_discord = TopRow {
            twitch_user_id: Some("111".into()),
            streamer: "nani".into(),
            avg_viewers: Some(100.0),
            max_viewers: Some(200),
            samples: Some(10),
            is_partner: Some(1),
            is_on_discord: Some(1),
            discord_user_id: Some("123456".into()),
            discord_display_name: Some("NaniDC".into()),
        };

        // Privilegiert → Discord-Felder sichtbar.
        let privileged = shape_entries(
            vec![TopRow {
                ..clone_row(&with_discord)
            }],
            SortKey::Avg,
            true,
            None,
            None,
            5,
            true,
        );
        assert_eq!(privileged[0]["discord_user_id"], "123456");
        assert_eq!(privileged[0]["discord_display_name"], "NaniDC");
        assert_eq!(privileged[0]["has_discord_profile"], 1);

        // Nicht privilegiert (Partner) → Discord-IDs maskiert, Flag bleibt.
        let partner = shape_entries(vec![with_discord], SortKey::Avg, true, None, None, 5, false);
        assert!(partner[0]["discord_user_id"].is_null());
        assert!(partner[0]["discord_display_name"].is_null());
        assert_eq!(
            partner[0]["has_discord_profile"], 1,
            "has_discord_profile bleibt sichtbar (kein ID-Leak)"
        );
    }

    fn clone_row(r: &TopRow) -> TopRow {
        TopRow {
            twitch_user_id: r.twitch_user_id.clone(),
            streamer: r.streamer.clone(),
            avg_viewers: r.avg_viewers,
            max_viewers: r.max_viewers,
            samples: r.samples,
            is_partner: r.is_partner,
            is_on_discord: r.is_on_discord,
            discord_user_id: r.discord_user_id.clone(),
            discord_display_name: r.discord_display_name.clone(),
        }
    }

    // ── DB-Logik (env-gated über TB_TEST_DATABASE_URL) ──────────────────────
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE"
        )))
        .execute(&admin)
        .await
        .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE twitch_stats_tracked (ts_utc TEXT, streamer TEXT, viewer_count INTEGER, is_partner BOOLEAN DEFAULT FALSE)",
            "CREATE TABLE twitch_stats_category (ts_utc TEXT, streamer TEXT, viewer_count INTEGER, is_partner BOOLEAN DEFAULT FALSE)",
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
        let Some(pool) = make_pool("t_lb").await else {
            return;
        };
        let now = chrono::Utc::now().to_rfc3339();
        // Partner "nani" (tracked), Nicht-Partner "rando" (category).
        sqlx::query("INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active, is_on_discord, discord_user_id, discord_display_name) VALUES ('nani', 1, 1, '123', 'NaniDC')")
            .execute(&pool).await.unwrap();
        for (tbl, streamer, vc) in [
            ("twitch_stats_tracked", "nani", 100),
            ("twitch_stats_tracked", "nani", 200),
            ("twitch_stats_category", "rando", 40),
        ] {
            sqlx::query(sqlx::AssertSqlSafe(format!("INSERT INTO {tbl} (ts_utc, streamer, viewer_count, is_partner) VALUES ($1, $2, $3, FALSE)")))
                .bind(&now).bind(streamer).bind(vc).execute(&pool).await.unwrap();
        }
        // Alte Zeile (>30 Tage) wird ausgefenstert.
        let old = (chrono::Utc::now() - chrono::Duration::days(40)).to_rfc3339();
        sqlx::query("INSERT INTO twitch_stats_tracked (ts_utc, streamer, viewer_count, is_partner) VALUES ($1, 'nani', 9999, FALSE)")
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
