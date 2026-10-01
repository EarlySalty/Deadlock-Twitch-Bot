use serde_json::Value;

use crate::correction::correct_transcript;
use crate::vocab::VocabEntry;

pub const MAX_LINE_CHARS: usize = 28;
pub const MAX_LINES: usize = 2;
pub const MIN_CUE_SECS: f64 = 0.8;
pub const MAX_CUE_SECS: f64 = 2.5;
pub const MAX_CUE_WORDS: usize = 5;

const GOLD_PRIMARY: &str = "&H0059A0C5";
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
            let overflow_words = trial.len() > MAX_CUE_WORDS;
            let overflow_time = !cur.is_empty() && (w_end - cur_start) > MAX_CUE_SECS;

            if !cur.is_empty() && (overflow_lines || overflow_words || overflow_time) {
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
pub fn correct_segments(
    segments: &[SubtitleSegment],
    vocab: &[VocabEntry],
) -> Vec<SubtitleSegment> {
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
    line.replace('\\', "\u{2216}")
        .replace('{', "(")
        .replace('}', ")")
        .replace(['\n', '\r'], " ")
}

fn hook_lines(title: &str) -> String {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in title.split_whitespace() {
        let shortened = (word.chars().count() > 18)
            .then(|| format!("{}…", word.chars().take(17).collect::<String>()));
        let word = shortened.as_deref().unwrap_or(word);
        if !current.is_empty() && current.chars().count() + 1 + word.chars().count() > 18 {
            if lines.len() == 2 {
                if current.chars().count() == 18 {
                    current.pop();
                }
                current.push('…');
                break;
            }
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
        .into_iter()
        .map(|line| ass_escape(&line))
        .collect::<Vec<_>>()
        .join("\\N")
}

pub fn build_branded_ass(
    cues: &[Cue],
    title: &str,
    login: &str,
    cam_height: i64,
    duration: f64,
) -> String {
    let mut out = String::new();
    out.push_str(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: 1080\nPlayResY: 1920\nWrapStyle: 2\n\n",
    );
    out.push_str("[V4+ Styles]\n");
    out.push_str("Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n");
    out.push_str(&format!(
        "Style: Default,DejaVu Sans,62,&H00FFFFFF,&H00FFFFFF,&H00000000,{box},-1,0,0,0,100,100,0,0,1,5,2,2,65,180,770,1\nStyle: Hook,DejaVu Sans,80,&H00FFFFFF,&H00FFFFFF,&H00000000,{box},-1,0,0,0,100,100,0,0,1,6,2,8,70,70,0,1\nStyle: Channel,DejaVu Sans,48,{gold},{gold},&H00000000,{box},-1,0,0,0,100,100,0,0,1,4,1,7,48,180,0,1\n\n",
        gold = GOLD_PRIMARY,
        box = BOX_BACK,
    ));
    out.push_str("[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n");
    let end = ass_time(duration.max(0.01));
    if !login.trim().is_empty() {
        out.push_str(&format!(
            "Dialogue: 1,0:00:00.00,{end},Channel,,0,0,0,,{{\\pos(48,1420)}}@{}\n",
            ass_escape(login.trim().trim_start_matches('@'))
        ));
    }
    if !title.trim().is_empty() {
        let hook_y = (cam_height + 32).clamp(550, 800);
        out.push_str(&format!(
            "Dialogue: 2,0:00:00.00,{},Hook,,0,0,0,,{{\\pos(540,{hook_y})}}{}\n",
            ass_time(duration.clamp(0.01, 2.8)),
            hook_lines(title),
        ));
    }
    for cue in cues {
        let text = cue
            .lines
            .iter()
            .map(|line| ass_escape(line))
            .collect::<Vec<_>>()
            .join("\\N");
        out.push_str(&format!(
            "Dialogue: 0,{},{},Default,,0,0,0,,{text}\n",
            ass_time(cue.start),
            ass_time(cue.end.min(duration).max(cue.start + 0.01)),
        ));
    }
    out
}

pub fn build_ass(cues: &[Cue]) -> String {
    build_branded_ass(cues, "", "", 600, 60.0)
}

/// Baut die fertige ASS-Datei aus rohen STT-Segmenten (JSONB) mit Korrektur.
pub fn ass_from_segments(segments: &[Value], vocab: &[VocabEntry]) -> Option<String> {
    let parsed: Vec<SubtitleSegment> = segments
        .iter()
        .filter_map(SubtitleSegment::from_json)
        .collect();
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
        assert!(
            cues.len() >= 2,
            "langer Abschnitt wird in mehrere Cues geschnitten: {cues:?}"
        );
        for c in &cues {
            assert!(c.lines.len() <= MAX_LINES, "hoechstens zwei Zeilen: {c:?}");
            for line in &c.lines {
                assert!(
                    line.chars().count() <= MAX_LINE_CHARS,
                    "Zeile zu lang: {line}"
                );
            }
            assert!(c.end > c.start, "Cue hat Dauer: {c:?}");
            assert!(
                c.end - c.start <= MAX_CUE_SECS + 1e-6,
                "Cue nicht laenger als 4s: {c:?}"
            );
        }
        // Cues laufen zeitlich vorwaerts.
        for pair in cues.windows(2) {
            assert!(
                pair[1].start >= pair[0].start,
                "Cues in Reihenfolge: {cues:?}"
            );
        }
    }

    #[test]
    fn kurze_cue_wird_auf_mindestdauer_gezogen() {
        let cues = segment_subtitles(&[seg(0.0, 0.2, "kurz")]);
        assert_eq!(cues.len(), 1);
        assert!(
            cues[0].end - cues[0].start >= MIN_CUE_SECS - 1e-6,
            "{cues:?}"
        );
    }

    #[test]
    fn ass_hat_lesbare_untertitel_und_getrennte_markenelemente() {
        let cues = segment_subtitles(&[seg(0.0, 2.0, "haze ist stark")]);
        let ass = build_branded_ass(&cues, "Wow {Haze} 😎", "earlysalty", 600, 10.0);
        assert!(ass.contains("Style: Default,DejaVu Sans,62,&H00FFFFFF"));
        assert!(ass.contains("Style: Channel,DejaVu Sans,48,"));
        assert!(ass.contains("Style: Hook,DejaVu Sans,80,&H00FFFFFF,&H00FFFFFF,&H00000000,"));
        assert!(ass.contains(",1,6,2,8,70,70,0,1"));
        assert!(ass.contains(GOLD_PRIMARY));
        assert!(ass.contains("Dialogue: 0,0:00:00.00,0:00:02.00,Default"));
        assert!(ass.contains("@earlysalty"));
        assert!(ass.contains("Wow (Haze) 😎"));
        assert!(ass.contains("0:00:02.80,Hook"));
        assert!(ass.contains(BOX_BACK));
    }

    #[test]
    fn langer_hook_bleibt_in_drei_lesbaren_zeilen() {
        let title =
            "Ein langer Clip Titel mit vielen verschiedenen Worten und einem überraschenden Ende";
        let lines = hook_lines(title);
        let parts: Vec<&str> = lines.split("\\N").collect();
        assert_eq!(parts.len(), 3);
        assert!(parts.iter().all(|line| line.chars().count() <= 18));
        assert!(parts[2].ends_with('…'));
    }

    #[test]
    fn einzelwort_hook_bleibt_auch_mit_umlauten_im_bild() {
        let lines = hook_lines("Überraschungsüberraschungsüberraschung");
        assert_eq!(lines.chars().count(), 18);
        assert!(lines.ends_with('…'));
        let lines = hook_lines(&"Ä".repeat(80));
        assert_eq!(lines, format!("{}…", "Ä".repeat(17)));
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
        let segments = vec![
            serde_json::json!({"start_seconds":0.0,"end_seconds":2.0,"text":"haze ist stark"}),
        ];
        let ass = ass_from_segments(&segments, &vocab).unwrap();
        assert!(
            ass.contains("Haze ist stark"),
            "Vokabel-Korrektur greift: {ass}"
        );
    }
}
