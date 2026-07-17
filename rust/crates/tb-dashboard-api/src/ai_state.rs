//! Prozess-globaler In-Memory-State der KI-Analyse-/Chat-Endpunkte.
//!
//! Port der Modul-Globals aus `bot/analytics/api_ai.py`: `_in_progress_analyses`
//! (laufende Analysen), `_chat_sessions` (Folgechat-Sessions, 24h-Retention) und
//! `_minimax_hourly_counts` (stündliches Follow-up-Ratelimit). Python nutzt
//! Modul-Dicts (single-threaded async) — das Rust-Pendant ist ein globaler
//! `Mutex`. Die Methoden sind synchron; der Aufrufer hält den Lock NIE über den
//! LLM-`await` (Guard vor dem Call droppen).

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};

pub const MINIMAX_HOURLY_FOLLOW_UP_LIMIT: i64 = 10;
pub const OPUS_SESSION_FOLLOW_UP_LIMIT: i64 = 3;
pub const CHAT_SESSION_RETENTION_HOURS: i64 = 24;

pub const AI_MODEL_OPUS: &str = "opus";
pub const AI_MODEL_MINIMAX: &str = "minimax";

/// Pentest-Schalter (Python `_DDC_PENTEST_DISABLE_RATE_LIMITS`): jeder Env-Wert
/// außer den „aus"-Werten deaktiviert die Ratelimits. Default (unset) = aus.
fn pentest_disable_rate_limits() -> bool {
    match std::env::var("DDC_PENTEST_DISABLE_RATE_LIMITS") {
        Ok(v) => !matches!(
            v.trim().to_lowercase().as_str(),
            "" | "0" | "false" | "no" | "off"
        ),
        Err(_) => false,
    }
}

/// Eine Folgechat-Session (Python `_chat_sessions[key]`).
#[derive(Clone)]
pub struct ChatSession {
    pub model: String,
    pub streamer: String,
    pub analysis_id: i64,
    pub days: i64,
    pub game_filter: String,
    pub user_context: String,
    pub ctx: Value,
    pub points: Value,
    pub history: Vec<Value>,
    pub follow_up_count: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Default)]
pub struct AiState {
    in_progress: HashSet<String>,
    sessions: HashMap<String, ChatSession>,
    hourly_counts: HashMap<String, (i64, DateTime<Utc>)>,
}

/// Globaler State-Singleton.
pub static AI_STATE: LazyLock<Mutex<AiState>> = LazyLock::new(|| Mutex::new(AiState::default()));

/// Session-Schlüssel (Python `_chat_session_key`).
pub fn chat_session_key(streamer: &str, analysis_id: i64) -> String {
    format!("{streamer}_{analysis_id}")
}

impl AiState {
    /// Entfernt abgelaufene Sessions (>24h) + Ratelimit-Fenster (>1h) — Python
    /// `_cleanup_ai_chat_state`.
    pub fn cleanup(&mut self, now: DateTime<Utc>) {
        let session_cutoff = now - Duration::hours(CHAT_SESSION_RETENTION_HOURS);
        self.sessions.retain(|_, s| s.created_at >= session_cutoff);
        let counter_cutoff = now - Duration::hours(1);
        self.hourly_counts
            .retain(|_, (_, window_start)| *window_start >= counter_cutoff);
    }

    pub fn in_progress_contains(&self, streamer: &str) -> bool {
        self.in_progress.contains(streamer)
    }
    pub fn in_progress_add(&mut self, streamer: &str) {
        self.in_progress.insert(streamer.to_string());
    }
    pub fn in_progress_remove(&mut self, streamer: &str) {
        self.in_progress.remove(streamer);
    }

    pub fn get_session(&self, key: &str) -> Option<ChatSession> {
        self.sessions.get(key).cloned()
    }
    pub fn insert_session(&mut self, key: String, session: ChatSession) {
        self.sessions.insert(key, session);
    }

    /// Verbleibende Follow-ups (+ optionaler Reset-Timestamp). Setzt das
    /// MiniMax-Stundenfenster zurück, wenn abgelaufen (Python `_remaining_follow_ups`).
    pub fn remaining_follow_ups(
        &mut self,
        streamer: &str,
        model: &str,
        follow_up_count: i64,
        now: DateTime<Utc>,
    ) -> (i64, Option<i64>) {
        if pentest_disable_rate_limits() {
            return (1_000_000_000, None);
        }
        if model == AI_MODEL_OPUS {
            return (
                (OPUS_SESSION_FOLLOW_UP_LIMIT - follow_up_count).max(0),
                None,
            );
        }
        let (mut count, mut window_start) = self
            .hourly_counts
            .get(streamer)
            .copied()
            .unwrap_or((0, now));
        if now - window_start >= Duration::hours(1) {
            count = 0;
            window_start = now;
            self.hourly_counts
                .insert(streamer.to_string(), (count, window_start));
        }
        let remaining = (MINIMAX_HOURLY_FOLLOW_UP_LIMIT - count).max(0);
        let reset_ts = (window_start + Duration::hours(1)).timestamp();
        (remaining, Some(reset_ts))
    }

    /// Hängt User-/Assistant-Nachricht an die Session-History und verbucht den
    /// Follow-up (Python: history.append ×2 + `_consume_follow_up`). Gibt die
    /// danach verbleibenden Follow-ups zurück.
    pub fn record_and_consume(
        &mut self,
        key: &str,
        streamer: &str,
        user_msg: &str,
        reply: &str,
        now: DateTime<Utc>,
    ) -> (i64, Option<i64>) {
        let now_iso = now.to_rfc3339_opts(chrono::SecondsFormat::Micros, false);
        let model = self
            .sessions
            .get(key)
            .map(|s| s.model.clone())
            .unwrap_or_default();
        if let Some(s) = self.sessions.get_mut(key) {
            s.history
                .push(json!({ "role": "user", "content": user_msg, "timestamp": now_iso }));
            s.history
                .push(json!({ "role": "assistant", "content": reply, "timestamp": now_iso }));
        }

        if pentest_disable_rate_limits() {
            let fc = self
                .sessions
                .get(key)
                .map(|s| s.follow_up_count)
                .unwrap_or(0);
            return self.remaining_follow_ups(streamer, &model, fc, now);
        }
        if model == AI_MODEL_OPUS {
            if let Some(s) = self.sessions.get_mut(key) {
                s.follow_up_count += 1;
            }
            let fc = self
                .sessions
                .get(key)
                .map(|s| s.follow_up_count)
                .unwrap_or(0);
            return self.remaining_follow_ups(streamer, &model, fc, now);
        }
        // MiniMax: Stundenzähler erhöhen (Fenster ggf. zurücksetzen).
        let (mut count, mut window_start) = self
            .hourly_counts
            .get(streamer)
            .copied()
            .unwrap_or((0, now));
        if now - window_start >= Duration::hours(1) {
            count = 0;
            window_start = now;
        }
        count += 1;
        self.hourly_counts
            .insert(streamer.to_string(), (count, window_start));
        let fc = self
            .sessions
            .get(key)
            .map(|s| s.follow_up_count)
            .unwrap_or(0);
        self.remaining_follow_ups(streamer, &model, fc, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(model: &str, follow_up_count: i64, created_at: DateTime<Utc>) -> ChatSession {
        ChatSession {
            model: model.to_string(),
            streamer: "nani".into(),
            analysis_id: 1,
            days: 30,
            game_filter: "all".into(),
            user_context: String::new(),
            ctx: json!({}),
            points: json!([]),
            history: Vec::new(),
            follow_up_count,
            created_at,
        }
    }

    #[test]
    fn session_key_format() {
        assert_eq!(chat_session_key("nani", 42), "nani_42");
    }

    #[test]
    fn opus_limit_3() {
        let mut st = AiState::default();
        let now = Utc::now();
        assert_eq!(st.remaining_follow_ups("nani", AI_MODEL_OPUS, 0, now).0, 3);
        assert_eq!(st.remaining_follow_ups("nani", AI_MODEL_OPUS, 2, now).0, 1);
        assert_eq!(st.remaining_follow_ups("nani", AI_MODEL_OPUS, 3, now).0, 0);
        // Über Limit → 0 (nicht negativ).
        assert_eq!(st.remaining_follow_ups("nani", AI_MODEL_OPUS, 5, now).0, 0);
        // Opus hat keinen reset_ts.
        assert_eq!(
            st.remaining_follow_ups("nani", AI_MODEL_OPUS, 0, now).1,
            None
        );
    }

    #[test]
    fn minimax_consume_und_reset() {
        let mut st = AiState::default();
        let now = Utc::now();
        st.insert_session("nani_1".into(), session(AI_MODEL_MINIMAX, 0, now));
        // Frisch: 10 übrig, reset_ts gesetzt.
        let (rem, reset) = st.remaining_follow_ups("nani", AI_MODEL_MINIMAX, 0, now);
        assert_eq!(rem, 10);
        assert!(reset.is_some());
        // Ein Consume → 9 übrig.
        let (rem_after, _) = st.record_and_consume("nani_1", "nani", "frage", "antwort", now);
        assert_eq!(rem_after, 9);
        // History gefüllt (user + assistant).
        let s = st.get_session("nani_1").unwrap();
        assert_eq!(s.history.len(), 2);
        assert_eq!(s.history[0]["role"], "user");
        assert_eq!(s.history[1]["content"], "antwort");
        // Stunde später → Fenster-Reset → wieder 10.
        let later = now + Duration::hours(2);
        assert_eq!(
            st.remaining_follow_ups("nani", AI_MODEL_MINIMAX, 0, later)
                .0,
            10
        );
    }

    #[test]
    fn opus_consume_erhoeht_session_count() {
        let mut st = AiState::default();
        let now = Utc::now();
        st.insert_session("nani_1".into(), session(AI_MODEL_OPUS, 0, now));
        let (rem, _) = st.record_and_consume("nani_1", "nani", "f", "a", now);
        assert_eq!(rem, 2); // 3 - 1 verbraucht
        assert_eq!(st.get_session("nani_1").unwrap().follow_up_count, 1);
    }

    #[test]
    fn cleanup_entfernt_alte() {
        let mut st = AiState::default();
        let now = Utc::now();
        st.insert_session("frisch".into(), session(AI_MODEL_OPUS, 0, now));
        st.insert_session(
            "alt".into(),
            session(AI_MODEL_OPUS, 0, now - Duration::hours(25)),
        );
        st.hourly_counts.insert("recent".into(), (3, now));
        st.hourly_counts
            .insert("stale".into(), (3, now - Duration::hours(2)));
        st.cleanup(now);
        assert!(st.get_session("frisch").is_some());
        assert!(st.get_session("alt").is_none());
        assert!(st.hourly_counts.contains_key("recent"));
        assert!(!st.hourly_counts.contains_key("stale"));
    }
}
