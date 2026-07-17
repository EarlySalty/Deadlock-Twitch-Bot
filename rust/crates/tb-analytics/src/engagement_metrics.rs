//! Engagement-Kennzahlen + Perzentil-Helfer (geteilte Datenschicht).
//!
//! Port von `bot/analytics/engagement_metrics.py:calculate_engagement` +
//! den Perzentil-Helfern aus `api_insights.py` (`_interpolated_percentile` =
//! [`quantile`], `_percentile_of` = [`percentile_of`]).
//!
//! Vorher lag `calculate_engagement`/`quantile` lokal im Handler
//! `audience_demographics.rs`; hierher gezogen, damit auch `chat-analytics`
//! denselben Code nutzt (Dedup, korrekte Schicht). `active_ratio` (round3, wie
//! Python) im Output ergänzt — von chat-analytics gebraucht.

pub struct EngagementInputs {
    pub total_messages: i64,
    pub active_chatters: usize,
    pub tracked_chat_accounts: usize,
    pub chatters_api_seen: usize,
    pub viewer_minutes: f64,
    pub viewer_minutes_has_real_samples: bool,
    pub avg_viewers: f64,
    pub session_count: i64,
    pub sessions_with_chat: i64,
}

pub struct EngagementOutputs {
    pub chat_penetration_pct: Option<f64>,
    pub chat_penetration_reliable: bool,
    pub messages_per_100_viewer_minutes: Option<f64>,
    pub viewer_minutes: f64,
    pub legacy_interaction_active_per_avg_viewer: Option<f64>,
    pub passive_viewer_samples: i64,
    pub chatters_coverage: f64,
    pub active_ratio: f64,
    pub method: &'static str,
    pub chat_session_coverage: f64,
}

fn safe_ratio(num: f64, den: f64) -> f64 {
    if den <= 0.0 {
        0.0
    } else {
        num / den
    }
}

pub fn calculate_engagement(inp: &EngagementInputs) -> EngagementOutputs {
    let tracked = inp.tracked_chat_accounts as f64;
    let active = inp.active_chatters as f64;
    let api_seen = inp.chatters_api_seen as f64;
    let msgs = inp.total_messages.max(0) as f64;
    let vm = inp.viewer_minutes.max(0.0);
    let avg_v = inp.avg_viewers.max(0.0);
    let sessions = inp.session_count.max(0) as f64;
    let chat_sess = inp.sessions_with_chat.max(0) as f64;

    let passive = ((tracked as i64) - (inp.active_chatters as i64)).max(0);
    let chatters_coverage = safe_ratio(api_seen, tracked);
    let active_ratio = safe_ratio(active, tracked);
    let chat_penetration_pct = if tracked > 0.0 {
        Some((active_ratio * 100.0 * 10.0).round() / 10.0)
    } else {
        None
    };
    let messages_per_100 = if vm > 0.0 {
        Some((msgs / vm * 100.0 * 100.0).round() / 100.0)
    } else {
        None
    };
    let legacy = if avg_v > 0.0 {
        Some((active / avg_v * 100.0 * 10.0).round() / 10.0)
    } else {
        None
    };
    let reliable = passive >= 1 && chatters_coverage >= 0.2;
    let has_data = tracked > 0.0 || active > 0.0 || msgs > 0.0 || vm > 0.0;
    let method: &'static str = if !has_data {
        "no_data"
    } else if reliable && inp.viewer_minutes_has_real_samples {
        "real_samples"
    } else {
        "low_coverage"
    };

    EngagementOutputs {
        chat_penetration_pct,
        chat_penetration_reliable: reliable,
        messages_per_100_viewer_minutes: messages_per_100,
        viewer_minutes: (vm * 100.0).round() / 100.0,
        legacy_interaction_active_per_avg_viewer: legacy,
        passive_viewer_samples: passive,
        chatters_coverage: (chatters_coverage * 1000.0).round() / 1000.0,
        active_ratio: (active_ratio * 1000.0).round() / 1000.0,
        method,
        chat_session_coverage: (safe_ratio(chat_sess, sessions) * 1000.0).round() / 1000.0,
    }
}

/// Interpolierter Perzentil (Python `_interpolated_percentile`). Eingabe MUSS sortiert sein.
pub fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let pos = q.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    if lo == hi {
        return sorted[lo];
    }
    let frac = pos - lo as f64;
    sorted[lo] + (sorted[hi] - sorted[lo]) * frac
}

/// Perzentil-Rang (0..1) von `value` in `sorted_avgs` (Python `_percentile_of`).
pub fn percentile_of(sorted_avgs: &[f64], value: f64) -> f64 {
    if sorted_avgs.is_empty() {
        return 0.5;
    }
    let below = sorted_avgs.iter().filter(|&&v| v < value).count() as f64;
    let equal = sorted_avgs.iter().filter(|&&v| v == value).count() as f64;
    (below + 0.5 * equal) / sorted_avgs.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantile_interpoliert() {
        let s = [10.0, 20.0, 30.0, 40.0];
        assert_eq!(quantile(&s, 0.5), 25.0); // (3*0.5=1.5) → 20+(30-20)*0.5
        assert_eq!(quantile(&s, 0.0), 10.0);
        assert_eq!(quantile(&s, 1.0), 40.0);
        assert_eq!(quantile(&[], 0.5), 0.0);
        assert_eq!(quantile(&[7.0], 0.5), 7.0);
    }

    #[test]
    fn percentile_rang() {
        let s = [10.0, 20.0, 20.0, 30.0];
        // value 20: below=1, equal=2 → (1 + 0.5*2)/4 = 0.5
        assert_eq!(percentile_of(&s, 20.0), 0.5);
        assert_eq!(percentile_of(&s, 5.0), 0.0); // below=0, equal=0
        assert_eq!(percentile_of(&[], 1.0), 0.5);
    }

    #[test]
    fn engagement_basis() {
        let out = calculate_engagement(&EngagementInputs {
            total_messages: 100,
            active_chatters: 5,
            tracked_chat_accounts: 10,
            chatters_api_seen: 4,
            viewer_minutes: 200.0,
            viewer_minutes_has_real_samples: true,
            avg_viewers: 50.0,
            session_count: 4,
            sessions_with_chat: 2,
        });
        assert_eq!(out.active_ratio, 0.5); // 5/10
        assert_eq!(out.chat_penetration_pct, Some(50.0));
        assert_eq!(out.messages_per_100_viewer_minutes, Some(50.0)); // 100/200*100
        assert_eq!(out.chatters_coverage, 0.4); // 4/10
        assert_eq!(out.chat_session_coverage, 0.5); // 2/4
        assert_eq!(out.method, "real_samples"); // passive=5>=1, coverage 0.4>=0.2, real_samples
    }

    #[test]
    fn engagement_no_data() {
        let out = calculate_engagement(&EngagementInputs {
            total_messages: 0,
            active_chatters: 0,
            tracked_chat_accounts: 0,
            chatters_api_seen: 0,
            viewer_minutes: 0.0,
            viewer_minutes_has_real_samples: false,
            avg_viewers: 0.0,
            session_count: 0,
            sessions_with_chat: 0,
        });
        assert_eq!(out.method, "no_data");
        assert_eq!(out.chat_penetration_pct, None);
    }
}
