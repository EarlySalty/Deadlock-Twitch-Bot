//! Reine Kennzahlen-Mathematik des Session-Abschlusses — kein I/O.
//! Semantik 1:1 zum Python-Finalize (`_finalize_stream_session`), inklusive
//! Baseline-Wahl, Fallbacks und Clamping.

/// Ein Viewer-Sample aus `twitch_session_viewers`, chronologisch sortiert.
#[derive(Debug, Clone, Copy)]
pub struct ViewerSample {
    pub minutes_from_start: i32,
    pub viewer_count: i32,
}

/// Retention zur Minute `minutes`: Zuschauer am/nach dem Zeitpunkt geteilt
/// durch den Peak davor (Baseline). Auf 0..=1 geklemmt; `None` ohne Daten.
pub fn retention_at(samples: &[ViewerSample], minutes: i32, start_viewers: i32) -> Option<f64> {
    if samples.is_empty() {
        return None;
    }
    let mut peak_before = start_viewers;
    for s in samples {
        if s.minutes_from_start < minutes {
            peak_before = peak_before.max(s.viewer_count);
        }
    }
    if peak_before <= 0 {
        peak_before = samples[0].viewer_count;
    }
    if peak_before <= 0 {
        return None;
    }
    let mut best: Option<&ViewerSample> = None;
    for s in samples {
        if s.minutes_from_start < minutes {
            continue;
        }
        if best.is_none_or(|b| s.minutes_from_start < b.minutes_from_start) {
            best = Some(s);
        }
    }
    // Stream endete vor der Ziel-Minute → letzter Datenpunkt.
    let best = best.unwrap_or_else(|| samples.last().expect("samples nicht leer"));
    let raw = f64::from(best.viewer_count) / f64::from(peak_before);
    if raw > 1.0 {
        tracing::warn!(
            minutes,
            current = best.viewer_count,
            baseline = peak_before,
            raw,
            "Retention über 100% — auf 1.0 geklemmt"
        );
    }
    Some(raw.clamp(0.0, 1.0))
}

/// Größter prozentualer Einbruch zwischen zwei aufeinanderfolgenden Samples.
#[derive(Debug, Clone, PartialEq)]
pub struct Dropoff {
    pub pct: f64,
    /// Menschenlesbares Label, z. B. `t=42m (120->80)`.
    pub label: String,
}

pub fn max_dropoff(samples: &[ViewerSample], start_viewers: i32) -> Option<Dropoff> {
    let mut prev = if start_viewers > 0 {
        start_viewers
    } else {
        samples.first().map_or(0, |s| s.viewer_count)
    };
    let mut out: Option<Dropoff> = None;
    for s in samples {
        if prev > 0 && s.viewer_count < prev {
            let pct = f64::from(prev - s.viewer_count) / f64::from(prev);
            if out.as_ref().is_none_or(|d| pct > d.pct) {
                out = Some(Dropoff {
                    pct,
                    label: format!("t={}m ({}->{})", s.minutes_from_start, prev, s.viewer_count),
                });
            }
        }
        prev = s.viewer_count;
    }
    out
}

/// Aggregat-Felder einer Session.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aggregates {
    pub end_viewers: i32,
    pub peak_viewers: i32,
    pub avg_viewers: f64,
    pub samples: i32,
}

/// Rechnet die Aggregate beim Finalize aus den Viewer-Samples neu; ohne
/// Samples bleiben die Werte aus der Session-Row. (Python-Eigenheit erhalten:
/// ein letzter Sample-Wert von 0 fällt auf die Session-`end_viewers` zurück.)
pub fn final_aggregates(samples: &[ViewerSample], from_session: Aggregates) -> Aggregates {
    if samples.is_empty() {
        return from_session;
    }
    let last = samples.last().expect("samples nicht leer").viewer_count;
    let end_viewers = if last != 0 {
        last
    } else {
        from_session.end_viewers
    };
    let sample_peak = samples.iter().map(|s| s.viewer_count).max().unwrap_or(0);
    let avg = samples
        .iter()
        .map(|s| f64::from(s.viewer_count))
        .sum::<f64>()
        / samples.len() as f64;
    Aggregates {
        end_viewers,
        peak_viewers: from_session.peak_viewers.max(sample_peak),
        avg_viewers: avg,
        samples: from_session.samples.max(samples.len() as i32),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(m: i32, v: i32) -> ViewerSample {
        ViewerSample {
            minutes_from_start: m,
            viewer_count: v,
        }
    }

    #[test]
    fn retention_nutzt_peak_vor_zielminute_als_baseline() {
        let samples = [s(1, 10), s(3, 40), s(6, 20)];
        // Baseline = max(start=5, peak<5m = 40) = 40; Wert bei >=5m = 20.
        assert_eq!(retention_at(&samples, 5, 5), Some(0.5));
    }

    #[test]
    fn retention_faellt_auf_letzten_punkt_zurueck() {
        let samples = [s(1, 10), s(2, 8)];
        // Kein Sample >= 20m → letzter Punkt 8; Baseline = max(10, 10) = 10.
        assert_eq!(retention_at(&samples, 20, 10), Some(0.8));
    }

    #[test]
    fn retention_clamps_und_none_ohne_daten() {
        assert_eq!(retention_at(&[], 5, 10), None);
        // Baseline 0 → Fallback auf ersten Sample; ist auch der 0 → None.
        assert_eq!(retention_at(&[s(6, 0)], 5, 0), None);
        assert_eq!(retention_at(&[s(6, 5)], 5, 0), Some(1.0));
        // Wachstum über Baseline → 1.0 geklemmt.
        assert_eq!(retention_at(&[s(1, 2), s(6, 10)], 5, 2), Some(1.0));
    }

    #[test]
    fn dropoff_findet_groessten_einbruch() {
        let samples = [s(1, 100), s(2, 90), s(3, 30), s(4, 60)];
        let d = max_dropoff(&samples, 100).expect("dropoff");
        assert_eq!(d.label, "t=3m (90->30)");
        assert!((d.pct - (60.0 / 90.0)).abs() < 1e-9);
        assert!(max_dropoff(&[s(1, 5), s(2, 7)], 5).is_none());
    }

    #[test]
    fn aggregates_aus_samples_mit_null_fallback() {
        let from_session = Aggregates {
            end_viewers: 7,
            peak_viewers: 50,
            avg_viewers: 99.0,
            samples: 2,
        };
        let agg = final_aggregates(&[s(1, 10), s(2, 30), s(3, 0)], from_session);
        // Letzter Wert 0 → Fallback auf Session-end_viewers (Python-`or`).
        assert_eq!(agg.end_viewers, 7);
        assert_eq!(agg.peak_viewers, 50);
        assert_eq!(agg.samples, 3);
        assert!((agg.avg_viewers - (40.0 / 3.0)).abs() < 1e-9);
        assert_eq!(final_aggregates(&[], from_session), from_session);
    }
}
