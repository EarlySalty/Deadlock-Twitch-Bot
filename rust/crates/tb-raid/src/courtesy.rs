//! Raid-Etikette: Einstufung eines Raiders im Zielchat und der daraus
//! abgeleitete Courtesy-Score. Reine Logik, kein DB-Zugriff.
//!
//! ## Warum
//!
//! Ein Raid ist nur dann etwas wert, wenn der raidende Streamer im Zielchat
//! auch auftaucht. Wer schweigend weiterzieht, schiebt Zuschauer in einen
//! fremden Chat, ohne die Verbindung herzustellen, um die es eigentlich geht.
//!
//! ## Die drei Klassen
//!
//! | Klasse                      | Bedingung                                   |
//! |-----------------------------|---------------------------------------------|
//! | [`CourtesyClass::Engaged`]  | >= 3 Nachrichten, oder >= 2 über >= 3 Minuten |
//! | [`CourtesyClass::Greeter`]  | 1 bis 2 Nachrichten                          |
//! | [`CourtesyClass::Silent`]   | keine Nachricht                              |
//!
//! `Engaged` und `Greeter` sind im **Score gleichwertig** — ein kurzes Hallo
//! reicht, mehr ist keine Pflicht. Die Klassen unterscheiden sich nur beim
//! Matching: Vielschreiber sollen zu Vielschreibern geraidet werden, damit auf
//! beiden Seiten jemand ist, der sich unterhalten will.
//!
//! [`CourtesyOutcome::Unknown`] steht für „nicht messbar" (Chat-Beobachtung
//! nicht verfügbar, Bot-Neustart, Zielstream vorzeitig beendet, Raid
//! umgeleitet). Unknown zählt **nie** als Schweigen — sonst würde ein
//! Infrastruktur-Ausfall pauschal alle Streamer abwerten.

use std::time::Duration;

/// Ab dieser Nachrichtenzahl gilt ein Raider als aktiv im Zielchat.
pub const ENGAGED_MIN_MESSAGES: u32 = 3;

/// Ab dieser Nachrichtenzahl reicht schon die Zeitspanne für „aktiv" — wer
/// hallo sagt, bleibt und sich später verabschiedet, ist kein reiner Grüßer.
pub const ENGAGED_SPAN_MESSAGES: u32 = 2;

/// Mindest-Zeitspanne zwischen erster und letzter Nachricht für [`ENGAGED_SPAN_MESSAGES`].
pub const ENGAGED_MIN_SPAN: Duration = Duration::from_secs(3 * 60);

/// Betrachtungszeitraum der Courtesy-Historie in Tagen (wie `LOOKBACK_DAYS`
/// beim übrigen Partner-Scoring).
pub const COURTESY_LOOKBACK_DAYS: i64 = 45;

/// Gewicht des Courtesy-Anteils im Base-Score.
pub const COURTESY_WEIGHT: f64 = 0.10;

/// Stärke des Shrinks: entspricht `COURTESY_PRIOR_STRENGTH` zusätzlichen
/// Beobachtungen zum Ausgangswert. Ein einzelner Ausrutscher soll niemanden
/// abstürzen lassen.
pub const COURTESY_PRIOR_STRENGTH: f64 = 3.0;

/// Ausgangswert ohne Datenlage: der **volle** Wert, kein Abzug.
///
/// Der Courtesy-Anteil ist ausschließlich ein Malus für belegtes Schweigen.
/// Wer schreibt, egal ob kurz oder ausführlich, bekommt den vollen Wert; wer
/// noch keine messbare Historie hat, ebenfalls. Ein Prior in der Mitte würde
/// auch tadellose Streamer einen Rest-Malus tragen lassen, und das ist nicht
/// gewollt.
pub const COURTESY_PRIOR: f64 = 1.0;

/// Wie sich ein Raider im Zielchat verhalten hat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CourtesyClass {
    /// Hat sich im Zielchat unterhalten.
    Engaged,
    /// Hat kurz hallo gesagt.
    Greeter,
    /// Hat nichts geschrieben.
    Silent,
}

impl CourtesyClass {
    /// Stabiler Bezeichner für DB und Logs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Engaged => "engaged",
            Self::Greeter => "greeter",
            Self::Silent => "silent",
        }
    }

    /// Liest den DB-Bezeichner zurück. Unbekannte Werte → `None`.
    ///
    /// Bewusst kein `FromStr`: das Gegenstück ist [`CourtesyClass::as_str`],
    /// und der Wertebereich ist der DB-Vertrag, keine allgemeine Textform.
    pub fn from_db(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "engaged" => Some(Self::Engaged),
            "greeter" => Some(Self::Greeter),
            "silent" => Some(Self::Silent),
            _ => None,
        }
    }

    /// Punktwert im Courtesy-Score. `Engaged` und `Greeter` sind bewusst
    /// gleichwertig — nur wer gar nichts schreibt, verliert.
    pub fn value(self) -> f64 {
        match self {
            Self::Engaged | Self::Greeter => 1.0,
            Self::Silent => 0.0,
        }
    }

    /// Ob diese Klasse überhaupt geschrieben hat.
    pub fn wrote(self) -> bool {
        !matches!(self, Self::Silent)
    }
}

/// Ergebnis einer einzelnen Raid-Beobachtung.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CourtesyOutcome {
    /// Messung gelungen.
    Classified(CourtesyClass),
    /// Nicht messbar — fließt nirgends ein.
    Unknown,
}

impl CourtesyOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Classified(class) => class.as_str(),
            Self::Unknown => "unknown",
        }
    }

    /// Liest den DB-Bezeichner zurück; alles Unbekannte gilt als
    /// [`CourtesyOutcome::Unknown`] und fließt damit nirgends ein.
    pub fn from_db(value: &str) -> Self {
        match CourtesyClass::from_db(value) {
            Some(class) => Self::Classified(class),
            None => Self::Unknown,
        }
    }

    pub fn class(self) -> Option<CourtesyClass> {
        match self {
            Self::Classified(class) => Some(class),
            Self::Unknown => None,
        }
    }
}

/// Stuft eine Beobachtung anhand Nachrichtenzahl und Zeitspanne ein.
///
/// `span` ist der Abstand zwischen erster und letzter Nachricht; bei weniger
/// als zwei Nachrichten ist er bedeutungslos.
pub fn classify(message_count: u32, span: Duration) -> CourtesyClass {
    if message_count == 0 {
        return CourtesyClass::Silent;
    }
    if message_count >= ENGAGED_MIN_MESSAGES
        || (message_count >= ENGAGED_SPAN_MESSAGES && span >= ENGAGED_MIN_SPAN)
    {
        return CourtesyClass::Engaged;
    }
    CourtesyClass::Greeter
}

/// Zusammenfassung der Courtesy-Historie eines Streamers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CourtesySummary {
    /// Anzahl auswertbarer Raids (ohne `Unknown`).
    pub observed: u32,
    /// Davon mit mindestens einer Nachricht.
    pub wrote: u32,
    /// Davon in der Klasse `Engaged`.
    pub engaged: u32,
    /// Score in `[0, 1]`, gegen [`COURTESY_PRIOR`] geshrinkt.
    pub score: f64,
    /// Vorherrschende Klasse für das Matching, `None` ohne Datenlage.
    pub class: Option<CourtesyClass>,
}

impl Default for CourtesySummary {
    fn default() -> Self {
        Self {
            observed: 0,
            wrote: 0,
            engaged: 0,
            score: COURTESY_PRIOR,
            class: None,
        }
    }
}

/// Anteil der `Engaged`-Beobachtungen, ab dem ein Streamer selbst als
/// `Engaged` gilt und bevorzugt zu anderen Vielschreibern geraidet wird.
pub const ENGAGED_CLASS_SHARE: f64 = 0.5;

/// Anteil der Beobachtungen mit Nachricht, ab dem ein Streamer nicht mehr als
/// `Silent` eingestuft wird. Wer in zwei von drei Fällen schreibt, ist ein
/// Schreiber — gelegentliche Aussetzer ändern daran nichts.
pub const WRITER_CLASS_SHARE: f64 = 0.34;

/// Fasst eine Folge von Beobachtungen zu Score und Matching-Klasse zusammen.
///
/// `Unknown` wird vollständig ignoriert. Der Score ist der Anteil der Raids
/// mit Nachricht, geshrinkt gegen [`COURTESY_PRIOR`]:
///
/// ```text
/// score = (wrote + PRIOR_STRENGTH * PRIOR) / (observed + PRIOR_STRENGTH)
/// ```
///
/// Weil der Prior der volle Wert ist, erreicht ein durchgängiger Schreiber
/// exakt 1.0 und trägt keinen Rest-Malus; ohne Beobachtungen bleibt es
/// ebenfalls bei 1.0 mit `class = None`. Nur belegtes Schweigen drückt den
/// Wert, und zwar anteilig zu seiner Häufigkeit.
pub fn summarize(outcomes: &[CourtesyOutcome]) -> CourtesySummary {
    let mut observed = 0_u32;
    let mut wrote = 0_u32;
    let mut engaged = 0_u32;

    for outcome in outcomes {
        let Some(class) = outcome.class() else {
            continue;
        };
        observed += 1;
        if class.wrote() {
            wrote += 1;
        }
        if class == CourtesyClass::Engaged {
            engaged += 1;
        }
    }

    let score = (f64::from(wrote) + COURTESY_PRIOR_STRENGTH * COURTESY_PRIOR)
        / (f64::from(observed) + COURTESY_PRIOR_STRENGTH);

    let class = if observed == 0 {
        None
    } else {
        let engaged_share = f64::from(engaged) / f64::from(observed);
        let writer_share = f64::from(wrote) / f64::from(observed);
        Some(if engaged_share >= ENGAGED_CLASS_SHARE {
            CourtesyClass::Engaged
        } else if writer_share >= WRITER_CLASS_SHARE {
            CourtesyClass::Greeter
        } else {
            CourtesyClass::Silent
        })
    };

    CourtesySummary {
        observed,
        wrote,
        engaged,
        score: crate::scoring::round_score(score),
        class,
    }
}

/// Mindestabstand zwischen zwei Erinnerungs-Whispers an denselben Streamer.
///
/// Wer ohnehin nicht schreibt, liest die Erinnerung vermutlich auch nicht.
/// Eine mehr oder weniger ändert daran nichts, aber Dauerfeuer nach jedem
/// einzelnen Raid wäre Spam. Einmal pro Woche ist die Erinnerung präsent,
/// ohne lästig zu werden.
pub const WHISPER_COOLDOWN_DAYS: i64 = 7;

/// Entscheidet, ob nach diesem Raid eine Erinnerung rausgeht.
///
/// Drei Bedingungen, alle müssen zutreffen:
///
/// 1. **Dieser Raid war `Silent`.** Wer geschrieben hat, bekommt nie eine.
/// 2. **Der Streamer ist auch sonst kein Schreiber.** Ein `Greeter` oder
///    `Engaged`, der einmal vergisst oder gerade keine Lust hat, wird in Ruhe
///    gelassen. Ohne Historie (`None`) geht die erste Erinnerung raus, das ist
///    der freundliche Hinweis für Neue.
/// 3. **Der Cooldown ist abgelaufen** ([`WHISPER_COOLDOWN_DAYS`]).
///
/// `Unknown` erfüllt Bedingung 1 nie und löst daher nie eine Erinnerung aus.
pub fn should_remind(
    outcome: CourtesyOutcome,
    history_class: Option<CourtesyClass>,
    days_since_last_whisper: Option<i64>,
) -> bool {
    if outcome != CourtesyOutcome::Classified(CourtesyClass::Silent) {
        return false;
    }
    if matches!(
        history_class,
        Some(CourtesyClass::Greeter) | Some(CourtesyClass::Engaged)
    ) {
        return false;
    }
    match days_since_last_whisper {
        Some(days) => days >= WHISPER_COOLDOWN_DAYS,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minutes(count: u64) -> Duration {
        Duration::from_secs(count * 60)
    }

    // ─── classify ────────────────────────────────────────────────────────────

    #[test]
    fn keine_nachricht_ist_silent() {
        assert_eq!(classify(0, Duration::ZERO), CourtesyClass::Silent);
        assert_eq!(classify(0, minutes(30)), CourtesyClass::Silent);
    }

    #[test]
    fn ein_bis_zwei_kurze_nachrichten_sind_greeter() {
        assert_eq!(classify(1, Duration::ZERO), CourtesyClass::Greeter);
        assert_eq!(classify(2, minutes(1)), CourtesyClass::Greeter);
    }

    #[test]
    fn ab_drei_nachrichten_ist_engaged() {
        assert_eq!(classify(3, Duration::ZERO), CourtesyClass::Engaged);
        assert_eq!(classify(20, minutes(15)), CourtesyClass::Engaged);
    }

    #[test]
    fn hallo_und_spaeter_tschuess_ist_engaged() {
        // Zwei Nachrichten, aber über ein echtes Zeitfenster verteilt: der
        // Raider war da und ist nicht sofort weitergezogen.
        assert_eq!(classify(2, minutes(3)), CourtesyClass::Engaged);
        assert_eq!(classify(2, minutes(12)), CourtesyClass::Engaged);
    }

    #[test]
    fn eine_einzelne_nachricht_wird_nie_engaged() {
        // Ohne zweite Nachricht gibt es keine Spanne, die zählen könnte.
        assert_eq!(classify(1, minutes(30)), CourtesyClass::Greeter);
    }

    // ─── Klassen-Werte ───────────────────────────────────────────────────────

    #[test]
    fn greeter_und_engaged_sind_im_score_gleichwertig() {
        assert_eq!(
            CourtesyClass::Greeter.value(),
            CourtesyClass::Engaged.value()
        );
        assert_eq!(CourtesyClass::Silent.value(), 0.0);
    }

    #[test]
    fn klassen_bezeichner_sind_rundreisefest() {
        for class in [
            CourtesyClass::Engaged,
            CourtesyClass::Greeter,
            CourtesyClass::Silent,
        ] {
            assert_eq!(CourtesyClass::from_db(class.as_str()), Some(class));
        }
        assert_eq!(CourtesyClass::from_db("unknown"), None);
        assert_eq!(
            CourtesyOutcome::from_db("unknown"),
            CourtesyOutcome::Unknown
        );
        assert_eq!(
            CourtesyOutcome::from_db("quatsch"),
            CourtesyOutcome::Unknown
        );
    }

    // ─── summarize ───────────────────────────────────────────────────────────

    fn classified(class: CourtesyClass, count: usize) -> Vec<CourtesyOutcome> {
        vec![CourtesyOutcome::Classified(class); count]
    }

    #[test]
    fn ohne_beobachtungen_gibt_es_keinen_malus() {
        let summary = summarize(&[]);
        assert_eq!(summary.score, COURTESY_PRIOR);
        assert_eq!(summary.class, None);
        assert_eq!(summary.observed, 0);
    }

    #[test]
    fn unknown_zaehlt_nirgends_mit() {
        let only_unknown = vec![CourtesyOutcome::Unknown; 10];
        let summary = summarize(&only_unknown);
        assert_eq!(summary.observed, 0);
        assert_eq!(summary.score, COURTESY_PRIOR);
        assert_eq!(summary.class, None);

        // Unknown darf ein sauberes Ergebnis auch nicht verwässern.
        let mut mixed = classified(CourtesyClass::Engaged, 4);
        mixed.extend(vec![CourtesyOutcome::Unknown; 6]);
        assert_eq!(
            summarize(&mixed).score,
            summarize(&classified(CourtesyClass::Engaged, 4)).score
        );
    }

    #[test]
    fn nur_dauerschweiger_verlieren_deutlich() {
        let summary = summarize(&classified(CourtesyClass::Silent, 10));
        assert_eq!(summary.class, Some(CourtesyClass::Silent));
        // (0 + 3*1.0) / (10 + 3) = 0.230769
        assert!(summary.score < 0.25, "{}", summary.score);
    }

    #[test]
    fn wer_immer_schreibt_verliert_gar_nichts() {
        // Die Kernregel: der Courtesy-Anteil ist reiner Malus für Schweigen.
        // Wer durchgängig schreibt, steht exakt auf dem vollen Wert.
        for class in [CourtesyClass::Engaged, CourtesyClass::Greeter] {
            let summary = summarize(&classified(class, 10));
            assert_eq!(
                summary.score,
                COURTESY_PRIOR,
                "{} darf keinen Rest-Malus tragen",
                class.as_str()
            );
        }
    }

    #[test]
    fn ein_einzelner_aussetzer_kostet_kaum() {
        let mut outcomes = classified(CourtesyClass::Engaged, 9);
        outcomes.push(CourtesyOutcome::Classified(CourtesyClass::Silent));
        let summary = summarize(&outcomes);
        // Klasse bleibt Engaged, Score fällt nur von 1.0 auf 0.923.
        assert_eq!(summary.class, Some(CourtesyClass::Engaged));
        assert!(summary.score > 0.9, "{}", summary.score);
    }

    #[test]
    fn sechzig_prozent_schreiber_gelten_weiter_als_schreiber() {
        // Der Nutzerfall: schreibt in 6 von 10 Fällen, mal nicht — bleibt Schreiber.
        let mut outcomes = classified(CourtesyClass::Greeter, 6);
        outcomes.extend(classified(CourtesyClass::Silent, 4));
        let summary = summarize(&outcomes);
        assert_eq!(summary.class, Some(CourtesyClass::Greeter));
        assert!(summary.score > 0.65, "{}", summary.score);
    }

    #[test]
    fn reine_gruesser_stehen_im_score_exakt_wie_vielschreiber() {
        let greeter = summarize(&classified(CourtesyClass::Greeter, 10));
        let engaged = summarize(&classified(CourtesyClass::Engaged, 10));
        // Unterschiedliche Matching-Klasse, identischer Score.
        assert_eq!(greeter.class, Some(CourtesyClass::Greeter));
        assert_eq!(engaged.class, Some(CourtesyClass::Engaged));
        assert_eq!(
            greeter.score, engaged.score,
            "Grüßer dürfen im Score nicht schlechter stehen als Vielschreiber"
        );
    }

    #[test]
    fn gemischte_schreiber_verlieren_nichts_egal_wie_sie_schreiben() {
        // Mal kurz, mal ausführlich, aber immer etwas: kein Abzug.
        let mut outcomes = classified(CourtesyClass::Greeter, 5);
        outcomes.extend(classified(CourtesyClass::Engaged, 5));
        assert_eq!(summarize(&outcomes).score, COURTESY_PRIOR);
    }

    #[test]
    fn haelfte_engaged_reicht_fuer_die_engaged_klasse() {
        let mut outcomes = classified(CourtesyClass::Engaged, 5);
        outcomes.extend(classified(CourtesyClass::Greeter, 5));
        assert_eq!(summarize(&outcomes).class, Some(CourtesyClass::Engaged));
    }

    #[test]
    fn seltene_schreiber_gelten_als_silent() {
        // 2 von 10 liegt unter WRITER_CLASS_SHARE.
        let mut outcomes = classified(CourtesyClass::Greeter, 2);
        outcomes.extend(classified(CourtesyClass::Silent, 8));
        let summary = summarize(&outcomes);
        assert_eq!(summary.class, Some(CourtesyClass::Silent));
        assert_eq!(summary.wrote, 2);
        assert_eq!(summary.observed, 10);
    }

    // ─── should_remind ───────────────────────────────────────────────────────

    #[test]
    fn wer_geschrieben_hat_bekommt_nie_eine_erinnerung() {
        for class in [CourtesyClass::Engaged, CourtesyClass::Greeter] {
            assert!(!should_remind(
                CourtesyOutcome::Classified(class),
                Some(CourtesyClass::Silent),
                Some(999)
            ));
        }
    }

    #[test]
    fn ein_aussetzer_eines_schreibers_bleibt_ohne_erinnerung() {
        // Der Nutzerfall: ein Grüßer vergisst es einmal. Das ist egal.
        assert!(!should_remind(
            CourtesyOutcome::Classified(CourtesyClass::Silent),
            Some(CourtesyClass::Greeter),
            Some(999)
        ));
        assert!(!should_remind(
            CourtesyOutcome::Classified(CourtesyClass::Silent),
            Some(CourtesyClass::Engaged),
            None
        ));
    }

    #[test]
    fn dauerschweiger_bekommen_gedrosselt_eine_erinnerung() {
        let silent = CourtesyOutcome::Classified(CourtesyClass::Silent);
        // Noch nie eine bekommen → ja.
        assert!(should_remind(silent, Some(CourtesyClass::Silent), None));
        // Cooldown abgelaufen → ja.
        assert!(should_remind(
            silent,
            Some(CourtesyClass::Silent),
            Some(WHISPER_COOLDOWN_DAYS)
        ));
        // Cooldown läuft noch → nein, kein Dauerfeuer.
        assert!(!should_remind(
            silent,
            Some(CourtesyClass::Silent),
            Some(WHISPER_COOLDOWN_DAYS - 1)
        ));
        assert!(!should_remind(silent, Some(CourtesyClass::Silent), Some(0)));
    }

    #[test]
    fn erster_stiller_raid_ohne_historie_bekommt_den_hinweis() {
        assert!(should_remind(
            CourtesyOutcome::Classified(CourtesyClass::Silent),
            None,
            None
        ));
    }

    #[test]
    fn unknown_loest_nie_eine_erinnerung_aus() {
        // Sonst würde ein Bot-Neustart Vorwürfe verschicken.
        assert!(!should_remind(CourtesyOutcome::Unknown, None, None));
        assert!(!should_remind(
            CourtesyOutcome::Unknown,
            Some(CourtesyClass::Silent),
            Some(999)
        ));
    }
}
