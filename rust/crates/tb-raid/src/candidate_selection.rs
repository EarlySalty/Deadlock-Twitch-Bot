//! Kandidaten-Auswahl für Raids — reine Logik ohne DB-Zugriff. Schritt 6d.
//!
//! Ports aus `bot/raid/services/candidate_selection.py`:
//! - [`select_by_score`]   → `select_partner_candidate_by_score` (Z. 210–356)
//! - [`select_fairest`]    → `select_fairest_candidate` (Z. 358–432)
//! - [`is_retryable_raid_error`] → `raid_pipeline.py::is_retryable_raid_error` (Z. 23–38)
//!
//! Der live geschaltete Rust-Pfad ergänzt den Nicht-Partner-Fallback um eine
//! Soft-Avoid-Liste; der Legacy-Python-Pfad bleibt unverändert.
//!
//! Alle Funktionen nehmen fertige Kandidaten-Listen + optionale Hilfs-Daten
//! als Parameter — kein DB-Zugriff, voll unit-testbar.

use std::collections::HashMap;
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Konstanten (identisch zu Python)
// ---------------------------------------------------------------------------

/// Tages-Soft-Cap für empfangene Raids (Python `DAILY_RAID_SOFT_CAP = 2`,
/// `candidate_selection.py` Z. 20).
pub const DAILY_RAID_SOFT_CAP: i32 = 2;

/// Maximale Score-Differenz, innerhalb derer Kandidaten als gleichwertig
/// gelten (Python `partner_score_threshold = 0.05`, Z. 74).
pub const PARTNER_SCORE_THRESHOLD: f64 = 0.05;

/// Sentinel für unbekannte/nicht-angereicherte Follower-Zahlen — sortiert in
/// allen Tie-Break-Pfaden ans **Ende** (niedrigere Follower-Zahlen gewinnen).
///
/// Python defaultet nicht-angereicherte `followers_total` auf `10**9`
/// (`_fallback_sort_key`/`_sort_key`: `_safe_int(..., 10**9)`). Eine echte
/// Follower-Zahl überschreibt diesen Sentinel; bleibt sie unbekannt (Helix
/// liefert nichts), landet der Kandidat hinter allen mit bekannten Zahlen —
/// statt sie wie ein Default von `0` an die Spitze zu ziehen.
pub const FOLLOWERS_UNKNOWN: i32 = 1_000_000_000;

const SOFT_AVOIDED_FALLBACK_RAID_LOGINS: &[&str] = &["edoeasy"];

/// Erkennt Logins, die nur als letzter Nicht-Partner-Fallback zulässig sind.
pub fn is_soft_avoided_fallback_login(login: &str) -> bool {
    SOFT_AVOIDED_FALLBACK_RAID_LOGINS
        .iter()
        .any(|avoided| login.trim().eq_ignore_ascii_case(avoided))
}

// ---------------------------------------------------------------------------
// Kandidaten-Typen
// ---------------------------------------------------------------------------

/// Ein Raid-Kandidat mit zugehörigem berechneten Score.
///
/// Entspricht dem Python-`Candidate`-Dict nach dem `_partner_score`-Enrich-
/// Schritt in `select_partner_candidate_by_score`.
#[derive(Debug, Clone)]
pub struct ScoredCandidate {
    pub user_id: String,
    pub user_login: String,
    /// Endgültig berechneter Score (aus `twitch_partner_raid_scores.final_score`).
    pub final_score: f64,
    /// Anzahl Raids, die dieser Kanal heute bereits empfangen hat.
    pub today_received_raids: i32,
    /// Ist der Kandidat laut Score-Cache live? (Python: `is_live`-Flag, INTEGER 0/1)
    pub is_live: bool,
    /// Viewer-Anzahl (für Tie-Break-Sortierung, Python `_fallback_sort_key`).
    pub viewer_count: i32,
    /// Follower-Anzahl (Tie-Break).
    pub followers_total: i32,
    /// Startzeit als ISO-8601-String (Tie-Break; Python: `"9999-99-99"` als Sentinel).
    pub started_at: String,
    /// Raid-Boost-Multiplikator (nur für Logging).
    pub raid_boost_multiplier: f64,
    /// New-Partner-Multiplikator (nur für Logging).
    pub new_partner_multiplier: f64,
}

/// Ein ungewerteter Kandidat für `select_fairest`.
///
/// Entspricht dem Python-`Candidate`-Dict in `select_fairest_candidate`.
#[derive(Debug, Clone)]
pub struct FairnessCandidate {
    pub user_id: String,
    pub user_login: String,
    pub viewer_count: i32,
    /// Follower-Gesamtzahl (Tie-Break-Ebene 3). [`FOLLOWERS_UNKNOWN`], solange
    /// nicht angereichert — sortiert dann ans Ende (Python-Default `10**9`).
    pub followers_total: i32,
    /// Startzeit als ISO-8601-String (Sentinel: `"9999-99-99"` wie Python).
    pub started_at: String,
}

/// Ergebnis von [`select_by_score`] mit Grund-Annotation (für Logging).
#[derive(Debug, Clone)]
pub struct SelectionResult<'a> {
    pub candidate: &'a ScoredCandidate,
    pub reason: SelectionReason,
}

/// Erklärt, warum dieser Kandidat gewählt wurde (Port der Python `selection_reason`-Strings).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionReason {
    /// Höchster `final_score` (kein Tie).
    HighestFinalScore,
    /// Daily-Soft-Cap hat gefiltert, dann höchster Score.
    DailyRaidSoftCap,
    /// Tie-Break via `today_received_raids` (ohne Daily-Cap-Filter).
    TodayReceivedRaids,
    /// Tie-Break via `today_received_raids` (nach Daily-Cap-Filter).
    DailyCapTodayReceivedRaids,
    /// Tie-Break via Viewer/Follower/StartedAt (ohne Daily-Cap-Filter).
    ViewerCountFollowersStartedAt,
    /// Tie-Break via Viewer/Follower/StartedAt (nach Daily-Cap-Filter).
    DailyCapViewerCountFollowersStartedAt,
}

impl SelectionReason {
    /// Lesbarer Name, identisch zu den Python-`selection_reason`-Strings.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::HighestFinalScore => "highest_final_score",
            Self::DailyRaidSoftCap => "daily_raid_soft_cap",
            Self::TodayReceivedRaids => "today_received_raids",
            Self::DailyCapTodayReceivedRaids => "daily_raid_soft_cap_today_received_raids",
            Self::ViewerCountFollowersStartedAt => "viewer_count_followers_started_at",
            Self::DailyCapViewerCountFollowersStartedAt => {
                "daily_raid_soft_cap_viewer_count_followers_started_at"
            }
        }
    }
}

// ---------------------------------------------------------------------------
// is_retryable_raid_error
// ---------------------------------------------------------------------------

/// Gibt `true` zurück, wenn der Fehler darauf hindeutet, dass das Raid-Ziel
/// keine Raids akzeptiert und ein anderes Ziel versucht werden soll.
///
/// Port von `is_retryable_raid_error` in `bot/raid/raid_pipeline.py` (Z. 23–38).
///
/// Matching: case-insensitive Substring-Suche (Python `error.lower()`).
pub fn is_retryable_raid_error(error: &str) -> bool {
    if error.is_empty() {
        return false;
    }
    let msg = error.to_lowercase();
    const MARKERS: &[&str] = &[
        "cannot be raided",
        "does not allow you to raid",
        "do not allow you to raid",
        "not allow you to raid",
        "settings do not allow you to raid",
        "not accepting raids",
        "does not allow raids",
        "raids are disabled",
    ];
    MARKERS.iter().any(|marker| msg.contains(marker))
}

// ---------------------------------------------------------------------------
// select_by_score
// ---------------------------------------------------------------------------

/// Wählt den besten Kandidaten nach `final_score` aus einer bereits bewerteten
/// und nach Live-Status gefilterten Liste.
///
/// Port von `CandidateSelector.select_partner_candidate_by_score` in
/// `bot/raid/services/candidate_selection.py` (Z. 210–356).
///
/// **Auswahl-Algorithmus:**
/// 1. Kandidaten ohne `is_live == true` wurden vor diesem Aufruf gefiltert
///    (Aufgabe des Aufrufers).
/// 2. Daily-Soft-Cap: Kandidaten mit `today_received_raids >= DAILY_RAID_SOFT_CAP`
///    werden herausgefiltert. Wenn danach nichts übrig bleibt, wird die
///    ungefilterte Liste als Fallback genutzt.
/// 3. Beste Kandidaten: alle Kandidaten mit
///    `|best_final_score - candidate.final_score| <= PARTNER_SCORE_THRESHOLD`.
/// 4. Wenn genau einer: fertig.
/// 5. Tie-Break 1: Kandidat mit kleinstem `today_received_raids`.
/// 6. Tie-Break 2 (noch immer Gleichstand): Sortierung nach
///    `(viewer_count, followers_total, started_at)` aufsteigend — d. h.
///    **niedrigster** viewer_count gewinnt (Python `_fallback_sort_key` ist
///    aufsteigend, Erster wird gewählt).
///
/// Gibt `None` zurück, wenn die Eingabeliste leer ist.
pub fn select_by_score<'a>(candidates: &'a [ScoredCandidate]) -> Option<SelectionResult<'a>> {
    if candidates.is_empty() {
        return None;
    }

    // Schritt 2: Daily-Cap-Filter.
    let under_cap: Vec<&ScoredCandidate> = candidates
        .iter()
        .filter(|c| c.today_received_raids < DAILY_RAID_SOFT_CAP)
        .collect();
    let daily_cap_filtered = candidates.len() - under_cap.len();
    let pool: Vec<&ScoredCandidate> = if under_cap.is_empty() {
        candidates.iter().collect()
    } else {
        under_cap
    };

    // Schritt 3: Kandidaten nahe am besten Score.
    let best_score = pool
        .iter()
        .map(|c| c.final_score)
        .fold(f64::NEG_INFINITY, f64::max);
    let close: Vec<&ScoredCandidate> = pool
        .iter()
        .copied()
        .filter(|c| (best_score - c.final_score).abs() <= PARTNER_SCORE_THRESHOLD)
        .collect();

    // Schritt 4: eindeutiger Gewinner.
    if close.len() == 1 {
        let reason = if daily_cap_filtered > 0 {
            SelectionReason::DailyRaidSoftCap
        } else {
            SelectionReason::HighestFinalScore
        };
        return Some(SelectionResult {
            candidate: close[0],
            reason,
        });
    }

    // Schritt 5: Tie-Break via today_received_raids.
    let lowest_today = close
        .iter()
        .map(|c| c.today_received_raids)
        .min()
        .unwrap_or(0);
    let tie1: Vec<&ScoredCandidate> = close
        .iter()
        .copied()
        .filter(|c| c.today_received_raids == lowest_today)
        .collect();

    if tie1.len() == 1 {
        let reason = if daily_cap_filtered > 0 {
            SelectionReason::DailyCapTodayReceivedRaids
        } else {
            SelectionReason::TodayReceivedRaids
        };
        return Some(SelectionResult {
            candidate: tie1[0],
            reason,
        });
    }

    // Schritt 6: Tie-Break via (viewer_count, followers_total, started_at) aufsteigend.
    let mut tie2 = tie1;
    tie2.sort_by(|a, b| {
        a.viewer_count
            .cmp(&b.viewer_count)
            .then_with(|| a.followers_total.cmp(&b.followers_total))
            .then_with(|| a.started_at.cmp(&b.started_at))
    });

    let reason = if daily_cap_filtered > 0 {
        SelectionReason::DailyCapViewerCountFollowersStartedAt
    } else {
        SelectionReason::ViewerCountFollowersStartedAt
    };
    Some(SelectionResult {
        candidate: tie2[0],
        reason,
    })
}

// ---------------------------------------------------------------------------
// select_fairest
// ---------------------------------------------------------------------------

/// Wählt den „fairsten" Kandidaten — wer am wenigsten Raids insgesamt erhalten
/// hat, wird bevorzugt. Ziele aus `recent_targets` werden zunächst gefiltert.
///
/// Port von `CandidateSelector.select_fairest_candidate` in
/// `bot/raid/services/candidate_selection.py` (Z. 358–432).
///
/// **Auswahl-Algorithmus (exakt wie Python):**
/// 1. Weich vermiedene Logins werden zurückgestellt, solange ein anderer
///    Fallback-Kandidat verfügbar ist.
/// 2. Kandidaten, deren `user_id` in `recent_targets` enthalten ist, werden
///    herausgefiltert. Wenn danach nichts übrig bleibt, wird die bevorzugte
///    Liste genutzt.
/// 3. Sortierung aufsteigend nach `(received_raids_total, viewer_count,
///    followers_total, started_at)`.
/// 4. Der erste Kandidat wird gewählt.
///
/// `received_raids_by_id`: Map von `user_id → received_successful_raids_total`
/// aus `twitch_partner_raid_scores`. Fehlende Einträge defaulten auf 0
/// (Python: unbekannte Kandidaten haben automatisch höchste Priorität).
///
/// Gibt `None` zurück, wenn die Eingabeliste leer ist.
pub fn select_fairest<'a>(
    candidates: &'a [FairnessCandidate],
    recent_targets: &HashSet<String>,
    received_raids_by_id: &HashMap<String, i32>,
) -> Option<&'a FairnessCandidate> {
    if candidates.is_empty() {
        return None;
    }

    let non_avoided: Vec<&FairnessCandidate> = candidates
        .iter()
        .filter(|candidate| !is_soft_avoided_fallback_login(&candidate.user_login))
        .collect();
    let preferred: Vec<&FairnessCandidate> = if non_avoided.is_empty() {
        candidates.iter().collect()
    } else {
        non_avoided
    };

    // Schritt 2: recent-targets herausfiltern.
    let filtered: Vec<&FairnessCandidate> = preferred
        .iter()
        .copied()
        .filter(|c| !recent_targets.contains(&c.user_id))
        .collect();
    let mut pool: Vec<&FairnessCandidate> = if filtered.is_empty() {
        preferred
    } else {
        filtered
    };

    // Schritt 3: Sortierung.
    pool.sort_by(|a, b| {
        let raids_a = received_raids_by_id.get(&a.user_id).copied().unwrap_or(0);
        let raids_b = received_raids_by_id.get(&b.user_id).copied().unwrap_or(0);
        raids_a
            .cmp(&raids_b)
            .then_with(|| a.viewer_count.cmp(&b.viewer_count))
            .then_with(|| a.followers_total.cmp(&b.followers_total))
            .then_with(|| a.started_at.cmp(&b.started_at))
    });

    // Schritt 4: Erster nach Sortierung.
    pool.into_iter().next()
}

// ---------------------------------------------------------------------------
// Unit-Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Hilfsfunktionen ----------------------------------------------------

    fn make_scored(user_id: &str, final_score: f64, today_received_raids: i32) -> ScoredCandidate {
        ScoredCandidate {
            user_id: user_id.to_string(),
            user_login: user_id.to_string(),
            final_score,
            today_received_raids,
            is_live: true,
            viewer_count: 50,
            followers_total: 1000,
            started_at: "2026-06-09T12:00:00Z".to_string(),
            raid_boost_multiplier: 1.0,
            new_partner_multiplier: 1.0,
        }
    }

    fn make_fairness(user_id: &str, viewer_count: i32, followers_total: i32) -> FairnessCandidate {
        FairnessCandidate {
            user_id: user_id.to_string(),
            user_login: user_id.to_string(),
            viewer_count,
            followers_total,
            started_at: "2026-06-09T12:00:00Z".to_string(),
        }
    }

    // -- is_retryable_raid_error --------------------------------------------

    #[test]
    fn retryable_leerer_string_false() {
        assert!(!is_retryable_raid_error(""));
    }

    #[test]
    fn retryable_bekannte_marker() {
        for msg in &[
            "cannot be raided",
            "RAIDS ARE DISABLED",
            "This channel does not allow you to raid",
            "settings do not allow you to raid",
            "not accepting raids",
        ] {
            assert!(
                is_retryable_raid_error(msg),
                "'{msg}' sollte als retryable gelten"
            );
        }
    }

    #[test]
    fn retryable_unbekannter_fehler_false() {
        assert!(!is_retryable_raid_error("network timeout"));
        assert!(!is_retryable_raid_error("unauthorized"));
    }

    // -- select_by_score: Basis --------------------------------------------

    #[test]
    fn score_leere_liste_none() {
        assert!(select_by_score(&[]).is_none());
    }

    #[test]
    fn score_einzelner_kandidat_gewaehlt() {
        let candidates = vec![make_scored("uid_a", 0.8, 0)];
        let result = select_by_score(&candidates).unwrap();
        assert_eq!(result.candidate.user_id, "uid_a");
        assert_eq!(result.reason, SelectionReason::HighestFinalScore);
    }

    #[test]
    fn score_hoechster_score_gewinnt() {
        let candidates = vec![
            make_scored("uid_a", 0.5, 0),
            make_scored("uid_b", 0.9, 0),
            make_scored("uid_c", 0.3, 0),
        ];
        let result = select_by_score(&candidates).unwrap();
        assert_eq!(result.candidate.user_id, "uid_b");
        assert_eq!(result.reason, SelectionReason::HighestFinalScore);
    }

    // -- select_by_score: Daily-Cap ----------------------------------------

    #[test]
    fn score_daily_cap_filtert_kandidaten() {
        // uid_a hat 2 Raids heute (>= CAP), uid_b hat 1.
        let candidates = vec![
            make_scored("uid_a", 0.9, 2), // wird rausgefiltert
            make_scored("uid_b", 0.5, 1),
        ];
        let result = select_by_score(&candidates).unwrap();
        assert_eq!(result.candidate.user_id, "uid_b");
        assert_eq!(result.reason, SelectionReason::DailyRaidSoftCap);
    }

    #[test]
    fn score_daily_cap_fallback_alle_ueber_cap() {
        // Alle über Cap — Fallback auf ungefilterte Liste, bester Score gewinnt.
        let candidates = vec![make_scored("uid_a", 0.9, 3), make_scored("uid_b", 0.5, 4)];
        let result = select_by_score(&candidates).unwrap();
        assert_eq!(result.candidate.user_id, "uid_a");
    }

    // -- select_by_score: Tie-Breaks ---------------------------------------

    #[test]
    fn score_tiebreak_today_received() {
        // uid_a und uid_b liegen innerhalb des Schwellwerts (0.05).
        let candidates = vec![
            make_scored("uid_a", 0.9, 1),
            make_scored("uid_b", 0.92, 0), // gleicher Score-Band, weniger Raids heute
        ];
        let result = select_by_score(&candidates).unwrap();
        assert_eq!(result.candidate.user_id, "uid_b");
        assert_eq!(result.reason, SelectionReason::TodayReceivedRaids);
    }

    #[test]
    fn score_tiebreak_viewer_count() {
        // Identischer Score und today_received_raids — Tie-Break via viewer_count.
        let mut cand_a = make_scored("uid_a", 0.9, 0);
        cand_a.viewer_count = 200;
        let mut cand_b = make_scored("uid_b", 0.9, 0);
        cand_b.viewer_count = 50; // niedrigerer viewer_count gewinnt

        let candidates = vec![cand_a, cand_b];
        let result = select_by_score(&candidates).unwrap();
        assert_eq!(result.candidate.user_id, "uid_b");
        assert_eq!(
            result.reason,
            SelectionReason::ViewerCountFollowersStartedAt
        );
    }

    #[test]
    fn score_tiebreak_followers_bei_gleichen_viewern() {
        let mut cand_a = make_scored("uid_a", 0.9, 0);
        cand_a.viewer_count = 50;
        cand_a.followers_total = 5000;

        let mut cand_b = make_scored("uid_b", 0.9, 0);
        cand_b.viewer_count = 50;
        cand_b.followers_total = 1000; // weniger Follower gewinnt

        let candidates = vec![cand_a, cand_b];
        let result = select_by_score(&candidates).unwrap();
        assert_eq!(result.candidate.user_id, "uid_b");
    }

    #[test]
    fn score_tiebreak_started_at_bei_gleichen_viewers_und_followers() {
        let mut cand_a = make_scored("uid_a", 0.9, 0);
        cand_a.viewer_count = 50;
        cand_a.followers_total = 1000;
        cand_a.started_at = "2026-06-09T14:00:00Z".to_string(); // später gestartet

        let mut cand_b = make_scored("uid_b", 0.9, 0);
        cand_b.viewer_count = 50;
        cand_b.followers_total = 1000;
        cand_b.started_at = "2026-06-09T10:00:00Z".to_string(); // früher gestartet → gewinnt

        let candidates = vec![cand_a, cand_b];
        let result = select_by_score(&candidates).unwrap();
        assert_eq!(result.candidate.user_id, "uid_b");
    }

    #[test]
    fn score_tiebreak_daily_cap_reason_korrekt() {
        // daily_cap_filtered > 0 + today_received_raids Tie-Break.
        let candidates = vec![
            make_scored("uid_a", 0.9, 0),  // unter Cap, weniger Raids
            make_scored("uid_b", 0.92, 1), // unter Cap, mehr Raids heute
            make_scored("uid_c", 0.95, 2), // >= Cap, wird rausgefiltert
        ];
        let result = select_by_score(&candidates).unwrap();
        assert_eq!(result.candidate.user_id, "uid_a");
        assert_eq!(result.reason, SelectionReason::DailyCapTodayReceivedRaids);
    }

    // -- select_fairest ----------------------------------------------------

    #[test]
    fn fairest_leere_liste_none() {
        let result = select_fairest(&[], &HashSet::new(), &HashMap::new());
        assert!(result.is_none());
    }

    #[test]
    fn fairest_weniger_raids_gewinnt() {
        let candidates = vec![
            make_fairness("uid_a", 100, 2000),
            make_fairness("uid_b", 100, 2000),
        ];
        let mut received = HashMap::new();
        received.insert("uid_a".to_string(), 10);
        received.insert("uid_b".to_string(), 2); // weniger Raids empfangen → gewinnt

        let result = select_fairest(&candidates, &HashSet::new(), &received).unwrap();
        assert_eq!(result.user_id, "uid_b");
    }

    #[test]
    fn fairest_unbekannter_kandidat_hat_prioritaet() {
        // uid_a nicht in received_raids → default 0 → höchste Priorität.
        let candidates = vec![
            make_fairness("uid_a", 100, 2000),
            make_fairness("uid_b", 100, 2000),
        ];
        let mut received = HashMap::new();
        received.insert("uid_b".to_string(), 5);
        // uid_a fehlt → default 0

        let result = select_fairest(&candidates, &HashSet::new(), &received).unwrap();
        assert_eq!(result.user_id, "uid_a");
    }

    #[test]
    fn fairest_recent_targets_werden_gefiltert() {
        let candidates = vec![
            make_fairness("uid_a", 100, 2000),
            make_fairness("uid_b", 200, 4000),
        ];
        let mut recent = HashSet::new();
        recent.insert("uid_a".to_string()); // wurde kürzlich angeraidet

        let result = select_fairest(&candidates, &recent, &HashMap::new()).unwrap();
        assert_eq!(
            result.user_id, "uid_b",
            "uid_a ist recent, uid_b muss gewählt werden"
        );
    }

    #[test]
    fn fairest_alle_recent_fallback_auf_alle() {
        let candidates = vec![
            make_fairness("uid_a", 50, 1000),
            make_fairness("uid_b", 200, 4000),
        ];
        let mut recent = HashSet::new();
        recent.insert("uid_a".to_string());
        recent.insert("uid_b".to_string());

        // Alle gefiltert → Fallback auf alle, niedrigste viewer_count gewinnt.
        let result = select_fairest(&candidates, &recent, &HashMap::new()).unwrap();
        assert_eq!(result.user_id, "uid_a"); // viewer 50 < 200
    }

    #[test]
    fn fairest_nutzt_edoeasy_nur_als_letzten_nicht_partner_fallback() {
        let mut edoeasy = make_fairness("uid_edoeasy", 1, 1);
        edoeasy.user_login = "EdoEasy".to_string();
        let alternative = make_fairness("uid_alternative", 25, 100);
        let candidates = vec![edoeasy, alternative];
        let recent = HashSet::from(["uid_alternative".to_string()]);

        let with_alternative = select_fairest(&candidates, &recent, &HashMap::new()).unwrap();
        let without_alternative =
            select_fairest(&candidates[..1], &recent, &HashMap::new()).unwrap();

        assert_eq!(with_alternative.user_id, "uid_alternative");
        assert_eq!(without_alternative.user_id, "uid_edoeasy");
    }

    #[test]
    fn fairest_tiebreak_viewer_count() {
        let candidates = vec![
            make_fairness("uid_a", 200, 2000), // mehr Viewer
            make_fairness("uid_b", 50, 2000),  // weniger Viewer → gewinnt
        ];
        let result = select_fairest(&candidates, &HashSet::new(), &HashMap::new()).unwrap();
        assert_eq!(result.user_id, "uid_b");
    }

    #[test]
    fn fairest_tiebreak_followers_bei_gleichen_viewern() {
        let candidates = vec![
            make_fairness("uid_a", 50, 5000), // mehr Follower
            make_fairness("uid_b", 50, 500),  // weniger Follower → gewinnt
        ];
        let result = select_fairest(&candidates, &HashSet::new(), &HashMap::new()).unwrap();
        assert_eq!(result.user_id, "uid_b");
    }

    #[test]
    fn fairest_unbekannte_follower_sortieren_ans_ende() {
        // Bei gleichen Raids + Viewern entscheidet die Follower-Zahl. Ein
        // Kandidat mit unbekannten Followern (Sentinel) muss HINTER einem mit
        // bekannter — selbst höherer — Zahl landen (Python-Default 10**9).
        let mut bekannt = make_fairness("uid_bekannt", 50, 9999);
        bekannt.followers_total = 9999;
        let mut unbekannt = make_fairness("uid_unbekannt", 50, 0);
        unbekannt.followers_total = FOLLOWERS_UNKNOWN;

        let candidates = vec![unbekannt, bekannt];
        let result = select_fairest(&candidates, &HashSet::new(), &HashMap::new()).unwrap();
        assert_eq!(
            result.user_id, "uid_bekannt",
            "bekannte Follower-Zahl gewinnt gegen unbekannten Sentinel"
        );
    }
}
