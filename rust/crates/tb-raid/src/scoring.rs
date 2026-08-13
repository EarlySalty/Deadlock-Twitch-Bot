//! Reine Score-Berechnung für Partner-Raid-Scores (kein DB-Zugriff).
//!
//! Port von `bot/raid/partner_scores.py` — die Formeln werden 1:1 übernommen.
//! Jede Funktion ist zustandslos und voll unit-testbar ohne Datenbankverbindung.
//!
//! Konstanten entsprechen denen in `partner_scores.py` (Z. 19–31), mit einer
//! bewussten Abweichung: der Base-Score hat mit dem Courtesy-Anteil eine dritte
//! Säule bekommen (siehe [`crate::courtesy`]). Readiness und Fairness geben
//! dafür gemeinsam 10 % ab und behalten ihr Verhältnis von 65:35.
//!
//! ```text
//! LOOKBACK_DAYS               = 45
//! MIN_RELIABLE_SESSIONS       = 3
//! NEUTRAL_SCORE               = 0.5
//! NEW_PARTNER_MAX_MULTIPLIER  = 1.25
//! NEW_PARTNER_RAID_THRESHOLD  = 10
//! RAID_BOOST_MULTIPLIER       = 1.15
//! DEFAULT_RAID_BOOST_MULTIPLIER = 1.0
//! READINESS_DURATION_WEIGHT   = 0.6
//! READINESS_TIME_WEIGHT       = 0.4
//! FINAL_READINESS_WEIGHT      = 0.585   (vorher 0.65)
//! FINAL_FAIRNESS_WEIGHT       = 0.315   (vorher 0.35)
//! COURTESY_WEIGHT             = 0.10    (neu)
//! FAIRNESS_BALANCE_DIVISOR    = 20.0
//! FAIRNESS_RECEIVED_7D_THRESHOLD = 5.0
//! ```

/// Neutraler Score wenn zu wenig Daten vorhanden (Python `NEUTRAL_SCORE`).
pub const NEUTRAL_SCORE: f64 = 0.5;
/// Ab dieser Raids-Anzahl gilt ein Partner nicht mehr als „neu" (Python `NEW_PARTNER_RAID_THRESHOLD`).
pub const NEW_PARTNER_RAID_THRESHOLD: i64 = 10;
/// Maximaler Multiplikator für neue Partner (Python `NEW_PARTNER_MAX_MULTIPLIER`).
pub const NEW_PARTNER_MAX_MULTIPLIER: f64 = 1.25;
/// Boost-Multiplikator bei aktivem Raid-Boost-Plan (Python `RAID_BOOST_MULTIPLIER`).
pub const RAID_BOOST_MULTIPLIER: f64 = 1.15;
/// Standard-Multiplikator ohne Boost (Python `DEFAULT_RAID_BOOST_MULTIPLIER`).
pub const DEFAULT_RAID_BOOST_MULTIPLIER: f64 = 1.0;

// Gewichtungskonstanten für Readiness (Python Z. 26–27).
const READINESS_DURATION_WEIGHT: f64 = 0.6;
const READINESS_TIME_WEIGHT: f64 = 0.4;

// Gewichtungskonstanten für Final-Score. Die ursprünglichen 0.65/0.35 (Python
// Z. 28–29) sind proportional heruntergerechnet, damit der Courtesy-Anteil
// (`courtesy::COURTESY_WEIGHT`, 10 %) daneben Platz hat. Das Verhältnis von
// Readiness zu Fairness bleibt dadurch unverändert bei 65:35.
const FINAL_READINESS_WEIGHT: f64 = 0.585;
const FINAL_FAIRNESS_WEIGHT: f64 = 0.315;

// Fairness-Parameter (Python Z. 30–31).
const FAIRNESS_BALANCE_DIVISOR: f64 = 20.0;
const FAIRNESS_RECEIVED_7D_THRESHOLD: f64 = 5.0;

/// Eingaben für die Score-Berechnung eines Partners.
///
/// Entspricht den Rohwerten aus `_build_score` in `partner_scores.py` — alle
/// Werte werden vom Aufrufer aus DB/Live-State zusammengesetzt.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoringInputs {
    /// Durchschnittliche Stream-Dauer in Sekunden (aus Session-History).
    pub avg_duration_sec: i64,
    /// Vorab berechneter Basis-Zeitraster-Score (matching / total sessions).
    /// Ist `NEUTRAL_SCORE` wenn zu wenig Sessions.
    pub time_pattern_score_base: f64,
    /// Ob die Session-History für Zeitraster zuverlässig ist (>= MIN_RELIABLE_SESSIONS).
    pub time_pattern_reliable: bool,
    /// Ob der Partner gerade live ist.
    pub is_live: bool,
    /// Bisherige Uptime in Sekunden (0 wenn nicht live).
    pub current_uptime_sec: i64,
    /// Ob die Duration-History zuverlässig ist (>= MIN_RELIABLE_SESSIONS und avg > 0).
    pub duration_history_reliable: bool,
    /// Anzahl aller bisherigen erfolgreichen Raids *zu* diesem Partner (total, nicht 30d).
    pub received_successful_raids_total: i64,
    /// Ob Raid-Boost aktiv ist (aus `streamer_plans`).
    pub raid_boost_enabled: bool,
    /// Raids, die wir intern IN den letzten 30 Tagen *gesendet* haben.
    pub internal_sent_raids_30d: i64,
    /// Raids, die wir intern IN den letzten 30 Tagen *empfangen* haben.
    pub internal_received_raids_30d: i64,
    /// Raids, die wir intern IN den letzten 7 Tagen *empfangen* haben.
    pub internal_received_raids_7d: i64,
    /// Raids heute (Berlin-Zeit) schon empfangen.
    pub today_received_raids: i64,
    /// Courtesy-Score aus [`crate::courtesy::summarize`]: Anteil der eigenen
    /// Raids, nach denen der Streamer im Zielchat etwas geschrieben hat.
    /// `1.0` = schreibt immer oder noch keine Historie, `0.0` = schweigt stets.
    pub courtesy_score: f64,
}

/// Vorhandene (gecachte) Score-Werte eines Partners aus der vorigen
/// Refresh-Runde (`twitch_partner_raid_scores`).
///
/// Entspricht den Feldern, die Python `_build_score` im
/// `elif existing_cache is not None`-Zweig (partner_scores.py:738-756) aus der
/// Cache-Zeile zurückliest, um sie bei offline-Partner zu erhalten statt auf
/// NEUTRAL zu setzen. Alle Felder werden vor der Übernahme mit `round_score`
/// auf 6 Stellen gerundet (wie Pythons `_round_score`).
#[derive(Debug, Clone, PartialEq)]
pub struct CachedScores {
    pub duration_score: f64,
    pub time_pattern_score: f64,
    pub readiness_score: f64,
    pub fairness_score: f64,
    pub base_score: f64,
    pub final_score: f64,
}

/// Alle berechneten Score-Komponenten eines Partners.
///
/// Entspricht den Feldern in `_PreparedScore` in `partner_scores.py`.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreComponents {
    /// Duration-Score: wie weit ist der Partner noch vom Durchschnitt entfernt.
    /// Nur sinnvoll wenn live; sonst `NEUTRAL_SCORE`.
    pub duration_score: f64,
    /// Zeitraster-Score: passt die Startzeit zum üblichen Muster.
    pub time_pattern_score: f64,
    /// Readiness = gewichtetes Mittel aus duration_score + time_pattern_score.
    pub readiness_score: f64,
    /// Fairness = Balance der gegenseitigen Raids + Recency-Strafe.
    pub fairness_score: f64,
    /// Courtesy = Anteil eigener Raids mit Nachricht im Zielchat.
    pub courtesy_score: f64,
    /// Base = readiness * 0.585 + fairness * 0.315 + courtesy * 0.10
    /// (vor Multiplikatoren).
    pub base_score: f64,
    /// New-Partner-Multiplikator (1.0 .. 1.25, sinkt mit jeder erhaltenen Raid).
    pub new_partner_multiplier: f64,
    /// Raid-Boost-Multiplikator (1.15 wenn aktiv, sonst 1.0).
    pub raid_boost_multiplier: f64,
    /// Final = base * new_partner_multiplier * raid_boost_multiplier.
    pub final_score: f64,
    /// Ob dieser Partner als „neu bevorzugt" gilt (< NEW_PARTNER_RAID_THRESHOLD Raids).
    pub is_new_partner_preferred: bool,
}

/// Rundet auf die nächste ganze Zahl mit Banker's Rounding (ties-to-even),
/// passend zu Python `round()` ohne `ndigits`.
#[inline]
pub(crate) fn round_ties_to_even(value: f64) -> f64 {
    if !value.is_finite() {
        return value;
    }
    let floor = value.floor();
    let diff = value - floor;
    let epsilon = f64::EPSILON * value.abs().max(1.0) * 4.0;
    if diff < 0.5 - epsilon {
        floor
    } else if diff > 0.5 + epsilon {
        floor + 1.0
    } else if floor.rem_euclid(2.0) == 0.0 {
        floor
    } else {
        floor + 1.0
    }
}

/// `round(value, 6)` — identisch zu Pythons `_round_score` (Z. 207–208).
#[inline]
pub fn round_score(value: f64) -> f64 {
    format!("{value:.6}").parse::<f64>().unwrap_or(value)
}

/// `max(minimum, min(maximum, value))` — identisch zu Pythons `_clamp` (Z. 185–186).
/// Standard: `[0.0, 1.0]`.
#[inline]
pub fn clamp(value: f64, minimum: f64, maximum: f64) -> f64 {
    value.max(minimum).min(maximum)
}

#[inline]
fn clamp01(value: f64) -> f64 {
    clamp(value, 0.0, 1.0)
}

/// Duration-Score: wie weit hat der Partner noch Zeit bis zum Durchschnitt.
///
/// Python `_build_score` Z. 720–725:
/// ```python
/// duration_score = round_score(clamp((avg_duration_sec - current_uptime_sec) / float(avg_duration_sec)))
/// ```
/// Bedingung: `duration_history_reliable and avg_duration_sec > 0 and is_live`.
/// Sonst → `NEUTRAL_SCORE`.
pub fn compute_duration_score(
    avg_duration_sec: i64,
    current_uptime_sec: i64,
    duration_history_reliable: bool,
    is_live: bool,
) -> f64 {
    if is_live && duration_history_reliable && avg_duration_sec > 0 {
        round_score(clamp01(
            (avg_duration_sec - current_uptime_sec) as f64 / avg_duration_sec as f64,
        ))
    } else {
        NEUTRAL_SCORE
    }
}

/// Time-Pattern-Score: tatsächlicher Wert oder Neutral wenn nicht zuverlässig.
///
/// Python `_build_score` Z. 726: `time_pattern_score = time_pattern_score_base if time_pattern_reliable else NEUTRAL_SCORE`
pub fn compute_time_pattern_score(
    time_pattern_score_base: f64,
    time_pattern_reliable: bool,
) -> f64 {
    if time_pattern_reliable {
        round_score(time_pattern_score_base)
    } else {
        NEUTRAL_SCORE
    }
}

/// Readiness-Score = duration_score * 0.6 + time_pattern_score * 0.4.
///
/// Python `_readiness_score` (Z. 254–258):
/// ```python
/// def _readiness_score(duration_score, time_pattern_score):
///     return round_score(duration_score * 0.6 + time_pattern_score * 0.4)
/// ```
pub fn compute_readiness_score(duration_score: f64, time_pattern_score: f64) -> f64 {
    round_score(
        duration_score * READINESS_DURATION_WEIGHT + time_pattern_score * READINESS_TIME_WEIGHT,
    )
}

/// Fairness-Score aus drei Komponenten:
///
/// Python `_fairness_score` (Z. 261–279):
/// ```python
/// balance_30d         = clamp(0.5 + (sent_30d - received_30d) / 20.0)
/// received_7d_penalty = clamp(1.0 - received_7d / 5.0)
/// today_penalty       = 1.0 / (1.0 + max(0, today_received_raids))
/// fairness_score      = round_score(balance_30d*0.5 + received_7d_penalty*0.3 + today_penalty*0.2)
/// ```
///
/// - `balance_30d`: wer mehr gibt als nimmt, hat höheren Score
/// - `received_7d_penalty`: Strafe für Partner, die in den letzten 7 Tagen viele Raids bekamen
/// - `today_penalty`: Strafe wenn heute schon Raids empfangen wurden (1/n+1 Decay)
pub fn compute_fairness_score(
    sent_30d: i64,
    received_30d: i64,
    received_7d: i64,
    today_received_raids: i64,
) -> f64 {
    let balance_30d = clamp01(0.5 + (sent_30d - received_30d) as f64 / FAIRNESS_BALANCE_DIVISOR);
    let received_7d_penalty = clamp01(1.0 - received_7d as f64 / FAIRNESS_RECEIVED_7D_THRESHOLD);
    let today_penalty = 1.0 / (1.0 + today_received_raids.max(0) as f64);
    round_score(balance_30d * 0.5 + received_7d_penalty * 0.3 + today_penalty * 0.2)
}

/// Base-Score (Pre-Boost) = readiness * 0.585 + fairness * 0.315 + courtesy * 0.10.
///
/// Erweitert das ursprüngliche `readiness * 0.65 + fairness * 0.35` (Python
/// `_combine_preboost_score`, Z. 282–286) um den Courtesy-Anteil aus
/// [`crate::courtesy`]. Readiness und Fairness behalten ihr Verhältnis
/// zueinander, geben aber gemeinsam 10 % ab.
///
/// `courtesy_score` ist ein reiner Malus für belegtes Schweigen nach eigenen
/// Raids: wer schreibt und wer keine Historie hat, steht bei 1.0 und verliert
/// nichts. Nur wer wiederholt schweigend weiterzieht, rutscht Richtung 0.0 und
/// verliert damit bis zu 10 Prozentpunkte Base-Score.
pub fn compute_base_score(readiness_score: f64, fairness_score: f64, courtesy_score: f64) -> f64 {
    round_score(
        readiness_score * FINAL_READINESS_WEIGHT
            + fairness_score * FINAL_FAIRNESS_WEIGHT
            + clamp01(courtesy_score) * crate::courtesy::COURTESY_WEIGHT,
    )
}

/// New-Partner-Multiplikator: linear von 1.25 (0 Raids) auf 1.0 (>= 10 Raids).
///
/// Python `_new_partner_multiplier` (Z. 248–251):
/// ```python
/// capped = max(0, min(received_successful_raids_total, NEW_PARTNER_RAID_THRESHOLD))
/// step   = (NEW_PARTNER_MAX_MULTIPLIER - 1.0) / float(NEW_PARTNER_RAID_THRESHOLD)
/// return round_score(max(1.0, NEW_PARTNER_MAX_MULTIPLIER - (step * capped)))
/// ```
pub fn compute_new_partner_multiplier(received_successful_raids_total: i64) -> f64 {
    let capped = received_successful_raids_total.clamp(0, NEW_PARTNER_RAID_THRESHOLD);
    let step = (NEW_PARTNER_MAX_MULTIPLIER - 1.0) / NEW_PARTNER_RAID_THRESHOLD as f64;
    round_score((NEW_PARTNER_MAX_MULTIPLIER - step * capped as f64).max(1.0))
}

/// Raid-Boost-Multiplikator: 1.15 wenn aktiv, sonst 1.0.
///
/// Python `_build_score` Z. 710–712:
/// ```python
/// raid_boost_multiplier = RAID_BOOST_MULTIPLIER if raid_boost_enabled else DEFAULT_RAID_BOOST_MULTIPLIER
/// ```
pub fn compute_raid_boost_multiplier(raid_boost_enabled: bool) -> f64 {
    if raid_boost_enabled {
        RAID_BOOST_MULTIPLIER
    } else {
        DEFAULT_RAID_BOOST_MULTIPLIER
    }
}

/// Final-Score = base_score * new_partner_multiplier * raid_boost_multiplier.
///
/// Python `_build_score` Z. 735–737:
/// ```python
/// final_score = round_score(base_score * new_partner_multiplier * raid_boost_multiplier)
/// ```
pub fn compute_final_score(
    base_score: f64,
    new_partner_multiplier: f64,
    raid_boost_multiplier: f64,
) -> f64 {
    round_score(base_score * new_partner_multiplier * raid_boost_multiplier)
}

/// Berechnet alle Score-Komponenten aus den Roheingaben.
///
/// Entspricht dem Kern von `_build_score` in `partner_scores.py` — nur der
/// reine Berechnungs-Pfad, ohne DB-Zugriffe oder Live-State-Abfragen.
///
/// Drei-Wege-Pfad analog Python `_build_score` (partner_scores.py:715-772):
///
/// 1. **live** (`is_live = true`): alle Komponenten frisch berechnet.
/// 2. **offline mit Cache** (`is_live = false`, `existing_cache = Some(..)`):
///    duration/time_pattern/readiness/fairness/base/final werden aus der
///    Cache-Zeile übernommen (gerundet), nicht auf NEUTRAL zurückgesetzt
///    (Python Z. 738–756). So verliert ein gerade offline gegangener Partner
///    seinen zuvor live berechneten Score nicht.
/// 3. **offline ohne Cache** (`is_live = false`, `existing_cache = None`):
///    NEUTRAL_SCORE für duration, time_pattern aus Reliable-Flag, Rest neu
///    berechnet (Python Z. 757–772).
///
/// Die Multiplikatoren (`new_partner_multiplier`, `raid_boost_multiplier`,
/// `is_new_partner_preferred`) werden in allen Pfaden gleich bestimmt — Python
/// setzt sie vor dem if/elif/else.
pub fn compute_scores_with_cache(
    inputs: &ScoringInputs,
    existing_cache: Option<&CachedScores>,
) -> ScoreComponents {
    let is_new_partner_preferred =
        inputs.received_successful_raids_total < NEW_PARTNER_RAID_THRESHOLD;
    let new_partner_multiplier =
        compute_new_partner_multiplier(inputs.received_successful_raids_total);
    let raid_boost_multiplier = compute_raid_boost_multiplier(inputs.raid_boost_enabled);

    // Offline-mit-Cache: gecachte Werte erhalten (Python Z. 738–756).
    if !inputs.is_live {
        if let Some(cache) = existing_cache {
            return ScoreComponents {
                duration_score: round_score(cache.duration_score),
                time_pattern_score: round_score(cache.time_pattern_score),
                readiness_score: round_score(cache.readiness_score),
                fairness_score: round_score(cache.fairness_score),
                // Courtesy hängt an der Raid-Historie, nicht am Live-Zustand:
                // der aktuelle Wert gilt weiter, auch wenn base/final aus dem
                // Cache stammen.
                courtesy_score: round_score(clamp01(inputs.courtesy_score)),
                base_score: round_score(cache.base_score),
                new_partner_multiplier,
                raid_boost_multiplier,
                final_score: round_score(cache.final_score),
                is_new_partner_preferred,
            };
        }
    }

    let duration_score = compute_duration_score(
        inputs.avg_duration_sec,
        inputs.current_uptime_sec,
        inputs.duration_history_reliable,
        inputs.is_live,
    );
    let time_pattern_score =
        compute_time_pattern_score(inputs.time_pattern_score_base, inputs.time_pattern_reliable);
    let readiness_score = compute_readiness_score(duration_score, time_pattern_score);
    let fairness_score = compute_fairness_score(
        inputs.internal_sent_raids_30d,
        inputs.internal_received_raids_30d,
        inputs.internal_received_raids_7d,
        inputs.today_received_raids,
    );
    let courtesy_score = round_score(clamp01(inputs.courtesy_score));
    let base_score = compute_base_score(readiness_score, fairness_score, courtesy_score);
    let final_score =
        compute_final_score(base_score, new_partner_multiplier, raid_boost_multiplier);

    ScoreComponents {
        duration_score,
        time_pattern_score,
        readiness_score,
        fairness_score,
        courtesy_score,
        base_score,
        new_partner_multiplier,
        raid_boost_multiplier,
        final_score,
        is_new_partner_preferred,
    }
}

/// Rückwärtskompatibler Einstieg ohne Cache (live- bzw. offline-ohne-Cache-Pfad).
///
/// Entspricht dem bisherigen Verhalten; ruft [`compute_scores_with_cache`] mit
/// `existing_cache = None`. Bestehende Aufrufer (Score-Refresh-Pipeline) bleiben
/// unverändert; der Offline-mit-Cache-Pfad (P2.41) wird über die `_with_cache`-
/// Variante erreicht, sobald die Pipeline die Cache-Zeile mitführt
/// (siehe WIRING-TODO).
pub fn compute_scores(inputs: &ScoringInputs) -> ScoreComponents {
    compute_scores_with_cache(inputs, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Hilfsmakro: rundet auf 6 Dezimalstellen für f64-Vergleiche.
    fn approx_eq(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    // ─── clamp / round_score ─────────────────────────────────────────────────

    #[test]
    fn clamp_klemmt_korrekt() {
        assert!(approx_eq(clamp01(1.5), 1.0));
        assert!(approx_eq(clamp01(-0.1), 0.0));
        assert!(approx_eq(clamp01(0.7), 0.7));
    }

    #[test]
    fn round_score_rundet_auf_6_stellen() {
        assert_eq!(round_score(0.123456789), 0.123457);
        assert_eq!(round_score(0.5), 0.5);
        assert_eq!(round_score(1.0000001), 1.0);
    }

    #[test]
    fn round_score_nutzt_bankers_rounding_bei_halben_mikroeinheiten() {
        assert_eq!(round_score(0.1234565), 0.123456);
        assert_eq!(round_score(0.1234575), 0.123457);
        assert_eq!(round_score(-0.1234565), -0.123456);
        assert_eq!(round_score(-0.1234575), -0.123457);
    }

    // ─── duration_score ──────────────────────────────────────────────────────

    #[test]
    fn duration_score_bei_haelfte_der_zeit() {
        // avg=7200s, uptime=3600s → (7200-3600)/7200 = 0.5
        let score = compute_duration_score(7200, 3600, true, true);
        assert_eq!(score, 0.5);
    }

    #[test]
    fn duration_score_am_anfang_fast_1() {
        // avg=7200s, uptime=100s → (7200-100)/7200 ≈ 0.986111
        let score = compute_duration_score(7200, 100, true, true);
        assert_eq!(score, round_score((7200.0 - 100.0) / 7200.0));
    }

    #[test]
    fn duration_score_neutral_wenn_nicht_live() {
        assert_eq!(
            compute_duration_score(7200, 3600, true, false),
            NEUTRAL_SCORE
        );
    }

    #[test]
    fn duration_score_neutral_wenn_history_unzuverlaessig() {
        assert_eq!(
            compute_duration_score(7200, 3600, false, true),
            NEUTRAL_SCORE
        );
    }

    #[test]
    fn duration_score_clampt_wenn_uptime_laenger_als_avg() {
        // Partner streamt länger als Durchschnitt → score = 0.0 (nicht negativ)
        let score = compute_duration_score(3600, 7200, true, true);
        assert_eq!(score, 0.0);
    }

    // ─── new_partner_multiplier ──────────────────────────────────────────────

    #[test]
    fn new_partner_multiplier_bei_null_raids_ist_maximum() {
        // capped=0, step=0.025, mult=max(1.0, 1.25-0) = 1.25
        assert_eq!(compute_new_partner_multiplier(0), 1.25);
    }

    #[test]
    fn new_partner_multiplier_bei_5_raids() {
        // capped=5, step=0.025, mult=max(1.0, 1.25-0.125)=1.125
        assert_eq!(compute_new_partner_multiplier(5), round_score(1.125));
    }

    #[test]
    fn new_partner_multiplier_ab_10_raids_ist_1() {
        // capped=10, step=0.025, mult=max(1.0, 1.25-0.25)=max(1.0,1.0)=1.0
        assert_eq!(compute_new_partner_multiplier(10), 1.0);
        // Mehr als 10 → immer noch 1.0 (max-clamp)
        assert_eq!(compute_new_partner_multiplier(100), 1.0);
    }

    #[test]
    fn new_partner_multiplier_negative_raids_werden_auf_null_normiert() {
        assert_eq!(compute_new_partner_multiplier(-5), 1.25);
    }

    // ─── fairness_score ──────────────────────────────────────────────────────

    #[test]
    fn fairness_score_ausgeglichenes_verhaeltnis() {
        // sent=5, received=5 → balance=0.5 | 7d=0 → penalty=1.0 | today=0 → 1.0
        // = 0.5*0.5 + 1.0*0.3 + 1.0*0.2 = 0.25+0.3+0.2 = 0.75
        let score = compute_fairness_score(5, 5, 0, 0);
        assert_eq!(score, 0.75);
    }

    #[test]
    fn fairness_score_bei_aktiven_heutigen_raids() {
        // sent=0, received=0, received_7d=0, today=3
        // balance=clamp(0.5+0)=0.5, 7d_penalty=1.0, today=1/4=0.25
        // = 0.5*0.5 + 1.0*0.3 + 0.25*0.2 = 0.25+0.3+0.05 = 0.6
        let score = compute_fairness_score(0, 0, 0, 3);
        assert_eq!(score, 0.6);
    }

    #[test]
    fn fairness_score_mit_vielen_7d_raids_und_unausgeglichenem_30d() {
        // sent=0, received=10 (deficit), received_7d=8, today=3
        // balance = clamp(0.5 + (0-10)/20.0) = clamp(0.0) = 0.0
        // 7d_penalty = clamp(1.0 - 8/5.0) = clamp(-0.6) = 0.0
        // today_penalty = 1/(1+3) = 0.25
        // = 0.0*0.5 + 0.0*0.3 + 0.25*0.2 = 0.05
        let score = compute_fairness_score(0, 10, 8, 3);
        assert_eq!(score, 0.05);
    }

    #[test]
    fn fairness_score_bei_sehr_positivem_30d_balance_wird_geclampt() {
        // sent=100, received=0 → balance=clamp(0.5+5.0)=clamp(5.5)=1.0
        // 7d=0 → 1.0, today=0 → 1.0
        // = 1.0*0.5 + 1.0*0.3 + 1.0*0.2 = 1.0
        let score = compute_fairness_score(100, 0, 0, 0);
        assert_eq!(score, 1.0);
    }

    // ─── readiness_score ─────────────────────────────────────────────────────

    #[test]
    fn readiness_score_beide_neutral() {
        // 0.5*0.6 + 0.5*0.4 = 0.5
        assert_eq!(compute_readiness_score(NEUTRAL_SCORE, NEUTRAL_SCORE), 0.5);
    }

    #[test]
    fn readiness_score_gewichtung_korrekt() {
        // duration=0.8, time_pattern=0.6 → 0.8*0.6 + 0.6*0.4 = 0.48+0.24 = 0.72
        let score = compute_readiness_score(0.8, 0.6);
        assert_eq!(score, round_score(0.72));
    }

    // ─── base_score ──────────────────────────────────────────────────────────

    #[test]
    fn base_score_gewichtung_korrekt() {
        // readiness=0.6, fairness=0.68, courtesy=1.0 (schreibt immer)
        // → 0.6*0.585 + 0.68*0.315 + 1.0*0.10 = 0.351 + 0.2142 + 0.10 = 0.6652
        let score = compute_base_score(0.6, 0.68, 1.0);
        assert_eq!(score, round_score(0.6652));
    }

    #[test]
    fn base_score_courtesy_kostet_hoechstens_zehn_punkte() {
        // Derselbe Partner einmal als Dauerschreiber, einmal als Dauerschweiger:
        // der Unterschied ist exakt das Courtesy-Gewicht.
        let schreiber = compute_base_score(0.6, 0.68, 1.0);
        let schweiger = compute_base_score(0.6, 0.68, 0.0);
        assert_eq!(
            round_score(schreiber - schweiger),
            round_score(crate::courtesy::COURTESY_WEIGHT)
        );
    }

    #[test]
    fn base_score_clampt_courtesy_ausserhalb_des_bereichs() {
        let oben = compute_base_score(0.6, 0.68, 5.0);
        let unten = compute_base_score(0.6, 0.68, -2.0);
        assert_eq!(oben, compute_base_score(0.6, 0.68, 1.0));
        assert_eq!(unten, compute_base_score(0.6, 0.68, 0.0));
    }

    // ─── compute_scores (Integrationstest) ───────────────────────────────────

    #[test]
    fn compute_scores_live_neuer_partner_beispiel_1() {
        // Manuell nachgerechnete Werte — dieser Test pinnt die exakten Ergebnisse.
        //
        // Eingaben:
        //   avg_duration=7200, uptime=3600, history_reliable=true, live=true
        //   time_pattern_base=0.75, time_reliable=true
        //   raids_total=5 (neuer Partner)
        //   sent_30d=3, received_30d=1, received_7d=2, today=0, boost=false
        //
        // Erwartete Zwischenwerte:
        //   duration_score     = (7200-3600)/7200 = 0.5
        //   time_pattern_score = 0.75
        //   readiness          = 0.5*0.6 + 0.75*0.4 = 0.6
        //   balance_30d        = clamp(0.5 + 2/20) = 0.6
        //   7d_penalty         = clamp(1 - 2/5)    = 0.6
        //   today_penalty      = 1/1               = 1.0
        //   fairness           = 0.6*0.5+0.6*0.3+1.0*0.2 = 0.68
        //   courtesy           = 1.0 (schreibt immer / keine Historie)
        //   base               = 0.6*0.585+0.68*0.315+1.0*0.10 = 0.6652
        //   new_mult           = 1.25 - 5*0.025 = 1.125
        //   boost_mult         = 1.0
        //   final              = 0.6652 * 1.125 * 1.0 = 0.74835
        let inputs = ScoringInputs {
            avg_duration_sec: 7200,
            current_uptime_sec: 3600,
            duration_history_reliable: true,
            is_live: true,
            time_pattern_score_base: 0.75,
            time_pattern_reliable: true,
            received_successful_raids_total: 5,
            raid_boost_enabled: false,
            internal_sent_raids_30d: 3,
            internal_received_raids_30d: 1,
            internal_received_raids_7d: 2,
            today_received_raids: 0,
            courtesy_score: 1.0,
        };
        let result = compute_scores(&inputs);

        assert_eq!(result.duration_score, 0.5);
        assert_eq!(result.time_pattern_score, 0.75);
        assert_eq!(result.readiness_score, 0.6);
        assert_eq!(result.fairness_score, round_score(0.68));
        assert_eq!(result.base_score, round_score(0.6652));
        assert_eq!(result.courtesy_score, 1.0);
        assert_eq!(result.new_partner_multiplier, round_score(1.125));
        assert_eq!(result.raid_boost_multiplier, 1.0);
        assert_eq!(result.final_score, round_score(0.6652 * 1.125));
        assert!(result.is_new_partner_preferred);
    }

    #[test]
    fn compute_scores_offline_eingesessener_partner_beispiel_2() {
        // Eingaben:
        //   nicht live, history unreliable → NEUTRAL_SCORE überall
        //   raids_total=15 (kein neuer Partner)
        //   sent=5, received=5, 7d=0, today=0, boost=false
        //
        // Erwartete Zwischenwerte:
        //   duration_score     = 0.5 (nicht live)
        //   time_pattern_score = 0.5 (nicht reliable)
        //   readiness          = 0.5
        //   fairness           = 0.5*0.5+1.0*0.3+1.0*0.2 = 0.75
        //   courtesy           = 1.0
        //   base               = 0.5*0.585+0.75*0.315+1.0*0.10 = 0.62875
        //   new_mult           = 1.0 (>= 10 Raids)
        //   final              = 0.62875
        let inputs = ScoringInputs {
            avg_duration_sec: 0,
            current_uptime_sec: 0,
            duration_history_reliable: false,
            is_live: false,
            time_pattern_score_base: NEUTRAL_SCORE,
            time_pattern_reliable: false,
            received_successful_raids_total: 15,
            raid_boost_enabled: false,
            internal_sent_raids_30d: 5,
            internal_received_raids_30d: 5,
            internal_received_raids_7d: 0,
            today_received_raids: 0,
            courtesy_score: 1.0,
        };
        let result = compute_scores(&inputs);

        assert_eq!(result.duration_score, NEUTRAL_SCORE);
        assert_eq!(result.time_pattern_score, NEUTRAL_SCORE);
        assert_eq!(result.readiness_score, 0.5);
        assert_eq!(result.fairness_score, 0.75);
        assert_eq!(result.base_score, round_score(0.62875));
        assert_eq!(result.new_partner_multiplier, 1.0);
        assert!(!result.is_new_partner_preferred);
        assert_eq!(result.final_score, round_score(0.62875));
    }

    #[test]
    fn compute_scores_dauerschweiger_verliert_genau_das_courtesy_gewicht() {
        // Zwei identische Partner, einziger Unterschied ist die Raid-Etikette.
        // Der Schweiger muss exakt COURTESY_WEIGHT im Base-Score verlieren,
        // nicht mehr und nicht weniger.
        let schreiber = ScoringInputs {
            avg_duration_sec: 7200,
            current_uptime_sec: 3600,
            duration_history_reliable: true,
            is_live: true,
            time_pattern_score_base: 0.75,
            time_pattern_reliable: true,
            received_successful_raids_total: 15,
            raid_boost_enabled: false,
            internal_sent_raids_30d: 3,
            internal_received_raids_30d: 1,
            internal_received_raids_7d: 2,
            today_received_raids: 0,
            courtesy_score: 1.0,
        };
        let schweiger = ScoringInputs {
            courtesy_score: 0.0,
            ..schreiber.clone()
        };

        let gut = compute_scores(&schreiber);
        let schlecht = compute_scores(&schweiger);

        // Alle übrigen Komponenten sind identisch.
        assert_eq!(gut.readiness_score, schlecht.readiness_score);
        assert_eq!(gut.fairness_score, schlecht.fairness_score);
        // Der Abstand ist genau das Courtesy-Gewicht.
        assert_eq!(
            round_score(gut.base_score - schlecht.base_score),
            round_score(crate::courtesy::COURTESY_WEIGHT)
        );
        assert!(gut.final_score > schlecht.final_score);
    }

    #[test]
    fn compute_scores_mit_raid_boost_erhoehung() {
        // Identisch zu Beispiel 2, aber raid_boost_enabled=true → final *= 1.15
        let inputs = ScoringInputs {
            avg_duration_sec: 0,
            current_uptime_sec: 0,
            duration_history_reliable: false,
            is_live: false,
            time_pattern_score_base: NEUTRAL_SCORE,
            time_pattern_reliable: false,
            received_successful_raids_total: 15,
            raid_boost_enabled: true,
            internal_sent_raids_30d: 5,
            internal_received_raids_30d: 5,
            internal_received_raids_7d: 0,
            today_received_raids: 0,
            courtesy_score: 1.0,
        };
        let result = compute_scores(&inputs);
        assert_eq!(result.raid_boost_multiplier, 1.15);
        assert_eq!(result.final_score, round_score(round_score(0.62875) * 1.15));
    }

    #[test]
    fn compute_scores_edge_maximale_fairness_strafe() {
        // sent=0, received=10, received_7d=8, today=3 → fairness=0.05 (Minimum)
        // live=false → duration=0.5, time=0.5, readiness=0.5
        // base = 0.5*0.585 + 0.05*0.315 + 1.0*0.10 = 0.2925 + 0.01575 + 0.1
        let inputs = ScoringInputs {
            avg_duration_sec: 0,
            current_uptime_sec: 0,
            duration_history_reliable: false,
            is_live: false,
            time_pattern_score_base: NEUTRAL_SCORE,
            time_pattern_reliable: false,
            received_successful_raids_total: 0,
            raid_boost_enabled: false,
            internal_sent_raids_30d: 0,
            internal_received_raids_30d: 10,
            internal_received_raids_7d: 8,
            today_received_raids: 3,
            courtesy_score: 1.0,
        };
        let result = compute_scores(&inputs);
        assert_eq!(result.fairness_score, round_score(0.05));
        assert_eq!(
            result.base_score,
            round_score(0.5 * 0.585 + 0.05 * 0.315 + 0.10)
        );
    }

    // ─── P2.41: offline-mit-Cache erhält den vorigen Live-Score ──────────────

    #[test]
    fn compute_scores_offline_mit_cache_erhaelt_werte() {
        // Partner offline, aber vorige Live-Runde hatte einen nicht-neutralen
        // Score gecacht. Python (partner_scores.py:738-756) übernimmt die Cache-
        // Werte statt sie auf NEUTRAL zurückzusetzen.
        let inputs = ScoringInputs {
            avg_duration_sec: 0,
            current_uptime_sec: 0,
            duration_history_reliable: false,
            is_live: false,
            time_pattern_score_base: NEUTRAL_SCORE,
            time_pattern_reliable: false,
            received_successful_raids_total: 5, // < 10 → neuer Partner, mult 1.125
            raid_boost_enabled: false,
            internal_sent_raids_30d: 0,
            internal_received_raids_30d: 0,
            internal_received_raids_7d: 0,
            today_received_raids: 0,
            courtesy_score: 1.0,
        };
        let cache = CachedScores {
            duration_score: 0.83,
            time_pattern_score: 0.72,
            readiness_score: 0.786,
            fairness_score: 0.61,
            base_score: 0.7245,
            final_score: 0.815,
        };

        let result = compute_scores_with_cache(&inputs, Some(&cache));

        // Komponenten 1:1 aus dem Cache (gerundet), NICHT auf NEUTRAL gesetzt.
        assert_eq!(result.duration_score, round_score(0.83));
        assert_eq!(result.time_pattern_score, round_score(0.72));
        assert_eq!(result.readiness_score, round_score(0.786));
        assert_eq!(result.fairness_score, round_score(0.61));
        assert_eq!(result.base_score, round_score(0.7245));
        assert_eq!(result.final_score, round_score(0.815));
        // Multiplikatoren werden wie in Python unabhängig vom Pfad bestimmt.
        assert_eq!(result.new_partner_multiplier, round_score(1.125));
        assert_eq!(result.raid_boost_multiplier, 1.0);
        assert!(result.is_new_partner_preferred);
    }

    #[test]
    fn compute_scores_offline_ohne_cache_bleibt_neutral() {
        // Gleiche Eingaben, aber existing_cache = None → offline-ohne-Cache-Pfad
        // (NEUTRAL/neu berechnet) — unterscheidet sich klar vom Cache-Pfad.
        let inputs = ScoringInputs {
            avg_duration_sec: 0,
            current_uptime_sec: 0,
            duration_history_reliable: false,
            is_live: false,
            time_pattern_score_base: NEUTRAL_SCORE,
            time_pattern_reliable: false,
            received_successful_raids_total: 5,
            raid_boost_enabled: false,
            internal_sent_raids_30d: 0,
            internal_received_raids_30d: 0,
            internal_received_raids_7d: 0,
            today_received_raids: 0,
            courtesy_score: 1.0,
        };
        let result = compute_scores_with_cache(&inputs, None);
        assert_eq!(result.duration_score, NEUTRAL_SCORE);
        assert_eq!(result.time_pattern_score, NEUTRAL_SCORE);
        // entspricht exakt dem alten compute_scores-Verhalten.
        assert_eq!(result, compute_scores(&inputs));
    }

    #[test]
    fn compute_scores_live_ignoriert_cache() {
        // Live-Partner: trotz vorhandenem Cache wird frisch berechnet
        // (Python: if is_live-Zweig hat Vorrang vor elif existing_cache).
        let inputs = ScoringInputs {
            avg_duration_sec: 7200,
            current_uptime_sec: 3600,
            duration_history_reliable: true,
            is_live: true,
            time_pattern_score_base: 0.75,
            time_pattern_reliable: true,
            received_successful_raids_total: 5,
            raid_boost_enabled: false,
            internal_sent_raids_30d: 3,
            internal_received_raids_30d: 1,
            internal_received_raids_7d: 2,
            today_received_raids: 0,
            courtesy_score: 1.0,
        };
        let cache = CachedScores {
            duration_score: 0.1,
            time_pattern_score: 0.1,
            readiness_score: 0.1,
            fairness_score: 0.1,
            base_score: 0.1,
            final_score: 0.1,
        };
        let with_cache = compute_scores_with_cache(&inputs, Some(&cache));
        let without_cache = compute_scores(&inputs);
        assert_eq!(
            with_cache, without_cache,
            "live-Pfad ignoriert den Cache vollständig"
        );
        assert_eq!(with_cache.duration_score, 0.5);
    }
}
