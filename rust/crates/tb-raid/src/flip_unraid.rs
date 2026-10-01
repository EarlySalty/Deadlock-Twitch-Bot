use std::collections::HashMap;

pub const FLIP_WINDOW_DEFAULT_SECS: f64 = 90.0;
pub const FLIP_REPEAT_WINDOW_DEFAULT_SECS: f64 = 600.0;
pub const FLIP_PAUSE_DEFAULT_SECS: f64 = 43200.0;

pub fn pending_within_flip_window(registered_ts: f64, now_ts: f64, window_secs: f64) -> bool {
    let age = now_ts - registered_ts;
    if !age.is_finite() || age < 0.0 {
        return true;
    }
    age <= window_secs
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlipOutcome {
    First,
    RepeatEntersPause,
    RepeatAlreadyPaused,
}

#[derive(Debug, Default)]
pub struct FlipRepeatTracker {
    last_flip_by_source: HashMap<String, f64>,
    paused_until_by_source: HashMap<String, f64>,
}

impl FlipRepeatTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_flip(
        &mut self,
        source_id: &str,
        now: f64,
        repeat_window_secs: f64,
        pause_secs: f64,
    ) -> FlipOutcome {
        let key = source_id.trim();
        if key.is_empty() {
            return FlipOutcome::First;
        }
        let is_repeat = self
            .last_flip_by_source
            .get(key)
            .map(|&previous| {
                let gap = now - previous;
                gap.is_finite() && gap >= 0.0 && gap <= repeat_window_secs
            })
            .unwrap_or(false);
        self.last_flip_by_source.insert(key.to_string(), now);
        if !is_repeat {
            return FlipOutcome::First;
        }
        let already_paused = self
            .paused_until_by_source
            .get(key)
            .map(|&until| now <= until)
            .unwrap_or(false);
        if already_paused {
            return FlipOutcome::RepeatAlreadyPaused;
        }
        self.paused_until_by_source
            .insert(key.to_string(), now + pause_secs);
        FlipOutcome::RepeatEntersPause
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_innerhalb_des_fensters_ist_cancelbar() {
        assert!(pending_within_flip_window(1000.0, 1050.0, 90.0));
        assert!(
            pending_within_flip_window(1000.0, 1090.0, 90.0),
            "Grenze inklusive"
        );
    }

    #[test]
    fn pending_aelter_als_fenster_wird_nur_aufgeraeumt() {
        assert!(!pending_within_flip_window(1000.0, 1091.0, 90.0));
        assert!(!pending_within_flip_window(1000.0, 1600.0, 90.0));
    }

    #[test]
    fn kaputte_zeitstempel_gelten_als_frisch() {
        assert!(
            pending_within_flip_window(1000.0, 990.0, 90.0),
            "Uhr rueckwaerts"
        );
        assert!(pending_within_flip_window(f64::NAN, 1000.0, 90.0));
        assert!(pending_within_flip_window(1000.0, f64::NAN, 90.0));
    }

    #[test]
    fn erster_flip_loest_keine_pause_aus() {
        let mut tracker = FlipRepeatTracker::new();
        assert_eq!(
            tracker.record_flip("42", 1000.0, 600.0, 43200.0),
            FlipOutcome::First
        );
    }

    #[test]
    fn zweiter_flip_im_fenster_loest_pause_aus() {
        let mut tracker = FlipRepeatTracker::new();
        assert_eq!(
            tracker.record_flip("42", 1000.0, 600.0, 43200.0),
            FlipOutcome::First
        );
        assert_eq!(
            tracker.record_flip("42", 1300.0, 600.0, 43200.0),
            FlipOutcome::RepeatEntersPause
        );
    }

    #[test]
    fn zweiter_flip_ausserhalb_des_fensters_bleibt_erster() {
        let mut tracker = FlipRepeatTracker::new();
        assert_eq!(
            tracker.record_flip("42", 1000.0, 600.0, 43200.0),
            FlipOutcome::First
        );
        assert_eq!(
            tracker.record_flip("42", 1700.0, 600.0, 43200.0),
            FlipOutcome::First
        );
    }

    #[test]
    fn weiterer_flip_waehrend_der_pause_wird_entprellt() {
        let mut tracker = FlipRepeatTracker::new();
        tracker.record_flip("42", 1000.0, 600.0, 43200.0);
        assert_eq!(
            tracker.record_flip("42", 1300.0, 600.0, 43200.0),
            FlipOutcome::RepeatEntersPause
        );
        assert_eq!(
            tracker.record_flip("42", 1400.0, 600.0, 43200.0),
            FlipOutcome::RepeatAlreadyPaused,
            "gleiche Pause, keine zweite Nachricht"
        );
    }

    #[test]
    fn zwei_quellen_beeinflussen_sich_nicht() {
        let mut tracker = FlipRepeatTracker::new();
        assert_eq!(
            tracker.record_flip("a", 1000.0, 600.0, 43200.0),
            FlipOutcome::First
        );
        assert_eq!(
            tracker.record_flip("b", 1100.0, 600.0, 43200.0),
            FlipOutcome::First
        );
        assert_eq!(
            tracker.record_flip("a", 1200.0, 600.0, 43200.0),
            FlipOutcome::RepeatEntersPause
        );
    }

    #[test]
    fn leere_quelle_ist_noop() {
        let mut tracker = FlipRepeatTracker::new();
        assert_eq!(
            tracker.record_flip("  ", 1000.0, 600.0, 43200.0),
            FlipOutcome::First
        );
        assert_eq!(
            tracker.record_flip("", 1100.0, 600.0, 43200.0),
            FlipOutcome::First
        );
    }
}
