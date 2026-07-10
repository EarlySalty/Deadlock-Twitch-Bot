//! LFG-Mitspieler-Pitch: billiger Regex-Vorfilter vor dem KI-Judge.

use std::sync::OnceLock;

use regex::Regex;

fn direct_lfg_re() -> &'static Result<Regex, regex::Error> {
    static RE: OnceLock<Result<Regex, regex::Error>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\b(lfg|looking\s+for\s+group)\b"))
}

fn search_lfg_re() -> &'static Result<Regex, regex::Error> {
    static RE: OnceLock<Result<Regex, regex::Error>> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\b(such\w*|brauche?\w*|wer\s+hat\s+bock|noch\s+jemand|jemand)\b")
    })
}

fn object_lfg_re() -> &'static Result<Regex, regex::Error> {
    static RE: OnceLock<Result<Regex, regex::Error>> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(rank(?:ed)?|zock\w*|spiel\w*|duo|trio|stack|lobby|team|gruppe|group|mate\w*|mitspieler\w*)\b",
        )
    })
}

fn is_match(re: &Result<Regex, regex::Error>, raw: &str) -> bool {
    re.as_ref().is_ok_and(|re| re.is_match(raw))
}

pub fn classify_lfg(content: &str) -> bool {
    let raw = content.trim();
    if raw.is_empty() || raw.starts_with('!') {
        return false;
    }

    is_match(direct_lfg_re(), raw)
        || (is_match(search_lfg_re(), raw) && is_match(object_lfg_re(), raw))
}
