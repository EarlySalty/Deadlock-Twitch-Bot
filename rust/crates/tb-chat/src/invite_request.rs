use std::sync::OnceLock;

use regex::Regex;

fn invite_term_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"invite|playtest|beta|\bein\w{0,3}lad|\blad(?:e|et|t)?\s+(?:\w+\s+){0,3}\bein\b",
        )
        .expect("valid invite term regex")
    })
}

fn question_signal_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?iu)\b(?:wer|mag|kann|könnte|koennte|jemand|würde|wuerde|hätte|haette)\b")
            .expect("valid invite question signal regex")
    })
}

fn friend_code_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b\d{6,12}\b").expect("valid friend code regex"))
}

fn steam_link_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)(?:https?://)?(?:www\.)?(?:s\.team/p/|steamcommunity\.com)")
            .expect("valid steam link regex")
    })
}

pub(crate) fn is_clear_invite_request(content: &str) -> bool {
    if has_friend_code(content) || is_offer(content) {
        return false;
    }
    let lower = content.to_lowercase();
    let folded = fold_german_umlauts(&lower);
    let words = words(&folded);
    if is_declined_or_done(&words) {
        return false;
    }
    let invite = invite_term_re().is_match(&folded)
        || words
            .iter()
            .any(|word| close_word(word, "einladen", 2) || close_word(word, "inviten", 1));
    let asks = words.iter().any(|word| {
        matches!(
            *word,
            "wie" | "benotige" | "brauche" | "krieg" | "kriege" | "mochte" | "will" | "suche"
        )
    });
    let self_request = (asks && words.contains(&"ich"))
        || words.iter().any(|word| {
            matches!(*word, "mich" | "mcih" | "mir" | "uns" | "me")
                || (word.starts_with('m') && word.len() >= 3 && close_word(word, "mich", 1))
        });
    if !invite || !self_request {
        return false;
    }
    let room_address = words.iter().any(|word| is_room_address(word));
    let direct_verb = words.iter().any(|word| {
        ["kannst", "kannste", "konntest", "wurdest", "magst"]
            .iter()
            .any(|candidate| close_word(word, candidate, 1))
    });
    room_address
        || asks
        || direct_verb
        || words.contains(&"du")
        || lower.contains('?')
        || question_signal_re().is_match(&lower)
}

fn is_declined_or_done(words: &[&str]) -> bool {
    let wants_future = words
        .iter()
        .any(|word| matches!(*word, "werden" | "werde" | "wird"));
    words.iter().enumerate().any(|(index, word)| match *word {
        "kein" | "keine" | "keinen" | "bereits" => true,
        "nicht" | "nich" | "net" => !words
            .get(index + 1)
            .is_some_and(|next| is_room_address(next)),
        "eingeladen" | "invited" | "geinvited" | "geinvitet" => !wants_future,
        _ => false,
    })
}

fn is_room_address(word: &str) -> bool {
    matches!(
        word,
        "jemand" | "wer" | "irgendwer" | "irgendjemand" | "einer" | "jmd"
    )
}

fn is_offer(content: &str) -> bool {
    let folded = fold_german_umlauts(&content.to_lowercase());
    let words = words(&folded);
    words.contains(&"ich")
        && words
            .iter()
            .any(|word| matches!(*word, "dich" | "euch" | "dir"))
        && words.iter().any(|word| {
            word.starts_with("lad") || word.starts_with("einlad") || word.starts_with("invit")
        })
}

fn has_friend_code(content: &str) -> bool {
    friend_code_re().is_match(content) || steam_link_re().is_match(content)
}

fn words(folded: &str) -> Vec<&str> {
    folded
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect()
}

fn close_word(word: &str, expected: &str, max_edits: usize) -> bool {
    if word.len().abs_diff(expected.len()) > max_edits {
        return false;
    }
    let expected: Vec<char> = expected.chars().collect();
    let mut row: Vec<usize> = (0..=expected.len()).collect();
    for (index, ch) in word.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = index + 1;
        for (column, target) in expected.iter().enumerate() {
            let old = row[column + 1];
            row[column + 1] = (row[column] + 1)
                .min(old + 1)
                .min(diagonal + usize::from(ch != *target));
            diagonal = old;
        }
    }
    row[expected.len()] <= max_edits
}

fn fold_german_umlauts(value: &str) -> String {
    value
        .replace('ä', "a")
        .replace('ö', "o")
        .replace('ü', "u")
        .replace('ß', "ss")
}

#[cfg(test)]
mod tests {
    use super::is_clear_invite_request;

    #[test]
    fn erkennt_invite_bitten_wie_der_discord_bot() {
        for raw in [
            "Hey, kann mich jemand einladen zum spiel?",
            "Hey, kann mich jemand bitte zu Deadlock inviten?",
            "Wer lädt mich ein?",
            "Kann mich jemand mal einladen",
            "suche jemand der mich in Deadlock einladen kann",
            "kann mich wer inviten pls",
            "Wie bekomme ich einen invite?",
            "Ich möchte eingeladen werden",
            "Kann mich schon mal jemand einladen?",
            "kann mich nicht jemand einladen?",
        ] {
            assert!(is_clear_invite_request(raw), "nicht erkannt: {raw}");
        }
    }

    #[test]
    fn angebote_codes_und_fremdthemen_sind_keine_bitte() {
        for raw in [
            "ich lad dich ein",
            "Kann mich jemand inviten? Mein Code ist 123456789",
            "Kann mich jemand inviten https://steamcommunity.com/id/example",
            "Wie bekomme ich Zugang?",
            "Das Game sieht echt mega aus",
            "der Invite ist raus, danke!",
            "Ich brauche keinen Invite mehr",
            "Du hast mich bereits eingeladen",
            "hat mich schon jemand eingeladen?",
            "Ich will nicht eingeladen werden",
            "Mich hat gestern einer eingeladen",
        ] {
            assert!(!is_clear_invite_request(raw), "falsch erkannt: {raw}");
        }
    }
}
