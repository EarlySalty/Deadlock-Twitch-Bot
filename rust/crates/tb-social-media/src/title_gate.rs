const GENERIC_TITLES: &[&str] = &[
    "clip",
    "clips",
    "!clip",
    "clipped",
    "clipit",
    "clip it",
    "highlight",
    "live",
    "stream",
    "vod",
    "twitch",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleDecision {
    UseExisting,
    GenerateFromMetadata,
    TranscribeThenGenerate,
}

pub fn is_generic_title(title: &str) -> bool {
    let title = title.trim();
    title.is_empty()
        || title.chars().all(char::is_numeric)
        || GENERIC_TITLES
            .iter()
            .any(|junk| title.eq_ignore_ascii_case(junk))
}

fn meaningful_word_count(title: &str) -> usize {
    title
        .split(|character: char| !character.is_alphabetic())
        .filter(|word| word.chars().count() >= 3)
        .count()
}

// ponytail: Aufrufer nutzt correction::correct_transcript aus correction.rs.
pub fn decide(
    existing_title: &str,
    title_has_vocab_hit: bool,
    has_transcript: bool,
    openai_available: bool,
) -> TitleDecision {
    let generic = is_generic_title(existing_title);
    let enough_words = meaningful_word_count(existing_title) >= 3;
    let good = title_has_vocab_hit && enough_words && !generic;

    let (verdict, reason) = if good {
        (TitleDecision::UseExisting, "gut")
    } else if !has_transcript && openai_available {
        let reason = if generic {
            "generisch"
        } else if !title_has_vocab_hit {
            "kein_vokab_treffer"
        } else {
            "zu_kurz"
        };
        (TitleDecision::TranscribeThenGenerate, reason)
    } else if !openai_available {
        (TitleDecision::GenerateFromMetadata, "openai_aus")
    } else {
        (TitleDecision::GenerateFromMetadata, "transkript_vorhanden")
    };

    let title = existing_title.trim().chars().take(80).collect::<String>();
    tracing::info!(
        title = %title,
        verdict = ?verdict,
        reason = reason,
        "Titel-Gate Entscheidung"
    );
    verdict
}

#[cfg(test)]
mod tests {
    use super::{decide, is_generic_title, TitleDecision};

    #[test]
    fn entscheidet_titel_anhand_qualitaet_und_verfuegbarkeit() {
        let cases = [
            (
                "",
                false,
                false,
                true,
                TitleDecision::TranscribeThenGenerate,
            ),
            (
                "clip",
                false,
                false,
                true,
                TitleDecision::TranscribeThenGenerate,
            ),
            (
                "bad title",
                false,
                false,
                false,
                TitleDecision::GenerateFromMetadata,
            ),
            (
                "bad title",
                false,
                true,
                true,
                TitleDecision::GenerateFromMetadata,
            ),
            (
                "Vindicta insane airshot triple kill",
                true,
                false,
                true,
                TitleDecision::UseExisting,
            ),
            (
                "gg",
                true,
                false,
                true,
                TitleDecision::TranscribeThenGenerate,
            ),
        ];

        for (title, vocab_hit, has_transcript, openai_available, expected) in cases {
            assert_eq!(
                decide(title, vocab_hit, has_transcript, openai_available),
                expected,
                "title={title:?}"
            );
        }
    }

    #[test]
    fn erkennt_generische_junk_titel() {
        for title in [
            "",
            "   ",
            "clip",
            "clips",
            "!clip",
            "clipped",
            "clipit",
            "clip it",
            "highlight",
            "live",
            "stream",
            "vod",
            "twitch",
            " CLIP ",
            "123456",
        ] {
            assert!(is_generic_title(title), "title={title:?}");
        }

        for title in ["Vindicta clip", "clip worthy", "123abc"] {
            assert!(!is_generic_title(title), "title={title:?}");
        }
    }
}
