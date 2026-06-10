//! Deadlock-Eligibility-Filter für Auto-Raid-Kandidaten. Reine Logik (kein DB).
//! Port von `raid_data_sources.py` `is_deadlock_partner_candidate_eligible`
//! (Z. ~294-345), `is_recent_deadlock` (Z. 86-97),
//! `filter_deadlock_eligible_partner_candidates` (Z. 294-346).
//!
//! Ein online Partner ist raid-fähig, wenn er **aktiv Deadlock** streamt — oder
//! **Just Chatting** mit einer Deadlock-Session, deren letzter Deadlock-Moment
//! ≤ 360 s zurückliegt. Aktive Deadlock-Streamer haben Vorrang vor „kürzlich".

use chrono::{DateTime, Utc};

/// Recency-Schwelle: letzter Deadlock-Moment darf max. so alt sein (Python
/// `recency_cap_seconds = 360`).
pub const DEADLOCK_RECENCY_CAP_SECONDS: i64 = 360;

/// Pro-Partner-Eingabe für die Eligibility-Prüfung.
#[derive(Debug, Clone)]
pub struct DeadlockEvalInput<'a> {
    /// Aktuell gestreamtes Spiel (aus den Live-Stream-Daten).
    pub game_name: &'a str,
    /// Hatte die laufende Session schon Deadlock (aus live_state).
    pub had_deadlock_session: bool,
    /// ISO-Zeitpunkt des letzten Deadlock-Moments (aus live_state), falls bekannt.
    pub last_deadlock_seen_at: Option<&'a str>,
}

/// `(now - last_deadlock_seen) <= cap` (Python `is_recent_deadlock`).
/// `None`/unparsebar → `false`.
pub fn is_recent_deadlock(
    last_deadlock_seen_at: Option<&str>,
    now: DateTime<Utc>,
    cap_seconds: i64,
) -> bool {
    match last_deadlock_seen_at.and_then(crate::util::parse_iso_utc) {
        Some(dt) => (now - dt).num_seconds() <= cap_seconds,
        None => false,
    }
}

/// Ist der Partner ein Deadlock-Auto-Raid-Kandidat? (Python
/// `is_deadlock_partner_candidate_eligible`). Leeres `target_game_lower` → immer
/// `true` (kein Filter konfiguriert).
pub fn is_deadlock_eligible(
    input: &DeadlockEvalInput<'_>,
    now: DateTime<Utc>,
    target_game_lower: &str,
) -> bool {
    if target_game_lower.is_empty() {
        return true;
    }
    let game_lower = input.game_name.trim().to_lowercase();
    if game_lower == target_game_lower {
        return true;
    }
    if game_lower == "just chatting" && input.had_deadlock_session {
        return is_recent_deadlock(
            input.last_deadlock_seen_at,
            now,
            DEADLOCK_RECENCY_CAP_SECONDS,
        );
    }
    false
}

/// Klassifiziert einen eligiblen Partner als **aktiv** (streamt Ziel-Spiel) oder
/// **kürzlich** (Just-Chatting-mit-frischer-Deadlock-Session).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EligibilityBucket {
    Active,
    Recent,
}

/// Wertet einen Partner aus: `None` = nicht eligible, sonst der Bucket.
pub fn classify_eligibility(
    input: &DeadlockEvalInput<'_>,
    now: DateTime<Utc>,
    target_game_lower: &str,
) -> Option<EligibilityBucket> {
    if !is_deadlock_eligible(input, now, target_game_lower) {
        return None;
    }
    let game_lower = input.game_name.trim().to_lowercase();
    Some(if game_lower == target_game_lower {
        EligibilityBucket::Active
    } else {
        EligibilityBucket::Recent
    })
}

/// Filtert eine Kandidatenliste: behält nur eligible, bevorzugt **aktive** vor
/// **kürzlichen** (Python: `filtered_active if filtered_active else filtered_recent`).
/// Generisch über den Kandidatentyp — der Aufrufer liefert die Eval-Eingabe je
/// Kandidat. Liefert `(eligible, count_filtered_out)`.
pub fn filter_eligible<T, F>(
    candidates: Vec<T>,
    now: DateTime<Utc>,
    target_game_lower: &str,
    mut eval_of: F,
) -> (Vec<T>, usize)
where
    F: for<'a> FnMut(&'a T) -> DeadlockEvalInput<'a>,
{
    if target_game_lower.is_empty() {
        return (candidates, 0);
    }
    let mut active = Vec::new();
    let mut recent = Vec::new();
    let mut filtered_out = 0usize;
    for candidate in candidates {
        let input = eval_of(&candidate);
        match classify_eligibility(&input, now, target_game_lower) {
            Some(EligibilityBucket::Active) => active.push(candidate),
            Some(EligibilityBucket::Recent) => recent.push(candidate),
            None => filtered_out += 1,
        }
    }
    let eligible = if active.is_empty() { recent } else { active };
    (eligible, filtered_out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-06-10T18:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn aktiv_deadlock_ist_eligible() {
        let input = DeadlockEvalInput {
            game_name: "Deadlock",
            had_deadlock_session: false,
            last_deadlock_seen_at: None,
        };
        assert_eq!(
            classify_eligibility(&input, now(), "deadlock"),
            Some(EligibilityBucket::Active)
        );
    }

    #[test]
    fn just_chatting_mit_frischem_deadlock_ist_recent() {
        let input = DeadlockEvalInput {
            game_name: "Just Chatting",
            had_deadlock_session: true,
            last_deadlock_seen_at: Some("2026-06-10T17:57:00+00:00"), // 180 s her
        };
        assert_eq!(
            classify_eligibility(&input, now(), "deadlock"),
            Some(EligibilityBucket::Recent)
        );
    }

    #[test]
    fn just_chatting_mit_altem_deadlock_faellt_raus() {
        let input = DeadlockEvalInput {
            game_name: "Just Chatting",
            had_deadlock_session: true,
            last_deadlock_seen_at: Some("2026-06-10T17:50:00+00:00"), // 600 s her > 360
        };
        assert_eq!(classify_eligibility(&input, now(), "deadlock"), None);
    }

    #[test]
    fn anderes_spiel_faellt_raus() {
        let input = DeadlockEvalInput {
            game_name: "Valorant",
            had_deadlock_session: true,
            last_deadlock_seen_at: Some("2026-06-10T17:59:00+00:00"),
        };
        assert_eq!(classify_eligibility(&input, now(), "deadlock"), None);
    }

    #[test]
    fn filter_bevorzugt_aktive_vor_recent() {
        let cands = vec!["aktiv", "recent", "raus"];
        let (eligible, out) = filter_eligible(cands, now(), "deadlock", |c| match *c {
            "aktiv" => DeadlockEvalInput {
                game_name: "Deadlock",
                had_deadlock_session: false,
                last_deadlock_seen_at: None,
            },
            "recent" => DeadlockEvalInput {
                game_name: "Just Chatting",
                had_deadlock_session: true,
                last_deadlock_seen_at: Some("2026-06-10T17:58:00+00:00"),
            },
            _ => DeadlockEvalInput {
                game_name: "Valorant",
                had_deadlock_session: false,
                last_deadlock_seen_at: None,
            },
        });
        // Aktive vorhanden → nur aktive, recent verworfen.
        assert_eq!(eligible, vec!["aktiv"]);
        assert_eq!(out, 1, "ein nicht-eligibler Kandidat");
    }
}
