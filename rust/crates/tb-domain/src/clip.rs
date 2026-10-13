pub const DEFAULT_DURATION_SECONDS: f64 = 60.0;
pub const MIN_DURATION_SECONDS: f64 = 5.0;
pub const MAX_DURATION_SECONDS: f64 = 60.0;

pub fn valid_duration(seconds: f64) -> bool {
    seconds.is_finite()
        && (MIN_DURATION_SECONDS..=MAX_DURATION_SECONDS).contains(&seconds)
        && (seconds * 10.0 - (seconds * 10.0).round()).abs() < 1e-7
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twitch_duration_contract() {
        for seconds in [5.0, 5.1, 30.0, 59.9, DEFAULT_DURATION_SECONDS] {
            assert!(valid_duration(seconds));
        }
        for seconds in [4.9, 60.1, 30.01, f64::NAN, f64::INFINITY] {
            assert!(!valid_duration(seconds));
        }
    }
}
