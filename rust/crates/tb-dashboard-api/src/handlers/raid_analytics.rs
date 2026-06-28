//! Handler für `GET /twitch/api/v2/raid-retention` und `GET /twitch/api/v2/raid-analytics`.
//!
//! Port von bot/analytics/api_overview.py:_load_raid_retention (Z.1955)
//! und bot/analytics/api_raids.py:_api_v2_raid_analytics (Z.29).
//! Kern-Trick: recalculate_raid_chat_metrics übergibt JSON-Array an Postgres
//! via json_to_recordset — identisch portiert.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use std::collections::HashMap;

use crate::auth::level::DashboardAuthLevel;

const KNOWN_CHAT_BOTS: &[&str] = &[
    "botrix",
    "deutschedeadlockcommunity",
    "fossabot",
    "moobot",
    "nightbot",
    "pretzelrocks",
    "soundalerts",
    "streamlabs",
    "streamelements",
    "wizebot",
];

// ─────────────────────────────────────────────────────────────────────────────
// recalculate_raid_chat_metrics (Parität zu raid_metrics.py)
//
// Gibt HashMap<(raid_id, executed_at_iso), metrics> zurück.
// Die drei SQL-Queries nutzen json_to_recordset wie in Python.
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
struct RaidMetric {
    plus5m: i64,
    plus15m: i64,
    plus30m: i64,
    known_from_raider: i64,
    new_chatters: i64,
}

async fn recalculate_raid_chat_metrics(
    pool: &PgPool,
    raids: &[Value], // Array aus {raid_id, executed_at_key, target_session_id, executed_at, from_login, to_login}
) -> HashMap<(i64, String), RaidMetric> {
    let mut metrics: HashMap<(i64, String), RaidMetric> = raids
        .iter()
        .filter_map(|r| {
            let id = r["raid_id"].as_i64()?;
            let key = r["executed_at_key"].as_str().unwrap_or("").to_string();
            Some(((id, key), RaidMetric::default()))
        })
        .collect();

    if metrics.is_empty() {
        return metrics;
    }

    let payload = serde_json::to_string(raids).unwrap_or_else(|_| "[]".into());
    let bots: Vec<String> = KNOWN_CHAT_BOTS.iter().map(|s| s.to_string()).collect();

    // Query 1: Retention (plus5m/15m/30m)
    let bot_not_in_sc: Vec<String> = (3..=bots.len() + 2).map(|i| format!("${i}")).collect();
    let ret_sql = String::from(
        r#"WITH raid_inputs AS (
               SELECT CAST(r.raid_id AS BIGINT) AS raid_id,
                      COALESCE(r.executed_at_key, '') AS executed_at_key,
                      CAST(r.target_session_id AS BIGINT) AS target_session_id,
                      CAST(r.executed_at AS TIMESTAMPTZ) AS executed_at
               FROM json_to_recordset($1::json) AS r(
                   raid_id TEXT, executed_at_key TEXT,
                   target_session_id TEXT, executed_at TEXT,
                   from_login TEXT, to_login TEXT
               )
           )
           SELECT ri.raid_id, ri.executed_at_key,
                  COUNT(DISTINCT CASE WHEN sc.last_seen_at <= ri.executed_at + INTERVAL '5 minutes'
                      THEN COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id) ELSE NULL END) AS plus5m,
                  COUNT(DISTINCT CASE WHEN sc.last_seen_at <= ri.executed_at + INTERVAL '15 minutes'
                      THEN COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id) ELSE NULL END) AS plus15m,
                  COUNT(DISTINCT COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id)) AS plus30m
           FROM raid_inputs ri
           LEFT JOIN twitch_session_chatters sc
               ON sc.session_id = ri.target_session_id
              AND ri.executed_at IS NOT NULL
              AND sc.last_seen_at >= ri.executed_at
              AND sc.last_seen_at <= ri.executed_at + INTERVAL '30 minutes'
              AND (sc.chatter_login IS NULL OR sc.chatter_login = '' OR LOWER(sc.chatter_login) != ALL($2))
           GROUP BY ri.raid_id, ri.executed_at_key"#,
    );

    let q = sqlx::query(&ret_sql).bind(&payload).bind(&bots);
    // extra binds für die NOT IN — aber hier nutzen wir != ALL($2) statt dynamische Placeholders
    // Hinweis: = ALL(array) ist das sqlx-idiom, kein extra bind nötig
    let _ = &bot_not_in_sc; // unused — Array-Approach braucht keinen dynamischen Clause
    let ret_rows = q.fetch_all(pool).await.unwrap_or_default();
    for r in &ret_rows {
        let id: i64 = r.try_get("raid_id").unwrap_or(0);
        let key: String = r.try_get("executed_at_key").unwrap_or_default();
        if let Some(m) = metrics.get_mut(&(id, key)) {
            m.plus5m = r.try_get("plus5m").unwrap_or(0);
            m.plus15m = r.try_get("plus15m").unwrap_or(0);
            m.plus30m = r.try_get("plus30m").unwrap_or(0);
        }
    }

    // Query 2: known_from_raider
    let known_sql = r#"WITH raid_inputs AS (
               SELECT CAST(r.raid_id AS BIGINT) AS raid_id,
                      COALESCE(r.executed_at_key, '') AS executed_at_key,
                      CAST(r.target_session_id AS BIGINT) AS target_session_id,
                      CAST(r.executed_at AS TIMESTAMPTZ) AS executed_at,
                      LOWER(COALESCE(r.from_login, '')) AS from_login
               FROM json_to_recordset($1::json) AS r(
                   raid_id TEXT, executed_at_key TEXT,
                   target_session_id TEXT, executed_at TEXT,
                   from_login TEXT, to_login TEXT
               )
           )
           SELECT ri.raid_id, ri.executed_at_key,
                  COUNT(DISTINCT LOWER(sc.chatter_login)) AS known
           FROM raid_inputs ri
           JOIN twitch_session_chatters sc
               ON sc.session_id = ri.target_session_id
              AND ri.executed_at IS NOT NULL
              AND sc.last_seen_at >= ri.executed_at
              AND sc.chatter_login IS NOT NULL AND sc.chatter_login <> ''
              AND (sc.chatter_login IS NULL OR sc.chatter_login = '' OR LOWER(sc.chatter_login) != ALL($2))
           JOIN twitch_chatter_rollup cr
               ON LOWER(cr.chatter_login) = LOWER(sc.chatter_login)
              AND LOWER(cr.streamer_login) = ri.from_login
              AND cr.first_seen_at < ri.executed_at
              AND (cr.chatter_login IS NULL OR cr.chatter_login = '' OR LOWER(cr.chatter_login) != ALL($2))
           GROUP BY ri.raid_id, ri.executed_at_key"#;

    let known_rows = sqlx::query(known_sql)
        .bind(&payload)
        .bind(&bots)
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    for r in &known_rows {
        let id: i64 = r.try_get("raid_id").unwrap_or(0);
        let key: String = r.try_get("executed_at_key").unwrap_or_default();
        if let Some(m) = metrics.get_mut(&(id, key)) {
            m.known_from_raider = r.try_get("known").unwrap_or(0);
        }
    }

    // Query 3: new_chatters
    let new_sql = r#"WITH raid_inputs AS (
               SELECT CAST(r.raid_id AS BIGINT) AS raid_id,
                      COALESCE(r.executed_at_key, '') AS executed_at_key,
                      CAST(r.target_session_id AS BIGINT) AS target_session_id,
                      CAST(r.executed_at AS TIMESTAMPTZ) AS executed_at,
                      LOWER(COALESCE(r.to_login, '')) AS to_login
               FROM json_to_recordset($1::json) AS r(
                   raid_id TEXT, executed_at_key TEXT,
                   target_session_id TEXT, executed_at TEXT,
                   from_login TEXT, to_login TEXT
               )
           )
           SELECT ri.raid_id, ri.executed_at_key,
                  COUNT(DISTINCT COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id)) AS new_chatters
           FROM raid_inputs ri
           JOIN twitch_session_chatters sc
               ON sc.session_id = ri.target_session_id
              AND ri.executed_at IS NOT NULL
              AND sc.first_message_at >= ri.executed_at
              AND sc.messages > 0
              AND (sc.chatter_login IS NULL OR sc.chatter_login = '' OR LOWER(sc.chatter_login) != ALL($2))
           LEFT JOIN twitch_chatter_rollup cr
               ON LOWER(cr.chatter_login) = LOWER(sc.chatter_login)
              AND LOWER(cr.streamer_login) = ri.to_login
              AND cr.first_seen_at < ri.executed_at
              AND (cr.chatter_login IS NULL OR cr.chatter_login = '' OR LOWER(cr.chatter_login) != ALL($2))
           WHERE sc.chatter_login IS NULL OR sc.chatter_login = '' OR cr.chatter_login IS NULL
           GROUP BY ri.raid_id, ri.executed_at_key"#;

    let new_rows = sqlx::query(new_sql)
        .bind(&payload)
        .bind(&bots)
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    for r in &new_rows {
        let id: i64 = r.try_get("raid_id").unwrap_or(0);
        let key: String = r.try_get("executed_at_key").unwrap_or_default();
        if let Some(m) = metrics.get_mut(&(id, key)) {
            m.new_chatters = r.try_get("new_chatters").unwrap_or(0);
        }
    }

    metrics
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /twitch/api/v2/raid-retention
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct RaidQuery {
    streamer: Option<String>,
    #[serde(default)]
    days: Option<i32>,
}

pub async fn raid_retention_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<RaidQuery>,
) -> impl IntoResponse {
    // Python api_raids.py:32 / api_overview.py:1950: _require_v2_auth + _require_extended_plan.
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }
    // IDOR-Klemme: Partner nur eigener Login; Admin braucht streamer (required).
    let streamer =
        match crate::auth::resolve_streamer_scope(&auth, params.streamer.as_deref(), true) {
            Ok(Some(s)) => s,
            Ok(None) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"dataAvailable":false,"message":"Streamer required"})),
                )
                    .into_response()
            }
            Err(resp) => return resp,
        };
    let days = params.days.unwrap_or(90).clamp(7, 365);
    let since: DateTime<Utc> = Utc::now() - chrono::Duration::days(days as i64);

    let base_rows = sqlx::query(
        r#"SELECT raid_id, from_broadcaster_login, to_broadcaster_login,
                  viewer_count_sent::bigint AS viewer_count_sent, executed_at, target_session_id,
                  chatters_at_plus5m::bigint AS chatters_at_plus5m, chatters_at_plus15m::bigint AS chatters_at_plus15m, chatters_at_plus30m::bigint AS chatters_at_plus30m,
                  new_chatters::bigint AS new_chatters, known_from_raider::bigint AS known_from_raider
           FROM twitch_raid_retention
           WHERE executed_at >= $1 AND LOWER(from_broadcaster_login) = $2
           ORDER BY executed_at DESC LIMIT 100"#,
    )
    .bind(since).bind(&streamer)
    .fetch_all(&pool).await;

    let base_rows = match base_rows {
        Err(e) => {
            // P2.69: Python (api_overview.py:1973-1979) fängt jede Exception und
            // liefert bewusst 200 + Demo/dataAvailable:false, damit das Frontend
            // (das auf dataAvailable verzweigt) bei einem DB-Fehler nicht hart
            // bricht. Muster wie lurker_analysis.rs. Fehler wird geloggt, aber
            // nicht als 500 propagiert.
            tracing::error!("raid-retention DB-Fehler: {e}");
            return Json(json!({"dataAvailable":false,"message":"Keine Daten verfügbar"}))
                .into_response();
        }
        Ok(r) => r,
    };

    if base_rows.is_empty() {
        return Json(json!({"dataAvailable":false,"message":"Keine Raids im Zeitraum"}))
            .into_response();
    }

    // JSON-Input für recalculate bauen
    let mut base_raids: Vec<Value> = vec![];
    let mut raid_inputs: Vec<Value> = vec![];

    for row in &base_rows {
        let raid_id: i64 = match row.try_get("raid_id") {
            Ok(v) => v,
            Err(_) => continue,
        };
        let executed_at: Option<DateTime<Utc>> = row.try_get("executed_at").ok();
        let executed_at_iso = executed_at.map(|t| t.to_rfc3339()).unwrap_or_default();
        let target_session_id: Option<i64> = row.try_get("target_session_id").ok();
        let from_login: String = row
            .try_get::<Option<String>, _>("from_broadcaster_login")
            .ok()
            .flatten()
            .unwrap_or_default()
            .to_lowercase();
        let to_login: String = row
            .try_get::<Option<String>, _>("to_broadcaster_login")
            .ok()
            .flatten()
            .unwrap_or_default()
            .to_lowercase();

        base_raids.push(json!({
            "raid_id": raid_id,
            "from_login": from_login,
            "to_broadcaster": row.try_get::<Option<String>, _>("to_broadcaster_login").ok().flatten(),
            "viewers_sent": row.try_get::<Option<i64>, _>("viewer_count_sent").ok().flatten().unwrap_or(0),
            "executed_at": executed_at.map(|t| t.to_rfc3339()),
            "executed_at_key": &executed_at_iso,
            "stored_plus5m": row.try_get::<Option<i64>, _>("chatters_at_plus5m").ok().flatten().unwrap_or(0),
            "stored_plus15m": row.try_get::<Option<i64>, _>("chatters_at_plus15m").ok().flatten().unwrap_or(0),
            "stored_plus30m": row.try_get::<Option<i64>, _>("chatters_at_plus30m").ok().flatten().unwrap_or(0),
            "stored_new_chatters": row.try_get::<Option<i64>, _>("new_chatters").ok().flatten().unwrap_or(0),
            "stored_known_from_raider": row.try_get::<Option<i64>, _>("known_from_raider").ok().flatten().unwrap_or(0),
        }));

        if let Some(session_id) = target_session_id {
            raid_inputs.push(json!({
                "raid_id": raid_id.to_string(),
                "executed_at_key": executed_at_iso,
                "target_session_id": session_id.to_string(),
                "executed_at": executed_at.map(|t| t.to_rfc3339()),
                "from_login": from_login,
                "to_login": to_login,
            }));
        }
    }

    let raid_metrics = recalculate_raid_chat_metrics(&pool, &raid_inputs).await;

    let mut raids: Vec<Value> = vec![];
    let mut retention_values: Vec<f64> = vec![];
    let mut conversion_values: Vec<f64> = vec![];
    let mut total_new_chatters = 0i64;
    let mut recalculated = 0;
    let mut stored_fallback = 0;

    for raid in &base_raids {
        let raid_id = raid["raid_id"].as_i64().unwrap_or(0);
        let exec_key = raid["executed_at_key"].as_str().unwrap_or("").to_string();
        let viewers_sent = raid["viewers_sent"].as_i64().unwrap_or(0);

        let (c5m, c15m, c30m, known, new_ch, used_recalc) =
            if let Some(m) = raid_metrics.get(&(raid_id, exec_key)) {
                recalculated += 1;
                (
                    m.plus5m,
                    m.plus15m,
                    m.plus30m,
                    m.known_from_raider,
                    m.new_chatters,
                    true,
                )
            } else {
                stored_fallback += 1;
                (
                    raid["stored_plus5m"].as_i64().unwrap_or(0),
                    raid["stored_plus15m"].as_i64().unwrap_or(0),
                    raid["stored_plus30m"].as_i64().unwrap_or(0),
                    raid["stored_known_from_raider"].as_i64().unwrap_or(0),
                    raid["stored_new_chatters"].as_i64().unwrap_or(0),
                    false,
                )
            };
        let _ = used_recalc;

        let ret_pct = if viewers_sent > 0 {
            c30m as f64 / viewers_sent as f64 * 100.0
        } else {
            0.0
        };
        let conv_pct = if viewers_sent > 0 {
            new_ch as f64 / viewers_sent as f64 * 100.0
        } else {
            0.0
        };
        retention_values.push(ret_pct);
        conversion_values.push(conv_pct);
        total_new_chatters += new_ch;

        raids.push(json!({
            "raidId": raid_id,
            "toBroadcaster": raid["to_broadcaster"],
            "viewersSent": viewers_sent,
            "executedAt": raid["executed_at"],
            "chattersAt5m": c5m,
            "chattersAt15m": c15m,
            "chattersAt30m": c30m,
            "retention30mPct": (ret_pct * 10.0).round() / 10.0,
            "newChatters": new_ch,
            "chatterConversionPct": (conv_pct * 10.0).round() / 10.0,
            "knownFromRaider": known,
        }));
    }

    let avg = |vals: &[f64]| -> f64 {
        if vals.is_empty() {
            0.0
        } else {
            (vals.iter().sum::<f64>() / vals.len() as f64 * 10.0).round() / 10.0
        }
    };

    let metric_source = if stored_fallback == 0 {
        "recalculated"
    } else if recalculated == 0 {
        "stored"
    } else {
        "mixed"
    };

    Json(json!({
        "dataAvailable": true,
        "summary": {
            "avgRetentionPct": avg(&retention_values),
            "avgConversionPct": avg(&conversion_values),
            "totalNewChatters": total_new_chatters,
            "raidCount": raids.len(),
        },
        "raids": raids,
        "dataQuality": {
            "botFilterApplied": metric_source == "recalculated",
            "raidMetricSource": metric_source,
            "recalculatedRaidCount": recalculated,
            "storedFallbackRaidCount": stored_fallback,
        },
    }))
    .into_response()
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /twitch/api/v2/raid-analytics
// ─────────────────────────────────────────────────────────────────────────────

pub async fn raid_analytics_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<RaidQuery>,
) -> impl IntoResponse {
    // Python api_raids.py:32 / api_overview.py:1950: _require_v2_auth + _require_extended_plan.
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }
    // IDOR-Klemme: Partner nur eigener Login; Admin braucht streamer (required).
    let streamer =
        match crate::auth::resolve_streamer_scope(&auth, params.streamer.as_deref(), true) {
            Ok(Some(s)) => s,
            Ok(None) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error":"Streamer required"})),
                )
                    .into_response()
            }
            Err(resp) => return resp,
        };
    let days = params.days.unwrap_or(30).clamp(7, 365);
    let since: DateTime<Utc> = Utc::now() - chrono::Duration::days(days as i64);

    let bots: Vec<String> = KNOWN_CHAT_BOTS.iter().map(|s| s.to_string()).collect();

    // 1. Outgoing raids aus twitch_raid_retention (bereits berechnete Metriken)
    let retention_rows = sqlx::query(
        r#"SELECT rr.raid_id, rh.from_broadcaster_login, rr.viewer_count_sent::bigint AS viewer_count_sent,
                  rr.executed_at, rr.target_session_id, rr.to_broadcaster_login
           FROM twitch_raid_retention rr
           JOIN twitch_raid_history rh ON rh.id = rr.raid_id AND rh.executed_at = rr.executed_at
           JOIN twitch_stream_sessions ss ON ss.id = rr.target_session_id
           WHERE LOWER(ss.streamer_login) = $1 AND ss.started_at >= $2
           ORDER BY ss.started_at DESC"#,
    )
    .bind(&streamer).bind(since)
    .fetch_all(&pool).await;

    // P2.98: Python (api_raids.py:473-475) umschließt den gesamten Body mit
    // try/except und liefert bei jedem DB-Fehler 500 statt leeres 200. Die drei
    // Primär-Queries werden daher zu 500 gemappt (Sub-Queries bleiben weich).
    let retention_rows = match retention_rows {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("raid-analytics retention-Query-Fehler: {e}");
            return crate::auth::analytics_request_failed_json().into_response();
        }
    };

    let mut base_raids_full: Vec<Value> = vec![];
    let mut base_raids_sample: Vec<Value> = vec![];
    for row in &retention_rows {
        let raid_id: i64 = match row.try_get("raid_id") {
            Ok(v) => v,
            Err(_) => continue,
        };
        let from: String = row
            .try_get::<Option<String>, _>("from_broadcaster_login")
            .ok()
            .flatten()
            .unwrap_or_default();
        let to: String = row
            .try_get::<Option<String>, _>("to_broadcaster_login")
            .ok()
            .flatten()
            .unwrap_or_default()
            .to_lowercase();
        let sent: i64 = row
            .try_get::<Option<i64>, _>("viewer_count_sent")
            .ok()
            .flatten()
            .unwrap_or(0);
        let exec_at: Option<DateTime<Utc>> = row.try_get("executed_at").ok();
        let exec_iso = exec_at.map(|t| t.to_rfc3339()).unwrap_or_default();
        let target_sid: i64 = row.try_get("target_session_id").unwrap_or(0);

        let entry = json!({
            "raid_id": raid_id.to_string(),
            "executed_at_key": exec_iso,
            "executed_at": exec_at.map(|t| t.to_rfc3339()),
            "target_session_id": target_sid.to_string(),
            "from": from.clone(),
            "from_login": from.to_lowercase(),
            "to_login": to,
            "viewers_sent": sent,
        });
        if base_raids_sample.len() < 50 {
            base_raids_sample.push(entry.clone());
        }
        base_raids_full.push(entry);
    }

    let sample_keys: std::collections::HashSet<(i64, String)> = base_raids_sample
        .iter()
        .filter_map(|r| {
            Some((
                r["raid_id"].as_str()?.parse::<i64>().ok()?,
                r["executed_at_key"].as_str()?.to_string(),
            ))
        })
        .collect();

    // Metriken berechnen (Full + Sample in einem Aufruf)
    let all_metrics = recalculate_raid_chat_metrics(&pool, &base_raids_full).await;

    // 2. Per-Source aggregieren
    let mut grouped_source: HashMap<String, Value> = HashMap::new();
    let mut sample_metrics: HashMap<(i64, String), Value> = HashMap::new();

    for raid in &base_raids_full {
        let raid_id: i64 = raid["raid_id"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let exec_key = raid["executed_at_key"].as_str().unwrap_or("").to_string();
        let src_key = raid["from_login"].as_str().unwrap_or("unknown").to_string();
        let sent = raid["viewers_sent"].as_i64().unwrap_or(0);
        let m = all_metrics
            .get(&(raid_id, exec_key.clone()))
            .cloned()
            .unwrap_or_default();

        let bucket = grouped_source.entry(src_key.clone()).or_insert(json!({
            "from_channel": raid["from"].as_str().unwrap_or("unknown"),
            "raids_received": 0,
            "total_viewers_sent": 0.0,
            "total_new_chatters": 0.0,
            "retention_ratio_sum": 0.0,
            "retention_ratio_count": 0,
            "overlap_ratio_sum": 0.0,
            "overlap_ratio_count": 0,
        }));
        bucket["raids_received"] = json!(bucket["raids_received"].as_i64().unwrap_or(0) + 1);
        let tv = bucket["total_viewers_sent"].as_f64().unwrap_or(0.0) + sent as f64;
        bucket["total_viewers_sent"] = json!(tv);
        bucket["total_new_chatters"] =
            json!(bucket["total_new_chatters"].as_f64().unwrap_or(0.0) + m.new_chatters as f64);
        if sent > 0 {
            let rs = bucket["retention_ratio_sum"].as_f64().unwrap_or(0.0)
                + m.plus30m as f64 / sent as f64;
            bucket["retention_ratio_sum"] = json!(rs);
            bucket["retention_ratio_count"] =
                json!(bucket["retention_ratio_count"].as_i64().unwrap_or(0) + 1);
            let os = bucket["overlap_ratio_sum"].as_f64().unwrap_or(0.0)
                + m.known_from_raider as f64 / sent as f64;
            bucket["overlap_ratio_sum"] = json!(os);
            bucket["overlap_ratio_count"] =
                json!(bucket["overlap_ratio_count"].as_i64().unwrap_or(0) + 1);
        }

        if sample_keys.contains(&(raid_id, exec_key.clone())) {
            sample_metrics.insert((raid_id, exec_key), json!({
                "plus5m": m.plus5m, "plus15m": m.plus15m, "plus30m": m.plus30m, "new_chatters": m.new_chatters,
            }));
        }
    }

    // 3. Follow-Attribution
    let follow_rows = sqlx::query(
        r#"SELECT fe.follower_login,
                  CASE WHEN rh.executed_at IS NOT NULL
                            AND sc.first_message_at >= rh.executed_at
                            AND cr_before.chatter_login IS NULL
                       THEN 'raid' ELSE 'organic' END AS follow_source,
                  rh.from_broadcaster_login AS raid_source
           FROM twitch_follow_events fe
           JOIN twitch_stream_sessions ss
               ON LOWER(ss.streamer_login) = LOWER(fe.streamer_login)
              AND fe.followed_at BETWEEN ss.started_at AND COALESCE(ss.ended_at, NOW())
           LEFT JOIN twitch_session_chatters sc
               ON sc.session_id = ss.id AND LOWER(sc.chatter_login) = LOWER(fe.follower_login)
           LEFT JOIN twitch_raid_retention rr ON rr.target_session_id = ss.id
           LEFT JOIN twitch_raid_history rh ON rh.id = rr.raid_id AND rh.executed_at = rr.executed_at
           LEFT JOIN twitch_chatter_rollup cr_before
               ON LOWER(cr_before.chatter_login) = LOWER(fe.follower_login)
              AND LOWER(cr_before.streamer_login) = LOWER(fe.streamer_login)
              AND cr_before.first_seen_at < ss.started_at
           WHERE LOWER(fe.streamer_login) = $1
             AND fe.followed_at >= $2
             AND LOWER(fe.follower_login) != ALL($3)"#,
    )
    .bind(&streamer).bind(since).bind(&bots)
    .fetch_all(&pool).await;
    let follow_rows = match follow_rows {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("raid-analytics follow-Query-Fehler: {e}");
            return crate::auth::analytics_request_failed_json().into_response();
        }
    };

    let mut follows_by_source: HashMap<String, i64> = HashMap::new();
    let raid_follows = follow_rows
        .iter()
        .filter(|r| r.try_get::<String, _>("follow_source").ok().as_deref() == Some("raid"))
        .count() as i64;
    let organic_follows = follow_rows
        .iter()
        .filter(|r| r.try_get::<String, _>("follow_source").ok().as_deref() == Some("organic"))
        .count() as i64;
    let total_follows = follow_rows.len() as i64;
    for r in &follow_rows {
        if r.try_get::<String, _>("follow_source").ok().as_deref() == Some("raid") {
            let src: String = r
                .try_get::<Option<String>, _>("raid_source")
                .ok()
                .flatten()
                .unwrap_or_default()
                .to_lowercase();
            *follows_by_source.entry(src).or_default() += 1;
        }
    }
    let follow_attribution = if total_follows > 0 {
        Some(json!({
            "total_follows": total_follows,
            "raid_follows": raid_follows,
            "organic_follows": organic_follows,
            "raid_conversion_rate": (raid_follows as f64 / total_follows as f64 * 1000.0).round() / 1000.0,
        }))
    } else {
        None
    };

    // 4. Per-Source finalisieren
    let mut per_source: Vec<Value> = grouped_source.iter().map(|(src_key, b)| {
        let raids_rcvd = b["raids_received"].as_i64().unwrap_or(0);
        let total_sent = b["total_viewers_sent"].as_f64().unwrap_or(0.0);
        let ret_count = b["retention_ratio_count"].as_i64().unwrap_or(0);
        let ovl_count = b["overlap_ratio_count"].as_i64().unwrap_or(0);
        let avg_ret = if ret_count > 0 { Some((b["retention_ratio_sum"].as_f64().unwrap_or(0.0) / ret_count as f64 * 1000.0).round() / 1000.0) } else { None };
        let avg_ovl = if ovl_count > 0 { Some((b["overlap_ratio_sum"].as_f64().unwrap_or(0.0) / ovl_count as f64 * 1000.0).round() / 1000.0) } else { None };
        let follows = *follows_by_source.get(src_key).unwrap_or(&0);
        json!({
            "from_channel": b["from_channel"],
            "raids_received": raids_rcvd,
            "avg_viewers_sent": if raids_rcvd > 0 { (total_sent / raids_rcvd as f64 * 10.0).round() / 10.0 } else { 0.0 },
            "avg_new_chatters": if raids_rcvd > 0 { (b["total_new_chatters"].as_f64().unwrap_or(0.0) / raids_rcvd as f64 * 10.0).round() / 10.0 } else { 0.0 },
            "avg_retention_30m": avg_ret,
            "follows_attributed": follows,
            "conversion_rate": if total_sent > 0.0 { Some((follows as f64 / total_sent * 1000.0).round() / 1000.0) } else { None },
            "known_audience_overlap": avg_ovl,
        })
    }).collect();
    per_source.sort_by_key(|v| std::cmp::Reverse(v["raids_received"].as_i64().unwrap_or(0)));
    per_source.truncate(20);

    // 5. Retention-Curves (Sample)
    let retention_curves: Vec<Value> = base_raids_sample
        .iter()
        .map(|raid| {
            let rid: i64 = raid["raid_id"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let ekey = raid["executed_at_key"].as_str().unwrap_or("").to_string();
            let sent = raid["viewers_sent"].as_i64().unwrap_or(0);
            let m = sample_metrics
                .get(&(rid, ekey))
                .cloned()
                .unwrap_or_else(|| json!({"plus5m":0,"plus15m":0,"plus30m":0,"new_chatters":0}));
            let p5m = m["plus5m"].as_i64().unwrap_or(0);
            let p15m = m["plus15m"].as_i64().unwrap_or(0);
            let p30m = m["plus30m"].as_i64().unwrap_or(0);
            let r5 = if sent > 0 {
                (p5m as f64 / sent as f64 * 1000.0).round() / 1000.0
            } else {
                0.0
            };
            let r15 = if sent > 0 {
                (p15m as f64 / sent as f64 * 1000.0).round() / 1000.0
            } else {
                0.0
            };
            let r30 = if sent > 0 {
                (p30m as f64 / sent as f64 * 1000.0).round() / 1000.0
            } else {
                0.0
            };
            json!({
                "raid_id": rid,
                "from": raid["from"],
                "viewers_sent": sent,
                "new_chatters": m["new_chatters"].as_i64().unwrap_or(0),
                "retention_curve": {"plus5m": r5, "plus15m": r15, "plus30m": r30},
            })
        })
        .collect();

    // 6. Incoming raids (twitch_raid_arrival_tracking) — N+1 für Session-Lookup + Timeline
    let incoming_raw = sqlx::query(
        r#"SELECT detected_at, from_broadcaster_login, viewer_count::bigint AS viewer_count,
                  classification, confirmation_signals, unraid_seen
           FROM twitch_raid_arrival_tracking
           WHERE LOWER(to_broadcaster_login) = $1 AND detected_at >= $2
           ORDER BY detected_at DESC LIMIT 50"#,
    )
    .bind(&streamer)
    .bind(since)
    .fetch_all(&pool)
    .await;
    let incoming_raw = match incoming_raw {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("raid-analytics incoming-Query-Fehler: {e}");
            return crate::auth::analytics_request_failed_json().into_response();
        }
    };

    let mut incoming_raids: Vec<Value> = vec![];
    let mut boost_values: Vec<f64> = vec![];
    let mut retention_15m_values: Vec<f64> = vec![];

    for rr in &incoming_raw {
        let detected_at: Option<DateTime<Utc>> = rr.try_get("detected_at").ok();
        let from_channel: String = rr
            .try_get::<Option<String>, _>("from_broadcaster_login")
            .ok()
            .flatten()
            .unwrap_or_else(|| "unknown".into());
        let viewers_sent: i64 = rr
            .try_get::<Option<i64>, _>("viewer_count")
            .ok()
            .flatten()
            .unwrap_or(0);

        let mut impact = json!({
            "viewers_before": null, "viewers_peak_after": null, "boost_pct": null,
            "retention_5m_pct": null, "retention_15m_pct": null, "retention_30m_pct": null,
            "follows_after_raid": 0,
        });

        if let Some(det) = detected_at {
            // Session suchen
            let sess = sqlx::query(
                r#"SELECT id, started_at FROM twitch_stream_sessions
                   WHERE LOWER(streamer_login) = $1 AND started_at <= $2
                     AND (ended_at IS NULL OR ended_at >= $2)
                   LIMIT 1"#,
            )
            .bind(&streamer)
            .bind(det)
            .fetch_optional(&pool)
            .await
            .ok()
            .flatten();

            if let Some(sess_row) = sess {
                let session_id: i64 = sess_row.try_get("id").unwrap_or(0);
                let sess_start: Option<DateTime<Utc>> = sess_row.try_get("started_at").ok();
                let raid_minute = sess_start
                    .map(|ss| ((det - ss).num_seconds() / 60) as i32)
                    .unwrap_or(0)
                    .max(0);

                let tl_rows = sqlx::query(
                    "SELECT minutes_from_start, viewer_count::bigint AS viewer_count FROM twitch_session_viewers WHERE session_id = $1 ORDER BY minutes_from_start",
                )
                .bind(session_id)
                .fetch_all(&pool).await.unwrap_or_default();

                if !tl_rows.is_empty() {
                    let timeline: HashMap<i32, i64> = tl_rows
                        .iter()
                        .filter_map(|r| {
                            Some((
                                r.try_get::<i32, _>("minutes_from_start").ok()?,
                                r.try_get::<i64, _>("viewer_count").ok()?,
                            ))
                        })
                        .collect();

                    let before: Vec<i64> = timeline
                        .iter()
                        .filter(|(&m, _)| (raid_minute - 3) <= m && m < raid_minute)
                        .map(|(_, &v)| v)
                        .collect();
                    let after: Vec<i64> = timeline
                        .iter()
                        .filter(|(&m, _)| m >= raid_minute && m <= raid_minute + 5)
                        .map(|(_, &v)| v)
                        .collect();

                    let avg_before = if !before.is_empty() {
                        Some(before.iter().sum::<i64>() as f64 / before.len() as f64)
                    } else {
                        None
                    };
                    let peak_after = after.iter().copied().max();

                    if let (Some(ab), Some(pa)) = (avg_before, peak_after) {
                        if ab > 0.0 {
                            let boost = ((pa as f64 - ab) / ab * 1000.0).round() / 10.0;
                            impact["viewers_before"] = json!((ab * 10.0).round() / 10.0);
                            impact["viewers_peak_after"] = json!(pa);
                            impact["boost_pct"] = json!(boost);
                            boost_values.push(boost);

                            for (offset, key) in [
                                (5i32, "retention_5m_pct"),
                                (15, "retention_15m_pct"),
                                (30, "retention_30m_pct"),
                            ] {
                                let target = raid_minute + offset;
                                let closest = timeline
                                    .keys()
                                    .min_by_key(|&&m| (m - target).abs())
                                    .copied();
                                if let Some(cm) = closest {
                                    if (cm - target).abs() <= 2 {
                                        let pct = (timeline[&cm] as f64 / pa as f64 * 1000.0)
                                            .round()
                                            / 10.0;
                                        impact[key] = json!(pct);
                                        if key == "retention_15m_pct" {
                                            retention_15m_values.push(pct);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Follows innerhalb 30 Min nach Raid
                let follow_row = sqlx::query(
                    "SELECT COUNT(*) AS follows FROM twitch_follow_events WHERE LOWER(streamer_login) = $1 AND followed_at BETWEEN $2 AND $2 + INTERVAL '30 minutes'",
                )
                .bind(&streamer).bind(det)
                .fetch_optional(&pool).await.ok().flatten();
                if let Some(fr) = follow_row {
                    impact["follows_after_raid"] =
                        json!(fr.try_get::<i64, _>("follows").unwrap_or(0));
                }
            }
        }

        incoming_raids.push(json!({
            "from_channel": from_channel,
            "detected_at": detected_at.map(|t| t.to_rfc3339()),
            "viewers_sent": viewers_sent,
            "classification": rr.try_get::<Option<String>,_>("classification").ok().flatten().unwrap_or_else(|| "unknown".into()),
            "unraid_seen": rr.try_get::<Option<bool>,_>("unraid_seen").ok().flatten().unwrap_or(false),
            "impact": impact,
        }));
    }

    let incoming_summary: Option<Value> = if !incoming_raids.is_empty() {
        let total_rcvd = incoming_raids.len() as i64;
        let avg_viewers_rcvd = incoming_raids
            .iter()
            .map(|r| r["viewers_sent"].as_i64().unwrap_or(0))
            .sum::<i64>() as f64
            / total_rcvd as f64;
        let avg_boost = if !boost_values.is_empty() {
            Some(
                (boost_values.iter().sum::<f64>() / boost_values.len() as f64 * 10.0).round()
                    / 10.0,
            )
        } else {
            None
        };
        let avg_ret_15m = if !retention_15m_values.is_empty() {
            Some(
                (retention_15m_values.iter().sum::<f64>() / retention_15m_values.len() as f64
                    * 10.0)
                    .round()
                    / 10.0,
            )
        } else {
            None
        };

        // Best raider by avg boost
        let mut raider_boosts: HashMap<String, Vec<f64>> = HashMap::new();
        for r in &incoming_raids {
            if let Some(b) = r["impact"]["boost_pct"].as_f64() {
                raider_boosts
                    .entry(r["from_channel"].as_str().unwrap_or("").to_string())
                    .or_default()
                    .push(b);
            }
        }
        let best_raider = raider_boosts
            .iter()
            .max_by(|(_, av), (_, bv)| {
                let a = av.iter().sum::<f64>() / av.len() as f64;
                let b = bv.iter().sum::<f64>() / bv.len() as f64;
                a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(k, _)| k.clone());

        Some(json!({
            "total_raids_received": total_rcvd,
            "avg_viewers_received": (avg_viewers_rcvd * 10.0).round() / 10.0,
            "avg_boost_pct": avg_boost,
            "avg_retention_15m": avg_ret_15m,
            "best_raider": best_raider,
            "raid_balance": {
                "sent": per_source.iter().map(|s| s["raids_received"].as_i64().unwrap_or(0)).sum::<i64>(),
                "received": total_rcvd,
            },
        }))
    } else {
        None
    };

    Json(json!({
        "per_source": per_source,
        "follow_attribution": follow_attribution,
        "retention_curves": retention_curves,
        "incoming_raids": incoming_raids,
        "incoming_summary": incoming_summary,
        "window_days": days,
        "dataQuality": {
            "botFilterApplied": true,
            "retentionCurveSampleSize": base_raids_sample.len(),
            "perSourceUsesFullWindow": true,
            "raidMetricBatchSize": 500,
        },
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn body_json(resp: axum::response::Response) -> Value {
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
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
            "CREATE TABLE twitch_session_chatters (\
                 session_id BIGINT, chatter_id TEXT, chatter_login TEXT, \
                 last_seen_at TIMESTAMPTZ, first_message_at TIMESTAMPTZ, messages INTEGER DEFAULT 0)",
            "CREATE TABLE twitch_chatter_rollup (\
                 chatter_login TEXT, streamer_login TEXT, first_seen_at TIMESTAMPTZ)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    fn raid_input(executed_at: DateTime<Utc>) -> Value {
        json!({
            "raid_id": 1,
            "executed_at_key": "k1",
            "target_session_id": 42,
            "executed_at": executed_at.to_rfc3339(),
            "from_login": "raider",
            "to_login": "host",
        })
    }

    /// P1.33: Chatter mit chatter_id aber NULL chatter_login muss in
    /// plus30m UND new_chatters mitgezählt werden (vorher fälschlich gefiltert).
    #[tokio::test]
    async fn null_login_chatter_wird_gezaehlt() {
        let Some(pool) = make_pool("t_raid_nulllogin").await else {
            return;
        };
        let executed = Utc::now() - chrono::Duration::minutes(60);
        let after = executed + chrono::Duration::minutes(2);

        // Anonymer Chatter: chatter_id gesetzt, chatter_login NULL.
        sqlx::query(
            "INSERT INTO twitch_session_chatters \
             (session_id, chatter_id, chatter_login, last_seen_at, first_message_at, messages) \
             VALUES (42, 'anon-1', NULL, $1, $1, 3)",
        )
        .bind(after)
        .execute(&pool)
        .await
        .unwrap();

        let raids = vec![raid_input(executed)];
        let metrics = recalculate_raid_chat_metrics(&pool, &raids).await;
        let m = metrics.get(&(1, "k1".to_string())).expect("metric");

        assert_eq!(m.plus30m, 1, "NULL-Login-Chatter muss in plus30m zählen");
        assert_eq!(
            m.new_chatters, 1,
            "NULL-Login-Chatter muss als new_chatter zählen"
        );
    }

    /// Gegenprobe: ein bekannter Bot-Login wird weiterhin ausgefiltert.
    #[tokio::test]
    async fn bot_login_wird_gefiltert() {
        let Some(pool) = make_pool("t_raid_botfilter").await else {
            return;
        };
        let executed = Utc::now() - chrono::Duration::minutes(60);
        let after = executed + chrono::Duration::minutes(2);

        sqlx::query(
            "INSERT INTO twitch_session_chatters \
             (session_id, chatter_id, chatter_login, last_seen_at, first_message_at, messages) \
             VALUES (42, 'bot-1', 'nightbot', $1, $1, 3), \
                    (42, 'human-1', 'echtuser', $1, $1, 3)",
        )
        .bind(after)
        .execute(&pool)
        .await
        .unwrap();

        let raids = vec![raid_input(executed)];
        let metrics = recalculate_raid_chat_metrics(&pool, &raids).await;
        let m = metrics.get(&(1, "k1".to_string())).expect("metric");

        assert_eq!(m.plus30m, 1, "Bot ausgefiltert, nur echter User zählt");
        assert_eq!(m.new_chatters, 1);
    }

    /// P2.69: raid-retention liefert bei DB-Fehler (Tabelle fehlt) 200 +
    /// dataAvailable:false statt 500 (graceful Python-Fallback).
    #[tokio::test]
    async fn raid_retention_db_fehler_liefert_200_dataavailable_false() {
        // make_pool legt twitch_raid_retention NICHT an → Primär-Query schlägt fehl.
        let Some(pool) = make_pool("t_raid_retention_dberr").await else {
            return;
        };
        let resp = raid_retention_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(RaidQuery {
                streamer: Some("nani".into()),
                days: Some(30),
            }),
        )
        .await
        .into_response();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "DB-Fehler darf kein 500 sein"
        );
        let v = body_json(resp).await;
        assert_eq!(v["dataAvailable"], false);
    }

    /// P2.98: raid-analytics liefert bei DB-Fehler (Primär-Query-Tabelle fehlt)
    /// 500 statt leeres 200 — inkonsistenz zur retention vermeiden.
    #[tokio::test]
    async fn raid_analytics_db_fehler_liefert_500() {
        // make_pool legt twitch_raid_retention/twitch_raid_history NICHT an →
        // die erste Primär-Query (retention_rows) schlägt fehl.
        let Some(pool) = make_pool("t_raid_analytics_dberr").await else {
            return;
        };
        let resp = raid_analytics_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(RaidQuery {
                streamer: Some("nani".into()),
                days: Some(30),
            }),
        )
        .await
        .into_response();
        assert_eq!(
            resp.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "DB-Fehler in Primär-Query muss 500 sein"
        );
    }

    /// IDOR: Partner mit fremdem `?streamer=` darf fremde Raid-Analytics nicht
    /// lesen → 403. Die Klemme greift vor jeder DB-Query (Dummy-Pool genügt).
    #[tokio::test]
    async fn partner_fremder_streamer_ist_403() {
        let auth = DashboardAuthLevel::Partner {
            twitch_login: "someone".into(),
            twitch_user_id: "1".into(),
            display_name: "someone".into(),
        };
        let pool = match PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://invalid:5432/none")
        {
            Ok(p) => p,
            Err(_) => return,
        };
        let resp = raid_analytics_handler(
            auth,
            State(pool),
            Query(RaidQuery {
                streamer: Some("fremd".into()),
                days: Some(30),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
