//! Konversations-Rhythmik: Anti-Flood + Anti-Burst (Port von
//! `bot/engagement/rhythm.py`).
//!
//! Drei einfache Gates statt Cooldown-Slider:
//! - Anti-Flood: letzter Bot-Post < `min_pause_sec` (default 5s) → kein Call.
//! - Anti-Burst: ≥ `burst_limit` (default 3) Bot-Posts in `burst_window_sec`
//!   (default 60s) ohne dazwischenliegende User-Reaktion → kein Call bis zur
//!   nächsten User-Message.
//!
//! In-Memory-State pro Channel (nach Restart leer — tolerierbar). `Mutex` weil
//! auch Background-Jobs Posts notieren könnten. `now` wird (wie in Python)
//! explizit übergeben → deterministisch testbar.

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};

#[derive(Debug, Default)]
struct ChannelRhythmState {
    bot_post_times: Vec<DateTime<Utc>>,
    user_post_since_last_bot: bool,
}

/// Anti-Flood- + Anti-Burst-Logik über alle Channels.
pub struct RhythmGuard {
    min_pause_sec: f64,
    burst_limit: usize,
    burst_window_sec: f64,
    state: Mutex<HashMap<String, ChannelRhythmState>>,
}

impl RhythmGuard {
    /// Baut den Guard; `None`-Parameter ziehen aus den `ENGAGEMENT_*`-Env-Vars
    /// bzw. den Defaults (5s / 3 / 60s).
    pub fn new(
        min_pause_sec: Option<f64>,
        burst_limit: Option<usize>,
        burst_window_sec: Option<f64>,
    ) -> Self {
        Self {
            min_pause_sec: min_pause_sec
                .unwrap_or_else(|| env_float("ENGAGEMENT_MIN_PAUSE_SEC", 5.0)),
            burst_limit: burst_limit.unwrap_or_else(|| env_int("ENGAGEMENT_BURST_LIMIT", 3)),
            burst_window_sec: burst_window_sec
                .unwrap_or_else(|| env_float("ENGAGEMENT_BURST_WINDOW_SEC", 60.0)),
            state: Mutex::new(HashMap::new()),
        }
    }

    /// True, wenn seit dem letzten Bot-Post mindestens `min_pause_sec` vergangen
    /// sind (oder noch nie gepostet wurde).
    pub fn anti_flood_ok(&self, channel_login: &str, now: DateTime<Utc>) -> bool {
        let mut guard = self.lock();
        let state = guard.entry(channel_login.to_string()).or_default();
        match state.bot_post_times.last() {
            None => true,
            Some(last) => secs_between(*last, now) >= self.min_pause_sec,
        }
    }

    /// True, wenn seit dem letzten Bot-Post eine User-Message kam, oder weniger
    /// als `burst_limit` Bot-Posts im aktuellen Window liegen.
    pub fn anti_burst_ok(&self, channel_login: &str, now: DateTime<Utc>) -> bool {
        let mut guard = self.lock();
        let state = guard.entry(channel_login.to_string()).or_default();
        if state.user_post_since_last_bot {
            return true;
        }
        let window_start = now - dur_secs(self.burst_window_sec);
        let recent = state
            .bot_post_times
            .iter()
            .filter(|&&t| t >= window_start)
            .count();
        recent < self.burst_limit
    }

    /// Notiert einen Bot-Post, trimmt den Buffer (2× Window) und setzt das
    /// User-Reaktions-Flag zurück.
    pub fn note_bot_post(&self, channel_login: &str, now: DateTime<Utc>) {
        let mut guard = self.lock();
        let state = guard.entry(channel_login.to_string()).or_default();
        state.bot_post_times.push(now);
        let cutoff = now - dur_secs(self.burst_window_sec * 2.0);
        state.bot_post_times.retain(|&t| t >= cutoff);
        state.user_post_since_last_bot = false;
    }

    /// Notiert eine User-Message (hebt die Burst-Sperre bis zum nächsten Bot-Post).
    pub fn note_user_post(&self, channel_login: &str) {
        let mut guard = self.lock();
        let state = guard.entry(channel_login.to_string()).or_default();
        state.user_post_since_last_bot = true;
    }

    /// Lock, der auch bei Poisoning die Daten zurückgibt (statt zu panicken).
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, ChannelRhythmState>> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }
}

fn secs_between(earlier: DateTime<Utc>, later: DateTime<Utc>) -> f64 {
    (later - earlier).num_milliseconds() as f64 / 1000.0
}

fn dur_secs(seconds: f64) -> Duration {
    Duration::milliseconds((seconds * 1000.0) as i64)
}

fn env_float(name: &str, default: f64) -> f64 {
    match std::env::var(name) {
        Ok(raw) if !raw.is_empty() => raw.trim().parse::<f64>().unwrap_or(default),
        _ => default,
    }
}

fn env_int(name: &str, default: usize) -> usize {
    match std::env::var(name) {
        Ok(raw) if !raw.is_empty() => raw.trim().parse::<usize>().unwrap_or(default),
        _ => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> DateTime<Utc> {
        DateTime::from_timestamp(1_000_000, 0).unwrap()
    }

    #[test]
    fn anti_flood_respektiert_pause() {
        let guard = RhythmGuard::new(Some(5.0), Some(3), Some(60.0));
        let t0 = base();
        assert!(guard.anti_flood_ok("ch", t0)); // kein Post → ok
        guard.note_bot_post("ch", t0);
        assert!(!guard.anti_flood_ok("ch", t0 + Duration::seconds(3))); // in Pause
        assert!(guard.anti_flood_ok("ch", t0 + Duration::seconds(5))); // Pause vorbei
    }

    #[test]
    fn anti_burst_limit_und_user_reset() {
        let guard = RhythmGuard::new(Some(0.0), Some(3), Some(60.0));
        let t0 = base();
        guard.note_bot_post("ch", t0);
        guard.note_bot_post("ch", t0 + Duration::seconds(1));
        guard.note_bot_post("ch", t0 + Duration::seconds(2));
        // 3 Posts ohne User-Reaktion im Window → Burst gesperrt.
        assert!(!guard.anti_burst_ok("ch", t0 + Duration::seconds(3)));
        // User-Message hebt die Sperre.
        guard.note_user_post("ch");
        assert!(guard.anti_burst_ok("ch", t0 + Duration::seconds(3)));
    }

    #[test]
    fn anti_burst_window_verfall() {
        let guard = RhythmGuard::new(Some(0.0), Some(3), Some(60.0));
        let t0 = base();
        guard.note_bot_post("ch", t0);
        guard.note_bot_post("ch", t0 + Duration::seconds(1));
        guard.note_bot_post("ch", t0 + Duration::seconds(2));
        // Bei t0+61 ist der erste Post aus dem 60s-Window → nur 2 zählen → ok.
        assert!(guard.anti_burst_ok("ch", t0 + Duration::seconds(61)));
    }

    #[test]
    fn note_bot_post_reset_user_flag() {
        let guard = RhythmGuard::new(Some(0.0), Some(3), Some(60.0));
        let t0 = base();
        guard.note_user_post("ch");
        // Bot-Post setzt das Flag zurück → danach zählt der Burst wieder.
        guard.note_bot_post("ch", t0);
        guard.note_bot_post("ch", t0 + Duration::seconds(1));
        guard.note_bot_post("ch", t0 + Duration::seconds(2));
        assert!(!guard.anti_burst_ok("ch", t0 + Duration::seconds(3)));
    }
}
