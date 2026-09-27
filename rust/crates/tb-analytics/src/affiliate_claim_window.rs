//! Gemeinsames Affiliate-Claim-Zeitfenster.
//!
//! `claimed_at` und `partnered_at` liegen in der Datenbank als TEXT vor. Die
//! produktiven Pfade parsen Aktivierungsfenster deshalb in Rust und degradieren
//! ungueltige Zeitstempel kontrolliert; SQL-Prädikate bleiben fuer einfache
//! Datenbank-Zeitfenster wie frische Reservierungen.

use chrono::{DateTime, Duration, Utc};

pub use tb_domain::referral_window::{ClaimWindowDuration, POST_ACTIVATION_GRACE, RESERVATION_TTL};

pub fn claim_in_activation_window(claimed_at: DateTime<Utc>, partnered_at: DateTime<Utc>) -> bool {
    claimed_at >= partnered_at - Duration::seconds(RESERVATION_TTL.seconds())
        && claimed_at <= partnered_at + Duration::seconds(POST_ACTIVATION_GRACE.seconds())
}

pub fn sql_claim_window_predicate(claimed_at_expr: &str, partnered_at_expr: &str) -> String {
    format!(
        "(({claimed_at_expr})::timestamptz >= ({partnered_at_expr})::timestamptz - INTERVAL '{}' \
          AND ({claimed_at_expr})::timestamptz <= ({partnered_at_expr})::timestamptz + INTERVAL '{}')",
        RESERVATION_TTL.sql_interval(),
        POST_ACTIVATION_GRACE.sql_interval()
    )
}

pub fn sql_reservation_fresh_predicate(now_expr: &str, claimed_at_expr: &str) -> String {
    format!(
        "(({now_expr})::timestamptz <= ({claimed_at_expr})::timestamptz + INTERVAL '{}')",
        RESERVATION_TTL.sql_interval()
    )
}

pub fn sql_activation_grace_predicate(now_expr: &str, partnered_at_expr: &str) -> String {
    format!(
        "(({now_expr})::timestamptz <= ({partnered_at_expr})::timestamptz + INTERVAL '{}')",
        POST_ACTIVATION_GRACE.sql_interval()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn claim_window_bounds_are_inclusive() {
        let partnered_at = Utc.with_ymd_and_hms(2026, 6, 3, 0, 0, 0).unwrap();
        assert!(claim_in_activation_window(
            partnered_at - Duration::seconds(RESERVATION_TTL.seconds()),
            partnered_at
        ));
        assert!(claim_in_activation_window(
            partnered_at + Duration::seconds(POST_ACTIVATION_GRACE.seconds()),
            partnered_at
        ));
        assert!(!claim_in_activation_window(
            partnered_at - Duration::seconds(RESERVATION_TTL.seconds() + 1),
            partnered_at
        ));
        assert!(!claim_in_activation_window(
            partnered_at + Duration::seconds(POST_ACTIVATION_GRACE.seconds() + 1),
            partnered_at
        ));
    }

    #[test]
    fn sql_predicate_uses_timestamptz_and_intervals() {
        let predicate = sql_claim_window_predicate("$1", "$2");
        assert!(predicate.contains("::timestamptz"));
        assert!(predicate.contains("INTERVAL '4 days'"));
        assert!(predicate.contains("INTERVAL '24 hours'"));
    }
}
