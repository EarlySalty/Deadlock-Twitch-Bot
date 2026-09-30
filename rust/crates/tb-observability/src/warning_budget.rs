use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

const MIN_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const ROLLING_WINDOW: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const MAX_WARNINGS: usize = 2;

#[derive(Default)]
pub struct WarningBudget {
    emitted_at: VecDeque<Instant>,
    suppressed_repeats: u64,
}

impl WarningBudget {
    /// Returns suppressed repeat count when this warning is allowed.
    pub fn allow(&mut self, now: Instant) -> Option<u64> {
        while self
            .emitted_at
            .front()
            .is_some_and(|emitted| now.saturating_duration_since(*emitted) >= ROLLING_WINDOW)
        {
            self.emitted_at.pop_front();
        }

        if self.emitted_at.len() >= MAX_WARNINGS
            || self
                .emitted_at
                .back()
                .is_some_and(|emitted| now.saturating_duration_since(*emitted) < MIN_INTERVAL)
        {
            self.suppressed_repeats = self.suppressed_repeats.saturating_add(1);
            return None;
        }

        let suppressed_repeats = std::mem::take(&mut self.suppressed_repeats);
        self.emitted_at.push_back(now);
        Some(suppressed_repeats)
    }
}

#[cfg(test)]
mod tests {
    use super::WarningBudget;
    use std::time::{Duration, Instant};

    #[test]
    fn allows_first_warning_and_coalesces_until_twenty_four_hours() {
        let start = Instant::now();
        let mut budget = WarningBudget::default();

        assert_eq!(budget.allow(start), Some(0));
        assert_eq!(budget.allow(start), None);
        assert_eq!(
            budget.allow(start + Duration::from_secs(24 * 60 * 60 - 1)),
            None
        );
        assert_eq!(
            budget.allow(start + Duration::from_secs(24 * 60 * 60)),
            Some(2)
        );
    }

    #[test]
    fn rolling_week_cap_reports_and_resets_suppression_count() {
        let start = Instant::now();
        let mut budget = WarningBudget::default();

        assert_eq!(budget.allow(start), Some(0));
        assert_eq!(
            budget.allow(start + Duration::from_secs(24 * 60 * 60)),
            Some(0)
        );
        assert_eq!(
            budget.allow(start + Duration::from_secs(2 * 24 * 60 * 60)),
            None
        );
        assert_eq!(
            budget.allow(start + Duration::from_secs(6 * 24 * 60 * 60)),
            None
        );
        assert_eq!(
            budget.allow(start + Duration::from_secs(7 * 24 * 60 * 60)),
            Some(2)
        );
        assert_eq!(
            budget.allow(start + Duration::from_secs(7 * 24 * 60 * 60)),
            None
        );
        assert_eq!(
            budget.allow(start + Duration::from_secs(8 * 24 * 60 * 60)),
            Some(1)
        );
    }
}
