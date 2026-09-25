//! Lokale, billige Spracherkennung für den Chat des Kategoriesammlers.
//!
//! whatlang läuft im Prozess (kein Cloud-Dienst, kein selbst gebastelter
//! Heuristikpfad). Erkennung gibt es nur für Text mit genug Buchstaben;
//! kurze, eindeutige und Emote-only-Nachrichten bekommen bewusst `lang =
//! NULL` und einen transparenten `method`-Marker, damit Aggregate nur auf
//! tragfähigen Erkennungen beruhen. Stream-Sprache und erkannte
//! Nachrichtensprache sind getrennte Dimensionen; aus einer Chat-Sprache
//! wird niemals Geografie abgeleitet.

/// Ergebnis einer Nachrichtensprach-Prüfung.
#[derive(Debug, Clone, PartialEq)]
pub struct Erkennung {
    /// ISO-Sprachcode, einheitlich zur Stream-Sprache (2-stellig, soweit
    /// eine stabile Entsprechung existiert; sonst whatlangs 3-stelliger Code).
    pub lang: Option<String>,
    /// whatlang-Konfidenz (0.0–1.0), `None` ohne Erkennungsversuch.
    pub confidence: Option<f32>,
    /// Transparenz-Marker: `ok`, `too_short`, `no_letters`, `emote_only`,
    /// `no_detection`, `low_confidence`, `unknown`.
    pub method: &'static str,
}

/// Mindestanzahl Zeichen, bevor ein Text erkenntbar ist (Chat-Kürzel wie
/// "lol", "+2" oder "?" tragen keine belastbare Sprachsignale).
const MIN_ZEICHEN: usize = 8;
/// Mindestanzahl Buchstaben, sonst ist der Text Emote-/Zahlen-lastig.
const MIN_BUCHSTABEN: usize = 4;
/// Unterhalb dieser whatlang-Konfidenz wird die Erkennung nicht verwendet.
const MIN_KONFIDENZ: f32 = 0.6;

/// Erkennt die Nachrichtensprache eines Chattexts.
pub fn erkenne(text: &str, emotes_tag: Option<&str>) -> Erkennung {
    let zeichen = text.chars().count();
    if zeichen < MIN_ZEICHEN {
        return Erkennung {
            lang: None,
            confidence: None,
            method: "too_short",
        };
    }
    // Emote-only: alle Nicht-Leerzeichen sind von Emote-Bereichen abgedeckt.
    if emotes_ohne_rest(text, emotes_tag) {
        return Erkennung {
            lang: None,
            confidence: None,
            method: "emote_only",
        };
    }
    let buchstaben = text.chars().filter(|c| c.is_alphabetic()).count();
    if buchstaben < MIN_BUCHSTABEN {
        return Erkennung {
            lang: None,
            confidence: None,
            method: "no_letters",
        };
    }
    let Some(output) = whatlang::detect(text) else {
        return Erkennung {
            lang: None,
            confidence: None,
            method: "no_detection",
        };
    };
    let confidence = output.confidence() as f32;
    if confidence < MIN_KONFIDENZ {
        return Erkennung {
            lang: None,
            confidence: Some(confidence),
            method: "low_confidence",
        };
    }
    Erkennung {
        lang: Some(normalisiere_code(output.lang().code())),
        confidence: Some(confidence),
        method: "ok",
    }
}

///whatlangs 3-stelligen ISO-639-3-Code auf den 2-stelligen Stream-Sprachcode
/// abbilden; ohne Entsprechung bleibt der 3-stellige Code (dokumentiert,
/// einheitlich statt gemischt zu raten).
fn normalisiere_code(code: &str) -> String {
    match code {
        "eng" => "en",
        "deu" => "de",
        "fra" => "fr",
        "spa" => "es",
        "ita" => "it",
        "por" => "pt",
        "rus" => "ru",
        "pol" => "pl",
        "tur" => "tr",
        "nld" => "nl",
        "ces" => "cs",
        "slk" => "sk",
        "ell" => "el",
        "swe" => "sv",
        "dan" => "da",
        "nob" | "nno" => "no",
        "fin" => "fi",
        "hun" => "hu",
        "ron" => "ro",
        "bul" => "bg",
        "ukr" => "uk",
        "ara" => "ar",
        "heb" => "he",
        "hin" => "hi",
        "tha" => "th",
        "vie" => "vi",
        "ind" => "id",
        "zho" => "zh",
        "jpn" => "ja",
        "kor" => "ko",
        other => other,
    }
    .to_string()
}

/// Deckt der `emotes`-Tag (IRCv3: `id:start-end/…`) alle Zeichen außer
/// Whitespace ab? Dann ist die Nachricht Emote-only.
fn emotes_ohne_rest(text: &str, emotes_tag: Option<&str>) -> bool {
    let Some(tag) = emotes_tag else {
        return false;
    };
    let belegt: Vec<(usize, usize)> = tag
        .split('/')
        .filter_map(|emote| emote.split_once(':'))
        .filter(|(id, _)| !id.trim().is_empty())
        .flat_map(|(_, bereiche)| bereiche.split(','))
        .filter_map(|bereich| bereich.split_once('-'))
        .filter_map(|(start, ende)| {
            let start: usize = start.trim().parse().ok()?;
            let ende: usize = ende.trim().parse().ok()?;
            (ende >= start).then_some((start, ende + 1))
        })
        .collect();
    if belegt.is_empty() {
        return false;
    }
    text.char_indices().all(|(index, c)| {
        c.is_whitespace()
            || belegt
                .iter()
                .any(|(start, ende)| index >= *start && index < *ende)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kurze_nachrichten_bekommen_keine_sprache() {
        let e = erkenne("lol", None);
        assert_eq!(e.lang, None);
        assert_eq!(e.method, "too_short");

        let e = erkenne("+2 :)", None);
        assert_eq!(e.lang, None);
        assert_eq!(e.method, "too_short");
    }

    #[test]
    fn emote_only_bleibt_unerkannt_mit_transparentem_marker() {
        // Emotes-Tag deckt Zeichen 0-3 (KEKW) und 6-9 (LUL) ab, Rest Whitespace.
        let e = erkenne("KEKW  LUL", Some("1234:0-3/567:6-9"));
        assert_eq!(e.lang, None);
        assert_eq!(e.method, "emote_only");
    }

    #[test]
    fn klare_deutsche_nachricht_wird_erkannt() {
        let e = erkenne("Das war echt ein überragender Kampf heuteabend", None);
        assert_eq!(e.lang.as_deref(), Some("de"));
        assert_eq!(e.method, "ok");
        assert!(e.confidence.unwrap_or(0.0) > 0.5);
    }

    #[test]
    fn russische_nachricht_wird_als_ru_erkannt() {
        let e = erkenne("Сегодня мы обсуждаем новую стратегию игры. Мне особенно понравилось, как команда защищала свою базу и помогала товарищам во время последнего сражения.", None);
        assert_eq!(e.lang.as_deref(), Some("ru"));
    }

    #[test]
    fn kurze_mehrdeutige_kyrillische_nachricht_wird_nicht_erzwungen() {
        let e = erkenne("Привет всем, как дела сегодня на стриме", None);
        assert_eq!(e.lang, None);
        assert_eq!(e.method, "low_confidence");
    }

    #[test]
    fn englische_nachricht_wird_als_en_erkannt() {
        let e = erkenne("that fight was absolutely insane to watch today", None);
        assert_eq!(e.lang.as_deref(), Some("en"));
    }

    #[test]
    fn zahlen_und_symbole_ohne_buchstaben_bleiben_leer() {
        let e = erkenne("12345678 ++++ ----", None);
        assert_eq!(e.lang, None);
        assert_eq!(e.method, "no_letters");
    }
}
