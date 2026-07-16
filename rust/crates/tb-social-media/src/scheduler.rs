use chrono::{DateTime, LocalResult, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

use crate::settings::PostingSchedule;

/// Berechnet den nächsten noch nicht belegten Posting-Slot ohne IO oder Systemzeit.
pub fn next_free_slot(
    now: DateTime<Utc>,
    already_taken: &[DateTime<Utc>],
    schedule: &PostingSchedule,
) -> DateTime<Utc> {
    let timezone = schedule
        .timezone
        .parse::<Tz>()
        .unwrap_or(chrono_tz::Europe::Berlin);
    let mut times: Vec<_> = schedule
        .times
        .iter()
        .filter_map(|time| NaiveTime::parse_from_str(time, "%H:%M").ok())
        .collect();
    if times.is_empty() {
        times = PostingSchedule::default()
            .times
            .iter()
            .filter_map(|time| NaiveTime::parse_from_str(time, "%H:%M").ok())
            .collect();
    }
    times.sort_unstable();

    let mut date = now.with_timezone(&timezone).date_naive();
    loop {
        for time in &times {
            let local = date.and_time(*time);
            let candidate = match timezone.from_local_datetime(&local) {
                LocalResult::Single(value) | LocalResult::Ambiguous(value, _) => {
                    value.with_timezone(&Utc)
                }
                LocalResult::None => continue,
            };
            if candidate > now && !already_taken.contains(&candidate) {
                return candidate;
            }
        }
        let Some(next_date) = date.succ_opt() else {
            return DateTime::<Utc>::MAX_UTC;
        };
        date = next_date;
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use super::next_free_slot;
    use crate::settings::PostingSchedule;

    fn utc(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn schedule() -> PostingSchedule {
        PostingSchedule::default()
    }

    #[test]
    fn returns_first_slot_today_before_schedule_starts() {
        assert_eq!(
            next_free_slot(utc("2026-07-16T10:00:00Z"), &[], &schedule()),
            utc("2026-07-16T12:00:00Z")
        );
    }

    #[test]
    fn rolls_to_first_slot_tomorrow_after_last_slot() {
        assert_eq!(
            next_free_slot(utc("2026-07-16T20:00:00Z"), &[], &schedule()),
            utc("2026-07-17T12:00:00Z")
        );
    }

    #[test]
    fn skips_taken_first_slot_today() {
        assert_eq!(
            next_free_slot(
                utc("2026-07-16T10:00:00Z"),
                &[utc("2026-07-16T12:00:00Z")],
                &schedule(),
            ),
            utc("2026-07-16T16:00:00Z")
        );
    }

    #[test]
    fn rolls_to_tomorrow_when_all_today_slots_are_taken() {
        assert_eq!(
            next_free_slot(
                utc("2026-07-16T10:00:00Z"),
                &[
                    utc("2026-07-16T12:00:00Z"),
                    utc("2026-07-16T16:00:00Z"),
                    utc("2026-07-16T19:00:00Z"),
                ],
                &schedule(),
            ),
            utc("2026-07-17T12:00:00Z")
        );
    }
}
