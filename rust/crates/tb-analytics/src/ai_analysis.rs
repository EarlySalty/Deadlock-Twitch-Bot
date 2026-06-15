//! Hilfsfunktionen für die KI-Analyse-Endpunkte (`/twitch/api/v2/ai/*`).
//!
//! Port der reinen Parser aus `bot/analytics/api_ai.py`. Die LLM-Antworten sind
//! oft „schmutzig" (Markdown-Fences, Präambeln, abgeschnitten) — diese Funktionen
//! bergen das strukturierte JSON-Array robust. Der eigentliche Anthropic-/MiniMax-
//! Call + In-Memory-State + Persistenz folgen in späteren Slices.

use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;

/// KI-Modell-Kennungen (Python `AI_MODEL_OPUS`/`AI_MODEL_MINIMAX`).
pub const AI_MODEL_OPUS: &str = "opus";
pub const AI_MODEL_MINIMAX: &str = "minimax";

/// Wochentags-Kürzel (Python `_DOW_NAMES`, Index = EXTRACT(DOW), So=0).
const DOW_NAMES: &[&str] = &["So", "Mo", "Di", "Mi", "Do", "Fr", "Sa"];

/// Erste 60 Zeichen eines optionalen Titels (Python `(str(x) if x else "")[:60]`).
fn title60(s: Option<&str>) -> String {
    s.filter(|t| !t.is_empty()).map(|t| t.chars().take(60).collect()).unwrap_or_default()
}

/// Echte Modellnamen (Python `CLAUDE_MODEL`/`MINIMAX_MODEL`), wie sie in
/// `ai_analyses.model` persistiert werden.
pub const CLAUDE_MODEL: &str = "claude-opus-4-6";
pub const MINIMAX_MODEL: &str = "MiniMax-M3";

/// Modellname für die Persistenz: `opus` → Claude, sonst MiniMax (1:1 Python
/// `CLAUDE_MODEL if ai_model == AI_MODEL_OPUS else MINIMAX_MODEL`).
pub fn model_name_for(ai_model: &str) -> &'static str {
    if ai_model == AI_MODEL_OPUS {
        CLAUDE_MODEL
    } else {
        MINIMAX_MODEL
    }
}

/// Reine Modellwahl aus Entitlements (Python `_plan_ai_model`-Logik):
/// `analytics.ai_full` → Opus, sonst `analytics.ai_mini` → MiniMax, sonst keins.
pub fn model_for_entitlements(entitlements: &[&str]) -> Option<&'static str> {
    if entitlements.contains(&"analytics.ai_full") {
        Some(AI_MODEL_OPUS)
    } else if entitlements.contains(&"analytics.ai_mini") {
        Some(AI_MODEL_MINIMAX)
    } else {
        None
    }
}

/// Plan-abhängiges KI-Modell eines Streamers (Python `_plan_ai_model`):
/// Plan-Snapshot (login-only) → Entitlements → Modellwahl.
pub async fn plan_ai_model(pool: &PgPool, streamer: &str) -> Result<Option<&'static str>, sqlx::Error> {
    let snapshot = crate::plan::resolve_plan_snapshot(pool, streamer, "").await?;
    Ok(model_for_entitlements(&snapshot.entitlements))
}

/// Sammelt den vollständigen Analytics-Kontext für die KI-Analyse (Port von
/// `_collect_ai_context`, 9 Queries → strukturiertes Dict). `streamer` muss
/// kleingeschrieben sein; `game_filter` ∈ {`deadlock`, `all`}. NUMERIC-Aggregate
/// werden `::float8` gecastet (sqlx ohne numeric-Feature). exp_sessions-Query
/// best-effort (Tabelle evtl. leer/fehlend → leeres `gamePerformance`).
pub async fn collect_ai_context(
    pool: &PgPool,
    streamer: &str,
    since: DateTime<Utc>,
    game_filter: &str,
) -> Result<Value, sqlx::Error> {
    let gf = if game_filter == "deadlock" {
        " AND had_deadlock_in_session = true"
    } else {
        ""
    };
    let since_iso = since.to_rfc3339_opts(SecondsFormat::Micros, false);

    // 1. Overview-KPIs.
    let ov: (i64, Option<f64>, Option<f64>, Option<i32>, i64, Option<f64>, Option<f64>, Option<f64>) =
        sqlx::query_as(&format!(
            "SELECT COUNT(*)::bigint, \
                    ROUND((SUM(duration_seconds) / 3600.0)::numeric, 1)::float8, \
                    ROUND(AVG(avg_viewers)::numeric, 1)::float8, \
                    MAX(peak_viewers), \
                    COALESCE(SUM(CASE WHEN follower_delta > 0 THEN follower_delta ELSE 0 END), 0)::bigint, \
                    ROUND((AVG(retention_10m) * 100)::numeric, 1)::float8, \
                    ROUND((AVG(dropoff_pct) * 100)::numeric, 1)::float8, \
                    ROUND(AVG(COALESCE(unique_chatters, 0))::numeric, 0)::float8 \
               FROM twitch_stream_sessions \
              WHERE LOWER(streamer_login) = $1 AND started_at >= $2 AND ended_at IS NOT NULL{gf}"
        ))
        .bind(streamer)
        .bind(since)
        .fetch_one(pool)
        .await?;

    // 2. Letzte 20 Sessions.
    let sessions: Vec<(NaiveDate, Option<String>, Option<f64>, Option<f64>, Option<i32>, Option<f64>, Option<f64>, i64, i64)> =
        sqlx::query_as(&format!(
            "SELECT started_at::date, stream_title, \
                    ROUND((duration_seconds / 3600.0)::numeric, 2)::float8, \
                    ROUND(avg_viewers::numeric, 1)::float8, peak_viewers, \
                    ROUND((retention_10m * 100)::numeric, 1)::float8, \
                    ROUND((dropoff_pct * 100)::numeric, 1)::float8, \
                    COALESCE(unique_chatters, 0)::bigint, COALESCE(follower_delta, 0)::bigint \
               FROM twitch_stream_sessions \
              WHERE LOWER(streamer_login) = $1 AND started_at >= $2 AND ended_at IS NOT NULL{gf} \
              ORDER BY started_at DESC LIMIT 20"
        ))
        .bind(streamer)
        .bind(since)
        .fetch_all(pool)
        .await?;

    // 3. Wochentags-Performance.
    let weekday: Vec<(i32, i64, Option<f64>, Option<f64>)> = sqlx::query_as(&format!(
        "SELECT EXTRACT(DOW FROM started_at)::int, COUNT(*)::bigint, \
                ROUND(AVG(avg_viewers)::numeric, 1)::float8, ROUND(AVG(peak_viewers)::numeric, 1)::float8 \
           FROM twitch_stream_sessions \
          WHERE LOWER(streamer_login) = $1 AND started_at >= $2 AND ended_at IS NOT NULL{gf} \
          GROUP BY 1 ORDER BY AVG(avg_viewers) DESC"
    ))
    .bind(streamer)
    .bind(since)
    .fetch_all(pool)
    .await?;

    // 4./5. Beste/schlechteste 5 Sessions.
    let best: Vec<(String, Option<f64>, Option<i32>, Option<f64>, NaiveDate)> = sqlx::query_as(&format!(
        "SELECT COALESCE(stream_title, ''), avg_viewers::float8, peak_viewers, \
                ROUND((retention_10m * 100)::numeric, 1)::float8, started_at::date \
           FROM twitch_stream_sessions \
          WHERE LOWER(streamer_login) = $1 AND started_at >= $2 AND ended_at IS NOT NULL{gf} \
          ORDER BY avg_viewers DESC NULLS LAST LIMIT 5"
    ))
    .bind(streamer)
    .bind(since)
    .fetch_all(pool)
    .await?;
    let worst: Vec<(String, Option<f64>, Option<i32>, Option<f64>, NaiveDate)> = sqlx::query_as(&format!(
        "SELECT COALESCE(stream_title, ''), avg_viewers::float8, peak_viewers, \
                ROUND((retention_10m * 100)::numeric, 1)::float8, started_at::date \
           FROM twitch_stream_sessions \
          WHERE LOWER(streamer_login) = $1 AND started_at >= $2 AND ended_at IS NOT NULL{gf} \
          ORDER BY avg_viewers ASC NULLS LAST LIMIT 5"
    ))
    .bind(streamer)
    .bind(since)
    .fetch_all(pool)
    .await?;

    // 6. Game-Breakdown aus exp_sessions (best-effort; Tabelle evtl. fehlend).
    let game_gf = if game_filter == "deadlock" {
        " AND LOWER(game_name) = 'deadlock'"
    } else {
        ""
    };
    let game_rows: Vec<(String, i64, Option<f64>, Option<i32>, Option<f64>)> = sqlx::query_as(&format!(
        "SELECT COALESCE(game_name, 'Unbekannt'), COUNT(*)::bigint, \
                ROUND(AVG(avg_viewers)::numeric, 1)::float8, MAX(peak_viewers), \
                ROUND(AVG(duration_min)::numeric, 1)::float8 \
           FROM exp_sessions \
          WHERE LOWER(streamer) = $1 AND started_at >= $2 AND ended_at IS NOT NULL{game_gf} \
          GROUP BY game_name ORDER BY AVG(avg_viewers) DESC LIMIT 10"
    ))
    .bind(streamer)
    .bind(&since_iso)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    // 7. Wöchentlicher Follower-Trend.
    let trend: Vec<(NaiveDate, i64, Option<i64>)> = sqlx::query_as(&format!(
        "SELECT DATE_TRUNC('week', started_at)::date, COUNT(*)::bigint, \
                SUM(CASE WHEN follower_delta > 0 THEN follower_delta ELSE 0 END)::bigint \
           FROM twitch_stream_sessions \
          WHERE LOWER(streamer_login) = $1 AND started_at >= $2 AND ended_at IS NOT NULL{gf} \
          GROUP BY 1 ORDER BY 1"
    ))
    .bind(streamer)
    .bind(since)
    .fetch_all(pool)
    .await?;

    // 8. Deadlock-spezifische KPIs.
    let dl: (i64, Option<f64>, Option<f64>, Option<i32>, i64) = sqlx::query_as(
        "SELECT COUNT(*)::bigint, ROUND((SUM(duration_seconds) / 3600.0)::numeric, 1)::float8, \
                ROUND(AVG(avg_viewers)::numeric, 1)::float8, MAX(peak_viewers), \
                COALESCE(SUM(CASE WHEN follower_delta > 0 THEN follower_delta ELSE 0 END), 0)::bigint \
           FROM twitch_stream_sessions \
          WHERE LOWER(streamer_login) = $1 AND started_at >= $2 AND ended_at IS NOT NULL \
            AND had_deadlock_in_session = true",
    )
    .bind(streamer)
    .bind(since)
    .fetch_one(pool)
    .await?;

    // 9. Per-Game-Breakdown aus Sessions (alle Kategorien).
    let game_sessions: Vec<(String, i64, Option<f64>, Option<i32>, Option<f64>, i64, Option<i64>, NaiveDate)> =
        sqlx::query_as(
            "SELECT COALESCE(game_name, 'Unbekannt'), COUNT(*)::bigint, \
                    ROUND(AVG(avg_viewers)::numeric, 1)::float8, MAX(peak_viewers), \
                    ROUND((SUM(duration_seconds) / 3600.0)::numeric, 1)::float8, \
                    COALESCE(SUM(CASE WHEN follower_delta > 0 THEN follower_delta ELSE 0 END), 0)::bigint, \
                    SUM(samples)::bigint, MAX(started_at)::date \
               FROM twitch_stream_sessions \
              WHERE LOWER(streamer_login) = $1 AND started_at >= $2 AND ended_at IS NOT NULL \
              GROUP BY game_name ORDER BY COUNT(*) DESC, AVG(avg_viewers) DESC NULLS LAST LIMIT 15",
        )
        .bind(streamer)
        .bind(since)
        .fetch_all(pool)
        .await?;

    Ok(json!({
        "summary": {
            "streamCount": ov.0,
            "totalHours": ov.1.unwrap_or(0.0),
            "avgViewers": ov.2.unwrap_or(0.0),
            "peakViewers": ov.3.unwrap_or(0) as i64,
            "followersGained": ov.4,
            "avgRetention10m": ov.5.unwrap_or(0.0),
            "avgDropoffPct": ov.6.unwrap_or(0.0),
            "avgChatters": ov.7.unwrap_or(0.0) as i64,
        },
        "recentSessions": sessions.iter().map(|r| json!({
            "date": r.0.to_string(),
            "title": title60(r.1.as_deref()),
            "hours": r.2.unwrap_or(0.0),
            "avgViewers": r.3.unwrap_or(0.0),
            "peakViewers": r.4.unwrap_or(0) as i64,
            "retention10m": r.5.unwrap_or(0.0),
            "dropoffPct": r.6.unwrap_or(0.0),
            "chatters": r.7,
            "followerDelta": r.8,
        })).collect::<Vec<_>>(),
        "weekdayPerformance": weekday.iter().map(|w| json!({
            "day": if (0..=6).contains(&w.0) { DOW_NAMES[w.0 as usize] } else { "?" },
            "streams": w.1,
            "avgViewers": w.2.unwrap_or(0.0),
            "avgPeak": w.3.unwrap_or(0.0),
        })).collect::<Vec<_>>(),
        "bestSessions": best.iter().map(|s| json!({
            "title": title60(Some(s.0.as_str())),
            "avgViewers": s.1.unwrap_or(0.0),
            "peakViewers": s.2.unwrap_or(0) as i64,
            "retention10m": s.3.unwrap_or(0.0),
            "date": s.4.to_string(),
        })).collect::<Vec<_>>(),
        "worstSessions": worst.iter().map(|s| json!({
            "title": title60(Some(s.0.as_str())),
            "avgViewers": s.1.unwrap_or(0.0),
            "peakViewers": s.2.unwrap_or(0) as i64,
            "retention10m": s.3.unwrap_or(0.0),
            "date": s.4.to_string(),
        })).collect::<Vec<_>>(),
        "gamePerformance": game_rows.iter().map(|g| json!({
            "game": g.0,
            "sessions": g.1,
            "avgViewers": g.2.unwrap_or(0.0),
            "peakViewers": g.3.unwrap_or(0) as i64,
            "avgDurationMin": g.4.unwrap_or(0.0),
        })).collect::<Vec<_>>(),
        "weeklyTrend": trend.iter().map(|t| json!({
            "week": t.0.to_string(),
            "streams": t.1,
            "followersGained": t.2.unwrap_or(0),
        })).collect::<Vec<_>>(),
        "deadlockSummary": {
            "sessionCount": dl.0,
            "totalHours": dl.1.unwrap_or(0.0),
            "avgViewers": dl.2.unwrap_or(0.0),
            "peakViewers": dl.3.unwrap_or(0) as i64,
            "followersGained": dl.4,
        },
        "gameBreakdown": game_sessions.iter().map(|g| json!({
            "game": g.0,
            "sessions": g.1,
            "avgViewers": g.2.unwrap_or(0.0),
            "peakViewers": g.3.unwrap_or(0) as i64,
            "totalHours": g.4.unwrap_or(0.0),
            "followersGained": g.5,
            "totalSamples": g.6.unwrap_or(0),
            "hasFullData": g.6.unwrap_or(0) > 2,
            "lastPlayed": g.7.to_string(),
        })).collect::<Vec<_>>(),
    }))
}

/// Extrahiert Text aus einer LLM-Antwort (Port von `_extract_text_response`).
/// String → getrimmt; Array von Content-Blocks → deren Text-Felder mit `\n`
/// verbunden + getrimmt (Claude `messages.content`); sonst best-effort.
pub fn extract_text_response(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.trim().to_string(),
        Value::Array(items) => {
            let mut parts: Vec<String> = Vec::new();
            for item in items {
                match item {
                    Value::String(s) => parts.push(s.clone()),
                    // dict-Block: type==text & text, sonst content (1:1 Python).
                    Value::Object(o) => {
                        let text = o.get("text").and_then(Value::as_str).filter(|s| !s.is_empty());
                        if o.get("type").and_then(Value::as_str) == Some("text") {
                            if let Some(t) = text {
                                parts.push(t.to_string());
                                continue;
                            }
                        }
                        if let Some(c) = o.get("content").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                            parts.push(c.to_string());
                        }
                    }
                    _ => {}
                }
            }
            parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join("\n").trim().to_string()
        }
        Value::Object(o) => match o.get("text").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            Some(t) => t.trim().to_string(),
            None => value.to_string().trim().to_string(),
        },
        other => other.to_string().trim().to_string(),
    }
}

/// Formatiert einen Wert wie Pythons `f"{value}"` (Float → repr „40.0", Integer
/// → „2", String unverändert); fehlend → „0" (Pythons `.get(k, 0)`-Default).
fn fmt_val(v: Option<&Value>) -> String {
    match v {
        Some(Value::Number(n)) => {
            if n.is_f64() {
                format!("{:?}", n.as_f64().unwrap())
            } else {
                n.to_string()
            }
        }
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => if *b { "True" } else { "False" }.to_string(),
        None | Some(Value::Null) => "0".to_string(),
        Some(other) => other.to_string(),
    }
}

/// `json.dumps(ctx.get(key, []), ensure_ascii=False)`. Hinweis: serde_json
/// serialisiert kompakt (`{"k":1}`), Python mit `, `/`: `-Trennern und in
/// Insertion-Order — der Prompt-Text weicht hier minimal ab, was NUR den an das
/// LLM gesendeten Prompt betrifft (nicht-deterministische Antwort → nicht
/// beobachtbar in der API-Antwort).
fn dumps_array(ctx: &Value, key: &str) -> String {
    match ctx.get(key) {
        Some(v) => serde_json::to_string(v).unwrap_or_else(|_| "[]".to_string()),
        None => "[]".to_string(),
    }
}

/// Statischer Prompt-Abschluss (Antwortformat + Beispiel-Array + Regeln),
/// 1:1 Python (escaped `{{`/`}}` → literale Klammern).
const PROMPT_TAIL: &str = r#"

---
Antworte NUR als JSON Array mit exakt 10 Objekten. Kein Markdown, kein Text außerhalb des JSON.

[
  {
    "number": 1,
    "priority": "kritisch",
    "title": "Titel (max 8 Wörter)",
    "analysis": "Tiefenanalyse 3-5 Sätze mit konkreten Zahlen aus den Daten.",
    "action": "Konkrete Handlungsempfehlung: Was, wann, wie oft, wie messen.",
    "expectedImpact": "Realistischer erwarteter Effekt basierend auf den Daten."
  }
]

Gültige priority-Werte: "kritisch", "hoch", "mittel"
Punkte 1-3: kritisch | Punkte 4-7: hoch | Punkte 8-10: mittel

DATENHINWEIS: Spiele mit "(Viewer-Daten unvollständig)" haben kein Viewer-Sampling –
avg_viewers/peak dort sind Initialwerte bei Stream-Start, nicht repräsentativ.
Vollständige Viewer-Metriken nur für Einträge ohne diesen Hinweis verwenden."#;

/// Baut den KI-Analyse-Prompt (Port von `_build_ai_analysis_prompt`): KPI-
/// Übersicht + Top/Schwächste/Letzte Sessions + Wochentag + Kategorien +
/// Multi-Game-Zeilen + Follower-Trend, dann das 10-Punkte-Antwortformat. Hängt
/// optional den `user_context` an.
pub fn build_ai_analysis_prompt(
    streamer: &str,
    days: i64,
    ctx: &Value,
    game_filter: &str,
    user_context: &str,
) -> String {
    let s = ctx.get("summary").cloned().unwrap_or_else(|| json!({}));
    let mode_label = if game_filter == "deadlock" {
        "Nur Deadlock-Sessions"
    } else {
        "Alle gespielten Kategorien"
    };

    // Kategorien-Performance (exp); leer → Hinweis-Objekt.
    let game_section = match ctx.get("gamePerformance").and_then(Value::as_array) {
        Some(arr) if !arr.is_empty() => serde_json::to_string(&Value::Array(arr.clone())).unwrap_or_else(|_| "[]".to_string()),
        _ => serde_json::to_string(&json!([{"note": "Keine Kategorie-Daten vorhanden (exp_sessions leer)"}])).unwrap_or_else(|_| "[]".to_string()),
    };

    // Multi-Game-Zeilen (Deadlock-Gesamt + Per-Game-Breakdown).
    let dl = ctx.get("deadlockSummary").cloned().unwrap_or_else(|| json!({}));
    let mut multi_lines = vec![format!(
        "Deadlock (gesamt): {} Sessions | {}h | Ø {} Viewer | Peak {} | +{} Follower",
        fmt_val(dl.get("sessionCount")),
        fmt_val(dl.get("totalHours")),
        fmt_val(dl.get("avgViewers")),
        fmt_val(dl.get("peakViewers")),
        fmt_val(dl.get("followersGained")),
    )];
    if let Some(gb) = ctx.get("gameBreakdown").and_then(Value::as_array) {
        for g in gb {
            let quality = if g.get("hasFullData").and_then(Value::as_bool).unwrap_or(false) {
                ""
            } else {
                " (Viewer-Daten unvollständig)"
            };
            multi_lines.push(format!(
                "  {}: {} Sessions | {}h | Ø {} Viewer | Peak {} | +{} Follower | zuletzt {}{}",
                fmt_val(g.get("game")),
                fmt_val(g.get("sessions")),
                fmt_val(g.get("totalHours")),
                fmt_val(g.get("avgViewers")),
                fmt_val(g.get("peakViewers")),
                fmt_val(g.get("followersGained")),
                fmt_val(g.get("lastPlayed")),
                quality,
            ));
        }
    }
    let multi = multi_lines.join("\n");

    let sc = fmt_val(s.get("streamCount"));
    let th = fmt_val(s.get("totalHours"));
    let av = fmt_val(s.get("avgViewers"));
    let pv = fmt_val(s.get("peakViewers"));
    let fg = fmt_val(s.get("followersGained"));
    let ret = fmt_val(s.get("avgRetention10m"));
    let dp = fmt_val(s.get("avgDropoffPct"));
    let ch = fmt_val(s.get("avgChatters"));
    let best = dumps_array(ctx, "bestSessions");
    let worst = dumps_array(ctx, "worstSessions");
    let recent = dumps_array(ctx, "recentSessions");
    let weekday = dumps_array(ctx, "weekdayPerformance");
    let trend = dumps_array(ctx, "weeklyTrend");

    let mut prompt = format!(
        "Du bist ein Experte für Twitch-Streaming-Analytik und Wachstumsstrategie.\n\n\
Analysiere die Streaming-Daten des Kanals **{streamer}** (letzte {days} Tage, Modus: {mode_label}) und erstelle einen TIEFEN, DATEN-BASIERTEN 10-Punkte-Verbesserungsplan.\n\n\
REGELN:\n\
- Referenziere IMMER konkrete Zahlen aus den Daten\n\
- Keine generischen Ratschläge\n\
- Erkläre das WARUM hinter jedem Pattern\n\
- Priorisiere nach maximalem Impact (#1 = wichtigster Hebel)\n\
- Zeige sowohl Chancen als auch Risiken auf\n\n\
=== KPI ÜBERSICHT ===\n\
Streams: {sc} | Gesamtzeit: {th}h\n\
Ø Viewer: {av} | Peak: {pv}\n\
Follower gewonnen: +{fg}\n\
Ø 10-Min-Retention: {ret}% | Ø Dropoff: {dp}%\n\
Ø Aktive Chatter: {ch}\n\n\
=== TOP 5 STREAMS (Ø Viewer) ===\n{best}\n\n\
=== SCHWÄCHSTE 5 STREAMS ===\n{worst}\n\n\
=== LETZTE 20 SESSIONS ===\n{recent}\n\n\
=== WOCHENTAG-PERFORMANCE ===\n{weekday}\n\n\
=== KATEGORIEN-PERFORMANCE ===\n{game_section}\n\n\
=== ALLE GESTREAMTEN SPIELE (inkl. Nicht-Deadlock) ===\n{multi}\n\n\
=== WÖCHENTLICHER FOLLOWER-TREND ===\n{trend}"
    );
    prompt.push_str(PROMPT_TAIL);
    if !user_context.is_empty() {
        prompt.push_str(&format!(
            "\n\n=== STREAMER-KONTEXT ===\nDer Streamer hat folgende eigene Eindrücke/Fragen mitgegeben: {user_context}"
        ));
    }
    prompt
}

/// Erstes vollständiges JSON-Array aus `text` (string-aware: `]` innerhalb von
/// Strings wird übersprungen). `None`, wenn das Array nicht terminiert ist
/// (abgeschnittene Antwort). Port von `_extract_json_array`.
pub fn extract_json_array(text: &str) -> Option<String> {
    let start = text.find('[')?;
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escape_next = false;
    for (i, ch) in text[start..].char_indices() {
        let byte = start + i;
        if escape_next {
            escape_next = false;
            continue;
        }
        if ch == '\\' && in_string {
            escape_next = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        if ch == '[' {
            depth += 1;
        } else if ch == ']' {
            depth -= 1;
            if depth == 0 {
                return Some(text[start..=byte].to_string());
            }
        }
    }
    None // abgeschnitten – kein passendes ]
}

/// Parst + birgt das strukturierte JSON-Array einer Modell-Antwort (Port von
/// `_parse_ai_analysis_points`). Drei Stufen: (1) Direktparse, (2) saubere
/// Bracket-Extraktion (Präambel/Trailing), (3) Truncation-Salvage (komplette
/// Objekte einsammeln). Liefert `[]`, wenn nichts Brauchbares gefunden wird.
pub fn parse_ai_analysis_points(raw: &str) -> Vec<Value> {
    let mut raw = raw.trim().to_string();

    // Markdown-Code-Fences entfernen.
    if raw.starts_with("```") {
        let kept: Vec<&str> = raw.lines().filter(|ln| !ln.trim().starts_with("```")).collect();
        raw = kept.join("\n").trim().to_string();
    }

    // 1) Direktparse – perfektes JSON.
    if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(&raw) {
        return arr;
    }

    // 2) Bracket-Extraktion – Präambel/Trailing + `]` in Strings.
    if let Some(extracted) = extract_json_array(&raw) {
        if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(&extracted) {
            return arr;
        }
    }

    // 3) Truncation-Salvage – komplette depth-1-Objekte einsammeln.
    if let Some(array_start) = raw.find('[') {
        let mut depth: i32 = 0;
        let mut in_string = false;
        let mut escape_next = false;
        let mut obj_start: Option<usize> = None;
        let mut salvaged: Vec<String> = Vec::new();
        for (i, ch) in raw[array_start..].char_indices() {
            let byte = array_start + i;
            if escape_next {
                escape_next = false;
                continue;
            }
            if ch == '\\' && in_string {
                escape_next = true;
                continue;
            }
            if ch == '"' {
                in_string = !in_string;
                continue;
            }
            if in_string {
                continue;
            }
            if ch == '{' {
                if depth == 0 {
                    obj_start = Some(byte);
                }
                depth += 1;
            } else if ch == '}' {
                depth -= 1;
                if depth == 0 {
                    if let Some(os) = obj_start.take() {
                        salvaged.push(raw[os..=byte].to_string());
                    }
                }
            } else if ch == ']' && depth == 0 {
                break;
            }
        }
        if !salvaged.is_empty() {
            let candidate = format!("[{}]", salvaged.join(","));
            if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(&candidate) {
                if !arr.is_empty() {
                    return arr;
                }
            }
        }
    }

    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema), ("timezone", "UTC")]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        sqlx::query("CREATE TABLE twitch_stream_sessions (id BIGSERIAL PRIMARY KEY, streamer_login TEXT, started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ, duration_seconds INTEGER, avg_viewers REAL, peak_viewers INTEGER, follower_delta INTEGER, retention_10m REAL, dropoff_pct REAL, unique_chatters INTEGER, stream_title TEXT, had_deadlock_in_session BOOLEAN, game_name TEXT, samples INTEGER)")
            .execute(&pool).await.unwrap();
        Some(pool)
    }

    #[test]
    fn extract_json_array_grundfaelle() {
        assert_eq!(extract_json_array("[1, 2, 3]").as_deref(), Some("[1, 2, 3]"));
        // Verschachtelt.
        assert_eq!(extract_json_array("pre [a, [b]] post").as_deref(), Some("[a, [b]]"));
        // `]` im String wird übersprungen.
        assert_eq!(extract_json_array(r#"["has ] bracket"]"#).as_deref(), Some(r#"["has ] bracket"]"#));
        // Abgeschnitten / keins.
        assert_eq!(extract_json_array("[1, 2"), None);
        assert_eq!(extract_json_array("kein array"), None);
    }

    #[test]
    fn parse_points_direkt_und_fence() {
        assert_eq!(parse_ai_analysis_points(r#"[{"a":1}]"#), vec![json!({"a": 1})]);
        // Markdown-Fence.
        assert_eq!(
            parse_ai_analysis_points("```json\n[{\"a\":1}]\n```"),
            vec![json!({"a": 1})]
        );
        // Präambel/Trailing → Stufe 2.
        assert_eq!(parse_ai_analysis_points(r#"Hier: [{"a":1}] fertig"#), vec![json!({"a": 1})]);
    }

    #[test]
    fn parse_points_nicht_array_und_salvage() {
        // Objekt statt Array → [].
        assert_eq!(parse_ai_analysis_points(r#"{"a":1}"#), Vec::<Value>::new());
        // Abgeschnitten → Salvage kompletter Objekte.
        assert_eq!(
            parse_ai_analysis_points(r#"[{"a":1}, {"b":2}, {"c":"#),
            vec![json!({"a": 1}), json!({"b": 2})]
        );
        // Gar nichts.
        assert_eq!(parse_ai_analysis_points("kaputt"), Vec::<Value>::new());
    }

    #[test]
    fn modellwahl_und_name() {
        // ai_full hat Vorrang vor ai_mini.
        assert_eq!(model_for_entitlements(&["analytics.ai_full"]), Some("opus"));
        assert_eq!(model_for_entitlements(&["analytics.ai_mini"]), Some("minimax"));
        assert_eq!(
            model_for_entitlements(&["analytics.ai_full", "analytics.ai_mini"]),
            Some("opus")
        );
        assert_eq!(model_for_entitlements(&["analytics.basic"]), None);
        assert_eq!(model_for_entitlements(&[]), None);
        // Persistenz-Modellname.
        assert_eq!(model_name_for("opus"), "claude-opus-4-6");
        assert_eq!(model_name_for("minimax"), "MiniMax-M3");
    }

    #[test]
    fn extract_text_response_faelle() {
        // String → getrimmt (MiniMax-Content).
        assert_eq!(extract_text_response(&json!("  hallo  ")), "hallo");
        // Claude content-Blocks → text-Felder mit \n.
        assert_eq!(
            extract_text_response(&json!([
                {"type": "text", "text": "Zeile 1"},
                {"type": "text", "text": "Zeile 2"}
            ])),
            "Zeile 1\nZeile 2"
        );
        // content-Fallback bei Nicht-text-Block.
        assert_eq!(
            extract_text_response(&json!([{"type": "tool", "content": "X"}])),
            "X"
        );
        // Null → leer.
        assert_eq!(extract_text_response(&Value::Null), "");
    }

    #[tokio::test]
    async fn collect_ai_context_aggregiert() {
        let Some(pool) = make_pool("t_ai_ctx").await else { return };
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (streamer_login, started_at, ended_at, duration_seconds, avg_viewers, peak_viewers, follower_delta, retention_10m, dropoff_pct, unique_chatters, stream_title, had_deadlock_in_session, game_name, samples) VALUES \
            ('nani', TIMESTAMPTZ '2026-06-10 14:00+00', TIMESTAMPTZ '2026-06-10 16:00+00', 7200, 50, 100, 20, 0.8, 0.1, 10, 'Deadlock Grind', true, 'Deadlock', 5), \
            ('nani', TIMESTAMPTZ '2026-06-11 18:00+00', TIMESTAMPTZ '2026-06-11 19:00+00', 3600, 30, 60, 5, 0.6, 0.2, 8, 'Chill', false, 'Just Chatting', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let since = Utc::now() - chrono::Duration::days(365);
        let v = collect_ai_context(&pool, "nani", since, "all").await.unwrap();

        // Summary über beide Sessions.
        assert_eq!(v["summary"]["streamCount"], 2);
        assert_eq!(v["summary"]["totalHours"], 3.0); // (7200+3600)/3600
        assert_eq!(v["summary"]["avgViewers"], 40.0);
        assert_eq!(v["summary"]["peakViewers"], 100);
        assert_eq!(v["summary"]["followersGained"], 25);
        assert_eq!(v["summary"]["avgRetention10m"], 70.0); // AVG(0.8,0.6)*100
        assert_eq!(v["summary"]["avgDropoffPct"], 15.0);
        assert_eq!(v["summary"]["avgChatters"], 9); // ROUND(AVG(10,8),0)

        // Recent: neueste zuerst.
        assert_eq!(v["recentSessions"].as_array().unwrap().len(), 2);
        assert_eq!(v["recentSessions"][0]["date"], "2026-06-11");
        assert_eq!(v["recentSessions"][0]["title"], "Chill");

        // Deadlock-Summary: nur Session 1.
        assert_eq!(v["deadlockSummary"]["sessionCount"], 1);
        assert_eq!(v["deadlockSummary"]["avgViewers"], 50.0);
        assert_eq!(v["deadlockSummary"]["followersGained"], 20);

        // gamePerformance leer (exp_sessions fehlt → best-effort []).
        assert_eq!(v["gamePerformance"], json!([]));

        // gameBreakdown: Deadlock (avg 50) vor Just Chatting (avg 30) bei gleichem Count.
        let gb = v["gameBreakdown"].as_array().unwrap();
        assert_eq!(gb.len(), 2);
        assert_eq!(gb[0]["game"], "Deadlock");
        assert_eq!(gb[0]["totalSamples"], 5);
        assert_eq!(gb[0]["hasFullData"], true); // 5 > 2
        let jc = gb.iter().find(|g| g["game"] == "Just Chatting").unwrap();
        assert_eq!(jc["hasFullData"], false); // 1 <= 2
    }

    #[tokio::test]
    async fn collect_ai_context_deadlock_filter() {
        let Some(pool) = make_pool("t_ai_ctx_dl").await else { return };
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (streamer_login, started_at, ended_at, duration_seconds, avg_viewers, peak_viewers, follower_delta, retention_10m, dropoff_pct, unique_chatters, stream_title, had_deadlock_in_session, game_name, samples) VALUES \
            ('nani', NOW()-INTERVAL '1 day', NOW(), 7200, 50, 100, 20, 0.8, 0.1, 10, 'A', true, 'Deadlock', 5), \
            ('nani', NOW()-INTERVAL '2 day', NOW(), 3600, 30, 60, 5, 0.6, 0.2, 8, 'B', false, 'Just Chatting', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let since = Utc::now() - chrono::Duration::days(365);
        // game_filter=deadlock → nur had_deadlock_in_session-Sessions in summary.
        let v = collect_ai_context(&pool, "nani", since, "deadlock").await.unwrap();
        assert_eq!(v["summary"]["streamCount"], 1);
        assert_eq!(v["summary"]["avgViewers"], 50.0);
    }

    #[test]
    fn build_prompt_kpi_und_floats() {
        let ctx = json!({
            "summary": {"streamCount": 2, "totalHours": 3.0, "avgViewers": 40.0, "peakViewers": 100,
                        "followersGained": 25, "avgRetention10m": 70.0, "avgDropoffPct": 15.0, "avgChatters": 9},
            "gamePerformance": [],
            "deadlockSummary": {"sessionCount": 1, "totalHours": 2.0, "avgViewers": 50.0, "peakViewers": 100, "followersGained": 20},
            "gameBreakdown": [
                {"game": "Deadlock", "sessions": 1, "totalHours": 2.0, "avgViewers": 50.0, "peakViewers": 100, "followersGained": 20, "lastPlayed": "2026-06-10", "hasFullData": true},
                {"game": "Just Chatting", "sessions": 1, "totalHours": 1.0, "avgViewers": 30.0, "peakViewers": 60, "followersGained": 5, "lastPlayed": "2026-06-11", "hasFullData": false}
            ],
            "bestSessions": [], "worstSessions": [], "recentSessions": [], "weekdayPerformance": [], "weeklyTrend": []
        });

        let p = build_ai_analysis_prompt("nani", 30, &ctx, "all", "");
        assert!(p.contains("Kanals **nani** (letzte 30 Tage, Modus: Alle gespielten Kategorien)"));
        // Float-Repr (40.0, nicht 40) + Integer (2).
        assert!(p.contains("Streams: 2 | Gesamtzeit: 3.0h"));
        assert!(p.contains("Ø Viewer: 40.0 | Peak: 100"));
        // Leere gamePerformance → Hinweis-Objekt.
        assert!(p.contains("Keine Kategorie-Daten vorhanden (exp_sessions leer)"));
        // Multi-Game-Zeilen + Qualitätshinweis bei hasFullData=false.
        assert!(p.contains("Deadlock (gesamt): 1 Sessions | 2.0h | Ø 50.0 Viewer | Peak 100 | +20 Follower"));
        assert!(p.contains("Just Chatting: 1 Sessions | 1.0h | Ø 30.0 Viewer | Peak 60 | +5 Follower | zuletzt 2026-06-11 (Viewer-Daten unvollständig)"));
        // Statischer Abschluss + kein user_context-Block.
        assert!(p.contains("Antworte NUR als JSON Array mit exakt 10 Objekten"));
        assert!(!p.contains("STREAMER-KONTEXT"));

        // Mit user_context + deadlock-Modus.
        let p2 = build_ai_analysis_prompt("nani", 7, &ctx, "deadlock", "Mehr Action gewünscht");
        assert!(p2.contains("Modus: Nur Deadlock-Sessions"));
        assert!(p2.ends_with("=== STREAMER-KONTEXT ===\nDer Streamer hat folgende eigene Eindrücke/Fragen mitgegeben: Mehr Action gewünscht"));
    }
}
