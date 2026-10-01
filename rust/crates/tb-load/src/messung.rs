//! Reine `/proc`-Messung fuer das Last-Gate.
//!
//! Bewusst getrennt vom [`crate::last::Lastwaechter`]: der Waechter ist die
//! testbare Entscheidung, hier steht die I/O. Beide teilen sich Audit und Bot,
//! damit es keine zweite Messung mit anderer Formel gibt.

/// Summe der Ticks aus `/proc/stat` und der davon untaetige Anteil.
///
/// Die CPU-Auslastung ist keine Momentaufnahme, sondern der Anteil belegter
/// Ticks zwischen zwei Messungen - deshalb der Zwischenstand.
#[derive(Clone, Copy)]
pub struct CpuStand {
    gesamt: u64,
    untaetig: u64,
}

/// Liest die Sammelzeile `cpu` aus `/proc/stat`. `None`, wenn die Datei fehlt
/// oder unerwartet aussieht - dann faellt die CPU als Signal aus, RAM traegt
/// weiter.
pub fn cpu_stand() -> Option<CpuStand> {
    let inhalt = std::fs::read_to_string("/proc/stat").ok()?;
    let zeile = inhalt.lines().next()?;
    let mut felder = zeile.split_whitespace();
    if felder.next()? != "cpu" {
        return None;
    }
    // Genau die acht Standardfelder, der Reihe nach:
    // user nice system idle iowait irq softirq steal. Positionsgenau lesen -
    // ein `filter_map` wuerde ein unparsbares Feld ueberspringen und idle/iowait
    // von der falschen Stelle holen. `guest`/`guest_nice` bleiben aussen vor:
    // der Kernel fuehrt sie bereits in user/nice, mitzusummieren zaehlte sie
    // doppelt.
    let mut werte = [0u64; 8];
    for feld in werte.iter_mut() {
        *feld = felder.next()?.parse().ok()?;
    }
    let gesamt: u64 = werte.iter().sum();
    // idle + iowait gelten als untaetig.
    let untaetig = werte[3] + werte[4];
    Some(CpuStand { gesamt, untaetig })
}

/// CPU-Auslastung in Prozent zwischen zwei Messungen. `None`, wenn die Uhr
/// nicht weitergelaufen ist (gleiche Messung).
pub fn cpu_prozent(vorher: CpuStand, jetzt: CpuStand) -> Option<f32> {
    let gesamt = jetzt.gesamt.checked_sub(vorher.gesamt)?;
    let untaetig = jetzt.untaetig.saturating_sub(vorher.untaetig);
    if gesamt == 0 {
        return None;
    }
    let belegt = gesamt.saturating_sub(untaetig);
    Some(belegt as f32 / gesamt as f32 * 100.0)
}

/// RAM-Auslastung in Prozent aus `/proc/meminfo`. `MemAvailable` ist der frei
/// nutzbare Speicher inklusive rueckholbarem Cache - naeher an "voll" als das
/// blosse `MemFree`.
pub fn ram_prozent() -> Option<f32> {
    let inhalt = std::fs::read_to_string("/proc/meminfo").ok()?;
    let mut gesamt = None;
    let mut verfuegbar = None;
    for zeile in inhalt.lines() {
        if let Some(rest) = zeile.strip_prefix("MemTotal:") {
            gesamt = rest.split_whitespace().next()?.parse::<u64>().ok();
        } else if let Some(rest) = zeile.strip_prefix("MemAvailable:") {
            verfuegbar = rest.split_whitespace().next()?.parse::<u64>().ok();
        }
    }
    let gesamt = gesamt?;
    let verfuegbar = verfuegbar?;
    if gesamt == 0 {
        return None;
    }
    let belegt = gesamt.saturating_sub(verfuegbar);
    Some(belegt as f32 / gesamt as f32 * 100.0)
}
