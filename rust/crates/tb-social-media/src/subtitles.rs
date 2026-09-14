use serde_json::Value;

use crate::correction::correct_transcript;
use crate::vocab::VocabEntry;

pub const MAX_LINE_CHARS: usize = 42;
pub const MAX_LINES: usize = 2;
pub const MIN_CUE_SECS: f64 = 1.0;
pub const MAX_CUE_SECS: f64 = 4.0;

const GOLD_PRIMARY: &str = "&H0021B6E6";
const BOX_BACK: &str = "&H96000000";

#[derive(Debug, Clone, PartialEq)]
pub struct SubtitleSegment {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

impl SubtitleSegment {
    pub fn from_json(value: &Value) -> Option<Self> {
        let start = value.get("start_seconds").and_then(Value::as_f64)?;
        let end = value.get("end_seconds").and_then(Value::as_f64)?;
        let text = value.get("text").and_then(Value::as_str)?.to_string();
        Some(Self { start, end, text })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub lines: Vec<String>,
}

fn wrap_words(words: &[String]) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in words {
        if current.is_empty() {
            current = word.clone();
        } else if current.chars().count() + 1 + word.chars().count() <= MAX_LINE_CHARS {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(std::mem::take(&mut current));
            current = word.clone();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Schneidet die STT-Segmente in Untertitel-Cues: hoechstens zwei Zeilen zu je
/// rund 42 Zeichen, jede Cue zwischen einer und vier Sekunden.
pub fn segment_subtitles(segments: &[SubtitleSegment]) -> Vec<Cue> {
    let mut cues: Vec<Cue> = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    let mut cur_start = 0.0;
    let mut cur_end = 0.0;

    for seg in segments {
        let text = seg.text.trim();
        if text.is_empty() {
            continue;
        }
        let words: Vec<&str> = text.split_whitespace().collect();
        let n = words.len().max(1);
        let dur = (seg.end - seg.start).max(0.0);
        for (i, word) in words.iter().enumerate() {
            let w_start = seg.start + dur * (i as f64) / (n as f64);
            let w_end = seg.start + dur * ((i + 1) as f64) / (n as f64);

            let mut trial = cur.clone();
            trial.push((*word).to_string());
            let overflow_lines = wrap_words(&trial).len() > MAX_LINES;
            let overflow_time = !cur.is_empty() && (w_end - cur_start) > MAX_CUE_SECS;

            if !cur.is_empty() && (overflow_lines || overflow_time) {
                cues.push(Cue {
                    start: cur_start,
                    end: cur_end,
                    lines: wrap_words(&cur),
                });
                cur.clear();
            }
            if cur.is_empty() {
                cur_start = w_start;
            }
            cur.push((*word).to_string());
            cur_end = w_end;
        }
    }
    if !cur.is_empty() {
        cues.push(Cue {
            start: cur_start,
            end: cur_end,
            lines: wrap_words(&cur),
        });
    }

    for i in 0..cues.len() {
        if cues[i].end - cues[i].start < MIN_CUE_SECS {
            let desired = cues[i].start + MIN_CUE_SECS;
            let cap = cues.get(i + 1).map(|next| next.start).unwrap_or(desired);
            cues[i].end = desired.min(cap).max(cues[i].start);
        }
    }
    cues
}

/// Wendet die Vokabel-Korrektur auf jeden Segmenttext an, bevor geschnitten wird.
pub fn correct_segments(segments: &[SubtitleSegment], vocab: &[VocabEntry]) -> Vec<SubtitleSegment> {
    segments
        .iter()
        .map(|seg| {
            let corrected = if seg.text.trim().is_empty() {
                seg.text.clone()
            } else {
                correct_transcript(&seg.text, vocab).corrected
            };
            SubtitleSegment {
                start: seg.start,
                end: seg.end,
                text: corrected,
            }
        })
        .collect()
}

fn ass_time(seconds: f64) -> String {
    let total = seconds.max(0.0);
    let centis = (total * 100.0).round() as i64;
    let cs = centis % 100;
    let total_secs = centis / 100;
    let s = total_secs % 60;
    let m = (total_secs / 60) % 60;
    let h = total_secs / 3600;
    format!("{h}:{m:02}:{s:02}.{cs:02}")
}

fn ass_escape(line: &str) -> String {
    line.replace('\\', "\u{2216}").replace('{', "(").replace('}', ")")
}

/// Baut eine ASS-Datei: Gold-Text auf dunklem Balken, unten mittig.
pub fn build_ass(cues: &[Cue]) -> String {
    let mut out = String::new();
    out.push_str("[Script Info]\n");
    out.push_str("ScriptType: v4.00+\n");
    out.push_str("PlayResX: 1080\n");
    out.push_str("PlayResY: 1920\n");
    out.push_str("WrapStyle: 0\n\n");
    out.push_str("[V4+ Styles]\n");
    out.push_str(
        "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n",
    );
    out.push_str(&format!(
        "Style: Default,Arial,54,{gold},&H000000FF,&H00101010,{box},-1,0,0,0,100,100,0,0,3,6,0,2,60,60,140,1\n\n",
        gold = GOLD_PRIMARY,
        box = BOX_BACK,
    ));
    out.push_str("[Events]\n");
    out.push_str("Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n");
    for cue in cues {
        let text = cue
            .lines
            .iter()
            .map(|l| ass_escape(l))
            .collect::<Vec<_>>()
            .join("\\N");
        out.push_str(&format!(
            "Dialogue: 0,{start},{end},Default,,0,0,0,,{text}\n",
            start = ass_time(cue.start),
            end = ass_time(cue.end),
        ));
    }
    out
}

/// Baut die fertige ASS-Datei aus rohen STT-Segmenten (JSONB) mit Korrektur.
pub fn ass_from_segments(segments: &[Value], vocab: &[VocabEntry]) -> Option<String> {
    let parsed: Vec<SubtitleSegment> = segments.iter().filter_map(SubtitleSegment::from_json).collect();
    if parsed.is_empty() {
        return None;
    }
    let corrected = correct_segments(&parsed, vocab);
    let cues = segment_subtitles(&corrected);
    if cues.is_empty() {
        return None;
    }
    Some(build_ass(&cues))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start: f64, end: f64, text: &str) -> SubtitleSegment {
        SubtitleSegment {
            start,
            end,
            text: text.to_string(),
        }
    }

    #[test]
    fn segmentiert_nach_zeilen_und_dauer() {
        let segments = vec![seg(
            0.0,
            8.0,
            "ein zwei drei vier fuenf sechs sieben acht neun zehn elf zwoelf dreizehn vierzehn fuenfzehn sechzehn siebzehn achtzehn",
        )];
        let cues = segment_subtitles(&segments);
        assert!(cues.len() >= 2, "langer Abschnitt wird in mehrere Cues geschnitten: {cues:?}");
        for c in &cues {
            assert!(c.lines.len() <= MAX_LINES, "hoechstens zwei Zeilen: {c:?}");
            for line in &c.lines {
                assert!(line.chars().count() <= MAX_LINE_CHARS, "Zeile zu lang: {line}");
            }
            assert!(c.end > c.start, "Cue hat Dauer: {c:?}");
            assert!(c.end - c.start <= MAX_CUE_SECS + 1e-6, "Cue nicht laenger als 4s: {c:?}");
        }
        // Cues laufen zeitlich vorwaerts.
        for pair in cues.windows(2) {
            assert!(pair[1].start >= pair[0].start, "Cues in Reihenfolge: {cues:?}");
        }
    }

    #[test]
    fn kurze_cue_wird_auf_mindestdauer_gezogen() {
        let cues = segment_subtitles(&[seg(0.0, 0.2, "kurz")]);
        assert_eq!(cues.len(), 1);
        assert!(cues[0].end - cues[0].start >= MIN_CUE_SECS - 1e-6, "{cues:?}");
    }

    #[test]
    fn ass_hat_gold_stil_und_dunklen_balken() {
        let cues = segment_subtitles(&[seg(0.0, 2.0, "haze ist stark")]);
        let ass = build_ass(&cues);
        assert!(ass.contains("[Script Info]"), "{ass}");
        assert!(ass.contains("[V4+ Styles]"));
        assert!(ass.contains(GOLD_PRIMARY), "Gold-Textfarbe fehlt: {ass}");
        // BorderStyle 3 = deckender Kasten (der dunkle Balken).
        assert!(ass.contains(",3,6,0,2,"), "Balken-BorderStyle fehlt: {ass}");
        assert!(ass.contains(BOX_BACK), "dunkle Balkenfarbe fehlt: {ass}");
        assert!(ass.contains("Dialogue: 0,0:00:00.00,0:00:02.00,Default"), "{ass}");
    }

    #[test]
    fn ass_from_segments_wendet_korrektur_an() {
        let vocab = vec![VocabEntry {
            term: "haze".into(),
            canonical: "Haze".into(),
            category: "hero".into(),
            source: "manual".into(),
            aliases: vec![],
            weight: 1,
            updated_at: None,
        }];
        let segments = vec![serde_json::json!({"start_seconds":0.0,"end_seconds":2.0,"text":"haze ist stark"})];
        let ass = ass_from_segments(&segments, &vocab).unwrap();
        assert!(ass.contains("Haze ist stark"), "Vokabel-Korrektur greift: {ass}");
    }
}
