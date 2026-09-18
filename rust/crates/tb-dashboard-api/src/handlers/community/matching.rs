//! Deterministic co-play matching. Stream schedules are observations, not bookings.
use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use chrono_tz::Europe::Berlin;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const SLOT_COUNT: usize = 7 * 48;
pub const RANK_NAMES: [&str; 11] = ["Initiate", "Seeker", "Alchemist", "Arcanist", "Ritualist", "Emissary", "Archon", "Oracle", "Phantom", "Ascendant", "Eternus"];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GameMode { Normal, StreetBrawl, Sandbox, Custom }

/// Titles are weak intent evidence. Ambiguous/negated titles stay unknown.
pub fn title_mode(title: &str) -> Option<GameMode> {
    let text = title.to_lowercase();
    let words: Vec<_> = text.split(|c: char| !c.is_alphanumeric()).filter(|s| !s.is_empty()).collect();
    if words.iter().any(|s| ["kein", "keine", "keinen", "nicht", "no", "not", "später", "later"].contains(s)) { return None; }
    let mut modes = Vec::new();
    if text.contains("street brawl") || text.contains("streetbrawl") || words.contains(&"brawl") { modes.push(GameMode::StreetBrawl); }
    if words.contains(&"sandbox") { modes.push(GameMode::Sandbox); }
    if words.iter().any(|s| ["scrim", "scrims", "custom", "turnier"].contains(s)) { modes.push(GameMode::Custom); }
    if words.iter().any(|s| ["ranked", "normal", "normals", "casual"].contains(s)) { modes.push(GameMode::Normal); }
    (modes.len() == 1).then(|| modes[0])
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct PlayerProfile {
    pub rank_name: Option<String>,
    pub rank_tier: Option<u8>,
    pub subrank: Option<u8>,
    pub rank_updated_at: Option<i64>,
    pub mode: Option<GameMode>,
    pub mode_source: Option<String>,
    pub mode_samples: usize,
    pub history_updated_at: Option<i64>,
    pub source_status: String,
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct Session {
    pub streamer_login: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub game_name: Option<String>,
    pub stream_title: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Schedule {
    pub slots: Vec<f64>,
    pub sessions: usize,
    pub games: Vec<String>,
    #[serde(skip)]
    pub intervals: Vec<(DateTime<Utc>, DateTime<Utc>)>,
}

/// Merge intervals first so duplicate/overlapping session rows cannot boost a match.
fn merge_intervals(mut intervals: Vec<(DateTime<Utc>, DateTime<Utc>)>) -> Vec<(DateTime<Utc>, DateTime<Utc>)> {
    intervals.sort_unstable();
    let mut merged: Vec<(DateTime<Utc>, DateTime<Utc>)> = Vec::new();
    for (start, end) in intervals {
        if let Some(last) = merged.last_mut() {
            if start <= last.1 { last.1 = last.1.max(end); continue; }
        }
        merged.push((start, end));
    }
    merged
}

pub fn slot_index(at: DateTime<Utc>) -> usize {
    let local = at.with_timezone(&Berlin);
    local.weekday().num_days_from_monday() as usize * 48 + local.hour() as usize * 2 + usize::from(local.minute() >= 30)
}

pub fn schedule(sessions: &[Session], since: DateTime<Utc>, now: DateTime<Utc>) -> Schedule {
    let valid: Vec<_> = sessions.iter().filter(|s| s.ended_at > s.started_at && s.ended_at <= now
        && s.ended_at > since && s.started_at < now && s.ended_at - s.started_at <= Duration::hours(48)).collect();
    let intervals = merge_intervals(valid.iter().map(|s| (s.started_at.max(since), s.ended_at.min(now))).collect());
    let mut slots = vec![0.0_f64; SLOT_COUNT];
    let mut denominator = vec![0.0_f64; SLOT_COUNT];
    let weight = |at: DateTime<Utc>| 2.0_f64.powf(-(now - at).num_seconds().max(0) as f64 / (21.0 * 86400.0));
    let rounded = |at: DateTime<Utc>| DateTime::from_timestamp(at.timestamp().div_euclid(1800) * 1800, 0).expect("valid session timestamp");
    // Real UTC instants converted individually: handles midnight, week boundaries
    // and both DST transitions without assuming a fixed UTC+1/+2 offset.
    let mut cursor = rounded(since);
    while cursor < now {
        let end = (cursor + Duration::minutes(30)).min(now);
        let fraction = (end - cursor.max(since)).num_seconds().max(0) as f64 / 1800.0;
        denominator[slot_index(cursor)] += fraction * weight(cursor);
        cursor += Duration::minutes(30);
    }
    for &(start, end) in &intervals {
        let mut cursor = rounded(start);
        while cursor < end {
            let length = ((cursor + Duration::minutes(30)).min(end) - cursor.max(start)).num_seconds().max(0) as f64;
            slots[slot_index(cursor)] += length / 1800.0 * weight(cursor);
            cursor += Duration::minutes(30);
        }
    }
    for (value, denom) in slots.iter_mut().zip(denominator) {
        *value = if denom > 0.0 { (*value / denom).clamp(0.0, 1.0) } else { 0.0 };
    }
    let games = valid.iter().filter_map(|s| s.game_name.as_deref()).map(str::trim)
        .filter(|s| !s.is_empty()).map(str::to_lowercase).collect::<BTreeSet<_>>().into_iter().collect();
    // Count distinct sessions, not duplicate DB rows.
    let count = valid.iter().map(|s| (s.started_at, s.ended_at)).collect::<BTreeSet<_>>().len();
    Schedule { slots, sessions: count, games, intervals }
}

pub fn observed_overlap_minutes(a: &Schedule, b: &Schedule) -> u64 {
    let (mut i, mut j, mut seconds) = (0, 0, 0_i64);
    while i < a.intervals.len() && j < b.intervals.len() {
        let (a0, a1) = a.intervals[i]; let (b0, b1) = b.intervals[j];
        seconds += (a1.min(b1) - a0.max(b0)).num_seconds().max(0);
        if a1 < b1 { i += 1; } else { j += 1; }
    }
    (seconds / 60) as u64
}

#[derive(Debug, Clone, Serialize)]
pub struct SharedWindow {
    /// ISO weekday, 1=Monday. Local wall-clock minutes, end may equal 1440.
    pub weekday: u8,
    pub start_minute: u16,
    pub end_minute: u16,
    pub strength: f64,
}

pub fn shared_windows(a: &Schedule, b: &Schedule) -> Vec<SharedWindow> {
    let mut windows: Vec<SharedWindow> = Vec::new();
    for i in 0..SLOT_COUNT {
        let strength = a.slots[i].min(b.slots[i]);
        if strength < 0.15 { continue; }
        let day = (i / 48 + 1) as u8;
        let start = (i % 48 * 30) as u16;
        if let Some(last) = windows.last_mut() {
            if last.weekday == day && last.end_minute == start {
                last.end_minute += 30;
                last.strength = last.strength.min(strength);
                continue;
            }
        }
        windows.push(SharedWindow { weekday: day, start_minute: start, end_minute: start + 30, strength });
    }
    windows.sort_by(|a, b| {
        b.strength.total_cmp(&a.strength)
            .then((b.end_minute-b.start_minute).cmp(&(a.end_minute-a.start_minute)))
            .then(a.weekday.cmp(&b.weekday))
            .then(a.start_minute.cmp(&b.start_minute))
    });
    windows.truncate(4);
    windows
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchScore {
    pub score: Option<u8>,
    pub schedule_overlap_pct: Option<u8>,
    pub observed_overlap_minutes: u64,
    pub shared_games: Vec<String>,
    pub shared_windows: Vec<SharedWindow>,
    pub compatible: bool,
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn match_score(own: &Schedule, candidate: &Schedule, own_profile: &PlayerProfile, other: &PlayerProfile) -> MatchScore {
    let shared_games = own.games.iter().filter(|g| candidate.games.contains(g)).cloned().collect::<Vec<_>>();
    let sum = own.slots.iter().sum::<f64>() + candidate.slots.iter().sum::<f64>();
    let shared = own.slots.iter().zip(&candidate.slots).map(|(a,b)| a.min(*b)).sum::<f64>();
    let enough = own.sessions >= 3 && candidate.sessions >= 3 && sum > 0.0;
    let overlap = if sum > 0.0 { 2.0 * shared / sum } else { 0.0 };
    let mut points = overlap * 65.0;
    let mut reasons = Vec::new(); let mut warnings = Vec::new(); let mut compatible = true;
    if enough && overlap > 0.0 { reasons.push("Ähnliche historische Streamzeiten".into()); }
    if !shared_games.is_empty() { points += 15.0; reasons.push("Gemeinsame Spiele in der Stream-Historie".into()); }
    match (own_profile.rank_tier, other.rank_tier) {
        (Some(a), Some(b)) => {
            let difference = a.abs_diff(b);
            if difference <= 2 { points += (15.0 - difference as f64 * 5.0).max(0.0); reasons.push("Ähnlicher bestätigter Steam-Rang".into()); }
            else { warnings.push("Deutlicher Rangunterschied – vorher absprechen".into()); compatible = false; }
        }
        _ => warnings.push("Rangvergleich nicht verfügbar".into()),
    }
    match (own_profile.mode, other.mode) {
        (Some(a), Some(b)) if a == b => {
            points += if own_profile.mode_source.as_deref() == Some("steam_history") && other.mode_source.as_deref() == Some("steam_history") { 5.0 } else { 2.0 };
            reasons.push("Gleicher erkannter Spielmodus".into());
        }
        (Some(_), Some(_)) => { compatible = false; warnings.push("Unterschiedliche erkannte Spielmodi".into()); }
        _ => warnings.push("Spielmodus nicht auf beiden Seiten bekannt".into()),
    }
    if !enough { warnings.push("Noch wenig Stream-Historie: mindestens drei Sessions je Person für eine Wertung".into()); }
    MatchScore { score: enough.then(|| points.round().clamp(0.0,100.0) as u8), schedule_overlap_pct: enough.then(|| (overlap *100.0).round() as u8),
        observed_overlap_minutes: observed_overlap_minutes(own,candidate), shared_games, shared_windows: shared_windows(own,candidate), compatible, reasons, warnings }
}

/// Only explicit protocol values count. `not_scored` does NOT mean a mode.
pub fn history_mode(matches: &[serde_json::Value], now: i64) -> (Option<GameMode>, usize) {
    let mut counts: BTreeMap<i64, usize> = BTreeMap::new();
    for m in matches.iter().take(30) {
        let Some(start) = m.get("start_time").and_then(|v| v.as_i64()) else { continue; };
        if start > now || now-start > 14*86400 { continue; }
        let Some(mode @ (1 | 3 | 4)) = m.get("game_mode").and_then(|v| v.as_i64()) else { continue; };
        *counts.entry(mode).or_default() += 1;
    }
    let total: usize = counts.values().sum();
    let best = counts.into_iter().max_by_key(|(_, count)| *count);
    match best {
        Some((mode, count)) if count >= 3 && count * 100 >= total * 60 => (Some(match mode { 1 => GameMode::Normal, 3 => GameMode::Sandbox, _ => GameMode::StreetBrawl }), total),
        _ => (None, total),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn at(v: &str) -> DateTime<Utc> { v.parse().unwrap() }
    fn session(start: &str, end: &str) -> Session { Session {streamer_login:"test".into(), started_at:at(start), ended_at:at(end), game_name:Some("Deadlock".into()),stream_title:None} }
    #[test] fn midnight_and_sunday_wrap_are_local() {
        assert_eq!(slot_index(at("2026-09-13T22:00:00Z")),0);
        assert_eq!(slot_index(at("2026-09-13T21:30:00Z")),335);
    }
    #[test] fn dst_uses_real_instants() {
        assert_eq!(slot_index(at("2026-03-29T00:30:00Z")),6*48+3);
        assert_eq!(slot_index(at("2026-03-29T01:00:00Z")),6*48+6);
        assert_eq!(slot_index(at("2026-10-25T00:00:00Z")),slot_index(at("2026-10-25T01:00:00Z")));
    }
    #[test] fn duplicates_and_future_sessions_do_not_boost_history() {
        let s=session("2026-09-10T18:00:00Z","2026-09-10T20:00:00Z");
        let single=schedule(&[s.clone()],at("2026-09-01T00:00:00Z"),at("2026-09-18T00:00:00Z"));
        let duplicate=schedule(&[s.clone(),s,session("2026-09-20T18:00:00Z","2026-09-20T20:00:00Z")],at("2026-09-01T00:00:00Z"),at("2026-09-18T00:00:00Z"));
        assert_eq!(single.slots,duplicate.slots); assert_eq!(duplicate.sessions,1);
        assert_eq!(observed_overlap_minutes(&single,&duplicate),120);
    }
    #[test] fn disjoint_halves_are_not_observed_overlap() {
        let a=schedule(&[session("2026-09-10T18:00:00Z","2026-09-10T18:15:00Z")],at("2026-09-01T00:00:00Z"),at("2026-09-18T00:00:00Z"));
        let b=schedule(&[session("2026-09-10T18:15:00Z","2026-09-10T18:30:00Z")],at("2026-09-01T00:00:00Z"),at("2026-09-18T00:00:00Z"));
        assert_eq!(observed_overlap_minutes(&a,&b),0);
    }
    #[test] fn unknown_is_not_perfect_match() {
        let empty=schedule(&[],at("2026-09-01T00:00:00Z"),at("2026-09-18T00:00:00Z"));
        let score=match_score(&empty,&empty,&PlayerProfile::default(),&PlayerProfile::default());
        assert_eq!(score.score,None); assert!(score.reasons.is_empty());
    }
    #[test] fn conflicting_modes_and_far_ranks_are_flagged() {
        let empty=schedule(&[],at("2026-09-01T00:00:00Z"),at("2026-09-18T00:00:00Z"));
        let own=PlayerProfile {rank_tier:Some(3),mode:Some(GameMode::Normal),..Default::default()};
        let other=PlayerProfile {rank_tier:Some(9),mode:Some(GameMode::StreetBrawl),..Default::default()};
        assert!(!match_score(&empty,&empty,&own,&other).compatible);
    }
    #[test] fn titles_are_conservative() {
        assert_eq!(title_mode("Street Brawl mit euch"),Some(GameMode::StreetBrawl));
        assert_eq!(title_mode("kein ranked heute"),None);
        assert_eq!(title_mode("Ranked und danach Brawl"),None);
        assert_eq!(title_mode("BrawlerFan spielt"),None);
    }
    #[test] fn mode_requires_recent_explicit_evidence() {
        let now=1_800_000_000;
        let matches=vec![serde_json::json!({"game_mode":4,"start_time":now-100});3];
        assert_eq!(history_mode(&matches,now),(Some(GameMode::StreetBrawl),3));
        assert_eq!(history_mode(&[serde_json::json!({"not_scored":true,"start_time":now})],now),(None,0));
        assert_eq!(history_mode(&matches,now+15*86400),(None,0));
    }
}
