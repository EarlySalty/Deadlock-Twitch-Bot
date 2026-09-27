use chrono::{DateTime, Datelike, Duration as ChronoDuration, NaiveDate, TimeZone, Utc};
use chrono_tz::Europe::Berlin;

pub fn berlin_week_start(at: DateTime<Utc>) -> NaiveDate {
    let local = at.with_timezone(&Berlin);
    local.date_naive() - ChronoDuration::days(i64::from(local.weekday().num_days_from_monday()))
}

pub fn berlin_week_bounds(at: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>, NaiveDate) {
    let date = berlin_week_start(at);
    let start = Berlin
        .from_local_datetime(&date.and_hms_opt(0, 0, 0).expect("midnight"))
        .earliest()
        .expect("Berlin midnight")
        .with_timezone(&Utc);
    let next_date = date + ChronoDuration::days(7);
    let end = Berlin
        .from_local_datetime(&next_date.and_hms_opt(0, 0, 0).expect("midnight"))
        .earliest()
        .expect("Berlin midnight")
        .with_timezone(&Utc);
    (start, end, date)
}

pub fn berlin_month_bounds(at: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>, String) {
    let local = at.with_timezone(&Berlin);
    let start_date = NaiveDate::from_ymd_opt(local.year(), local.month(), 1).expect("valid month");
    let (next_year, next_month) = if local.month() == 12 {
        (local.year() + 1, 1)
    } else {
        (local.year(), local.month() + 1)
    };
    let end_date = NaiveDate::from_ymd_opt(next_year, next_month, 1).expect("valid next month");
    let start = Berlin
        .from_local_datetime(&start_date.and_hms_opt(0, 0, 0).expect("midnight"))
        .earliest()
        .expect("Berlin midnight")
        .with_timezone(&Utc);
    let end = Berlin
        .from_local_datetime(&end_date.and_hms_opt(0, 0, 0).expect("midnight"))
        .earliest()
        .expect("Berlin midnight")
        .with_timezone(&Utc);
    (
        start,
        end,
        format!("{:04}-{:02}", local.year(), local.month()),
    )
}

pub fn midnight(date: NaiveDate) -> DateTime<Utc> {
    Berlin
        .from_local_datetime(&date.and_hms_opt(0, 0, 0).expect("midnight"))
        .single()
        .expect("Berlin midnight")
        .with_timezone(&Utc)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn berlin_weeks_follow_dst_and_months_follow_local_midnight() {
        for (time, hours) in [("2026-03-25T12:00:00Z", 167), ("2026-10-21T12:00:00Z", 169)] {
            let time = DateTime::parse_from_rfc3339(time)
                .unwrap()
                .with_timezone(&Utc);
            let (start, end, _) = berlin_week_bounds(time);
            assert_eq!((end - start).num_hours(), hours);
        }
        let time = DateTime::parse_from_rfc3339("2026-09-30T22:01:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(berlin_month_bounds(time).2, "2026-10");
        assert_eq!(
            berlin_week_start(time),
            NaiveDate::from_ymd_opt(2026, 9, 28).unwrap()
        );
    }
}
