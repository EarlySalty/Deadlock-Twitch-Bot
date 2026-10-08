use std::num::IntErrorKind;

use axum::{http::StatusCode, Json};
use serde_json::{json, Value};

pub const MAX_ANALYTICS_DAYS: i64 = 3650;

pub type QueryIntError = (StatusCode, Json<Value>);

pub fn parse_bounded_query_int(
    raw: Option<&str>,
    name: &str,
    default: i64,
    minimum: i64,
    maximum: i64,
) -> Result<i64, QueryIntError> {
    let trimmed = raw.map(str::trim).unwrap_or("");
    let parsed = if trimmed.is_empty() {
        default
    } else {
        trimmed.parse::<i64>().or_else(|error| {
            let digits = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
            if digits.bytes().all(|digit| digit.is_ascii_digit()) {
                match error.kind() {
                    IntErrorKind::PosOverflow => return Ok(maximum),
                    IntErrorKind::NegOverflow => return Ok(minimum),
                    _ => {}
                }
            }
            Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("{name} must be an integer") })),
            ))
        })?
    };
    Ok(parsed.clamp(minimum, maximum))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fehlend_ergibt_default() {
        assert_eq!(
            parse_bounded_query_int(None, "days", 30, 7, 365).unwrap(),
            30
        );
    }

    #[test]
    fn leer_und_whitespace_ergibt_default() {
        assert_eq!(
            parse_bounded_query_int(Some(""), "days", 30, 7, 365).unwrap(),
            30
        );
        assert_eq!(
            parse_bounded_query_int(Some("   "), "days", 30, 7, 365).unwrap(),
            30
        );
    }

    #[test]
    fn numerisch_wird_geparst_und_getrimmt() {
        assert_eq!(
            parse_bounded_query_int(Some(" 90 "), "days", 30, 7, 365).unwrap(),
            90
        );
    }

    #[test]
    fn out_of_range_wird_geklemmt_nicht_400() {
        assert_eq!(
            parse_bounded_query_int(Some("1"), "days", 30, 7, 365).unwrap(),
            7
        );
        assert_eq!(
            parse_bounded_query_int(Some("9999"), "days", 30, 7, 365).unwrap(),
            365
        );
    }

    #[test]
    fn analytics_zeitraum_erlaubt_mehr_als_ein_jahr() {
        assert_eq!(
            parse_bounded_query_int(Some("730"), "days", 30, 7, MAX_ANALYTICS_DAYS).unwrap(),
            730
        );
        assert_eq!(
            parse_bounded_query_int(Some("5000"), "days", 30, 7, MAX_ANALYTICS_DAYS).unwrap(),
            MAX_ANALYTICS_DAYS
        );
    }

    #[test]
    fn dezimalinteger_ausserhalb_i64_werden_geklemmt() {
        let huge = "9".repeat(1000);
        let negative_huge = format!("-{huge}");
        for (raw, days, months) in [
            ("10000000000000000000", 3650, 120),
            ("9223372036854775808", 3650, 120),
            ("+9223372036854775808", 3650, 120),
            ("  +0009223372036854775808  ", 3650, 120),
            ("-9223372036854775809", 7, 1),
            ("  -0009223372036854775809  ", 7, 1),
            (huge.as_str(), 3650, 120),
            (negative_huge.as_str(), 7, 1),
        ] {
            assert_eq!(
                parse_bounded_query_int(Some(raw), "days", 30, 7, MAX_ANALYTICS_DAYS).unwrap(),
                days,
                "{raw}"
            );
            assert_eq!(
                parse_bounded_query_int(Some(raw), "months", 12, 1, 120).unwrap(),
                months,
                "{raw}"
            );
        }
    }

    #[test]
    fn i64_grenzen_und_bisherige_zahlengrammatik_bleiben_erhalten() {
        for (raw, expected) in [
            ("9223372036854775807", i64::MAX),
            ("-9223372036854775808", i64::MIN),
            ("+00090", 90),
            ("-00090", -90),
            ("+0", 0),
            ("-0", 0),
        ] {
            assert_eq!(
                parse_bounded_query_int(Some(raw), "days", 30, i64::MIN, i64::MAX).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn syntaxfehler_auch_hinter_ueberlauf_bleiben_400() {
        for raw in [
            "9223372036854775808000x",
            "-9223372036854775809000x",
            "9223372036854775808000.0",
            "9223372036854775808000_0",
            "9223372036854775808000 0",
            "1.5",
            "1_000",
            "0x10",
            "+",
            "-",
            "++1",
            "+-1",
            "１２",
        ] {
            let (status, Json(body)) =
                parse_bounded_query_int(Some(raw), "days", 30, 7, MAX_ANALYTICS_DAYS).unwrap_err();
            assert_eq!(status, StatusCode::BAD_REQUEST, "{raw}");
            assert_eq!(body, json!({ "error": "days must be an integer" }), "{raw}");
        }
    }

    #[test]
    fn nicht_numerisch_ergibt_python_konformes_400_json() {
        let (status, Json(body)) =
            parse_bounded_query_int(Some("abc"), "days", 30, 7, 365).unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, json!({ "error": "days must be an integer" }));
    }

    #[test]
    fn fehlername_steckt_in_der_meldung() {
        let (_, Json(body)) = parse_bounded_query_int(Some("x"), "months", 12, 1, 24).unwrap_err();
        assert_eq!(body, json!({ "error": "months must be an integer" }));
    }
}
