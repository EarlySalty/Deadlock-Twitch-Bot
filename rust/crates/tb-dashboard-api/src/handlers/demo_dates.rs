use axum::{
    body::Body,
    http::header,
    middleware::Next,
    response::{IntoResponse, Response},
};
use chrono::{Datelike, Duration, Months, NaiveDate, Utc};
use serde_json::Value;

const ANCHOR: (i32, u32, u32) = (2026, 6, 30);
const MAX_BODY: usize = 4 << 20;
const MONTH_LABELS: [&str; 12] = [
    "Jan", "Feb", "Mär", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DateShift {
    days: i64,
    months: u32,
}

impl DateShift {
    pub fn until(today: NaiveDate) -> Self {
        let Some(anchor) = NaiveDate::from_ymd_opt(ANCHOR.0, ANCHOR.1, ANCHOR.2) else {
            return Self { days: 0, months: 0 };
        };
        let raw = (today - Duration::days(1) - anchor).num_days();
        let days = if raw > 0 { raw - raw % 7 } else { 0 };
        let shifted = anchor + Duration::days(days);
        let months = month_index(shifted.year(), shifted.month())
            .saturating_sub(month_index(anchor.year(), anchor.month()));
        Self {
            days,
            months: u32::try_from(months).unwrap_or(0),
        }
    }

    fn is_noop(self) -> bool {
        self.days == 0 && self.months == 0
    }

    pub fn apply(self, value: &mut Value) {
        match value {
            Value::String(text) => {
                if let Some(next) = self.shift_text(text) {
                    *text = next;
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|item| self.apply(item)),
            Value::Object(map) => {
                map.values_mut().for_each(|item| self.apply(item));
                self.shift_year_month(map);
            }
            _ => {}
        }
    }

    fn shift_text(self, text: &str) -> Option<String> {
        if let (Some(head), Some(tail)) = (text.get(..10), text.get(10..)) {
            if is_date_prefix(head) {
                let date = NaiveDate::parse_from_str(head, "%Y-%m-%d").ok()?;
                let moved = date + Duration::days(self.days);
                return Some(format!("{}{}", moved.format("%Y-%m-%d"), tail));
            }
        }
        if text.len() == 7 && is_month(text) {
            let date = NaiveDate::parse_from_str(&format!("{text}-01"), "%Y-%m-%d").ok()?;
            let moved = date.checked_add_months(Months::new(self.months))?;
            return Some(moved.format("%Y-%m").to_string());
        }
        None
    }

    fn shift_year_month(self, map: &mut serde_json::Map<String, Value>) {
        let (Some(year), Some(month)) = (
            map.get("year").and_then(Value::as_i64),
            map.get("month").and_then(Value::as_u64),
        ) else {
            return;
        };
        let Some(date) = i32::try_from(year)
            .ok()
            .and_then(|y| NaiveDate::from_ymd_opt(y, u32::try_from(month).ok()?, 1))
            .and_then(|d| d.checked_add_months(Months::new(self.months)))
        else {
            return;
        };
        map.insert("year".into(), Value::from(date.year()));
        map.insert("month".into(), Value::from(date.month()));
        if map.contains_key("monthLabel") {
            let label = MONTH_LABELS[date.month0() as usize];
            map.insert("monthLabel".into(), Value::from(label));
        }
    }
}

fn month_index(year: i32, month: u32) -> i64 {
    i64::from(year) * 12 + i64::from(month)
}

fn is_date_prefix(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

fn is_month(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 7
        && b[4] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || c.is_ascii_digit())
}

fn is_json(resp: &Response) -> bool {
    resp.headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"))
}

pub async fn shift_demo_dates(req: axum::extract::Request, next: Next) -> Response {
    let is_api = req.uri().path().starts_with("/twitch/demo/api/");
    let resp = next.run(req).await;
    let shift = DateShift::until(Utc::now().date_naive());
    if !is_api || !is_json(&resp) || shift.is_noop() {
        return resp;
    }
    let (mut parts, body) = resp.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, MAX_BODY).await else {
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Ok(mut value) = serde_json::from_slice::<Value>(&bytes) else {
        return Response::from_parts(parts, Body::from(bytes));
    };
    shift.apply(&mut value);
    let Ok(out) = serde_json::to_vec(&value) else {
        return Response::from_parts(parts, Body::from(bytes));
    };
    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(parts, Body::from(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).expect("valid date")
    }

    #[test]
    fn newest_demo_day_lands_within_last_week_and_keeps_weekday() {
        let today = day(2026, 10, 29);
        let shift = DateShift::until(today);
        let mut v = json!({"date": "2026-06-30", "at": "2026-06-30T19:00:00Z"});
        shift.apply(&mut v);
        let moved = NaiveDate::parse_from_str(v["date"].as_str().unwrap_or(""), "%Y-%m-%d")
            .expect("shifted date");
        assert!(moved < today && (today - moved).num_days() <= 7);
        assert_eq!(moved.weekday(), day(2026, 6, 30).weekday());
        assert!(v["at"].as_str().unwrap_or("").ends_with("T19:00:00Z"));
    }

    #[test]
    fn year_month_objects_move_with_labels() {
        let shift = DateShift::until(day(2026, 10, 29));
        let mut v = json!([{"year": 2026, "month": 6, "monthLabel": "Jun"}, {"year": 2025, "month": 12, "monthLabel": "Dez"}]);
        shift.apply(&mut v);
        assert_eq!(v[0]["month"], 10);
        assert_eq!(v[0]["monthLabel"], "Okt");
        assert_eq!(v[1]["year"], 2026);
        assert_eq!(v[1]["month"], 4);
    }

    #[test]
    fn non_dates_stay_untouched() {
        let shift = DateShift::until(day(2026, 10, 29));
        let mut v =
            json!({"title": "Ranked 2026-Push", "id": "1234-56-78xx", "n": 5, "t": "Spielstraße"});
        let before = v.clone();
        shift.apply(&mut v);
        assert_eq!(v, before);
    }
}
