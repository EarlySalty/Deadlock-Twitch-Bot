//! Community-Punkte (Community-Streamer-Brücke, Paket B).
//!
//! Zuschauer sammeln in Kanälen aktiver Partner Punkte für Anwesenheit und
//! echtes Chat-Engagement, Partner bekommen Tageswerte für ihre Community.
//! Grundlage sind ausschließlich vorhandene Rohdaten, es gibt keinen zweiten
//! Erfassungspfad:
//!
//! - Anwesenheit: `twitch_viewer_presence_ticks` (Chatters-Poller, 30 s) plus
//!   jede Chat-Nachricht aus `twitch_chat_messages`. Die Twitch-User-ID der
//!   Ticks kommt aus `twitch_session_chatters.chatter_id` derselben Session.
//! - Chat: `twitch_chat_messages` (Inhalt, Befehls-Flag, Moderationsaktion).
//! - Raids: `twitch_raid_history` (`success`).
//!
//! [`run_aggregation`] rechnet die Tageswerte (Tag nach Europe/Berlin) aus den
//! Rohdaten komplett neu und schreibt nur geänderte Zeilen. Jeder Lauf ist
//! idempotent. Einzig der Entdecker-Bonus wird als Entscheidung festgehalten
//! (`twitch_community_points_discoveries`), weil "noch nie dort gewesen" sich
//! nach dem ersten Besuch nicht mehr aus den Rohdaten ablesen lässt.
//!
//! `updated_at` ist je Tabelle streng monoton und eindeutig (Mikrosekunden),
//! damit der Lese-Cursor `updated_since` ohne Schlüsselanteil stabil bleibt.

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{DateTime, Duration, NaiveDate, SecondsFormat, TimeZone, Utc};
use chrono_tz::Europe::Berlin;
use serde::Serialize;
use sqlx::{PgConnection, PgPool};

use crate::bekannte_bots;

// ─── Punkteregeln (verbindlich laut PLAN, keine ENV) ────────────────────────

/// 1 Punkt je volle 5 Minuten Anwesenheit.
pub const WATCH_MINUTES_PER_POINT: i32 = 5;
/// Deckel Watchtime-Punkte je Kanal und Tag.
pub const WATCH_POINTS_CAP_PER_CHANNEL_DAY: i32 = 72;
/// Deckel Watchtime-Punkte je Zuschauer und Tag über alle Kanäle.
pub const WATCH_POINTS_CAP_PER_VIEWER_DAY: i32 = 144;
/// Deckel Chat-Punkte je Kanal und Tag.
pub const CHAT_POINTS_CAP_PER_CHANNEL_DAY: i32 = 30;
/// Mindestlänge einer zählenden Nachricht (Zeichen nach Trimmen).
pub const CHAT_MIN_CHARS: usize = 10;
/// Frühestens so viele Sekunden nach der letzten zählenden Nachricht.
pub const CHAT_COOLDOWN_SECS: i64 = 60;
/// Entdecker-Bonus beim ersten Auftauchen in einem neuen Partnerkanal.
pub const DISCOVERY_BONUS_POINTS: i32 = 10;
/// Höchstens so viele Entdecker-Boni je Zuschauer und Tag.
pub const DISCOVERY_BONUS_MAX_PER_DAY: usize = 3;

// ─── Betriebswerte der Aggregation ──────────────────────────────────────────

/// Eine Anwesenheitsstichprobe (Tick oder Nachricht) deckt höchstens so viele
/// Sekunden ab. Bei 30-s-Polling überbrückt das einen ausgefallenen Poll.
pub const PRESENCE_SAMPLE_COVERAGE_SECS: i64 = 60;
/// Chat-Regeln brauchen die Vorgeschichte (Duplikat, Cooldown) über die
/// Tagesgrenze hinweg; so weit wird vor Tagesbeginn zurückgeschaut.
pub const CHAT_RULE_LOOKBACK_SECS: i64 = 2 * 60 * 60;
/// Takt des Aggregationslaufs im tb-bot.
pub const AGGREGATION_INTERVAL_SECS: u64 = 300;
/// So lange nach Mitternacht (Berlin) wird der Vortag mitgerechnet.
pub const PREVIOUS_DAY_GRACE_SECS: i64 = 60 * 60;
/// Standard- und Maximalgröße einer Leseseite.
pub const DEFAULT_PAGE_LIMIT: i64 = 1000;
pub const MAX_PAGE_LIMIT: i64 = 5000;

/// Advisory-Lock: genau ein Schreiber gleichzeitig (eindeutige `updated_at`).
const AGGREGATION_LOCK_KEY: i64 = 0x7462_5f63_705f_6167; // "tb_cp_ag"

// ─── Tagesgrenze Europe/Berlin ──────────────────────────────────────────────

/// Kalendertag eines Zeitpunkts in Europe/Berlin.
pub fn berlin_day(ts: DateTime<Utc>) -> NaiveDate {
    ts.with_timezone(&Berlin).date_naive()
}

/// `[Beginn, Ende)` eines Berliner Kalendertags in UTC (23, 24 oder 25 h).
pub fn berlin_day_bounds(day: NaiveDate) -> (DateTime<Utc>, DateTime<Utc>) {
    let start_of = |d: NaiveDate| {
        let local = d.and_hms_opt(0, 0, 0).expect("Mitternacht ist gültig");
        // Mitternacht liegt in Berlin nie in einer Umstellungslücke.
        Berlin
            .from_local_datetime(&local)
            .earliest()
            .expect("Berliner Mitternacht existiert")
            .with_timezone(&Utc)
    };
    let next = day.succ_opt().expect("Folgetag existiert");
    (start_of(day), start_of(next))
}

/// Welche Tage ein Lauf neu rechnet: immer heute, den Vortag zusätzlich beim
/// ersten Lauf nach dem Start und in der ersten Stunde nach Mitternacht.
pub fn days_to_aggregate(now: DateTime<Utc>, first_run: bool) -> Vec<NaiveDate> {
    let today = berlin_day(now);
    let (start, _) = berlin_day_bounds(today);
    let mut days = Vec::with_capacity(2);
    if first_run || (now - start).num_seconds() < PREVIOUS_DAY_GRACE_SECS {
        if let Some(prev) = today.pred_opt() {
            days.push(prev);
        }
    }
    days.push(today);
    days
}

// ─── Ausschlüsse ────────────────────────────────────────────────────────────

/// Broadcaster im eigenen Kanal, bekannte Bot-Konten (`WHITELISTED_BOTS`,
/// anonyme `justinfan`-Logins) und Zeilen ohne Twitch-User-ID zählen nie.
pub fn is_excluded_viewer(viewer_id: &str, viewer_login: &str, channel_id: &str) -> bool {
    let viewer_id = viewer_id.trim();
    !is_twitch_id(viewer_id)
        || viewer_id == channel_id.trim()
        || bekannte_bots::ist_ausgeschlossener_login(viewer_login)
}

/// Twitch-User-IDs sind rein numerisch; alles andere wird nie gezählt.
pub fn is_twitch_id(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit())
}

// ─── Watchtime ──────────────────────────────────────────────────────────────

/// Punkte für die Anwesenheitsminuten in einem Kanal an einem Tag.
pub fn watch_points_for_minutes(minutes: i32) -> i32 {
    (minutes.max(0) / WATCH_MINUTES_PER_POINT).min(WATCH_POINTS_CAP_PER_CHANNEL_DAY)
}

/// Tagesdeckel über alle Kanäle: die Kanäle in der übergebenen Reihenfolge
/// (früheste Anwesenheit zuerst) behalten ihre Punkte, bis der Deckel voll ist.
pub fn cap_viewer_watch_points(points_in_order: &[i32]) -> Vec<i32> {
    let mut left = WATCH_POINTS_CAP_PER_VIEWER_DAY;
    points_in_order
        .iter()
        .map(|&p| {
            let granted = p.max(0).min(left);
            left -= granted;
            granted
        })
        .collect()
}

// ─── Chat ───────────────────────────────────────────────────────────────────

/// Eine Chat-Nachricht einer Person in einem Kanal.
#[derive(Debug, Clone)]
pub struct ChatSample {
    pub at: DateTime<Utc>,
    pub content: String,
    pub is_command: bool,
    /// Vom Bot moderiert (gelöscht, Timeout, Bann): zählt nie.
    pub moderated: bool,
}

/// Vergleichsform für die Duplikat-Regel: getrimmt, Kleinbuchstaben,
/// Leerraum zusammengefasst.
pub fn duplicate_key(content: &str) -> String {
    content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Befehl (`!` am Anfang) oder vom Chat-Tracker als Befehl markiert.
pub fn is_command(content: &str, flagged: bool) -> bool {
    flagged || content.trim_start().starts_with('!')
}

/// Mindestens [`CHAT_MIN_CHARS`] Zeichen nach Trimmen.
pub fn meets_min_length(content: &str) -> bool {
    content.trim().chars().count() >= CHAT_MIN_CHARS
}

/// Kennzeichnet für chronologisch sortierte Nachrichten EINER Person in EINEM
/// Kanal, welche zählen: kein Befehl, Mindestlänge, nicht identisch mit der
/// vorherigen Nachricht (egal ob diese zählte), frühestens
/// [`CHAT_COOLDOWN_SECS`] nach der letzten zählenden Nachricht.
pub fn countable_flags(messages: &[ChatSample]) -> Vec<bool> {
    let mut last_counted: Option<DateTime<Utc>> = None;
    let mut previous_key: Option<String> = None;
    let mut out = Vec::with_capacity(messages.len());
    for msg in messages {
        let key = duplicate_key(&msg.content);
        let duplicate = previous_key.as_deref() == Some(key.as_str());
        let cooled = last_counted
            .map(|last| (msg.at - last).num_seconds() >= CHAT_COOLDOWN_SECS)
            .unwrap_or(true);
        let counts = !msg.moderated
            && !is_command(&msg.content, msg.is_command)
            && meets_min_length(&msg.content)
            && !duplicate
            && cooled;
        if counts {
            last_counted = Some(msg.at);
        }
        previous_key = Some(key);
        out.push(counts);
    }
    out
}

/// Zählende Nachrichten mit Zeitstempel in `[from, to)`; die Nachrichten davor
/// dienen nur als Vorgeschichte.
pub fn countable_in_window(messages: &[ChatSample], from: DateTime<Utc>, to: DateTime<Utc>) -> i32 {
    countable_flags(messages)
        .into_iter()
        .zip(messages)
        .filter(|(counts, m)| *counts && m.at >= from && m.at < to)
        .count() as i32
}

/// Chat-Punkte aus der Zahl zählender Nachrichten (Deckel je Kanal und Tag).
pub fn chat_points(countable: i32) -> i32 {
    countable.clamp(0, CHAT_POINTS_CAP_PER_CHANNEL_DAY)
}

// ─── Entdecker-Bonus ────────────────────────────────────────────────────────

/// Ein Kanal, in dem die Person heute zum ersten Mal erfasst wurde.
#[derive(Debug, Clone)]
pub struct DiscoveryCandidate {
    pub channel_id: String,
    pub first_seen_at: DateTime<Utc>,
    /// Es gibt ältere Spuren der Person in diesem Kanal (vor Tagesbeginn).
    pub seen_before: bool,
}

/// Vergibt Boni in der Reihenfolge des ersten Auftauchens, höchstens
/// [`DISCOVERY_BONUS_MAX_PER_DAY`] je Tag inklusive bereits vergebener.
/// Ergebnis in Eingabereihenfolge.
pub fn select_discovery_bonuses(
    candidates: &[DiscoveryCandidate],
    already_awarded_today: usize,
) -> Vec<bool> {
    let mut order: Vec<usize> = (0..candidates.len()).collect();
    order.sort_by(|&a, &b| {
        candidates[a]
            .first_seen_at
            .cmp(&candidates[b].first_seen_at)
            .then_with(|| candidates[a].channel_id.cmp(&candidates[b].channel_id))
    });
    let mut left = DISCOVERY_BONUS_MAX_PER_DAY.saturating_sub(already_awarded_today);
    let mut out = vec![false; candidates.len()];
    for idx in order {
        if left == 0 {
            break;
        }
        if !candidates[idx].seen_before {
            out[idx] = true;
            left -= 1;
        }
    }
    out
}

// ─── Tagesrechnung (rein) ───────────────────────────────────────────────────

/// Aggregierte Anwesenheit einer Person in einem Partnerkanal an einem Tag.
#[derive(Debug, Clone)]
pub struct PresenceRow {
    pub channel_id: String,
    pub viewer_id: String,
    pub viewer_login: String,
    pub seconds: f64,
    pub first_seen_at: DateTime<Utc>,
}

/// Ein aktiver Partnerkanal.
#[derive(Debug, Clone)]
pub struct Partner {
    pub twitch_user_id: String,
    pub login: String,
    pub discord_user_id: Option<String>,
}

/// Bereits festgehaltene Entdeckung.
#[derive(Debug, Clone)]
pub struct KnownDiscovery {
    pub day: NaiveDate,
    pub bonus_awarded: bool,
}

/// Alles, was die reine Tagesrechnung braucht.
#[derive(Debug, Default)]
pub struct DayInput {
    pub day: NaiveDate,
    pub partners: Vec<Partner>,
    pub presence: Vec<PresenceRow>,
    /// (Kanal, Zuschauer) → chronologische Nachrichten inkl. Vorgeschichte.
    pub chat: HashMap<(String, String), Vec<ChatSample>>,
    /// (Kanal, Zuschauer) mit dauerhaftem Kanal-Bann.
    pub channel_bans: HashSet<(String, String)>,
    /// Global gebannte Twitch-User-IDs.
    pub global_ban_ids: HashSet<String>,
    /// (Zuschauer, Kanal) → festgehaltene Entdeckung.
    pub discoveries: HashMap<(String, String), KnownDiscovery>,
    /// (Kanal, Zuschauer) mit Spuren vor Tagesbeginn (nur für neue Paare nötig).
    pub seen_before: HashSet<(String, String)>,
    /// Kanal → erfolgreiche Raids an andere aktive Partner an diesem Tag.
    pub raids_to_partners: HashMap<String, i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewerDayRow {
    pub twitch_user_id: String,
    pub twitch_login: String,
    pub channel_twitch_user_id: String,
    pub watch_minutes: i32,
    pub chat_messages: i32,
    pub points_watch: i32,
    pub points_chat: i32,
    pub points_discovery: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamerDayRow {
    pub streamer_twitch_user_id: String,
    pub streamer_login: String,
    pub discord_user_id: Option<String>,
    pub viewer_minutes: i32,
    pub unique_viewers: i32,
    pub raids_to_partners: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDiscovery {
    pub twitch_user_id: String,
    pub channel_twitch_user_id: String,
    pub first_seen_at: DateTime<Utc>,
    pub bonus_awarded: bool,
}

#[derive(Debug, Default)]
pub struct DayOutput {
    pub viewers: Vec<ViewerDayRow>,
    pub streamers: Vec<StreamerDayRow>,
    pub new_discoveries: Vec<NewDiscovery>,
}

/// Rechnet einen Tag aus den geladenen Rohdaten. Keine DB, keine Uhr.
pub fn compute_day(input: &DayInput) -> DayOutput {
    let (day_start, day_end) = berlin_day_bounds(input.day);
    let partner_ids: HashSet<&str> = input
        .partners
        .iter()
        .map(|p| p.twitch_user_id.as_str())
        .collect();

    // 1. Zählende Anwesenheit je Zuschauer, Ausschlüsse angewandt.
    let mut by_viewer: BTreeMap<&str, Vec<&PresenceRow>> = BTreeMap::new();
    for row in &input.presence {
        if !partner_ids.contains(row.channel_id.as_str())
            || is_excluded_viewer(&row.viewer_id, &row.viewer_login, &row.channel_id)
            || input.global_ban_ids.contains(&row.viewer_id)
            || input
                .channel_bans
                .contains(&(row.channel_id.clone(), row.viewer_id.clone()))
        {
            continue;
        }
        by_viewer.entry(&row.viewer_id).or_default().push(row);
    }

    let mut out = DayOutput::default();
    for (viewer_id, mut rows) in by_viewer {
        rows.sort_by(|a, b| {
            a.first_seen_at
                .cmp(&b.first_seen_at)
                .then_with(|| a.channel_id.cmp(&b.channel_id))
        });

        // 2. Watchtime mit Kanal- und Tagesdeckel.
        let minutes: Vec<i32> = rows
            .iter()
            .map(|r| (r.seconds.max(0.0) / 60.0).floor() as i32)
            .collect();
        let raw_points: Vec<i32> = minutes
            .iter()
            .map(|&m| watch_points_for_minutes(m))
            .collect();
        let watch_points = cap_viewer_watch_points(&raw_points);

        // 3. Entdecker-Bonus: bestehende Entscheidungen + neue Paare.
        let already_awarded_today = input
            .discoveries
            .iter()
            .filter(|((v, _), d)| v == viewer_id && d.day == input.day && d.bonus_awarded)
            .count();
        let new_candidates: Vec<DiscoveryCandidate> = rows
            .iter()
            .filter(|r| {
                !input
                    .discoveries
                    .contains_key(&(viewer_id.to_string(), r.channel_id.clone()))
            })
            .map(|r| DiscoveryCandidate {
                channel_id: r.channel_id.clone(),
                first_seen_at: r.first_seen_at,
                seen_before: input
                    .seen_before
                    .contains(&(r.channel_id.clone(), viewer_id.to_string())),
            })
            .collect();
        let awards = select_discovery_bonuses(&new_candidates, already_awarded_today);
        let mut bonus_today: HashSet<String> = input
            .discoveries
            .iter()
            .filter(|((v, _), d)| v == viewer_id && d.day == input.day && d.bonus_awarded)
            .map(|((_, c), _)| c.clone())
            .collect();
        for (cand, awarded) in new_candidates.iter().zip(awards) {
            if awarded {
                bonus_today.insert(cand.channel_id.clone());
            }
            out.new_discoveries.push(NewDiscovery {
                twitch_user_id: viewer_id.to_string(),
                channel_twitch_user_id: cand.channel_id.clone(),
                first_seen_at: cand.first_seen_at,
                bonus_awarded: awarded,
            });
        }

        for ((row, minutes), points_watch) in rows.iter().zip(minutes).zip(watch_points) {
            let countable = input
                .chat
                .get(&(row.channel_id.clone(), viewer_id.to_string()))
                .map(|msgs| countable_in_window(msgs, day_start, day_end))
                .unwrap_or(0);
            out.viewers.push(ViewerDayRow {
                twitch_user_id: viewer_id.to_string(),
                twitch_login: row.viewer_login.clone(),
                channel_twitch_user_id: row.channel_id.clone(),
                watch_minutes: minutes,
                chat_messages: countable,
                points_watch,
                points_chat: chat_points(countable),
                points_discovery: if bonus_today.contains(&row.channel_id) {
                    DISCOVERY_BONUS_POINTS
                } else {
                    0
                },
            });
        }
    }

    // 4. Streamer-Tageswerte.
    for partner in &input.partners {
        let (minutes, viewers) = out
            .viewers
            .iter()
            .filter(|v| v.channel_twitch_user_id == partner.twitch_user_id)
            .fold((0i32, 0i32), |(m, n), v| (m + v.watch_minutes, n + 1));
        let raids = input
            .raids_to_partners
            .get(&partner.twitch_user_id)
            .copied()
            .unwrap_or(0);
        if viewers == 0 && raids == 0 {
            continue;
        }
        out.streamers.push(StreamerDayRow {
            streamer_twitch_user_id: partner.twitch_user_id.clone(),
            streamer_login: partner.login.clone(),
            discord_user_id: partner.discord_user_id.clone(),
            viewer_minutes: minutes,
            unique_viewers: viewers,
            raids_to_partners: raids,
        });
    }
    out
}

// ─── DB: Laden ──────────────────────────────────────────────────────────────

/// Aktive Partner (eine Zeile je Twitch-User-ID).
pub async fn load_active_partners(pool: &PgPool) -> Result<Vec<Partner>, sqlx::Error> {
    let mut conn = pool.acquire().await?;
    load_active_partners_on_conn(&mut conn).await
}

async fn load_active_partners_on_conn(
    conn: &mut PgConnection,
) -> Result<Vec<Partner>, sqlx::Error> {
    let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT DISTINCT ON (twitch_user_id)
                twitch_user_id, LOWER(TRIM(twitch_login)), NULLIF(TRIM(discord_user_id), '')
           FROM twitch_streamers_partner_state
          WHERE COALESCE(is_partner_active, 0) = 1
            AND COALESCE(TRIM(twitch_user_id), '') <> ''
          ORDER BY twitch_user_id, twitch_login",
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .filter(|(id, _, _)| is_twitch_id(id))
        .map(|(id, login, discord)| Partner {
            twitch_user_id: id,
            login,
            discord_user_id: discord,
        })
        .collect())
}

/// Sessions der Partner, die das Fenster berühren, als CTE-Baustein.
/// $1 = Partner-IDs, $2 = Partner-Logins (gleiche Reihenfolge),
/// $3 = Fensterbeginn, $4 = Fensterende.
const PARTNER_SESSIONS_CTE: &str = "partners AS (
        SELECT * FROM UNNEST($1::text[], $2::text[]) AS p(channel_id, channel_login)
    ),
    sessions AS (
        SELECT s.id, p.channel_id, s.started_at, s.ended_at
          FROM twitch_stream_sessions s
          JOIN partners p
            ON s.twitch_user_id = p.channel_id
         WHERE s.started_at < $4
           AND (s.ended_at IS NULL OR s.ended_at >= $3)
    )";

/// Anwesenheit je (Kanal, Zuschauer) im Tag: Vereinigung der Intervalle
/// `[t, t + 60 s)` aller Stichproben (Ticks und Nachrichten), geschnitten mit
/// `[Tagesbeginn, min(Tagesende, jetzt))`.
async fn load_presence(
    conn: &mut PgConnection,
    partners: &[Partner],
    day_start: DateTime<Utc>,
    day_end: DateTime<Utc>,
    upper: DateTime<Utc>,
) -> Result<Vec<PresenceRow>, sqlx::Error> {
    let (ids, logins) = partner_arrays(partners);
    let window_start = day_start - Duration::seconds(PRESENCE_SAMPLE_COVERAGE_SECS);
    let sql = format!(
        "WITH {PARTNER_SESSIONS_CTE},
        samples AS (
            SELECT ss.channel_id, t.viewer_twitch_user_id AS viewer_id, t.viewer_login AS login,
                   t.tick_at AS at, ss.ended_at AS session_ended_at
              FROM twitch_viewer_presence_ticks t
              JOIN sessions ss ON ss.id = t.session_id
             WHERE t.tick_at >= $3 AND t.tick_at < $4
               AND t.tick_at >= ss.started_at
               AND (ss.ended_at IS NULL OR t.tick_at < ss.ended_at)
               AND COALESCE(TRIM(t.viewer_twitch_user_id), '') <> ''
            UNION ALL
            SELECT ss.channel_id, m.chatter_id, LOWER(m.chatter_login), m.message_ts,
                   ss.ended_at
              FROM twitch_chat_messages m
              JOIN sessions ss ON ss.id = m.session_id
             WHERE m.message_ts >= $3 AND m.message_ts < $4
               AND m.message_ts >= ss.started_at
               AND (ss.ended_at IS NULL OR m.message_ts < ss.ended_at)
               AND COALESCE(TRIM(m.chatter_id), '') <> ''
        ),
        ordered AS (
            SELECT channel_id, viewer_id, login, at, session_ended_at,
                   LEAD(at) OVER (PARTITION BY channel_id, viewer_id ORDER BY at) AS next_at
              FROM samples
        ),
        covered AS (
            SELECT channel_id, viewer_id, login, at,
                   GREATEST(0, EXTRACT(EPOCH FROM (
                       LEAST(COALESCE(next_at, at + $6 * INTERVAL '1 second'),
                             at + $6 * INTERVAL '1 second', $7,
                             COALESCE(session_ended_at, $7))
                       - GREATEST(at, $5)))) AS secs
              FROM ordered
        )
        SELECT channel_id, viewer_id,
               (ARRAY_AGG(COALESCE(login, '') ORDER BY at DESC))[1] AS login,
               SUM(secs)::float8 AS seconds,
               MIN(at) FILTER (WHERE at >= $5) AS first_seen_at
          FROM covered
         GROUP BY channel_id, viewer_id
        HAVING COUNT(*) FILTER (WHERE at >= $5 AND at < $7) > 0"
    );
    let rows: Vec<(String, String, String, f64, DateTime<Utc>)> =
        sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(&ids)
            .bind(&logins)
            .bind(window_start)
            .bind(day_end)
            .bind(day_start)
            .bind(PRESENCE_SAMPLE_COVERAGE_SECS as f64)
            .bind(upper)
            .fetch_all(&mut *conn)
            .await?;
    Ok(rows
        .into_iter()
        .map(
            |(channel_id, viewer_id, login, seconds, first_seen_at)| PresenceRow {
                channel_id,
                viewer_id: viewer_id.trim().to_string(),
                viewer_login: login,
                seconds,
                first_seen_at,
            },
        )
        .collect())
}

type ChatRow = (String, String, DateTime<Utc>, String, bool, bool);

/// Nachrichten der Partnerkanäle inkl. Vorgeschichte für die Chat-Regeln.
async fn load_chat(
    conn: &mut PgConnection,
    partners: &[Partner],
    day_start: DateTime<Utc>,
    upper: DateTime<Utc>,
) -> Result<HashMap<(String, String), Vec<ChatSample>>, sqlx::Error> {
    let (ids, logins) = partner_arrays(partners);
    let window_start = day_start - Duration::seconds(CHAT_RULE_LOOKBACK_SECS);
    let sql = format!(
        "WITH {PARTNER_SESSIONS_CTE}
        SELECT ss.channel_id, TRIM(m.chatter_id), m.message_ts, COALESCE(m.content, ''),
               COALESCE(m.is_command, FALSE), m.moderation_action IS NOT NULL
          FROM twitch_chat_messages m
          JOIN sessions ss ON ss.id = m.session_id
         WHERE m.message_ts >= $3 AND m.message_ts < $4
           AND m.message_ts >= ss.started_at
           AND (ss.ended_at IS NULL OR m.message_ts < ss.ended_at)
           AND COALESCE(TRIM(m.chatter_id), '') <> ''
         ORDER BY ss.channel_id, TRIM(m.chatter_id), m.message_ts, m.id"
    );
    let rows: Vec<ChatRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(&ids)
        .bind(&logins)
        .bind(window_start)
        .bind(upper)
        .fetch_all(&mut *conn)
        .await?;
    let mut map: HashMap<(String, String), Vec<ChatSample>> = HashMap::new();
    for (channel, viewer, at, content, is_command, moderated) in rows {
        map.entry((channel, viewer)).or_default().push(ChatSample {
            at,
            content,
            is_command,
            moderated,
        });
    }
    Ok(map)
}

fn partner_arrays(partners: &[Partner]) -> (Vec<String>, Vec<String>) {
    partners
        .iter()
        .map(|p| (p.twitch_user_id.clone(), p.login.clone()))
        .unzip()
}

/// Dauerhafte Kanal-Banns (letztes Ereignis ist `ban` ohne Ende) und globale Banns.
async fn load_bans(
    conn: &mut PgConnection,
    partner_ids: &[String],
    viewer_ids: &[String],
    input: &mut DayInput,
) -> Result<(), sqlx::Error> {
    let channel_bans: Vec<(String, String)> = sqlx::query_as(
        "SELECT twitch_user_id, target_id FROM (
             SELECT DISTINCT ON (twitch_user_id, target_id)
                    twitch_user_id, target_id, event_type, ends_at
               FROM twitch_ban_events
              WHERE twitch_user_id = ANY($1) AND target_id = ANY($2)
              ORDER BY twitch_user_id, target_id, received_at DESC, id DESC
         ) last
         WHERE event_type = 'ban' AND ends_at IS NULL",
    )
    .bind(partner_ids)
    .bind(viewer_ids)
    .fetch_all(&mut *conn)
    .await?;
    input.channel_bans = channel_bans.into_iter().collect();

    let global: Vec<String> = sqlx::query_scalar(
        "SELECT TRIM(chatter_id) FROM twitch_chatter_global_ban
         WHERE COALESCE(TRIM(chatter_id), '') <> ''",
    )
    .fetch_all(&mut *conn)
    .await?;
    input.global_ban_ids.extend(global);
    Ok(())
}

async fn load_discoveries(
    conn: &mut PgConnection,
    viewer_ids: &[String],
) -> Result<HashMap<(String, String), KnownDiscovery>, sqlx::Error> {
    let rows: Vec<(String, String, NaiveDate, bool)> = sqlx::query_as(
        "SELECT twitch_user_id, channel_twitch_user_id, day, bonus_awarded
           FROM twitch_community_points_discoveries
          WHERE twitch_user_id = ANY($1)",
    )
    .bind(viewer_ids)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(v, c, day, bonus_awarded)| ((v, c), KnownDiscovery { day, bonus_awarded }))
        .collect())
}

/// Spuren vor Tagesbeginn werden ausschließlich anhand der Zuschauer-ID zugeordnet.
async fn load_seen_before(
    conn: &mut PgConnection,
    pairs: &[(String, String, String, String)],
    day_start: DateTime<Utc>,
) -> Result<HashSet<(String, String)>, sqlx::Error> {
    if pairs.is_empty() {
        return Ok(HashSet::new());
    }
    let mut channel_logins: Vec<String> = pairs.iter().map(|p| p.1.to_lowercase()).collect();
    channel_logins.sort();
    channel_logins.dedup();
    let viewer_ids: Vec<String> = pairs.iter().map(|p| p.2.clone()).collect();
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT LOWER(streamer_login), chatter_id
           FROM twitch_chatter_rollup
          WHERE streamer_login = ANY($1)
            AND chatter_id = ANY($2)
            AND first_seen_at < $3",
    )
    .bind(&channel_logins)
    .bind(&viewer_ids)
    .bind(day_start)
    .fetch_all(&mut *conn)
    .await?;
    let by_id: HashSet<(String, String)> = rows.into_iter().collect();
    Ok(pairs
        .iter()
        .filter(|(_, cl, vid, _)| by_id.contains(&(cl.clone(), vid.clone())))
        .map(|(c, _, v, _)| (c.clone(), v.clone()))
        .collect())
}

async fn load_raids(
    conn: &mut PgConnection,
    partner_ids: &[String],
    day_start: DateTime<Utc>,
    day_end: DateTime<Utc>,
) -> Result<HashMap<String, i32>, sqlx::Error> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT from_broadcaster_id, COUNT(*)
           FROM twitch_raid_history
          WHERE executed_at >= $2 AND executed_at < $3
            AND COALESCE(success, FALSE)
            AND from_broadcaster_id = ANY($1)
            AND to_broadcaster_id = ANY($1)
            AND to_broadcaster_id <> from_broadcaster_id
          GROUP BY from_broadcaster_id",
    )
    .bind(partner_ids)
    .bind(day_start)
    .bind(day_end)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, n)| (id, i32::try_from(n).unwrap_or(i32::MAX)))
        .collect())
}

/// Lädt alle Rohdaten eines Tages.
pub async fn load_day_input(
    pool: &PgPool,
    day: NaiveDate,
    now: DateTime<Utc>,
) -> Result<DayInput, sqlx::Error> {
    let mut conn = pool.acquire().await?;
    load_day_input_on_conn(&mut conn, day, now).await
}

async fn load_day_input_on_conn(
    conn: &mut PgConnection,
    day: NaiveDate,
    now: DateTime<Utc>,
) -> Result<DayInput, sqlx::Error> {
    let (day_start, day_end) = berlin_day_bounds(day);
    let upper = day_end.min(now);
    let partners = load_active_partners_on_conn(conn).await?;
    let mut input = DayInput {
        day,
        ..DayInput::default()
    };
    if partners.is_empty() || upper <= day_start {
        input.partners = partners;
        return Ok(input);
    }
    let partner_ids: Vec<String> = partners.iter().map(|p| p.twitch_user_id.clone()).collect();
    input.presence = load_presence(conn, &partners, day_start, day_end, upper).await?;
    input.chat = load_chat(conn, &partners, day_start, upper).await?;
    let mut viewer_ids: Vec<String> = input.presence.iter().map(|p| p.viewer_id.clone()).collect();
    viewer_ids.sort();
    viewer_ids.dedup();
    load_bans(conn, &partner_ids, &viewer_ids, &mut input).await?;
    input.discoveries = load_discoveries(conn, &viewer_ids).await?;

    let login_of: HashMap<&str, &str> = partners
        .iter()
        .map(|p| (p.twitch_user_id.as_str(), p.login.as_str()))
        .collect();
    let new_pairs: Vec<(String, String, String, String)> = input
        .presence
        .iter()
        .filter(|p| {
            !input
                .discoveries
                .contains_key(&(p.viewer_id.clone(), p.channel_id.clone()))
        })
        .filter_map(|p| {
            login_of.get(p.channel_id.as_str()).map(|login| {
                (
                    p.channel_id.clone(),
                    login.to_string(),
                    p.viewer_id.clone(),
                    p.viewer_login.clone(),
                )
            })
        })
        .collect();
    input.seen_before = load_seen_before(conn, &new_pairs, day_start).await?;
    input.raids_to_partners = load_raids(conn, &partner_ids, day_start, day_end).await?;
    input.partners = partners;
    Ok(input)
}

// ─── DB: Schreiben ──────────────────────────────────────────────────────────

/// Zähler eines Tageslaufs (Logging/Tests).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DayWriteStats {
    pub viewer_rows_changed: u64,
    pub viewer_rows_zeroed: u64,
    pub streamer_rows_changed: u64,
    pub streamer_rows_zeroed: u64,
    pub discoveries_inserted: u64,
}

/// Nächster freier `updated_at`-Basiswert einer Tabelle (streng monoton).
async fn next_base(
    tx: &mut sqlx::PgConnection,
    table: &'static str,
) -> Result<DateTime<Utc>, sqlx::Error> {
    let sql = format!(
        "SELECT GREATEST(clock_timestamp(),
                         COALESCE(MAX(updated_at), '-infinity'::timestamptz) + INTERVAL '1 microsecond')
           FROM {table}"
    );
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_one(&mut *tx)
        .await
}

/// Schreibt die Tageswerte: Upsert geänderter Zeilen, Nullen verschwundener
/// Zeilen des Tages, neue Entdeckungen. Eine Transaktion, ein Schreiber.
pub async fn write_day(
    pool: &PgPool,
    day: NaiveDate,
    output: &DayOutput,
) -> Result<DayWriteStats, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(AGGREGATION_LOCK_KEY)
        .execute(&mut *tx)
        .await?;
    let stats = write_day_on_conn(&mut tx, day, output).await?;
    tx.commit().await?;
    Ok(stats)
}

async fn write_day_on_conn(
    tx: &mut PgConnection,
    day: NaiveDate,
    output: &DayOutput,
) -> Result<DayWriteStats, sqlx::Error> {
    let mut stats = DayWriteStats::default();

    // Entdeckungen zuerst: Entscheidung ist endgültig.
    if !output.new_discoveries.is_empty() {
        let v: Vec<&str> = output
            .new_discoveries
            .iter()
            .map(|d| d.twitch_user_id.as_str())
            .collect();
        let c: Vec<&str> = output
            .new_discoveries
            .iter()
            .map(|d| d.channel_twitch_user_id.as_str())
            .collect();
        let f: Vec<DateTime<Utc>> = output
            .new_discoveries
            .iter()
            .map(|d| d.first_seen_at)
            .collect();
        let b: Vec<bool> = output
            .new_discoveries
            .iter()
            .map(|d| d.bonus_awarded)
            .collect();
        stats.discoveries_inserted = sqlx::query(
            "INSERT INTO twitch_community_points_discoveries
                 (twitch_user_id, channel_twitch_user_id, day, first_seen_at, bonus_awarded)
             SELECT v, c, $5, f, b FROM UNNEST($1::text[], $2::text[], $3::timestamptz[], $4::bool[])
                 AS x(v, c, f, b)
             ON CONFLICT (twitch_user_id, channel_twitch_user_id) DO NOTHING",
        )
        .bind(&v)
        .bind(&c)
        .bind(&f)
        .bind(&b)
        .bind(day)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    }

    // Zuschauer-Tageswerte.
    let rows = &output.viewers;
    let vid: Vec<&str> = rows.iter().map(|r| r.twitch_user_id.as_str()).collect();
    let cid: Vec<&str> = rows
        .iter()
        .map(|r| r.channel_twitch_user_id.as_str())
        .collect();
    let login: Vec<&str> = rows.iter().map(|r| r.twitch_login.as_str()).collect();
    let wm: Vec<i32> = rows.iter().map(|r| r.watch_minutes).collect();
    let cm: Vec<i32> = rows.iter().map(|r| r.chat_messages).collect();
    let pw: Vec<i32> = rows.iter().map(|r| r.points_watch).collect();
    let pc: Vec<i32> = rows.iter().map(|r| r.points_chat).collect();
    let pd: Vec<i32> = rows.iter().map(|r| r.points_discovery).collect();
    let base = next_base(tx, "twitch_community_points_viewer_daily").await?;
    stats.viewer_rows_changed = sqlx::query(
        "INSERT INTO twitch_community_points_viewer_daily AS t
             (twitch_user_id, channel_twitch_user_id, day, twitch_login, watch_minutes,
              chat_messages, points_watch, points_chat, points_discovery, updated_at)
         SELECT v, c, $9, l, wm, cm, pw, pc, pd, $10 + n * INTERVAL '1 microsecond'
           FROM UNNEST($1::text[], $2::text[], $3::text[], $4::int[], $5::int[],
                       $6::int[], $7::int[], $8::int[])
                WITH ORDINALITY AS x(v, c, l, wm, cm, pw, pc, pd, n)
         ON CONFLICT (twitch_user_id, channel_twitch_user_id, day) DO UPDATE SET
             twitch_login = EXCLUDED.twitch_login,
             watch_minutes = EXCLUDED.watch_minutes,
             chat_messages = EXCLUDED.chat_messages,
             points_watch = EXCLUDED.points_watch,
             points_chat = EXCLUDED.points_chat,
             points_discovery = EXCLUDED.points_discovery,
             updated_at = EXCLUDED.updated_at
         WHERE (t.twitch_login, t.watch_minutes, t.chat_messages, t.points_watch,
                t.points_chat, t.points_discovery)
               IS DISTINCT FROM
               (EXCLUDED.twitch_login, EXCLUDED.watch_minutes, EXCLUDED.chat_messages,
                EXCLUDED.points_watch, EXCLUDED.points_chat, EXCLUDED.points_discovery)",
    )
    .bind(&vid)
    .bind(&cid)
    .bind(&login)
    .bind(&wm)
    .bind(&cm)
    .bind(&pw)
    .bind(&pc)
    .bind(&pd)
    .bind(day)
    .bind(base)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    let base = next_base(tx, "twitch_community_points_viewer_daily").await?;
    stats.viewer_rows_zeroed = sqlx::query(
        "WITH stale AS (
             SELECT t.twitch_user_id, t.channel_twitch_user_id,
                    ROW_NUMBER() OVER (ORDER BY t.twitch_user_id, t.channel_twitch_user_id) AS n
               FROM twitch_community_points_viewer_daily t
              WHERE t.day = $3
                AND (t.watch_minutes, t.chat_messages, t.points_watch, t.points_chat,
                     t.points_discovery) IS DISTINCT FROM (0, 0, 0, 0, 0)
                AND NOT EXISTS (
                    SELECT 1 FROM UNNEST($1::text[], $2::text[]) AS k(v, c)
                     WHERE k.v = t.twitch_user_id AND k.c = t.channel_twitch_user_id)
         )
         UPDATE twitch_community_points_viewer_daily t
            SET watch_minutes = 0, chat_messages = 0, points_watch = 0, points_chat = 0,
                points_discovery = 0, updated_at = $4 + stale.n * INTERVAL '1 microsecond'
           FROM stale
          WHERE t.day = $3
            AND t.twitch_user_id = stale.twitch_user_id
            AND t.channel_twitch_user_id = stale.channel_twitch_user_id",
    )
    .bind(&vid)
    .bind(&cid)
    .bind(day)
    .bind(base)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    // Streamer-Tageswerte.
    let rows = &output.streamers;
    let sid: Vec<&str> = rows
        .iter()
        .map(|r| r.streamer_twitch_user_id.as_str())
        .collect();
    let slogin: Vec<&str> = rows.iter().map(|r| r.streamer_login.as_str()).collect();
    let discord: Vec<Option<&str>> = rows.iter().map(|r| r.discord_user_id.as_deref()).collect();
    let vm: Vec<i32> = rows.iter().map(|r| r.viewer_minutes).collect();
    let uv: Vec<i32> = rows.iter().map(|r| r.unique_viewers).collect();
    let rp: Vec<i32> = rows.iter().map(|r| r.raids_to_partners).collect();
    let base = next_base(tx, "twitch_community_points_streamer_daily").await?;
    stats.streamer_rows_changed = sqlx::query(
        "INSERT INTO twitch_community_points_streamer_daily AS t
             (streamer_twitch_user_id, day, streamer_login, discord_user_id, viewer_minutes,
              unique_viewers, raids_to_partners, updated_at)
         SELECT s, $7, l, d, vm, uv, rp, $8 + n * INTERVAL '1 microsecond'
           FROM UNNEST($1::text[], $2::text[], $3::text[], $4::int[], $5::int[], $6::int[])
                WITH ORDINALITY AS x(s, l, d, vm, uv, rp, n)
         ON CONFLICT (streamer_twitch_user_id, day) DO UPDATE SET
             streamer_login = EXCLUDED.streamer_login,
             discord_user_id = EXCLUDED.discord_user_id,
             viewer_minutes = EXCLUDED.viewer_minutes,
             unique_viewers = EXCLUDED.unique_viewers,
             raids_to_partners = EXCLUDED.raids_to_partners,
             updated_at = EXCLUDED.updated_at
         WHERE (t.streamer_login, t.discord_user_id, t.viewer_minutes, t.unique_viewers,
                t.raids_to_partners)
               IS DISTINCT FROM
               (EXCLUDED.streamer_login, EXCLUDED.discord_user_id, EXCLUDED.viewer_minutes,
                EXCLUDED.unique_viewers, EXCLUDED.raids_to_partners)",
    )
    .bind(&sid)
    .bind(&slogin)
    .bind(&discord)
    .bind(&vm)
    .bind(&uv)
    .bind(&rp)
    .bind(day)
    .bind(base)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    let base = next_base(tx, "twitch_community_points_streamer_daily").await?;
    stats.streamer_rows_zeroed = sqlx::query(
        "WITH stale AS (
             SELECT t.streamer_twitch_user_id,
                    ROW_NUMBER() OVER (ORDER BY t.streamer_twitch_user_id) AS n
               FROM twitch_community_points_streamer_daily t
              WHERE t.day = $2
                AND (t.viewer_minutes, t.unique_viewers, t.raids_to_partners)
                    IS DISTINCT FROM (0, 0, 0)
                AND NOT (t.streamer_twitch_user_id = ANY($1))
         )
         UPDATE twitch_community_points_streamer_daily t
            SET viewer_minutes = 0, unique_viewers = 0, raids_to_partners = 0,
                updated_at = $3 + stale.n * INTERVAL '1 microsecond'
           FROM stale
          WHERE t.day = $2 AND t.streamer_twitch_user_id = stale.streamer_twitch_user_id",
    )
    .bind(&sid)
    .bind(day)
    .bind(base)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    Ok(stats)
}

/// Rechnet einen Tag neu und schreibt ihn (idempotent).
pub async fn aggregate_day(
    pool: &PgPool,
    day: NaiveDate,
    now: DateTime<Utc>,
) -> Result<DayWriteStats, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(AGGREGATION_LOCK_KEY)
        .execute(&mut *tx)
        .await?;

    let input = load_day_input_on_conn(&mut tx, day, now).await?;
    let output = compute_day(&input);
    let stats = write_day_on_conn(&mut tx, day, &output).await?;
    tx.commit().await?;
    Ok(stats)
}

/// Ein Aggregationslauf über [`days_to_aggregate`]. Ältere Tage zuerst, damit
/// Entdeckungen dem richtigen Tag zugeordnet werden.
pub async fn run_aggregation(
    pool: &PgPool,
    now: DateTime<Utc>,
    first_run: bool,
) -> Result<Vec<(NaiveDate, DayWriteStats)>, sqlx::Error> {
    let mut out = Vec::new();
    for day in days_to_aggregate(now, first_run) {
        out.push((day, aggregate_day(pool, day, now).await?));
    }
    Ok(out)
}

// ─── DB: Lesen für die interne Schnittstelle ────────────────────────────────

/// RFC3339 in UTC mit so vielen Nachkommastellen wie nötig (Cursor-genau).
pub fn format_cursor(ts: DateTime<Utc>) -> String {
    ts.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ViewerPointsRow {
    pub twitch_user_id: String,
    pub twitch_login: String,
    pub channel_twitch_user_id: String,
    pub day: String,
    pub watch_minutes: i32,
    pub chat_messages: i32,
    pub points_watch: i32,
    pub points_chat: i32,
    pub points_discovery: i32,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StreamerPointsRow {
    pub streamer_twitch_user_id: String,
    pub streamer_login: String,
    pub discord_user_id: Option<String>,
    pub day: String,
    pub viewer_minutes: i32,
    pub unique_viewers: i32,
    pub raids_to_partners: i32,
    pub updated_at: String,
}

/// Eine Seite ab Cursor (exklusiv), sortiert nach `updated_at`, dann Schlüssel.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Page<T> {
    pub rows: Vec<T>,
    pub next_updated_since: Option<String>,
    pub has_more: bool,
}

fn finish_page<T>(
    mut rows: Vec<T>,
    stamps: Vec<DateTime<Utc>>,
    limit: i64,
    since: Option<DateTime<Utc>>,
) -> Page<T> {
    let limit = limit as usize;
    let has_more = rows.len() > limit;
    rows.truncate(limit);
    let last = rows
        .len()
        .checked_sub(1)
        .and_then(|i| stamps.get(i).copied());
    let next = last.or(since).map(format_cursor);
    Page {
        rows,
        next_updated_since: next,
        has_more,
    }
}

type ViewerDbRow = (
    String,
    String,
    String,
    NaiveDate,
    i32,
    i32,
    i32,
    i32,
    i32,
    DateTime<Utc>,
);

pub async fn list_viewer_points(
    pool: &PgPool,
    since: Option<DateTime<Utc>>,
    limit: i64,
) -> Result<Page<ViewerPointsRow>, sqlx::Error> {
    let rows: Vec<ViewerDbRow> = sqlx::query_as(
        "SELECT twitch_user_id, twitch_login, channel_twitch_user_id, day, watch_minutes,
                chat_messages, points_watch, points_chat, points_discovery, updated_at
           FROM twitch_community_points_viewer_daily
          WHERE updated_at > COALESCE($1, '-infinity'::timestamptz)
          ORDER BY updated_at, twitch_user_id, channel_twitch_user_id, day
          LIMIT $2",
    )
    .bind(since)
    .bind(limit + 1)
    .fetch_all(pool)
    .await?;
    let stamps = rows.iter().map(|r| r.9).collect();
    let rows = rows
        .into_iter()
        .map(|r| ViewerPointsRow {
            twitch_user_id: r.0,
            twitch_login: r.1,
            channel_twitch_user_id: r.2,
            day: r.3.format("%Y-%m-%d").to_string(),
            watch_minutes: r.4,
            chat_messages: r.5,
            points_watch: r.6,
            points_chat: r.7,
            points_discovery: r.8,
            updated_at: format_cursor(r.9),
        })
        .collect();
    Ok(finish_page(rows, stamps, limit, since))
}

type StreamerDbRow = (
    String,
    String,
    Option<String>,
    NaiveDate,
    i32,
    i32,
    i32,
    DateTime<Utc>,
);

pub async fn list_streamer_points(
    pool: &PgPool,
    since: Option<DateTime<Utc>>,
    limit: i64,
) -> Result<Page<StreamerPointsRow>, sqlx::Error> {
    let rows: Vec<StreamerDbRow> = sqlx::query_as(
        "SELECT streamer_twitch_user_id, streamer_login, discord_user_id, day, viewer_minutes,
                unique_viewers, raids_to_partners, updated_at
           FROM twitch_community_points_streamer_daily
          WHERE updated_at > COALESCE($1, '-infinity'::timestamptz)
          ORDER BY updated_at, streamer_twitch_user_id, day
          LIMIT $2",
    )
    .bind(since)
    .bind(limit + 1)
    .fetch_all(pool)
    .await?;
    let stamps = rows.iter().map(|r| r.7).collect();
    let rows = rows
        .into_iter()
        .map(|r| StreamerPointsRow {
            streamer_twitch_user_id: r.0,
            streamer_login: r.1,
            discord_user_id: r.2,
            day: r.3.format("%Y-%m-%d").to_string(),
            viewer_minutes: r.4,
            unique_viewers: r.5,
            raids_to_partners: r.6,
            updated_at: format_cursor(r.7),
        })
        .collect();
    Ok(finish_page(rows, stamps, limit, since))
}

#[cfg(test)]
#[path = "community_points_tests.rs"]
mod tests;
