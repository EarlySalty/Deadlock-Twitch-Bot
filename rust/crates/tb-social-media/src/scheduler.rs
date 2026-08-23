use chrono::{DateTime, Duration, LocalResult, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

use crate::settings::PostingSchedule;

/// Wie weit [`next_cadence_slot`] höchstens in die Zukunft sucht. Bei sehr engen
/// Kadenzen (etwa ein Post pro Woche) kann der nächste freie Termin weit weg
/// liegen; jenseits dieser Grenze gilt der Plan als voll.
const CADENCE_HORIZON_DAYS: i64 = 180;

/// Obergrenzen aus der Kadenz-Einstellung einer Plattform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CadenceLimits {
    pub posts_per_week: u32,
    pub max_posts_per_day: u32,
}

impl Default for CadenceLimits {
    /// Recherche-Default: höchstens ein Post pro Tag, rund vier pro Woche.
    fn default() -> Self {
        Self {
            posts_per_week: 4,
            max_posts_per_day: 1,
        }
    }
}

impl CadenceLimits {
    /// `true`, wenn die Kadenz jedes Posten verbietet.
    pub fn blocks_everything(&self) -> bool {
        self.posts_per_week == 0 || self.max_posts_per_day == 0
    }
}

/// Nächster freier Slot unter Beachtung der Kadenz-Obergrenzen.
///
/// Anders als [`next_free_slot`] zählt diese Variante die schon belegten Termine
/// mit: pro lokalem Kalendertag höchstens `max_posts_per_day`, und in jedem
/// rollierenden Sieben-Tage-Fenster, das am Kandidaten endet, höchstens
/// `posts_per_week`. Ohne IO und ohne Systemzeit, damit sie testbar bleibt.
///
/// `None` bedeutet: innerhalb von [`CADENCE_HORIZON_DAYS`] ist kein Termin frei,
/// oder die Kadenz steht auf null.
pub fn next_cadence_slot(
    now: DateTime<Utc>,
    already_taken: &[DateTime<Utc>],
    schedule: &PostingSchedule,
    limits: &CadenceLimits,
) -> Option<DateTime<Utc>> {
    if limits.blocks_everything() {
        return None;
    }
    let timezone = resolve_timezone(&schedule.timezone);
    let times = resolve_times(schedule);
    let week = Duration::days(7);

    let mut date = now.with_timezone(&timezone).date_naive();
    let last_date = date.checked_add_signed(Duration::days(CADENCE_HORIZON_DAYS))?;
    while date <= last_date {
        let taken_that_day = already_taken
            .iter()
            .filter(|taken| taken.with_timezone(&timezone).date_naive() == date)
            .count();
        if taken_that_day < limits.max_posts_per_day as usize {
            for time in &times {
                let Some(candidate) = local_slot(timezone, date, *time) else {
                    continue;
                };
                if candidate <= now || already_taken.contains(&candidate) {
                    continue;
                }
                let taken_that_week = already_taken
                    .iter()
                    .filter(|taken| **taken > candidate - week && **taken <= candidate)
                    .count();
                if taken_that_week >= limits.posts_per_week as usize {
                    continue;
                }
                return Some(candidate);
            }
        }
        date = date.succ_opt()?;
    }
    None
}

fn resolve_timezone(name: &str) -> Tz {
    name.parse::<Tz>().unwrap_or(chrono_tz::Europe::Berlin)
}

fn resolve_times(schedule: &PostingSchedule) -> Vec<NaiveTime> {
    let parse = |values: &[String]| -> Vec<NaiveTime> {
        values
            .iter()
            .filter_map(|time| NaiveTime::parse_from_str(time, "%H:%M").ok())
            .collect()
    };
    let mut times = parse(&schedule.times);
    if times.is_empty() {
        times = parse(&PostingSchedule::default().times);
    }
    times.sort_unstable();
    times.dedup();
    times
}

/// Lokale Zeit in UTC. `None` bei Zeitumstellungs-Lücken (Sommerzeitbeginn).
fn local_slot(timezone: Tz, date: NaiveDate, time: NaiveTime) -> Option<DateTime<Utc>> {
    match timezone.from_local_datetime(&date.and_time(time)) {
        LocalResult::Single(value) | LocalResult::Ambiguous(value, _) => {
            Some(value.with_timezone(&Utc))
        }
        LocalResult::None => None,
    }
}

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

    use super::{next_cadence_slot, next_free_slot, CadenceLimits};
    use crate::settings::PostingSchedule;

    fn utc(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn schedule() -> PostingSchedule {
        PostingSchedule::default()
    }

    /// Ein Slot pro Tag um 18:00 Europe/Berlin, wie der Default der Kadenz-Tabelle.
    fn abendschedule() -> PostingSchedule {
        PostingSchedule {
            times: vec!["18:00".to_string()],
            timezone: "Europe/Berlin".to_string(),
        }
    }

    #[test]
    fn kadenz_default_entspricht_der_recherche() {
        assert_eq!(
            CadenceLimits::default(),
            CadenceLimits {
                posts_per_week: 4,
                max_posts_per_day: 1,
            }
        );
    }

    #[test]
    fn kadenz_nimmt_den_ersten_freien_abend() {
        // 2026-07-16 10:00 UTC ist 12:00 Berlin, der 18:00-Slot liegt noch vor uns.
        assert_eq!(
            next_cadence_slot(
                utc("2026-07-16T10:00:00Z"),
                &[],
                &abendschedule(),
                &CadenceLimits::default(),
            ),
            Some(utc("2026-07-16T16:00:00Z"))
        );
    }

    #[test]
    fn tageslimit_schiebt_auf_den_folgetag() {
        // Der heutige Abend ist belegt, mehr als ein Post pro Tag ist nicht erlaubt.
        assert_eq!(
            next_cadence_slot(
                utc("2026-07-16T10:00:00Z"),
                &[utc("2026-07-16T16:00:00Z")],
                &abendschedule(),
                &CadenceLimits::default(),
            ),
            Some(utc("2026-07-17T16:00:00Z"))
        );
    }

    #[test]
    fn tageslimit_groesser_eins_erlaubt_zwei_slots_am_tag() {
        let limits = CadenceLimits {
            posts_per_week: 14,
            max_posts_per_day: 2,
        };
        assert_eq!(
            next_cadence_slot(
                utc("2026-07-16T10:00:00Z"),
                &[utc("2026-07-16T12:00:00Z")],
                &schedule(),
                &limits,
            ),
            Some(utc("2026-07-16T16:00:00Z"))
        );
    }

    #[test]
    fn wochenlimit_ueberspringt_bis_das_fenster_frei_wird() {
        // Vier Posts am 13. bis 16. Juli. Das rollierende Sieben-Tage-Fenster ist
        // damit voll; der naechste Termin ist erst, wenn der 13. herausrutscht.
        let taken = vec![
            utc("2026-07-13T16:00:00Z"),
            utc("2026-07-14T16:00:00Z"),
            utc("2026-07-15T16:00:00Z"),
            utc("2026-07-16T16:00:00Z"),
        ];
        assert_eq!(
            next_cadence_slot(
                utc("2026-07-16T17:00:00Z"),
                &taken,
                &abendschedule(),
                &CadenceLimits::default(),
            ),
            Some(utc("2026-07-20T16:00:00Z"))
        );
    }

    #[test]
    fn kadenz_null_blockt_alles() {
        for limits in [
            CadenceLimits {
                posts_per_week: 0,
                max_posts_per_day: 1,
            },
            CadenceLimits {
                posts_per_week: 4,
                max_posts_per_day: 0,
            },
        ] {
            assert_eq!(
                next_cadence_slot(utc("2026-07-16T10:00:00Z"), &[], &abendschedule(), &limits),
                None
            );
        }
    }

    #[test]
    fn leere_zeitliste_faellt_auf_den_default_zurueck() {
        let leer = PostingSchedule {
            times: Vec::new(),
            timezone: "Europe/Berlin".to_string(),
        };
        assert_eq!(
            next_cadence_slot(
                utc("2026-07-16T10:00:00Z"),
                &[],
                &leer,
                &CadenceLimits::default(),
            ),
            Some(utc("2026-07-16T12:00:00Z"))
        );
    }

    #[test]
    fn unbekannte_zeitzone_faellt_auf_berlin_zurueck() {
        let kaputt = PostingSchedule {
            times: vec!["18:00".to_string()],
            timezone: "Nirgendwo/Erfunden".to_string(),
        };
        assert_eq!(
            next_cadence_slot(
                utc("2026-07-16T10:00:00Z"),
                &[],
                &kaputt,
                &CadenceLimits::default(),
            ),
            Some(utc("2026-07-16T16:00:00Z"))
        );
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
