//! Score-Refresh-Resolver — berechnet Raid-Scores für alle Partner-User-IDs
//! neu und schreibt sie in `twitch_partner_raid_scores`.
//!
//! Dieses Modul gehört absichtlich in `tb-bot` (Composition-Root), weil es
//! Monitoring-Tabellen (`twitch_live_state`, `twitch_stream_sessions`) und
//! tb-raid-Typen überbrückt. Eine Abhängigkeit in die andere Richtung wäre
//! ein Zyklus.
//!
//! **Python-Herkunft:** `bot/raid/partner_scores.py`,
//! Klasse `PartnerRaidScoreService`, Methode `_refresh_scores_for_ids` (Z. 340+).
//! Die Input-Berechnung entspricht `_build_score` (Z. 656–770).
//!
//! Das Modul ist absichtlich noch nicht aus `main.rs` aufgerufen (Cutover-Gate).
#![allow(dead_code)]

use chrono::{DateTime, Datelike, Timelike, Utc};
use chrono_tz::Europe::Berlin;
use sqlx::PgPool;
use tb_raid::{compute_scores, PartnerRaidScoreUpsert, ScoreStore, ScoringInputs};

/// Anzahl Tage Lookback für Sessions (identisch zu Python LOOKBACK_DAYS = 45, Z. 19).
const LOOKBACK_DAYS: i64 = 45;

/// Mindest-Sessions für Zuverlässigkeit (Python MIN_RELIABLE_SESSIONS = 3, Z. 20).
const MIN_RELIABLE_SESSIONS: usize = 3;

/// Neutral-Score wenn zu wenig Daten (Python NEUTRAL_SCORE = 0.5, Z. 21).
const NEUTRAL_SCORE: f64 = 0.5;

// ─── Interne Query-Structs ─────────────────────────────────────────────────

/// Aus `twitch_live_state` gelesene Felder für einen Partner.
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct LiveStateRaw {
    twitch_user_id: String,
    /// INTEGER (0/1) in Prod — als Option<i32> gemappt (NULL-safe).
    is_live: Option<i32>,
    /// TEXT-Timestamp in dieser Tabelle (nicht timestamptz!), Prod-verifiziert.
    /// Siehe live_state.rs Zeile 11: "Timestamps in dieser Tabelle sind TEXT".
    last_started_at: Option<String>,
}

/// Aus `twitch_stream_sessions` gelesene Felder — started_at ist timestamptz,
/// duration_seconds INTEGER. Prod-Schema via tb-monitoring sessions/store.rs Z. 3.
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct SessionRaw {
    streamer_login: String,
    started_at: DateTime<Utc>,
    duration_seconds: Option<i32>,
}

/// Aus `twitch_raid_history` gelesene executed_at-Timestamps für einen Partner.
#[derive(Debug, sqlx::FromRow)]
struct RaidTimestampRaw {
    to_broadcaster_id: String,
    executed_at: DateTime<Utc>,
}

/// Zähler aus den drei GROUP-BY-Queries für interne Raid-Metriken.
#[derive(Debug, Default, Clone, Copy)]
struct InternalMetrics {
    sent_30d: i64,
    received_30d: i64,
    received_7d: i64,
}

/// Boost-Zeile aus `streamer_plans` für einen Partner.
/// Python-Herkunft: `_load_boost_flags` Z. 587–625 — inklusive der
/// Entitlement-Katalog-Prüfung auf `"raid.priority"` (siehe [`boost_active`]).
#[derive(Debug, sqlx::FromRow)]
struct BoostFlagRaw {
    twitch_user_id: String,
    raid_boost_enabled: Option<i32>,
    plan_name: Option<String>,
    manual_plan_id: Option<String>,
    /// TEXT-Spalte mit ISO-Timestamp (Python schreibt isoformat).
    manual_plan_expires_at: Option<String>,
}

// ─── Entitlement-Katalog (raid.priority-Teilmenge) ──────────────────────────
//
// 1:1 aus `bot/entitlements/catalog.py` (PLAN_ENTITLEMENTS_MAP +
// LEGACY_PLAN_NAME_TO_ID_MAP). Der Katalog ist dort statischer Code — bei
// Plan-Änderungen BEIDE Stellen pflegen (vollständige Portierung: Schritt 7).

/// Plan-IDs, deren Entitlements `raid.priority` enthalten.
fn plan_id_has_raid_priority(plan_id: &str) -> bool {
    matches!(
        plan_id,
        "raid_boost"
            | "bundle_chat_quiet_raid_boost"
            | "bundle_analysis_raid_boost"
            | "bundle_komplett"
    )
}

/// Legacy-`plan_name` → Plan-ID → raid.priority?
/// (Nur die Teilmenge der Legacy-Map, die auf Pläne mit raid.priority zeigt;
/// alle anderen Namen normalisieren auf Pläne ohne dieses Entitlement.)
fn legacy_plan_name_has_raid_priority(plan_name: &str) -> bool {
    let plan_id = match plan_name {
        "raid_boost" => "raid_boost",
        "chat_quiet_bundle" | "bundle_chat_quiet_raid_boost" => "bundle_chat_quiet_raid_boost",
        "bundle" | "bundle_analysis_raid_boost" => "bundle_analysis_raid_boost",
        "bundle_komplett" => "bundle_komplett",
        _ => return false,
    };
    plan_id_has_raid_priority(plan_id)
}

/// ISO-Timestamp wie Pythons `_parse_dt`: "Z" → +00:00, naive Werte als UTC.
fn parse_plan_expiry(raw: &str) -> Option<DateTime<Utc>> {
    let text = raw.trim();
    if text.is_empty() {
        return None;
    }
    let normalized = text.replace('Z', "+00:00");
    if let Ok(dt) = DateTime::parse_from_rfc3339(&normalized) {
        return Some(dt.with_timezone(&Utc));
    }
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(text, fmt) {
            return Some(naive.and_utc());
        }
    }
    None
}

/// Boost-Entscheidung mit Python-Parität (`_load_boost_flags`):
/// `raid_boost_enabled`-Flag ODER Legacy-Plan-Name mit raid.priority ODER
/// aktiver manueller Plan-Override (nicht abgelaufen) mit raid.priority.
fn boost_active(row: &BoostFlagRaw, now: DateTime<Utc>) -> bool {
    if row.raid_boost_enabled.unwrap_or(0) != 0 {
        return true;
    }
    let plan_name = row
        .plan_name
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if legacy_plan_name_has_raid_priority(&plan_name) {
        return true;
    }
    let manual_plan_id = row
        .manual_plan_id
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if manual_plan_id.is_empty() {
        return false;
    }
    let expires = row
        .manual_plan_expires_at
        .as_deref()
        .and_then(parse_plan_expiry);
    let override_active = match expires {
        None => true,
        Some(ts) => ts >= now,
    };
    override_active && plan_id_has_raid_priority(&manual_plan_id)
}

// ─── Haupt-Resolver ────────────────────────────────────────────────────────

/// Composition-Root-Resolver: lädt alle nötigen Daten, berechnet Scores und
/// schreibt sie per Upsert in `twitch_partner_raid_scores`.
#[derive(Clone)]
pub struct ScoreRefreshResolver {
    pool: PgPool,
    score_store: ScoreStore,
}

impl ScoreRefreshResolver {
    pub fn new(pool: PgPool) -> Self {
        let score_store = ScoreStore::new(pool.clone());
        Self { pool, score_store }
    }

    /// Berechnet Scores für alle übergebenen Partner-IDs und schreibt sie in die DB.
    ///
    /// `partner_user_ids`: Slice aus `(twitch_user_id, twitch_login)`-Paaren.
    /// `now`: Referenz-Zeitpunkt (UTC) — in Tests gepinnt, live = `Utc::now()`.
    ///
    /// Gibt die Anzahl der erfolgreich geschriebenen Score-Zeilen zurück.
    ///
    /// Python-Herkunft: `_refresh_scores_for_ids` Z. 340–395.
    pub async fn refresh_scores(
        &self,
        partner_user_ids: &[(String, String)],
        now: DateTime<Utc>,
    ) -> Result<usize, sqlx::Error> {
        let upserts = self.compute_upserts(partner_user_ids, now).await?;
        let mut written = 0usize;
        for upsert in &upserts {
            self.score_store.upsert(upsert).await?;
            written += 1;
        }
        Ok(written)
    }

    /// Compute-only-Pfad: berechnet die Score-Zeilen ohne zu schreiben.
    /// Genutzt vom `refresh_scores`-Schreibpfad und vom read-only
    /// Prod-Cross-Check (Pre-Cutover-Gate).
    pub async fn compute_upserts(
        &self,
        partner_user_ids: &[(String, String)],
        now: DateTime<Utc>,
    ) -> Result<Vec<PartnerRaidScoreUpsert>, sqlx::Error> {
        if partner_user_ids.is_empty() {
            return Ok(Vec::new());
        }

        let ids: Vec<&str> = partner_user_ids.iter().map(|(id, _)| id.as_str()).collect();
        let logins: Vec<&str> = partner_user_ids.iter().map(|(_, l)| l.as_str()).collect();

        // 1. Alle Datenquellen laden — Reihenfolge entspricht Python _refresh_scores_for_ids
        let live_states = load_live_states(&self.pool, &ids).await?;
        let sessions_by_login = load_sessions(&self.pool, &logins, now).await?;
        let raid_timestamps = load_raid_timestamps(&self.pool, &ids).await?;
        let internal_metrics = load_internal_metrics(&self.pool, &ids, now).await?;
        let boost_flags = load_boost_flags(&self.pool, &ids).await?;
        // Bestehende Scores für den "offline + Cache"-Zweig (Python Z. 738–757):
        // ein nicht-live Partner mit vorhandenem Cache behält seine Score-
        // Komponenten (einfrieren statt mit NEUTRAL neu berechnen).
        let cached_scores = self.score_store.load_many(&ids).await?;

        let lookback_cutoff = now - chrono::Duration::days(LOOKBACK_DAYS);
        let mut upserts = Vec::with_capacity(partner_user_ids.len());

        for (user_id, login) in partner_user_ids {
            let live_row = live_states.iter().find(|r| r.twitch_user_id == *user_id);
            let sessions = sessions_by_login
                .iter()
                .filter(|s| s.streamer_login.to_lowercase() == login.to_lowercase())
                .collect::<Vec<_>>();
            let raids = raid_timestamps
                .iter()
                .filter(|r| r.to_broadcaster_id == *user_id)
                .map(|r| r.executed_at)
                .collect::<Vec<_>>();
            let metrics = internal_metrics
                .get(user_id.as_str())
                .copied()
                .unwrap_or_default();
            let boost = boost_flags
                .iter()
                .find(|b| b.twitch_user_id == *user_id)
                .map(|b| boost_active(b, now))
                .unwrap_or(false);

            let inputs = build_scoring_inputs(&PartnerBuildCtx {
                live_row,
                sessions: &sessions,
                raid_timestamps: &raids,
                metrics,
                raid_boost_enabled: boost,
                now,
                lookback_cutoff,
            });

            let scores = compute_scores(&inputs);

            // is_live-Zweig: current_started_at + current_uptime_sec
            let (is_live_flag, mut current_started_at, mut current_uptime_sec) =
                resolve_live_fields(live_row, now);

            // Score-Komponenten: live → frisch; offline+Cache → eingefroren
            // (Python-Zweig 2); offline ohne Cache → frisch (compute_scores
            // liefert dann bereits NEUTRAL-duration, weil is_live=false).
            let cached = cached_scores.iter().find(|c| c.twitch_user_id == *user_id);
            let (
                duration_score,
                time_pattern_score,
                readiness_score,
                fairness_score,
                base_score,
                final_score,
            ) = match (is_live_flag, cached) {
                (false, Some(c)) => {
                    current_started_at = c.current_started_at.clone();
                    current_uptime_sec = i64::from(c.current_uptime_sec);
                    (
                        c.duration_score,
                        c.time_pattern_score,
                        c.readiness_score,
                        c.fairness_score,
                        c.base_score,
                        c.final_score,
                    )
                }
                _ => (
                    scores.duration_score,
                    scores.time_pattern_score,
                    scores.readiness_score,
                    scores.fairness_score,
                    scores.base_score,
                    scores.final_score,
                ),
            };

            let upsert = PartnerRaidScoreUpsert {
                twitch_user_id: user_id.clone(),
                twitch_login: login.clone(),
                avg_duration_sec: inputs.avg_duration_sec as i32,
                time_pattern_score_base: inputs.time_pattern_score_base,
                received_successful_raids_total: inputs.received_successful_raids_total as i32,
                is_new_partner_preferred: if scores.is_new_partner_preferred {
                    1
                } else {
                    0
                },
                new_partner_multiplier: scores.new_partner_multiplier,
                raid_boost_multiplier: scores.raid_boost_multiplier,
                is_live: if is_live_flag { 1 } else { 0 },
                current_started_at,
                current_uptime_sec: current_uptime_sec as i32,
                duration_score,
                time_pattern_score,
                readiness_score,
                fairness_score,
                base_score,
                final_score,
                internal_sent_raids_30d: metrics.sent_30d as i32,
                internal_received_raids_30d: metrics.received_30d as i32,
                internal_received_raids_7d: metrics.received_7d as i32,
                today_received_raids: inputs.today_received_raids as i32,
                last_computed_at: now.format("%Y-%m-%dT%H:%M:%S+00:00").to_string(),
            };

            upserts.push(upsert);
        }

        Ok(upserts)
    }
}

// ─── Input-Berechnung ──────────────────────────────────────────────────────

/// Aggregierte Eingaben für einen einzelnen Partner — fasst die vielen
/// Parameter von `build_scoring_inputs` zusammen.
struct PartnerBuildCtx<'a> {
    live_row: Option<&'a LiveStateRaw>,
    sessions: &'a [&'a SessionRaw],
    raid_timestamps: &'a [DateTime<Utc>],
    metrics: InternalMetrics,
    raid_boost_enabled: bool,
    now: DateTime<Utc>,
    lookback_cutoff: DateTime<Utc>,
}

/// Berechnet `ScoringInputs` aus den Rohdaten eines einzelnen Partners.
///
/// Python-Herkunft: `_build_score` Z. 656–770. Logik 1:1 portiert.
fn build_scoring_inputs(ctx: &PartnerBuildCtx<'_>) -> ScoringInputs {
    let PartnerBuildCtx {
        live_row,
        sessions,
        raid_timestamps,
        metrics,
        raid_boost_enabled,
        now,
        lookback_cutoff,
    } = ctx;
    let live_row = *live_row;
    let now = *now;
    let lookback_cutoff = *lookback_cutoff;
    let metrics = *metrics;
    let raid_boost_enabled = *raid_boost_enabled;
    // Sicherheitsnetz: SQL filtert bereits auf started_at >= cutoff (load_sessions),
    // dieser Filter kann keine zusätzlichen Rows mehr wegwerfen. Er bleibt,
    // um gegen versehentliche Query-Änderungen abzusichern (Python Z. 673–682).
    let recent_sessions: Vec<&&SessionRaw> = sessions
        .iter()
        .filter(|s| s.started_at >= lookback_cutoff)
        .collect();

    let (avg_duration_sec, duration_history_reliable) = compute_avg_duration(&recent_sessions);

    let (time_pattern_score_base, time_pattern_reliable) =
        compute_time_pattern(&recent_sessions, now);

    let today_received_raids = count_today_raids(raid_timestamps, now);

    let (is_live, current_uptime_sec) = {
        let live = live_row.and_then(|r| {
            let is_live = r.is_live.unwrap_or(0) != 0;
            if is_live {
                r.last_started_at.as_deref().and_then(parse_text_timestamp)
            } else {
                None
            }
        });
        match live {
            Some(started_at) => {
                let uptime = (now - started_at).num_seconds().max(0);
                (true, uptime)
            }
            None => (false, 0i64),
        }
    };

    ScoringInputs {
        avg_duration_sec,
        time_pattern_score_base,
        time_pattern_reliable,
        is_live,
        current_uptime_sec,
        duration_history_reliable,
        received_successful_raids_total: raid_timestamps.len() as i64,
        raid_boost_enabled,
        internal_sent_raids_30d: metrics.sent_30d,
        internal_received_raids_30d: metrics.received_30d,
        internal_received_raids_7d: metrics.received_7d,
        today_received_raids: today_received_raids as i64,
    }
}

/// Gibt `(is_live, current_started_at_iso, current_uptime_sec)` zurück.
///
/// Python-Herkunft: `_build_score` Z. 716–720.
/// Nur wenn is_live=true UND last_started_at parsebar: echte Uptime.
/// Sonst: is_live=false, started_at=None, uptime=0.
fn resolve_live_fields(
    live_row: Option<&LiveStateRaw>,
    now: DateTime<Utc>,
) -> (bool, Option<String>, i64) {
    let Some(row) = live_row else {
        return (false, None, 0);
    };
    let is_live = row.is_live.unwrap_or(0) != 0;
    if !is_live {
        return (false, None, 0);
    }
    let Some(started_at) = row
        .last_started_at
        .as_deref()
        .and_then(parse_text_timestamp)
    else {
        return (false, None, 0);
    };
    let uptime = (now - started_at).num_seconds().max(0);
    // ISO-UTC-String wie Python _iso_utc: "2026-06-09T18:00:00+00:00"
    let started_iso = started_at.format("%Y-%m-%dT%H:%M:%S+00:00").to_string();
    (true, Some(started_iso), uptime)
}

// ─── Reine Hilfsfunktionen (unit-getestet) ─────────────────────────────────

/// Berechnet Durchschnittsdauer und Reliability-Flag.
///
/// Python-Herkunft: `_build_score` Z. 677–683.
/// - `avg_duration_sec` = round(mean(durations)), 0 wenn leer.
/// - `duration_history_reliable` = #sessions >= MIN_RELIABLE_SESSIONS UND avg > 0.
///   WICHTIG: Python filtert `duration_seconds > 0` VOR dem Zählen — Null-Sessions
///   erhöhen also NICHT die Zähler, senken aber auch nicht die Reliability-Grenze.
pub(crate) fn compute_avg_duration(sessions: &[&&SessionRaw]) -> (i64, bool) {
    let durations: Vec<i64> = sessions
        .iter()
        .filter_map(|s| s.duration_seconds)
        .filter(|&d| d > 0)
        .map(|d| d as i64)
        .collect();

    if durations.is_empty() {
        return (0, false);
    }
    let avg = (durations.iter().sum::<i64>() as f64 / durations.len() as f64).round() as i64;
    let reliable = durations.len() >= MIN_RELIABLE_SESSIONS && avg > 0;
    (avg, reliable)
}

/// Berechnet `time_pattern_score_base` und `time_pattern_reliable`.
///
/// Python-Herkunft: `_build_score` Z. 685–696.
/// Logik: Anteil der Sessions, deren Berlin-Wochentag UND Berlin-Stunde mit
/// `now` (Berlin) übereinstimmen. Nur wenn #recent_started >= MIN_RELIABLE_SESSIONS;
/// sonst NEUTRAL_SCORE + reliable=false.
///
/// WICHTIG: Der Vergleich ist exakt Wochentag (0=Mo..6=So) UND volle Stunde —
/// NICHT Tageszeit-Window. Entspricht Python weekday() == now.weekday() AND hour == now.hour.
pub(crate) fn compute_time_pattern(sessions: &[&&SessionRaw], now: DateTime<Utc>) -> (f64, bool) {
    if sessions.len() < MIN_RELIABLE_SESSIONS {
        return (NEUTRAL_SCORE, false);
    }
    let now_berlin = now.with_timezone(&Berlin);
    let now_weekday = now_berlin.weekday().num_days_from_monday(); // 0=Mo
    let now_hour = now_berlin.hour();

    let matching = sessions
        .iter()
        .filter(|s| {
            let s_berlin = s.started_at.with_timezone(&Berlin);
            s_berlin.weekday().num_days_from_monday() == now_weekday && s_berlin.hour() == now_hour
        })
        .count();

    let score = (matching as f64 / sessions.len() as f64 * 1_000_000.0).round() / 1_000_000.0;
    (score, true)
}

/// Zählt Raids, deren `executed_at` (in Berlin-Zeit) == heute (Berlin-Datum).
///
/// Python-Herkunft: `_build_score` Z. 703–706 + `_today_in_berlin` Z. 244–245.
/// WICHTIG: "heute" ist das aktuelle Datum in Europe/Berlin, nicht UTC.
/// Ein Raid um 23:30 UTC kann in Berlin schon der nächste Tag sein.
pub(crate) fn count_today_raids(timestamps: &[DateTime<Utc>], now: DateTime<Utc>) -> usize {
    let today_berlin = now.with_timezone(&Berlin).date_naive();
    timestamps
        .iter()
        .filter(|ts| ts.with_timezone(&Berlin).date_naive() == today_berlin)
        .count()
}

/// Parst einen TEXT-Timestamp aus `twitch_live_state.last_started_at`.
///
/// Diese Spalte enthält ISO-8601-Strings (z. B. "2026-06-09T18:00:00+00:00"),
/// KEINE timestamptz — Prod-verifiziert (live_state.rs Z. 11).
/// Gibt `None` zurück wenn leer, NULL oder nicht parsebar.
fn parse_text_timestamp(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    // Versuche zuerst RFC3339 (normale Form), dann ISO ohne Offset
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    // Fallback: naive Datetime ohne TZ → als UTC interpretieren
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Some(naive.and_utc());
    }
    None
}

// ─── DB-Ladefunktionen ─────────────────────────────────────────────────────

/// Lädt Live-State-Zeilen für alle übergebenen user_ids.
///
/// Python-Herkunft: `_load_live_state` Z. ~430–450.
async fn load_live_states(
    pool: &PgPool,
    user_ids: &[&str],
) -> Result<Vec<LiveStateRaw>, sqlx::Error> {
    if user_ids.is_empty() {
        return Ok(vec![]);
    }
    // sqlx unterstützt kein IN ($1) mit Vec direkt — ANY-Syntax
    sqlx::query_as::<_, LiveStateRaw>(
        "SELECT twitch_user_id, is_live, last_started_at \
         FROM twitch_live_state \
         WHERE twitch_user_id = ANY($1)",
    )
    .bind(user_ids)
    .fetch_all(pool)
    .await
}

/// Lädt Sessions für alle Partner-Logins — MIT Lookback-Filter in SQL
/// (Abweichung von Python, das alle Sessions lädt und per Code filtert).
/// Wir ziehen den Filter in die DB, weil bei wachsender `twitch_stream_sessions`-
/// Tabelle sonst unnötige Datenmengen übertragen werden. Das Ergebnis ist
/// bit-identisch: der Code-seitige Filter in `build_scoring_inputs` bleibt
/// als Sicherheitsnetz erhalten, kann aber keine zusätzlichen Rows mehr
/// wegwerfen, solange beide denselben `LOOKBACK_DAYS`-Wert nutzen.
///
/// Semantik: `started_at >= cutoff` (inklusiv, spiegelt `>= lookback_cutoff`
/// in Z. 374 exakt wider). `cutoff` wird als gebundener Timestamp-Parameter
/// übergeben — typsicher, kein Casting/String-Interpolation nötig.
///
/// Python-Herkunft: `_load_sessions` Z. ~460–480.
/// Lookup ist case-insensitive (Python: `LOWER(streamer_login) IN (...)`).
/// LOWER(streamer_login) = ANY($1) sabotiert prinzipiell einen einfachen
/// B-Tree-Index auf `streamer_login`; da die Spalte textlich klein ist und
/// der Filter hauptsächlich über `started_at` schneidet, ist das vertretbar.
/// Ein funktionaler Index `ON twitch_stream_sessions (LOWER(streamer_login))`
/// würde helfen, ist aber außerhalb dieses Scope.
async fn load_sessions(
    pool: &PgPool,
    logins: &[&str],
    now: DateTime<Utc>,
) -> Result<Vec<SessionRaw>, sqlx::Error> {
    if logins.is_empty() {
        return Ok(vec![]);
    }
    let lower_logins: Vec<String> = logins.iter().map(|l| l.to_lowercase()).collect();
    let cutoff = now - chrono::Duration::days(LOOKBACK_DAYS);
    sqlx::query_as::<_, SessionRaw>(
        "SELECT streamer_login, started_at, duration_seconds \
         FROM twitch_stream_sessions \
         WHERE LOWER(streamer_login) = ANY($1) \
           AND started_at >= $2",
    )
    .bind(&lower_logins)
    .bind(cutoff)
    .fetch_all(pool)
    .await
}

/// Lädt alle erfolgreichen Raid-executed_at-Timestamps für die Partner-IDs.
///
/// Python-Herkunft: `_load_raid_timestamps` Z. 499–517.
/// Query: `to_broadcaster_id IN (partner_set) AND COALESCE(success, FALSE) IS TRUE`.
/// WICHTIG: Es gibt KEINE zeitliche Einschränkung — `received_successful_raids_total`
/// ist die Gesamtanzahl aller Raids, nicht 30d.
async fn load_raid_timestamps(
    pool: &PgPool,
    user_ids: &[&str],
) -> Result<Vec<RaidTimestampRaw>, sqlx::Error> {
    if user_ids.is_empty() {
        return Ok(vec![]);
    }
    sqlx::query_as::<_, RaidTimestampRaw>(
        "SELECT to_broadcaster_id, executed_at \
         FROM twitch_raid_history \
         WHERE to_broadcaster_id = ANY($1) \
           AND COALESCE(success, FALSE) IS TRUE",
    )
    .bind(user_ids)
    .fetch_all(pool)
    .await
}

/// Lädt interne Raid-Metriken (sent_30d, received_30d, received_7d) für alle Partner.
///
/// Python-Herkunft: `_load_internal_raid_metrics` Z. 518–600.
/// Drei separate GROUP-BY-Queries:
/// 1. sent_30d:     GROUP BY from_broadcaster_id (Partner hat gesendet)
/// 2. received_30d: GROUP BY to_broadcaster_id   (Partner hat empfangen, 30d)
/// 3. received_7d:  GROUP BY to_broadcaster_id   (Partner hat empfangen, 7d)
///
/// WICHTIG: Beide Seiten (from UND to) müssen im Partner-Set sein — das entspricht
/// dem internen Netzwerk. Raids von/zu Externen zählen nicht.
async fn load_internal_metrics(
    pool: &PgPool,
    user_ids: &[&str],
    now: DateTime<Utc>,
) -> Result<std::collections::HashMap<String, InternalMetrics>, sqlx::Error> {
    let mut map: std::collections::HashMap<String, InternalMetrics> = user_ids
        .iter()
        .map(|id| (id.to_string(), InternalMetrics::default()))
        .collect();

    let cutoff_30d = now - chrono::Duration::days(30);
    let cutoff_7d = now - chrono::Duration::days(7);

    // Query 1: sent_30d — wer hat wie viele Raids gesendet?
    let sent_rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT from_broadcaster_id, COUNT(*)::bigint AS cnt \
         FROM twitch_raid_history \
         WHERE COALESCE(success, FALSE) IS TRUE \
           AND from_broadcaster_id = ANY($1) \
           AND to_broadcaster_id   = ANY($1) \
           AND executed_at >= $2 \
         GROUP BY from_broadcaster_id",
    )
    .bind(user_ids)
    .bind(cutoff_30d)
    .fetch_all(pool)
    .await?;
    for (uid, cnt) in sent_rows {
        if let Some(m) = map.get_mut(&uid) {
            m.sent_30d = cnt;
        }
    }

    // Query 2: received_30d
    let recv_30d_rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT to_broadcaster_id, COUNT(*)::bigint AS cnt \
         FROM twitch_raid_history \
         WHERE COALESCE(success, FALSE) IS TRUE \
           AND from_broadcaster_id = ANY($1) \
           AND to_broadcaster_id   = ANY($1) \
           AND executed_at >= $2 \
         GROUP BY to_broadcaster_id",
    )
    .bind(user_ids)
    .bind(cutoff_30d)
    .fetch_all(pool)
    .await?;
    for (uid, cnt) in recv_30d_rows {
        if let Some(m) = map.get_mut(&uid) {
            m.received_30d = cnt;
        }
    }

    // Query 3: received_7d
    let recv_7d_rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT to_broadcaster_id, COUNT(*)::bigint AS cnt \
         FROM twitch_raid_history \
         WHERE COALESCE(success, FALSE) IS TRUE \
           AND from_broadcaster_id = ANY($1) \
           AND to_broadcaster_id   = ANY($1) \
           AND executed_at >= $2 \
         GROUP BY to_broadcaster_id",
    )
    .bind(user_ids)
    .bind(cutoff_7d)
    .fetch_all(pool)
    .await?;
    for (uid, cnt) in recv_7d_rows {
        if let Some(m) = map.get_mut(&uid) {
            m.received_7d = cnt;
        }
    }

    Ok(map)
}

/// Lädt Boost-Flags aus `streamer_plans`.
///
/// Python-Herkunft: `_load_boost_flags` Z. 587–625.
/// Vereinfachung: Nur `raid_boost_enabled`-Spalte, kein Entitlement-Katalog.
/// Fehlende Zeilen → boost = false (kein Plan = kein Boost).
async fn load_boost_flags(
    pool: &PgPool,
    user_ids: &[&str],
) -> Result<Vec<BoostFlagRaw>, sqlx::Error> {
    if user_ids.is_empty() {
        return Ok(vec![]);
    }
    sqlx::query_as::<_, BoostFlagRaw>(
        "SELECT twitch_user_id, COALESCE(raid_boost_enabled, 0) AS raid_boost_enabled, \
                plan_name, manual_plan_id, manual_plan_expires_at \
         FROM streamer_plans \
         WHERE twitch_user_id = ANY($1)",
    )
    .bind(user_ids)
    .fetch_all(pool)
    .await
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    // ─── Unit-Tests: Boost-Entitlement (Python-Parität _load_boost_flags) ───

    fn boost_row(
        enabled: i32,
        plan_name: &str,
        manual_plan_id: &str,
        expires: Option<&str>,
    ) -> BoostFlagRaw {
        BoostFlagRaw {
            twitch_user_id: "1".into(),
            raid_boost_enabled: Some(enabled),
            plan_name: Some(plan_name.into()),
            manual_plan_id: Some(manual_plan_id.into()),
            manual_plan_expires_at: expires.map(str::to_string),
        }
    }

    #[test]
    fn boost_aus_db_flag() {
        let now = Utc.with_ymd_and_hms(2026, 6, 10, 12, 0, 0).unwrap();
        assert!(boost_active(&boost_row(1, "", "", None), now));
        assert!(!boost_active(&boost_row(0, "", "", None), now));
    }

    #[test]
    fn boost_aus_legacy_plan_name() {
        let now = Utc.with_ymd_and_hms(2026, 6, 10, 12, 0, 0).unwrap();
        // Großschreibung wird wie in Python vor dem Lookup normalisiert.
        assert!(boost_active(&boost_row(0, "Bundle", "", None), now));
        assert!(boost_active(&boost_row(0, "raid_boost", "", None), now));
        assert!(boost_active(&boost_row(0, "chat_quiet_bundle", "", None), now));
        // Pläne ohne raid.priority-Entitlement:
        assert!(!boost_active(&boost_row(0, "werbefrei", "", None), now));
        assert!(!boost_active(&boost_row(0, "analysis", "", None), now));
        assert!(!boost_active(&boost_row(0, "unbekannt", "", None), now));
    }

    #[test]
    fn boost_aus_manuellem_plan_override() {
        let now = Utc.with_ymd_and_hms(2026, 6, 10, 12, 0, 0).unwrap();
        // Ohne Ablauf: aktiv.
        assert!(boost_active(&boost_row(0, "", "bundle_komplett", None), now));
        // Zukunft: aktiv; Vergangenheit: abgelaufen.
        assert!(boost_active(
            &boost_row(0, "", "bundle_komplett", Some("2026-12-31T00:00:00+00:00")),
            now
        ));
        assert!(!boost_active(
            &boost_row(0, "", "bundle_komplett", Some("2026-01-01T00:00:00Z")),
            now
        ));
        // Plan ohne raid.priority bleibt aus, auch wenn aktiv.
        assert!(!boost_active(&boost_row(0, "", "analysis_dashboard", None), now));
    }

    #[test]
    fn plan_expiry_parsing_wie_python() {
        // RFC3339, Z-Suffix und naive ISO-Formate (Python fromisoformat).
        assert!(parse_plan_expiry("2026-06-10T12:00:00+00:00").is_some());
        assert!(parse_plan_expiry("2026-06-10T12:00:00Z").is_some());
        assert!(parse_plan_expiry("2026-06-10T12:00:00").is_some());
        assert!(parse_plan_expiry("2026-06-10 12:00:00").is_some());
        assert!(parse_plan_expiry("").is_none());
        assert!(parse_plan_expiry("kaputt").is_none());
    }

    // ─── Unit-Tests: reine Funktionen ──────────────────────────────────────

    /// Hilfsfunktion: baut minimale SessionRaw ohne DB
    fn make_session(
        login: &str,
        started_at: DateTime<Utc>,
        duration_seconds: Option<i32>,
    ) -> SessionRaw {
        SessionRaw {
            streamer_login: login.to_string(),
            started_at,
            duration_seconds,
        }
    }

    #[test]
    fn avg_duration_leer_gibt_null_und_unreliable() {
        let sessions: Vec<&&SessionRaw> = vec![];
        let (avg, reliable) = compute_avg_duration(&sessions);
        assert_eq!(avg, 0);
        assert!(!reliable);
    }

    #[test]
    fn avg_duration_unter_min_reliable_sessions_ist_unreliable() {
        let s1 = make_session("alice", Utc::now(), Some(3600));
        let s2 = make_session("alice", Utc::now(), Some(7200));
        let refs = [&&s1, &&s2];
        let (avg, reliable) = compute_avg_duration(&refs);
        assert_eq!(avg, 5400);
        assert!(!reliable); // 2 < MIN_RELIABLE_SESSIONS(3)
    }

    #[test]
    fn avg_duration_drei_sessions_ist_reliable() {
        let s1 = make_session("alice", Utc::now(), Some(3600));
        let s2 = make_session("alice", Utc::now(), Some(7200));
        let s3 = make_session("alice", Utc::now(), Some(5400));
        let refs = [&&s1, &&s2, &&s3];
        let (avg, reliable) = compute_avg_duration(&refs);
        assert_eq!(avg, 5400);
        assert!(reliable);
    }

    #[test]
    fn avg_duration_ignoriert_null_seconds() {
        // NULL-Sessions erhöhen nicht den Zähler (Python: if duration_seconds > 0)
        let s1 = make_session("alice", Utc::now(), None);
        let s2 = make_session("alice", Utc::now(), Some(3600));
        let s3 = make_session("alice", Utc::now(), Some(7200));
        let refs = [&&s1, &&s2, &&s3];
        // Nur 2 valide Durations → unreliable, aber avg korrekt
        let (avg, reliable) = compute_avg_duration(&refs);
        assert_eq!(avg, 5400);
        assert!(!reliable); // nur 2 valide Durations
    }

    #[test]
    fn time_pattern_zu_wenig_sessions_gibt_neutral() {
        let s1 = make_session("alice", Utc::now(), Some(3600));
        let s2 = make_session("alice", Utc::now(), Some(3600));
        let refs = [&&s1, &&s2];
        let (score, reliable) = compute_time_pattern(&refs, Utc::now());
        assert!((score - NEUTRAL_SCORE).abs() < 1e-9);
        assert!(!reliable);
    }

    #[test]
    fn time_pattern_alle_matching_gibt_1_0() {
        // Gepinnter now: Mittwoch 2026-06-10 20:00 UTC = 22:00 Berlin (CEST)
        // Berlin: weekday=2 (Mittwoch), hour=22
        let now = Utc.with_ymd_and_hms(2026, 6, 10, 20, 0, 0).unwrap();
        // Sessions alle Mittwoch 20:00 UTC (= 22:00 Berlin)
        let ts = Utc.with_ymd_and_hms(2026, 6, 3, 20, 0, 0).unwrap();
        let s1 = make_session("alice", ts, Some(3600));
        let s2 = make_session("alice", ts, Some(3600));
        let s3 = make_session("alice", ts, Some(3600));
        let refs = [&&s1, &&s2, &&s3];
        let (score, reliable) = compute_time_pattern(&refs, now);
        assert!(reliable);
        assert!((score - 1.0).abs() < 1e-6, "erwartet 1.0, war {score}");
    }

    #[test]
    fn time_pattern_keine_matching_gibt_0_0() {
        // now: Mittwoch 22:00 Berlin
        let now = Utc.with_ymd_and_hms(2026, 6, 10, 20, 0, 0).unwrap();
        // Sessions alle Donnerstag 10:00 UTC (= 12:00 Berlin → anderer Wochentag)
        let ts = Utc.with_ymd_and_hms(2026, 6, 11, 8, 0, 0).unwrap();
        let s1 = make_session("alice", ts, Some(3600));
        let s2 = make_session("alice", ts, Some(3600));
        let s3 = make_session("alice", ts, Some(3600));
        let refs = [&&s1, &&s2, &&s3];
        let (score, reliable) = compute_time_pattern(&refs, now);
        assert!(reliable);
        assert!((score - 0.0).abs() < 1e-6, "erwartet 0.0, war {score}");
    }

    /// Stellt sicher, dass der Code-seitige Lookback-Filter (Sicherheitsnetz)
    /// das 45-Tage-Fenster korrekt abschneidet — Session knapp innerhalb vs.
    /// knapp außerhalb der Grenze.
    #[test]
    fn lookback_filter_grenze_korrekt() {
        let now = Utc.with_ymd_and_hms(2026, 6, 11, 12, 0, 0).unwrap();
        let lookback_cutoff = now - chrono::Duration::days(LOOKBACK_DAYS);

        // Knapp innerhalb: genau auf der Grenze (>= cutoff → soll enthalten sein)
        let s_grenze = make_session("alice", lookback_cutoff, Some(3600));
        // Knapp außerhalb: eine Sekunde vor der Grenze (soll herausgefiltert werden)
        let s_aussen = make_session("alice", lookback_cutoff - chrono::Duration::seconds(1), Some(3600));
        // Normal innerhalb
        let s_innen = make_session("alice", now - chrono::Duration::days(1), Some(3600));

        let all = [&s_grenze, &s_aussen, &s_innen];
        let recent: Vec<_> = all.iter().filter(|s| s.started_at >= lookback_cutoff).collect();

        assert_eq!(recent.len(), 2, "Grenz-Session und innere Session sollen enthalten sein");
        assert!(recent.iter().any(|s| s.started_at == lookback_cutoff), "Grenz-Session (>=) muss drin sein");
        assert!(!recent.iter().any(|s| s.started_at == s_aussen.started_at), "Session außerhalb muss raus");
    }

    #[test]
    fn count_today_raids_berlin_datum_korrekt() {
        // now: 2026-06-10 22:00 UTC = 2026-06-11 00:00 Berlin (CEST, UTC+2)
        // → "heute Berlin" ist der 11. Juni
        let now = Utc.with_ymd_and_hms(2026, 6, 10, 22, 0, 0).unwrap();
        let ts_heute_berlin = Utc.with_ymd_and_hms(2026, 6, 10, 23, 0, 0).unwrap(); // 01:00 Berlin = 11. Juni
        let ts_gestern_berlin = Utc.with_ymd_and_hms(2026, 6, 10, 20, 0, 0).unwrap(); // 22:00 Berlin = 10. Juni
        let timestamps = vec![ts_heute_berlin, ts_gestern_berlin];
        let count = count_today_raids(&timestamps, now);
        assert_eq!(count, 1, "nur ein Raid hat Berlin-Datum == heute");
    }

    #[test]
    fn count_today_raids_leer_gibt_null() {
        let now = Utc.with_ymd_and_hms(2026, 6, 10, 12, 0, 0).unwrap();
        assert_eq!(count_today_raids(&[], now), 0);
    }

    // ─── Integrationstests gegen echte DB ──────────────────────────────────

    /// Setup-Helfer: erstellt ein Schema pro Test (schema-isoliert).
    #[cfg(feature = "integration")]
    async fn setup_test_db(schema: &str) -> PgPool {
        use std::str::FromStr;
        let url = std::env::var("TB_TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:tbtest@127.0.0.1:5434/postgres".to_string());
        // Schema anlegen über eine Admin-Verbindung …
        let admin = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("DB-Verbindung fehlgeschlagen");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        // … und ALLE Pool-Connections auf search_path setzen (echte Isolation,
        // nicht nur eine Connection wie bei `SET search_path`).
        let opts = sqlx::postgres::PgConnectOptions::from_str(&url)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await
            .expect("DB-Verbindung fehlgeschlagen");

        // DDL nach Prod-Typen
        sqlx::query(
            r#"
            CREATE TABLE twitch_live_state (
                twitch_user_id TEXT PRIMARY KEY,
                streamer_login TEXT NOT NULL DEFAULT '',
                is_live        INTEGER NOT NULL DEFAULT 0,
                last_started_at TEXT,
                last_seen_at   TEXT,
                last_stream_id TEXT,
                last_title     TEXT,
                last_game_id   TEXT,
                last_game      TEXT,
                last_viewer_count INTEGER,
                last_tracking_token TEXT,
                active_session_id   BIGINT,
                had_deadlock_in_session INTEGER,
                last_deadlock_seen_at   TEXT,
                last_discord_message_id TEXT,
                last_notified_at        TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            CREATE TABLE twitch_stream_sessions (
                id               BIGSERIAL PRIMARY KEY,
                streamer_login   TEXT NOT NULL,
                stream_id        TEXT,
                started_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                ended_at         TIMESTAMPTZ,
                duration_seconds INTEGER,
                is_automatic     BOOLEAN DEFAULT FALSE,
                start_viewers    INTEGER,
                peak_viewers     INTEGER,
                end_viewers      INTEGER,
                avg_viewers      NUMERIC,
                samples          INTEGER DEFAULT 0
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            CREATE TABLE twitch_raid_history (
                id                   BIGSERIAL PRIMARY KEY,
                from_broadcaster_id  TEXT NOT NULL,
                to_broadcaster_id    TEXT NOT NULL,
                executed_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                success              BOOLEAN
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            CREATE TABLE streamer_plans (
                twitch_user_id       TEXT PRIMARY KEY,
                raid_boost_enabled   INTEGER DEFAULT 0,
                plan_name            TEXT,
                manual_plan_id       TEXT,
                manual_plan_expires_at TIMESTAMPTZ
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            CREATE TABLE twitch_partner_raid_scores (
                twitch_user_id               TEXT PRIMARY KEY,
                twitch_login                 TEXT NOT NULL DEFAULT '',
                avg_duration_sec             INTEGER NOT NULL DEFAULT 0,
                time_pattern_score_base      DOUBLE PRECISION NOT NULL DEFAULT 0.5,
                received_successful_raids_total INTEGER NOT NULL DEFAULT 0,
                is_new_partner_preferred     INTEGER NOT NULL DEFAULT 0,
                new_partner_multiplier       DOUBLE PRECISION NOT NULL DEFAULT 1.0,
                raid_boost_multiplier        DOUBLE PRECISION NOT NULL DEFAULT 1.0,
                is_live                      INTEGER NOT NULL DEFAULT 0,
                current_started_at           TEXT,
                current_uptime_sec           INTEGER NOT NULL DEFAULT 0,
                duration_score               DOUBLE PRECISION NOT NULL DEFAULT 0.5,
                time_pattern_score           DOUBLE PRECISION NOT NULL DEFAULT 0.5,
                readiness_score              DOUBLE PRECISION NOT NULL DEFAULT 0.5,
                fairness_score               DOUBLE PRECISION NOT NULL DEFAULT 0.5,
                base_score                   DOUBLE PRECISION NOT NULL DEFAULT 0.5,
                final_score                  DOUBLE PRECISION NOT NULL DEFAULT 0.5,
                internal_sent_raids_30d      INTEGER NOT NULL DEFAULT 0,
                internal_received_raids_30d  INTEGER NOT NULL DEFAULT 0,
                internal_received_raids_7d   INTEGER NOT NULL DEFAULT 0,
                today_received_raids         INTEGER NOT NULL DEFAULT 0,
                last_computed_at             TEXT NOT NULL DEFAULT ''
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        pool
    }

    /// Voller DB-Pfad: schreibt Scores für einen live + einen offline Partner
    /// und beweist den Cache-Freeze (offline + Cache → Score-Komponenten bleiben).
    #[cfg(feature = "integration")]
    #[tokio::test]
    async fn refresh_scores_schreibt_und_friert_offline_cache_ein() {
        let pool = setup_test_db("t6f_score_refresh").await;
        let now = chrono::DateTime::parse_from_rfc3339("2026-06-10T18:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        // Partner A (live): Start vor 1h → uptime; eine Session (Dauer) als History.
        sqlx::query(
            "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_started_at)
             VALUES ('100', 'a', 1, '2026-06-10T17:00:00+00:00')",
        ).execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (streamer_login, started_at, duration_seconds)
             VALUES ('a', NOW() - INTERVAL '2 days', 7200)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Partner B (offline) mit vorhandenem Cache-Score (final_score 0.99).
        sqlx::query(
            "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live) VALUES ('200', 'b', 0)",
        ).execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO twitch_partner_raid_scores
                (twitch_user_id, twitch_login, duration_score, time_pattern_score,
                 readiness_score, fairness_score, base_score, final_score, last_computed_at)
             VALUES ('200', 'b', 0.91, 0.92, 0.93, 0.94, 0.95, 0.99, 'alt')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let resolver = ScoreRefreshResolver::new(pool.clone());
        let written = resolver
            .refresh_scores(
                &[("100".into(), "a".into()), ("200".into(), "b".into())],
                now,
            )
            .await
            .unwrap();
        assert_eq!(written, 2);

        // Partner A: frisch berechnet, is_live=1.
        let (a_live, a_final): (i32, f64) = sqlx::query_as(
            "SELECT is_live, final_score FROM twitch_partner_raid_scores WHERE twitch_user_id='100'",
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(a_live, 1);
        assert!((0.0..=2.0).contains(&a_final));

        // Partner B: offline + Cache → final_score EINGEFROREN auf 0.99.
        let (b_live, b_final, b_dur): (i32, f64, f64) = sqlx::query_as(
            "SELECT is_live, final_score, duration_score FROM twitch_partner_raid_scores WHERE twitch_user_id='200'",
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(b_live, 0);
        assert_eq!(b_final, 0.99, "offline+Cache: final_score eingefroren");
        assert_eq!(b_dur, 0.91, "offline+Cache: duration_score eingefroren");
    }
}

// ─── Prod-Cross-Check (Pre-Cutover-Gate) ────────────────────────────────────

/// Read-only-Vergleich gegen eine ECHTE Datenbank: Rust-Compute vs. die von
/// Python geschriebenen `twitch_partner_raid_scores`-Zeilen. Läuft nur, wenn
/// `TB_CROSSCHECK_DATABASE_URL` gesetzt ist (bewusstes Opt-in), und öffnet die
/// Verbindung mit `default_transaction_read_only=on` — Schreiben ist damit
/// auf Verbindungsebene unmöglich.
///
/// Der Test FAILT nicht auf Daten-Drift (live Werte ändern sich laufend) —
/// er druckt den Report (`--nocapture`), der vor dem Flip manuell geprüft
/// wird. Offline-Partner mit Cache müssen exakt matchen (Freeze-Zweig).
#[cfg(all(test, feature = "integration"))]
mod prod_crosscheck {
    use std::str::FromStr;

    use chrono::Utc;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

    use super::ScoreRefreshResolver;

    #[tokio::test]
    async fn report_rust_vs_python_scores() {
        let Some(dsn) = std::env::var("TB_CROSSCHECK_DATABASE_URL").ok() else {
            eprintln!("SKIP: TB_CROSSCHECK_DATABASE_URL nicht gesetzt");
            return;
        };
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("default_transaction_read_only", "on")]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();

        #[derive(sqlx::FromRow)]
        struct PyRow {
            twitch_user_id: String,
            twitch_login: String,
            final_score: f64,
            base_score: f64,
            duration_score: f64,
            time_pattern_score: f64,
            new_partner_multiplier: f64,
            raid_boost_multiplier: f64,
            is_live: i32,
            today_received_raids: i32,
            received_successful_raids_total: i32,
            last_computed_at: String,
        }
        let py_rows: Vec<PyRow> = sqlx::query_as(
            "SELECT twitch_user_id, twitch_login, final_score, base_score,
                    duration_score, time_pattern_score, new_partner_multiplier,
                    raid_boost_multiplier, is_live, today_received_raids,
                    received_successful_raids_total, last_computed_at
               FROM twitch_partner_raid_scores ORDER BY twitch_login",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert!(!py_rows.is_empty(), "keine Python-Score-Zeilen gefunden");

        let pairs: Vec<(String, String)> = py_rows
            .iter()
            .map(|r| (r.twitch_user_id.clone(), r.twitch_login.clone()))
            .collect();
        let computed = ScoreRefreshResolver::new(pool.clone())
            .compute_upserts(&pairs, Utc::now())
            .await
            .unwrap();

        let mut exact = 0usize;
        let mut nah = 0usize;
        let mut diffs = Vec::new();
        for py in &py_rows {
            let Some(rs) = computed
                .iter()
                .find(|c| c.twitch_user_id == py.twitch_user_id)
            else {
                diffs.push(format!("{}: keine Rust-Berechnung", py.twitch_login));
                continue;
            };
            let d_final = (rs.final_score - py.final_score).abs();
            let d_base = (rs.base_score - py.base_score).abs();
            let live_match = rs.is_live == py.is_live;
            if d_final < 1e-9 && d_base < 1e-9 && live_match {
                exact += 1;
            } else if d_final <= 0.1 && live_match {
                nah += 1;
            } else {
                diffs.push(format!(
                    "{login}: final py={pyf:.4} rs={rsf:.4} | base py={pyb:.4} rs={rsb:.4} | \
                     dur py={pyd:.4} rs={rsd:.4} | tp py={pyt:.4} rs={rst:.4} | \
                     boost py={pybo:.2} rs={rsbo:.2} | npm py={pyn:.2} rs={rsn:.2} | \
                     live py={pyl} rs={rsl} | today py={pyto} rs={rsto} | recv py={pyr} rs={rsr} | \
                     py_computed_at={at}",
                    login = py.twitch_login,
                    pyf = py.final_score,
                    rsf = rs.final_score,
                    pyb = py.base_score,
                    rsb = rs.base_score,
                    pyd = py.duration_score,
                    rsd = rs.duration_score,
                    pyt = py.time_pattern_score,
                    rst = rs.time_pattern_score,
                    pybo = py.raid_boost_multiplier,
                    rsbo = rs.raid_boost_multiplier,
                    pyn = py.new_partner_multiplier,
                    rsn = rs.new_partner_multiplier,
                    pyl = py.is_live,
                    rsl = rs.is_live,
                    pyto = py.today_received_raids,
                    rsto = rs.today_received_raids,
                    pyr = py.received_successful_raids_total,
                    rsr = rs.received_successful_raids_total,
                    at = py.last_computed_at,
                ));
            }
        }

        eprintln!("──── Score-Cross-Check ────");
        eprintln!(
            "Zeilen: {} | exakt: {} | nah (Δfinal ≤ 0.1): {} | abweichend: {}",
            py_rows.len(),
            exact,
            nah,
            diffs.len()
        );
        for d in &diffs {
            eprintln!("  {d}");
        }
    }
}
